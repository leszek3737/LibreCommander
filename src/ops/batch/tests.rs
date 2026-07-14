use super::*;
use crate::app::types::{PendingAction, TransferAction};
use std::fs;
use std::sync::atomic::Ordering;

const EXPECTED_DUP_MSG: &str = "duplicate destination";

fn make_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).unwrap();
    p
}

#[test]
fn batch_copy_files_to_dest() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();

    let f1 = make_file(src_dir.path(), "a.txt", b"hello");
    let f2 = make_file(src_dir.path(), "b.txt", b"world");

    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch(action);

    assert_eq!(report.success_count, 2);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
    assert!(dest_dir.path().join("a.txt").exists());
    assert!(dest_dir.path().join("b.txt").exists());
}

#[test]
fn batch_copy_duplicate_dest_reports_error() {
    let src_a = tempfile::tempdir().unwrap();
    let src_b = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();

    let f1 = make_file(src_a.path(), "same.txt", b"a");
    let f2 = make_file(src_b.path(), "same.txt", b"b");

    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch(action);

    assert_eq!(report.success_count, 1);
    assert_eq!(report.errors.len(), 1);
    assert!(!report.canceled);
    assert!(report.errors[0].contains(EXPECTED_DUP_MSG));
}

#[test]
fn batch_copy_overwrite_true_replaces_existing() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();

    let f1 = make_file(src.path(), "a.txt", b"new content");
    make_file(dest.path(), "a.txt", b"old content");

    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1],
        dest: dest.path().to_path_buf(),
        overwrite: true,
    });

    let report = execute_batch(action);

    assert_eq!(report.success_count, 1);
    assert!(report.errors.is_empty());
    assert_eq!(fs::read(dest.path().join("a.txt")).unwrap(), b"new content");
}

#[test]
fn batch_move_files_to_dest() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();

    let f1 = make_file(src_dir.path(), "x.txt", b"data");
    let f2 = make_file(src_dir.path(), "y.txt", b"more");

    let action = PendingAction::Move(TransferAction {
        sources: vec![f1.clone(), f2.clone()],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch(action);

    assert_eq!(report.success_count, 2);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
    assert!(!f1.exists());
    assert!(!f2.exists());
    assert!(dest_dir.path().join("x.txt").exists());
    assert!(dest_dir.path().join("y.txt").exists());
}

#[test]
fn batch_delete_files() {
    let dir = tempfile::tempdir().unwrap();

    let f1 = make_file(dir.path(), "del1.txt", b"a");
    let f2 = make_file(dir.path(), "del2.txt", b"b");

    let action = PendingAction::Delete {
        paths: vec![f1.clone(), f2.clone()],
    };

    let report = execute_batch(action);

    assert_eq!(report.success_count, 2);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
    assert!(!f1.exists());
    assert!(!f2.exists());
}

#[test]
fn batch_delete_nonexistent_reports_error() {
    let action = PendingAction::Delete {
        paths: vec![PathBuf::from("lc_nonexistent_delete_test_xyz")],
    };

    let report = execute_batch(action);

    assert_eq!(report.success_count, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(!report.canceled);
}

#[test]
fn batch_delete_reports_progress() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = make_file(dir.path(), "one.txt", b"1");
    let f2 = make_file(dir.path(), "two.txt", b"2");
    let action = PendingAction::Delete {
        paths: vec![f1, f2],
    };
    let mut updates = Vec::new();

    let report = execute_batch_with_byte_progress(
        action,
        |progress| updates.push(progress),
        &None,
        "Delete",
    );

    assert_eq!(report.success_count, 2);
    assert!(!report.canceled);
    assert_eq!(
        updates.first().map(|p| (p.completed, p.total)),
        Some((0, 2))
    );
    assert_eq!(updates.last().map(|p| (p.completed, p.total)), Some((2, 2)));
    assert_eq!(updates.last().map(BatchProgress::percent), Some(1.0));
}

#[test]
fn batch_copy_cancel_stops_between_items() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "first.txt", b"1");
    let f2 = make_file(src_dir.path(), "second.txt", b"2");
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_progress = Arc::clone(&cancel);
    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch_with_byte_progress(
        action,
        |progress| {
            if progress.completed == 1 {
                cancel_for_progress.store(true, Ordering::Relaxed);
            }
        },
        &Some(cancel),
        "Copy",
    );

    assert_eq!(report.success_count, 1);
    assert!(report.canceled);
    assert!(dest_dir.path().join("first.txt").exists());
    assert!(!dest_dir.path().join("second.txt").exists());
}

#[test]
fn batch_copy_post_action_cancel_reports_canceled() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "first.txt", b"1");
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_progress = Arc::clone(&cancel);
    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch_with_byte_progress(
        action,
        |progress| {
            if progress.completed == 1 {
                cancel_for_progress.store(true, Ordering::Relaxed);
            }
        },
        &Some(cancel),
        "Copy",
    );

    assert_eq!(report.success_count, 1);
    assert!(report.canceled);
    assert!(dest_dir.path().join("first.txt").exists());
}

#[test]
fn batch_copy_reports_cumulative_byte_progress() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "first.txt", b"12345");
    let f2 = make_file(src_dir.path(), "second.txt", b"1234567");
    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let mut updates = Vec::new();

    let report =
        execute_batch_with_byte_progress(action, |progress| updates.push(progress), &None, "Copy");

    assert_eq!(report.success_count, 2);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
    assert_eq!(updates.first().map(|p| p.bytes_total), Some(12));
    assert_eq!(updates.last().map(|p| p.bytes_done), Some(12));
    assert_eq!(updates.last().map(|p| p.current_file_total), Some(0));
    assert!(updates.iter().any(|p| {
        p.current
            .as_ref()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == "second.txt"))
            && p.bytes_done == 12
            && p.current_file_bytes == 7
            && p.current_file_total == 7
    }));
}

#[test]
fn batch_copy_cancel_before_start_copies_nothing() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "a.txt", b"data");
    let f2 = make_file(src_dir.path(), "b.txt", b"more");
    let cancel = Arc::new(AtomicBool::new(true));
    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch_with_byte_progress(action, |_| {}, &Some(cancel), "Copy");
    assert!(report.canceled);
    assert_eq!(report.success_count, 0);
    assert!(!dest_dir.path().join("a.txt").exists());
    assert!(!dest_dir.path().join("b.txt").exists());
}

#[test]
fn batch_delete_cancel_before_start_deletes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = make_file(dir.path(), "a.txt", b"data");
    let f2 = make_file(dir.path(), "b.txt", b"more");
    let cancel = Arc::new(AtomicBool::new(true));
    let action = PendingAction::Delete {
        paths: vec![f1.clone(), f2.clone()],
    };
    let report = execute_batch_with_byte_progress(action, |_| {}, &Some(cancel), "Delete");
    assert!(report.canceled);
    assert!(f1.exists());
    assert!(f2.exists());
}

#[test]
fn dedup_paths_parent_removes_child() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("parent");
    fs::create_dir(&parent).unwrap();
    let child = parent.join("child.txt");
    fs::write(&child, b"x").unwrap();
    let result = dedup_paths(&[child, parent.clone()]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], parent);
}

#[test]
fn empty_sources_batch_copy_returns_empty() {
    let dest_dir = tempfile::tempdir().unwrap();
    let action = PendingAction::Copy(TransferAction {
        sources: vec![],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
}

#[test]
fn empty_sources_batch_delete_returns_empty() {
    let action = PendingAction::Delete { paths: vec![] };
    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
}

#[test]
fn empty_sources_batch_move_returns_empty() {
    let dest_dir = tempfile::tempdir().unwrap();
    let action = PendingAction::Move(TransferAction {
        sources: vec![],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
}

#[test]
fn batch_copy_nonexistent_source_reports_error() {
    let dest_dir = tempfile::tempdir().unwrap();
    let action = PendingAction::Copy(TransferAction {
        sources: vec![PathBuf::from("lc_nonexistent_copy_test_xyz")],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(!report.canceled);
}

#[test]
fn batch_move_nonexistent_source_reports_error() {
    let dest_dir = tempfile::tempdir().unwrap();
    let action = PendingAction::Move(TransferAction {
        sources: vec![PathBuf::from("lc_nonexistent_move_test_xyz")],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(!report.canceled);
}

#[test]
fn batch_copy_small_files_progress_invariants() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "a.txt", b"hello");
    let f2 = make_file(src_dir.path(), "b.txt", b"world");
    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let mut updates = Vec::new();
    let report = execute_batch_with_byte_progress(action, |p| updates.push(p), &None, "Copy");
    assert_eq!(report.success_count, 2);
    assert!(updates.iter().all(|p| p.bytes_done <= p.bytes_total));
    assert!(
        updates
            .iter()
            .all(|p| p.current_file_bytes <= p.current_file_total)
    );
    assert!(
        updates
            .windows(2)
            .all(|pair| pair[0].bytes_done <= pair[1].bytes_done)
    );
}

#[test]
fn batch_progress_clamps_bytes_to_total() {
    let progress = BatchProgress {
        completed: 1,
        total: 1,
        current: None,
        bytes_done: 11,
        bytes_total: 10,
        current_file_bytes: 6,
        current_file_total: 5,
        start_time: None,
    }
    .with_clamped_bytes();

    assert_eq!(progress.bytes_done, 10);
    assert_eq!(progress.bytes_total, 10);
    assert_eq!(progress.current_file_bytes, 5);
    assert_eq!(progress.current_file_total, 5);
}

#[test]
fn batch_copy_large_file_progress_never_exceeds_total() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let data = vec![b'x'; 4097];
    let file = make_file(src_dir.path(), "large.bin", &data);
    let action = PendingAction::Copy(TransferAction {
        sources: vec![file],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let mut updates = Vec::new();

    let report =
        execute_batch_with_byte_progress(action, |progress| updates.push(progress), &None, "Copy");

    assert_eq!(report.success_count, 1);
    assert!(report.errors.is_empty());
    assert!(updates.iter().all(|p| p.bytes_done <= p.bytes_total));
    assert!(
        updates
            .iter()
            .all(|p| p.current_file_bytes <= p.current_file_total)
    );
    assert!(
        updates
            .windows(2)
            .all(|pair| pair[0].bytes_done <= pair[1].bytes_done)
    );
    assert_eq!(updates.last().map(BatchProgress::byte_percent), Some(100.0));
}

#[test]
fn batch_delete_reports_item_byte_progress() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = make_file(dir.path(), "one.txt", b"123");
    let f2 = make_file(dir.path(), "two.txt", b"1234");
    let action = PendingAction::Delete {
        paths: vec![f1, f2],
    };
    let mut updates = Vec::new();

    let report = execute_batch_with_byte_progress(
        action,
        |progress| updates.push(progress),
        &None,
        "Delete",
    );

    assert_eq!(report.success_count, 2);
    assert!(report.errors.is_empty());
    assert!(!report.canceled);
    assert_eq!(updates.first().map(|p| p.bytes_total), Some(7));
    assert_eq!(updates.last().map(|p| p.bytes_done), Some(7));
    assert_eq!(updates.last().map(BatchProgress::byte_percent), Some(100.0));
}

#[test]
fn format_summary_copy_success() {
    let report = BatchReport {
        errors: vec![],
        success_count: 3,
        canceled: false,
        action_label: "Copy",
    };
    assert_eq!(report.format_summary(), "Copied 3 files");
}

#[test]
fn format_summary_delete_single() {
    let report = BatchReport {
        errors: vec![],
        success_count: 1,
        canceled: false,
        action_label: "Delete",
    };
    assert_eq!(report.format_summary(), "Deleted 1 file");
}

#[test]
fn format_summary_move_partial_error() {
    let report = BatchReport {
        errors: vec!["foo: permission denied".into()],
        success_count: 2,
        canceled: false,
        action_label: "Move",
    };
    assert_eq!(report.format_summary(), "Moved 2 file(s), 1 error(s)");
}

#[test]
fn format_summary_all_errors() {
    let report = BatchReport {
        errors: vec!["a: not found".into(), "b: not found".into()],
        success_count: 0,
        canceled: false,
        action_label: "Delete",
    };
    assert_eq!(report.format_summary(), "Deleted failed: 2 error(s)");
}

#[test]
fn format_summary_single_error() {
    let report = BatchReport {
        errors: vec!["file.txt: not found".into()],
        success_count: 0,
        canceled: false,
        action_label: "Copy",
    };
    assert_eq!(
        report.format_summary(),
        "Copied failed: file.txt: not found"
    );
}

#[test]
fn format_summary_canceled_with_progress() {
    let report = BatchReport {
        errors: vec![],
        success_count: 5,
        canceled: true,
        action_label: "Copy",
    };
    assert_eq!(report.format_summary(), "Copied canceled after 5 file(s)");
}

#[test]
fn format_summary_canceled_no_progress() {
    let report = BatchReport {
        errors: vec![],
        success_count: 0,
        canceled: true,
        action_label: "Move",
    };
    assert_eq!(report.format_summary(), "Moved canceled");
}

#[test]
fn format_summary_unknown_label_passes_through() {
    let report = BatchReport {
        errors: vec![],
        success_count: 2,
        canceled: false,
        action_label: "Foobar",
    };
    assert_eq!(report.format_summary(), "Foobar 2 files");
}

#[test]
fn format_summary_unknown_default_label() {
    let report = BatchReport {
        errors: vec!["e: x".into()],
        success_count: 0,
        canceled: false,
        action_label: "Unknown",
    };
    assert_eq!(report.format_summary(), "Unknown failed: e: x");
}

#[test]
fn dedup_paths_keeps_symlink_and_target_separate() {
    let dir = tempfile::tempdir().unwrap();
    let real = make_file(dir.path(), "real.txt", b"content");
    let link = dir.path().join("link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&real, &link).unwrap();

    let paths = vec![real.clone(), link.clone()];
    let result = dedup_paths(&paths);

    assert_eq!(result.len(), 2);
    assert!(result.contains(&real));
    assert!(result.contains(&link));
}

#[test]
fn dedup_paths_preserves_originals() {
    let dir = tempfile::tempdir().unwrap();
    let a = make_file(dir.path(), "a.txt", b"a");
    let b = make_file(dir.path(), "b.txt", b"b");

    let result = dedup_paths(&[a.clone(), b.clone()]);

    assert_eq!(result.len(), 2);
    assert!(result.contains(&a));
    assert!(result.contains(&b));
}

#[test]
fn dedup_paths_canonical_failure_keeps_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let real = make_file(dir.path(), "exists.txt", b"x");
    let missing = dir.path().join("no_such_file_xyz.txt");

    let result = dedup_paths(&[real.clone(), missing.clone()]);

    assert_eq!(result.len(), 2);
    assert!(result.contains(&real));
    assert!(result.contains(&missing));
}

#[cfg(unix)]
#[test]
fn batch_copy_symlink_preserves_link_not_target() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let target = make_file(src_dir.path(), "target.txt", b"data");
    let link = src_dir.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let action = PendingAction::Copy(TransferAction {
        sources: vec![link],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);

    assert_eq!(report.success_count, 1);
    assert!(report.errors.is_empty());
    let dest_link = dest_dir.path().join("link.txt");
    assert!(dest_link.is_symlink());
    assert_eq!(fs::read_link(&dest_link).unwrap(), target);
}

#[cfg(unix)]
#[test]
fn batch_move_symlink_preserves_link_not_target() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let target = make_file(src_dir.path(), "target.txt", b"data");
    let link = src_dir.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let action = PendingAction::Move(TransferAction {
        sources: vec![link.clone()],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);

    assert_eq!(report.success_count, 1);
    assert!(report.errors.is_empty());
    assert!(!link.exists());
    let dest_link = dest_dir.path().join("link.txt");
    assert!(dest_link.is_symlink());
    assert_eq!(fs::read_link(&dest_link).unwrap(), target);
}

#[test]
fn batch_copy_unicode_filenames() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let f1 = make_file(src_dir.path(), "日本語ファイル.txt", b"nihongo");
    let f2 = make_file(src_dir.path(), "\u{1f680}launch.txt", b"emoji");
    let f3 = make_file(src_dir.path(), "caf\u{e9}.txt", b"accent");

    let action = PendingAction::Copy(TransferAction {
        sources: vec![f1, f2, f3],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });
    let report = execute_batch(action);

    assert_eq!(report.success_count, 3);
    assert!(report.errors.is_empty());
    assert!(dest_dir.path().join("日本語ファイル.txt").exists());
    assert!(dest_dir.path().join("🚀launch.txt").exists());
    assert!(dest_dir.path().join("café.txt").exists());
}

#[cfg(unix)]
#[test]
fn batch_delete_symlink_preserves_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = make_file(dir.path(), "target.txt", b"keep me");
    let link = dir.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let action = PendingAction::Delete {
        paths: vec![link.clone()],
    };
    let report = execute_batch(action);

    assert_eq!(report.success_count, 1);
    assert!(report.errors.is_empty());
    assert!(!link.exists());
    assert!(target.exists());
    assert_eq!(fs::read(&target).unwrap(), b"keep me");
}

#[test]
fn format_summary_canceled_with_errors() {
    let report = BatchReport {
        errors: vec!["x: permission denied".into()],
        success_count: 2,
        canceled: true,
        action_label: "Copy",
    };
    // Canceled takes priority — errors not included in summary.
    assert_eq!(report.format_summary(), "Copied canceled after 2 file(s)");
}

#[test]
fn format_summary_canceled_all_errors() {
    let report = BatchReport {
        errors: vec!["a: fail".into(), "b: fail".into()],
        success_count: 0,
        canceled: true,
        action_label: "Delete",
    };
    // Canceled with zero progress — errors not included.
    assert_eq!(report.format_summary(), "Deleted canceled");
}

#[test]
fn batch_move_cancel_reports_canceled() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();

    make_file(src_dir.path(), "a.txt", b"content");
    let cancel = Arc::new(AtomicBool::new(false));

    let mut progress = |_p: BatchProgress| {
        cancel.store(true, Ordering::Relaxed);
    };
    let sources = vec![src_dir.path().join("a.txt")];
    let _sizes = helpers::path_sizes(&sources);
    let action_label = "Move";

    let report = execute_batch_generic(
        &sources,
        dest_dir.path(),
        |src, dest, on_progress| move_entry(src, dest, &cancel, on_progress, false),
        &mut progress,
        &Some(Arc::clone(&cancel)),
        action_label,
    );

    assert!(report.canceled);
}

#[test]
fn batch_copy_error_has_expected_kind() {
    let src_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();

    let nonexistent = src_dir.path().join("no_such_file.txt");

    let action = PendingAction::Copy(TransferAction {
        sources: vec![nonexistent],
        dest: dest_dir.path().to_path_buf(),
        overwrite: false,
    });

    let report = execute_batch(action);
    assert_eq!(report.success_count, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(
        report.errors[0].contains("entity not found")
            || report.errors[0].contains("No such file")
            || report.errors[0].contains("Not Found")
            // Windows wording for ERROR_FILE_NOT_FOUND (os error 2).
            || report.errors[0].contains("cannot find the file"),
        "expected not-found error, got: {}",
        report.errors[0]
    );
}
