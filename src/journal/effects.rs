use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    Action, DirectoryIdentity, Error, Fingerprint, TransferItem, TransferKind, TrashItem,
    TreeFingerprint, store::Effect, trash,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Direction {
    Undo,
    Redo,
}

pub(super) fn apply(
    action: &mut Action,
    direction: Direction,
    checkpoint: &mut dyn FnMut(&Action) -> Result<(), Error>,
) -> Result<Effect, Error> {
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
        } => apply_transfer(*kind, items, direction, transfer, checkpoint),
        Action::Trash { items, transfer } => {
            apply_trash(items, direction, transfer, &mut |items, transfer| {
                checkpoint(&Action::Trash {
                    items: items.to_vec(),
                    transfer: transfer.clone(),
                })
            })
        }
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
                &mut |items, transfer| {
                    checkpoint(&Action::Restore {
                        items: items.to_vec(),
                        transfer: transfer.clone(),
                        replaced_existing: false,
                    })
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

type TrashCheckpoint<'a> =
    dyn FnMut(&[TrashItem], &crate::fs::JournalTransfer) -> Result<(), Error> + 'a;

fn apply_trash(
    items: &mut [TrashItem],
    direction: Direction,
    transfer: &mut crate::fs::JournalTransfer,
    checkpoint: &mut TrashCheckpoint<'_>,
) -> Result<Effect, Error> {
    for item in items.iter() {
        if item.restoration.is_some() || item.trashing.is_some() {
            continue;
        }
        match direction {
            Direction::Undo if item.restore_pending => {
                verify_tree(&item.original, &item.fingerprint)?
            }
            Direction::Undo => {
                verify_tree(&item.trashed, &item.fingerprint)?;
                ensure_absent(&item.original)?;
            }
            Direction::Redo if item.trash_pending => verify_tree(&item.trashed, &item.fingerprint)?,
            Direction::Redo => verify_tree(&item.original, &item.fingerprint)?,
        }
    }
    for index in 0..items.len() {
        let mut item = items[index].clone();
        match direction {
            Direction::Undo => {
                if !item.restore_pending {
                    if item.restoration.is_none() {
                        let cleanup = super::removal::RemovalPlan::capture(&item.trashed)?;
                        let source = item.trashed.clone();
                        let target = item.original.clone();
                        let result = transfer.apply_checkpointed(
                            crate::transfer::Action::Move,
                            &source,
                            &target,
                            &mut |staging, context| {
                                item.restoration = Some(
                                    super::recovery::Publication::capture(
                                        staging,
                                        Some(cleanup.clone()),
                                    )
                                    .map_err(|e| e.to_string())?,
                                );
                                items[index] = item.clone();
                                checkpoint(items, context).map_err(|e| e.to_string())
                            },
                        );
                        items[index] = item.clone();
                        result.map_err(Error::message)?;
                    }
                    let result = item
                        .restoration
                        .as_mut()
                        .unwrap()
                        .finish(&item.trashed, &item.original);
                    items[index] = item.clone();
                    item.fingerprint = result?;
                    item.restoration = None;
                    item.restore_pending = true;
                    items[index] = item.clone();
                    checkpoint(items, transfer)?;
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
            Direction::Redo => {
                if item.trash_pending {
                    continue;
                }
                if item.trashing.is_none() {
                    item.trashing =
                        Some(super::recovery::Publication::capture(&item.original, None)?);
                    items[index] = item.clone();
                    checkpoint(items, transfer)?;
                }
                let identity = item.trashing.as_ref().unwrap();
                let receipt = match fs::symlink_metadata(&item.original) {
                    Ok(_) => {
                        identity.verify(&item.original)?;
                        trash(&item.original)?
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        super::trash_receipt::recover_trash(&item.original, identity)?
                    }
                    Err(error) => {
                        return Err(Error::io("could not inspect pending Trash source", error));
                    }
                };
                identity.verify(&receipt.trashed)?;
                item.trashed = receipt.trashed;
                item.info = receipt.info;
                item.trash_pending = true;
                item.trashing = None;
                items[index] = item.clone();
                checkpoint(items, transfer)?;
            }
        }
        items[index] = item;
    }
    for item in items.iter_mut() {
        item.restore_pending = false;
        item.trash_pending = false;
    }
    *transfer = Default::default();
    Ok(trash_effect(items, direction))
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
    checkpoint: &mut dyn FnMut(&Action) -> Result<(), Error>,
) -> Result<Effect, Error> {
    if items.iter().any(|item| item.replaced_existing) {
        return Err(Error::message(
            "Refused Undo: this transfer replaced an existing destination that cannot be restored",
        ));
    }
    let save = |items: &[TransferItem],
                transfer: &crate::fs::JournalTransfer,
                checkpoint: &mut dyn FnMut(&Action) -> Result<(), Error>| {
        checkpoint(&Action::Transfer {
            kind,
            items: items.to_vec(),
            transfer: transfer.clone(),
        })
    };
    // Validate all untouched entries before making new changes. Pending effects
    // validate their recorded identities when they resume below.
    for item in items.iter() {
        if item.publication.is_some() || item.removal.is_some() {
            continue;
        }
        match direction {
            Direction::Undo if item.undone => {
                if matches!(kind, TransferKind::Move) {
                    verify_tree(&item.source, &item.source_fingerprint)?;
                }
            }
            Direction::Undo => {
                verify_tree(&item.destination, &item.result_fingerprint)?;
                if matches!(kind, TransferKind::Move) {
                    ensure_absent(&item.source)?;
                }
            }
            Direction::Redo if item.undone => {
                verify_tree(&item.source, &item.source_fingerprint)?;
                ensure_absent(&item.destination)?;
            }
            Direction::Redo => verify_tree(&item.destination, &item.result_fingerprint)?,
        }
    }
    let indices: Vec<_> = if direction == Direction::Undo {
        (0..items.len()).rev().collect()
    } else {
        (0..items.len()).collect()
    };
    for index in indices {
        if items[index].undone == (direction == Direction::Undo) {
            continue;
        }
        let mut item = items[index].clone();
        if matches!((kind, direction), (TransferKind::Copy, Direction::Undo)) {
            if item.removal.is_none() {
                item.removal = Some(super::removal::RemovalPlan::capture(&item.destination)?);
                verify_tree(&item.destination, &item.result_fingerprint)?;
                items[index] = item.clone();
                save(items, transfer, checkpoint)?;
            }
            let result = item.removal.as_mut().unwrap().remove(&item.destination);
            items[index] = item.clone();
            result?;
            item.removal = None;
        } else {
            let (source, destination) = if direction == Direction::Undo {
                (item.destination.clone(), item.source.clone())
            } else {
                (item.source.clone(), item.destination.clone())
            };
            if item.publication.is_none() {
                let action = if matches!(kind, TransferKind::Move) {
                    crate::transfer::Action::Move
                } else {
                    crate::transfer::Action::Copy
                };
                let cleanup = if matches!(kind, TransferKind::Move) {
                    Some(super::removal::RemovalPlan::capture(&source)?)
                } else {
                    None
                };
                let result = transfer.apply_checkpointed(
                    action,
                    &source,
                    &destination,
                    &mut |staging, context| {
                        item.publication = Some(
                            super::recovery::Publication::capture(staging, cleanup.clone())
                                .map_err(|e| e.to_string())?,
                        );
                        items[index] = item.clone();
                        save(items, context, checkpoint).map_err(|e| e.to_string())
                    },
                );
                items[index] = item.clone();
                result.map_err(Error::message)?;
            }
            let result = item
                .publication
                .as_mut()
                .unwrap()
                .finish(&source, &destination);
            items[index] = item.clone();
            let fingerprint = result?;
            if direction == Direction::Undo {
                item.source_fingerprint = fingerprint;
            } else {
                item.result_fingerprint = fingerprint;
            }
            item.publication = None;
        }
        item.undone = direction == Direction::Undo;
        items[index] = item;
        save(items, transfer, checkpoint)?;
    }
    *transfer = Default::default();
    Ok(transfer_effect(kind, items, direction))
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

pub(super) fn verify_tree(path: &Path, expected: &TreeFingerprint) -> Result<(), Error> {
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

pub(super) fn ensure_absent(path: &Path) -> Result<(), Error> {
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
pub(super) fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), Error> {
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
pub(super) fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), Error> {
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
