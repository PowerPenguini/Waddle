fn audit_source_edit(action: Action, replace: bool, sparse: bool) {
    use std::io::{Seek, SeekFrom, Write};
    let temp = tempfile::tempdir().unwrap();
    let target = if matches!(action, Action::Move) {
        tempfile::tempdir_in("/dev/shm").unwrap()
    } else {
        tempfile::tempdir().unwrap()
    };
    let source = temp.path().join("source.bin");
    let destination = target.path().join("source.bin");
    let size = 4 * 1024 * 1024;
    let mut initial = vec![b'A'; size];
    if sparse {
        initial[1024 * 1024..3 * 1024 * 1024].fill(0);
        let mut file = fs::File::create(&source).unwrap();
        file.set_len(size as u64).unwrap();
        file.write_all(&initial[..1024 * 1024]).unwrap();
        file.seek(SeekFrom::Start(3 * 1024 * 1024)).unwrap();
        file.write_all(&initial[3 * 1024 * 1024..]).unwrap();
        assert!(fs::metadata(&source).unwrap().blocks() * 512 < size as u64);
    } else {
        fs::write(&source, &initial).unwrap();
    }
    if replace {
        fs::write(&destination, b"original destination").unwrap();
    }
    let batch =
        TransferBatch::try_new(vec![source.clone()], target.path().to_path_buf(), action).unwrap();
    let batch = if replace {
        let TransferBatchOutcome::Conflict { batch, .. } = batch.run() else {
            panic!("expected conflict")
        };
        batch.resolve(ConflictChoice::Replace, false)
    } else {
        batch
    };
    let mut edited_at = None;
    let outcome = batch.run_with(
        || false,
        |progress| {
            if edited_at.is_none()
                && progress.completed_bytes > 0
                && progress.completed_bytes < size as u64
            {
                edited_at = Some(progress.completed_bytes);
                let mut file = fs::OpenOptions::new().write(true).open(&source).unwrap();
                file.seek(SeekFrom::Start(0)).unwrap();
                file.write_all(&vec![b'B'; size]).unwrap();
            }
        },
    );
    assert!(edited_at.is_some(), "must edit during chunked copying");
    let TransferBatchOutcome::Complete(report) = outcome else {
        panic!("unexpected conflict")
    };
    let copied = fs::read(&destination).ok();
    eprintln!(
        "{action:?}, replace={replace}, sparse={sparse}: edit after {} bytes; receipts={}, failures={}, warnings={}",
        edited_at.unwrap(),
        report.receipts.len(),
        report.failures.len(),
        report.warnings.len()
    );
    if let Some(copied) = &copied {
        eprintln!(
            "destination old bytes: {}; new bytes: {}",
            copied.iter().filter(|b| **b == b'A').count(),
            copied.iter().filter(|b| **b == b'B').count()
        );
    }
    if matches!(action, Action::Move) {
        assert!(
            !report.failures.is_empty(),
            "changed source must prevent cross-device publication"
        );
        assert_eq!(fs::read(&source).unwrap(), vec![b'B'; size]);
        assert_eq!(copied, replace.then(|| b"original destination".to_vec()));
    } else {
        let coherent = copied.as_ref().is_none_or(|bytes| {
            bytes == &initial
                || bytes == &vec![b'B'; size]
                || (replace && bytes == b"original destination")
        });
        assert!(coherent, "published a mixture of two source versions");
    }
}

#[test]
#[ignore = "Known defect under audit: concurrent source edit publishes a mixed copy"]
fn audit_copy_rejects_mixed_source_versions() {
    audit_source_edit(Action::Copy, false, false);
}
#[test]
#[ignore = "Known defect under audit: concurrent source edit corrupts replacement copy"]
fn audit_replace_copy_rejects_mixed_source_versions() {
    audit_source_edit(Action::Copy, true, false);
}
#[test]
#[ignore = "Known defect under audit: concurrent source edit corrupts sparse copy"]
fn audit_sparse_copy_rejects_mixed_source_versions() {
    audit_source_edit(Action::Copy, false, true);
}
#[test]
fn audit_cross_device_move_rejects_mixed_source_versions() {
    audit_source_edit(Action::Move, false, false);
}
#[test]
fn audit_cross_device_replace_move_rejects_mixed_source_versions() {
    audit_source_edit(Action::Move, true, false);
}
