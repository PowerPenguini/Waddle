use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    Action, DirectoryIdentity, Error, Fingerprint, TransferItem, TransferKind, TrashItem,
    TreeFingerprint, store::Effect, trash,
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
        Action::NewFolder {
            path,
            fingerprint,
            identity,
        } => match direction {
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
                if let Some(expected) = identity {
                    if DirectoryIdentity::read(path)? != *expected {
                        return Err(Error::message(format!(
                            "Refused Undo: {} is a different folder",
                            path.display()
                        )));
                    }
                } else {
                    // Older records have no identity; retain their conservative check.
                    verify(path, fingerprint)?;
                }
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
                *identity = Some(DirectoryIdentity::read(path)?);
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
        Action::Transfer {
            kind,
            items,
            transfer,
        } => apply_transfer(*kind, items, direction, transfer),
        Action::Trash { items, transfer } => apply_trash(items, direction, transfer),
        Action::Restore {
            items,
            transfer,
            replaced_existing,
        } => {
            if *replaced_existing {
                return Err(Error::message(
                    "Refused Undo: this Restore merged with or replaced an existing destination that cannot be restored",
                ));
            }
            let mut effect = apply_trash(
                items,
                match direction {
                    Direction::Undo => Direction::Redo,
                    Direction::Redo => Direction::Undo,
                },
                transfer,
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

fn apply_trash(
    items: &mut [TrashItem],
    direction: Direction,
    transfer: &mut crate::fs::JournalTransfer,
) -> Result<Effect, Error> {
    let effect: Result<Effect, Error> = match direction {
        Direction::Undo => {
            for item in items.iter() {
                if item.restore_pending {
                    verify_tree(&item.original, &item.fingerprint)?;
                } else {
                    verify_tree(&item.trashed, &item.fingerprint)?;
                    ensure_absent(&item.original)?;
                }
            }
            for item in items.iter_mut() {
                if !item.restore_pending {
                    transfer.apply(crate::transfer::Action::Move, &item.trashed, &item.original)?;
                    item.restore_pending = true;
                    item.fingerprint = TreeFingerprint::read(&item.original)?;
                }
                match fs::remove_file(&item.info) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(Error::io(
                            "restored the item but could not remove Trash metadata",
                            error,
                        ));
                    }
                }
            }
            // Journal::undo/redo persists these flags on failure. Clear them only
            // once every restore and cleanup has completed and the cursor can advance.
            for item in items.iter_mut() {
                item.restore_pending = false;
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
    };
    let effect = effect?;
    *transfer = Default::default();
    Ok(effect)
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
    transfer: &mut crate::fs::JournalTransfer,
) -> Result<Effect, Error> {
    if items.iter().any(|item| item.replaced_existing) {
        return Err(Error::message(
            "Refused Undo: this transfer replaced an existing destination that cannot be restored",
        ));
    }
    let effect: Result<Effect, Error> = match direction {
        Direction::Undo => {
            for item in items.iter() {
                if item.undone {
                    if matches!(kind, TransferKind::Move) {
                        verify_tree(&item.source, &item.source_fingerprint)?;
                    }
                    continue;
                }
                if let Some(removal) = &item.removal {
                    removal.verify(&item.destination)?;
                } else {
                    verify_tree(&item.destination, &item.result_fingerprint)?;
                }
                if matches!(kind, TransferKind::Move) {
                    ensure_absent(&item.source)?;
                }
            }
            for item in items.iter_mut().rev().filter(|item| !item.undone) {
                let result = match kind {
                    // A single unlink cannot partially remove an item. Only
                    // directories need the persisted per-entry removal plan.
                    TransferKind::Copy if !item.result_fingerprint.is_directory() => {
                        crate::fs::journal_remove(&item.destination)
                    }
                    TransferKind::Copy => {
                        if item.removal.is_none() {
                            let plan = super::removal::RemovalPlan::capture(&item.destination)?;
                            verify_tree(&item.destination, &item.result_fingerprint)?;
                            item.removal = Some(plan);
                        }
                        item.removal
                            .as_mut()
                            .unwrap()
                            .remove(&item.destination)
                            .map_err(|e| e.to_string())
                    }
                    TransferKind::Move => transfer.apply(
                        crate::transfer::Action::Move,
                        &item.destination,
                        &item.source,
                    ),
                };
                if let Err(error) = result {
                    return Err(error.into());
                }
                item.undone = true;
                item.removal = None;
                if matches!(kind, TransferKind::Move) {
                    item.source_fingerprint = TreeFingerprint::read(&item.source)?;
                }
            }
            Ok(transfer_effect(kind, items, Direction::Undo))
        }
        Direction::Redo => {
            for item in items.iter() {
                if item.undone {
                    verify_tree(&item.source, &item.source_fingerprint)?;
                    ensure_absent(&item.destination)?;
                } else {
                    verify_tree(&item.destination, &item.result_fingerprint)?;
                }
            }
            // As with Undo, retain completed entries when a later entry fails.
            // Journal saves their state on error so a retry can resume safely.
            for item in items.iter_mut().filter(|item| item.undone) {
                let result = match kind {
                    TransferKind::Copy => transfer.apply(
                        crate::transfer::Action::Copy,
                        &item.source,
                        &item.destination,
                    ),
                    TransferKind::Move => transfer.apply(
                        crate::transfer::Action::Move,
                        &item.source,
                        &item.destination,
                    ),
                };
                if let Err(error) = result {
                    return Err(error.into());
                }
                item.result_fingerprint = TreeFingerprint::read(&item.destination)?;
                item.undone = false;
            }
            Ok(transfer_effect(kind, items, Direction::Redo))
        }
    };
    let effect = effect?;
    *transfer = Default::default();
    Ok(effect)
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

#[cfg(test)]
mod regressions {
    use super::*;

    #[test]
    fn cross_filesystem_move_can_redo_after_undo() {
        let original_fs = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let other_fs = tempfile::tempdir_in("/dev/shm").unwrap();
        let source = original_fs.path().join("folder");
        let destination = other_fs.path().join("folder");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("item"), "data").unwrap();
        crate::fs::journal_move(&source, &destination).unwrap();
        let receipt = crate::fs::TransferReceipt {
            source: source.clone(),
            destination: destination.clone(),
            replaced_existing: false,
        };
        let action = Action::transfer(TransferKind::Move, &[receipt])
            .unwrap()
            .unwrap();
        let mut journal = super::super::Journal::in_memory();
        journal.record(action).unwrap();
        journal.undo().unwrap();
        assert!(source.exists());
        journal
            .redo()
            .expect("an unchanged directory should be redoable across filesystems");
    }
    #[test]
    fn copy_undo_can_recover_after_partial_removal_failure() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let first_parent = temp.path().join("first");
        let second_parent = temp.path().join("second");
        fs::create_dir(&first_parent).unwrap();
        fs::create_dir(&second_parent).unwrap();
        let mut receipts = Vec::new();
        for (i, parent) in [first_parent.clone(), second_parent.clone()]
            .into_iter()
            .enumerate()
        {
            let source = temp.path().join(format!("source{i}"));
            let destination = parent.join("copy");
            fs::write(&source, "data").unwrap();
            crate::fs::journal_copy(&source, &destination).unwrap();
            receipts.push(crate::fs::TransferReceipt {
                source,
                destination,
                replaced_existing: false,
            });
        }
        let action = Action::transfer(TransferKind::Copy, &receipts)
            .unwrap()
            .unwrap();
        let journal_path = temp.path().join("history.json");
        let mut journal = super::super::Journal::open(journal_path.clone()).unwrap();
        journal.record(action).unwrap();
        fs::set_permissions(&first_parent, fs::Permissions::from_mode(0o500)).unwrap();
        let failed = journal.undo();
        fs::set_permissions(&first_parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed.is_err());
        assert!(
            !receipts[1].destination.exists(),
            "second copy was already removed"
        );
        let mut journal = super::super::Journal::open(journal_path).unwrap();
        journal
            .undo()
            .expect("retry Undo should handle already removed entries");
        journal.redo().unwrap();
        for receipt in &receipts {
            assert!(receipt.destination.exists());
        }
    }
}
