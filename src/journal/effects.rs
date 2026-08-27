use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    Action, Error, Fingerprint, TransferItem, TransferKind, TrashItem, TreeFingerprint,
    store::Effect, trash,
};

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Undo,
    Redo,
}

pub(super) fn apply(action: &mut Action, direction: Direction) -> Result<Effect, Error> {
    match action {
        Action::Rename {
            before,
            after,
            fingerprint,
        } => {
            let (source, destination, label) = match direction {
                Direction::Undo => (after.as_path(), before.as_path(), "Undid rename"),
                Direction::Redo => (before.as_path(), after.as_path(), "Redid rename"),
            };
            verify(source, fingerprint)?;
            ensure_absent(destination)?;
            rename_noreplace(source, destination)?;
            *fingerprint = Fingerprint::read(destination)?;
            Ok(Effect {
                status: label.to_owned(),
                changed_folders: parent_folders(source, destination),
                select: Some(destination.to_path_buf()),
            })
        }
        Action::NewFolder { path, fingerprint } => match direction {
            Direction::Undo => {
                let mut entries = fs::read_dir(&*path).map_err(|error| {
                    Error::io(format!("could not inspect {}", path.display()), error)
                })?;
                if entries.next().is_some() {
                    return Err(Error::message(format!(
                        "Refused Undo: {} is no longer empty",
                        path.display()
                    )));
                }
                verify(path, fingerprint)?;
                fs::remove_dir(&*path)
                    .map_err(|error| Error::io("could not undo New Folder", error))?;
                Ok(Effect {
                    status: "Undid New Folder".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: None,
                })
            }
            Direction::Redo => {
                ensure_absent(path)?;
                fs::create_dir(&*path)
                    .map_err(|error| Error::io("could not redo New Folder", error))?;
                *fingerprint = Fingerprint::read(path)?;
                Ok(Effect {
                    status: "Redid New Folder".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: Some(path.clone()),
                })
            }
        },
        Action::NewFile { path, fingerprint } => match direction {
            Direction::Undo => {
                verify(path, fingerprint)?;
                fs::remove_file(&*path)
                    .map_err(|error| Error::io("could not undo New File", error))?;
                Ok(Effect {
                    status: "Undid New File".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: None,
                })
            }
            Direction::Redo => {
                ensure_absent(path)?;
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&*path)
                    .map_err(|error| Error::io("could not redo New File", error))?;
                *fingerprint = Fingerprint::read(path)?;
                Ok(Effect {
                    status: "Redid New File".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: Some(path.clone()),
                })
            }
        },
        Action::Transfer { kind, items } => apply_transfer(*kind, items, direction),
        Action::Trash { items } => apply_trash(items, direction),
        Action::Restore { items } => {
            let mut effect = apply_trash(
                items,
                match direction {
                    Direction::Undo => Direction::Redo,
                    Direction::Redo => Direction::Undo,
                },
            )?;
            effect.status = match direction {
                Direction::Undo => "Undid Restore",
                Direction::Redo => "Redid Restore",
            }
            .to_owned();
            Ok(effect)
        }
    }
}

fn apply_trash(items: &mut [TrashItem], direction: Direction) -> Result<Effect, Error> {
    match direction {
        Direction::Undo => {
            for item in items.iter() {
                verify_tree(&item.trashed, &item.fingerprint)?;
                ensure_absent(&item.original)?;
            }
            let mut moved: Vec<TrashItem> = Vec::new();
            for item in items.iter() {
                if let Err(error) = crate::fs::journal_move(&item.trashed, &item.original) {
                    for previous in moved.iter().rev() {
                        let _ = crate::fs::journal_move(&previous.original, &previous.trashed);
                    }
                    return Err(error.into());
                }
                moved.push(item.clone());
            }
            for item in items.iter() {
                fs::remove_file(&item.info).map_err(|error| {
                    Error::io(
                        "restored the item but could not remove Trash metadata",
                        error,
                    )
                })?;
            }
            Ok(trash_effect(items, Direction::Undo))
        }
        Direction::Redo => {
            for item in items.iter() {
                verify_tree(&item.original, &item.fingerprint)?;
            }
            let mut receipts = Vec::new();
            for item in items.iter() {
                match trash(&item.original) {
                    Ok(receipt) => receipts.push(receipt),
                    Err(error) => {
                        for receipt in receipts.iter().rev() {
                            let _ = crate::fs::journal_move(&receipt.trashed, &receipt.original);
                            let _ = fs::remove_file(&receipt.info);
                        }
                        return Err(error);
                    }
                }
            }
            for (item, receipt) in items.iter_mut().zip(receipts) {
                item.trashed = receipt.trashed;
                item.info = receipt.info;
                item.fingerprint = TreeFingerprint::read(&item.trashed)?;
            }
            Ok(trash_effect(items, Direction::Redo))
        }
    }
}

fn trash_effect(items: &[TrashItem], direction: Direction) -> Effect {
    Effect {
        status: match direction {
            Direction::Undo => "Undid Trash",
            Direction::Redo => "Redid Trash",
        }
        .to_owned(),
        changed_folders: items
            .iter()
            .filter_map(|item| item.original.parent().map(Path::to_path_buf))
            .fold(Vec::new(), |mut folders, folder| {
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
                folders
            }),
        select: matches!(direction, Direction::Undo)
            .then(|| items.first().map(|item| item.original.clone()))
            .flatten(),
    }
}

fn apply_transfer(
    kind: TransferKind,
    items: &mut [TransferItem],
    direction: Direction,
) -> Result<Effect, Error> {
    if items.iter().any(|item| item.replaced_existing) {
        return Err(Error::message(
            "Refused Undo: this transfer replaced an existing destination that cannot be restored",
        ));
    }
    match direction {
        Direction::Undo => {
            for item in items.iter() {
                verify_tree(&item.destination, &item.result_fingerprint)?;
                if matches!(kind, TransferKind::Move) {
                    ensure_absent(&item.source)?;
                }
            }
            let mut completed = Vec::new();
            for item in items.iter().rev() {
                let result = match kind {
                    TransferKind::Copy => crate::fs::journal_remove(&item.destination),
                    TransferKind::Move => crate::fs::journal_move(&item.destination, &item.source),
                };
                if let Err(error) = result {
                    rollback_transfer(kind, items, &completed, Direction::Undo);
                    return Err(error.into());
                }
                completed.push(item.source.clone());
            }
            Ok(transfer_effect(kind, items, Direction::Undo))
        }
        Direction::Redo => {
            for item in items.iter() {
                verify_tree(&item.source, &item.source_fingerprint)?;
                ensure_absent(&item.destination)?;
            }
            let mut completed = Vec::new();
            for item in items.iter_mut() {
                let result = match kind {
                    TransferKind::Copy => crate::fs::journal_copy(&item.source, &item.destination),
                    TransferKind::Move => crate::fs::journal_move(&item.source, &item.destination),
                };
                if let Err(error) = result {
                    rollback_transfer(kind, items, &completed, Direction::Redo);
                    return Err(error.into());
                }
                item.result_fingerprint = TreeFingerprint::read(&item.destination)?;
                completed.push(item.destination.clone());
            }
            Ok(transfer_effect(kind, items, Direction::Redo))
        }
    }
}

fn rollback_transfer(
    kind: TransferKind,
    items: &[TransferItem],
    completed: &[PathBuf],
    direction: Direction,
) {
    for path in completed.iter().rev() {
        let Some(item) = items
            .iter()
            .find(|item| item.source == *path || item.destination == *path)
        else {
            continue;
        };
        match (kind, direction) {
            (TransferKind::Copy, Direction::Redo) => {
                let _ = crate::fs::journal_remove(&item.destination);
            }
            (TransferKind::Move, Direction::Undo) => {
                let _ = crate::fs::journal_move(&item.source, &item.destination);
            }
            (TransferKind::Move, Direction::Redo) => {
                let _ = crate::fs::journal_move(&item.destination, &item.source);
            }
            (TransferKind::Copy, Direction::Undo) => {}
        }
    }
}

fn transfer_effect(kind: TransferKind, items: &[TransferItem], direction: Direction) -> Effect {
    let verb = match (kind, direction) {
        (TransferKind::Copy, Direction::Undo) => "Undid Copy",
        (TransferKind::Copy, Direction::Redo) => "Redid Copy",
        (TransferKind::Move, Direction::Undo) => "Undid Move",
        (TransferKind::Move, Direction::Redo) => "Redid Move",
    };
    let mut changed_folders = Vec::new();
    for item in items {
        for path in [&item.source, &item.destination] {
            if let Some(parent) = path.parent()
                && !changed_folders.iter().any(|existing| existing == parent)
            {
                changed_folders.push(parent.to_path_buf());
            }
        }
    }
    let select = items.first().map(|item| match (kind, direction) {
        (TransferKind::Copy, Direction::Undo) => item.source.clone(),
        (_, Direction::Undo) => item.source.clone(),
        (_, Direction::Redo) => item.destination.clone(),
    });
    Effect {
        status: verb.to_owned(),
        changed_folders,
        select,
    }
}

fn verify_tree(path: &Path, expected: &TreeFingerprint) -> Result<(), Error> {
    if &TreeFingerprint::read(path)? == expected {
        Ok(())
    } else {
        Err(Error::message(format!(
            "Refused operation: {} or its contents changed after it was recorded",
            path.display()
        )))
    }
}

fn verify(path: &Path, expected: &Fingerprint) -> Result<(), Error> {
    if &Fingerprint::read(path)? == expected {
        Ok(())
    } else {
        Err(Error::message(format!(
            "Refused operation: {} changed after it was recorded",
            path.display()
        )))
    }
}

fn ensure_absent(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(Error::message(format!(
            "Refused operation: {} now exists",
            path.display()
        ))),
        Err(error) => Err(Error::io(
            format!("could not inspect {}", path.display()),
            error,
        )),
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), Error> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| Error::message("source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| Error::message("destination path contains NUL"))?;
    // SAFETY: both C strings remain valid for the duration of the syscall.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(Error::io(
            "could not move entry",
            io::Error::last_os_error(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), Error> {
    ensure_absent(destination)?;
    fs::rename(source, destination).map_err(|error| Error::io("could not move entry", error))
}

fn parent_folders(first: &Path, second: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for path in [first, second] {
        if let Some(parent) = path.parent()
            && !folders.iter().any(|existing| existing == parent)
        {
            folders.push(parent.to_path_buf());
        }
    }
    folders
}
