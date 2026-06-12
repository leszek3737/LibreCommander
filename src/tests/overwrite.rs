use crate::input::dialogs;
use lc::app::types::{ActivePanel, AppState, PendingAction};
use lc::ops::archive::ArchiveFormat;
use std::path::{Path, PathBuf};

fn setup_src_dest(src: &Path, dest: &Path, files: &[&str]) {
    std::fs::create_dir_all(src).unwrap();
    std::fs::create_dir_all(dest).unwrap();
    for name in files {
        std::fs::write(src.join(name), b"x").unwrap();
    }
}

fn setup_dest_files(dest: &Path, files: &[&str]) {
    std::fs::create_dir_all(dest).unwrap();
    for name in files {
        std::fs::write(dest.join(name), b"x").unwrap();
    }
}

#[test]
fn check_overwrite_no_conflicts_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["new.txt"]);

    let state = AppState {
        pending_action: Some(PendingAction::Copy {
            sources: vec![src.join("new.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_one_conflict_returns_some() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["clash.txt"]);
    setup_dest_files(&dest, &["clash.txt"]);

    let state = AppState {
        pending_action: Some(PendingAction::Copy {
            sources: vec![src.join("clash.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["clash.txt"]);
}

#[test]
fn check_overwrite_all_conflicts_returns_all_names() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["a.txt", "b.txt"]);
    setup_dest_files(&dest, &["a.txt", "b.txt"]);

    let state = AppState {
        pending_action: Some(PendingAction::Copy {
            sources: vec![src.join("a.txt"), src.join("b.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.iter().any(|s| s == "a.txt"));
    assert!(conflicts.iter().any(|s| s == "b.txt"));
}

#[test]
fn check_overwrite_source_equals_dest_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("same.txt");
    std::fs::write(&file, b"data").unwrap();

    let state = AppState {
        pending_action: Some(PendingAction::Copy {
            sources: vec![file],
            dest: tmp.path().to_path_buf(),
            overwrite: false,
        }),
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[cfg(unix)]
#[test]
fn check_overwrite_broken_symlink_at_dest_is_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["link.txt"]);

    std::os::unix::fs::symlink("/nonexistent/broken", dest.join("link.txt")).unwrap();

    let state = AppState {
        pending_action: Some(PendingAction::Copy {
            sources: vec![src.join("link.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["link.txt"]);
}

#[test]
fn check_overwrite_move_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["file.txt"]);
    setup_dest_files(&dest, &["file.txt"]);
    let state = AppState {
        active_panel: ActivePanel::Left,
        pending_action: Some(PendingAction::Move {
            sources: vec![src.join("file.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert_eq!(conflicts, Some(vec!["file.txt".into()]));
}

#[test]
fn check_overwrite_move_same_file_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    setup_src_dest(&src, &src, &["file.txt"]);
    let file = src.join("file.txt");
    let state = AppState {
        active_panel: ActivePanel::Left,
        pending_action: Some(PendingAction::Move {
            sources: vec![file],
            dest: src,
            overwrite: false,
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

#[test]
fn check_overwrite_move_overwrite_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["file.txt"]);
    setup_dest_files(&dest, &["file.txt"]);
    let state = AppState {
        active_panel: ActivePanel::Left,
        pending_action: Some(PendingAction::Move {
            sources: vec![src.join("file.txt")],
            dest,
            overwrite: true,
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

#[test]
fn check_overwrite_delete_no_conflict() {
    let state = AppState {
        pending_action: Some(PendingAction::Delete {
            paths: vec![PathBuf::from("/tmp/nonexistent")],
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

// TODO: ExtractArchive does not check for existing files in dest — add test for
// conflict when dest already contains a file that would be overwritten by extraction.
#[test]
fn check_overwrite_extract_archive_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("archive.tar.gz");
    let dest = tmp.path().join("dest");
    std::fs::write(&src, b"fake").unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let state = AppState {
        pending_action: Some(PendingAction::ExtractArchive { source: src, dest }),
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

// TODO: CreateArchive does not check for existing output file — add test for
// conflict when dest archive path already exists on disk.
#[test]
fn check_overwrite_create_archive_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("out.tar.gz");
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, b"data").unwrap();
    let state = AppState {
        pending_action: Some(PendingAction::CreateArchive {
            sources: vec![file],
            dest,
            format: ArchiveFormat::TarGz,
        }),
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

// TODO: Missing test coverage:
// - Directories as sources (src is a dir, not a file)
// - Symlinks as sources (src is a symlink to a file)
// - Empty sources list (vec![]) — verify no panic/correct none result
