use crate::input::dialogs;
use crate::*;
use app::types::{ActivePanel, PendingAction};
use std::path::PathBuf;

#[test]
fn check_overwrite_no_conflicts_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("new.txt"), b"hello").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
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
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("clash.txt"), b"src").unwrap();
    std::fs::write(dest.join("clash.txt"), b"dest").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
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
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    std::fs::write(src.join("b.txt"), b"b").unwrap();
    std::fs::write(dest.join("a.txt"), b"a").unwrap();
    std::fs::write(dest.join("b.txt"), b"b").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("a.txt"), src.join("b.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.contains(&"a.txt".to_string()));
    assert!(conflicts.contains(&"b.txt".to_string()));
}

#[test]
fn check_overwrite_source_equals_dest_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("same.txt");
    std::fs::write(&file, b"data").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
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
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("link.txt"), b"src").unwrap();

    std::os::unix::fs::symlink("/nonexistent/broken", dest.join("link.txt")).unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
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
fn check_overwrite_conflict_move_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("file.txt"), "a").unwrap();
    std::fs::write(dest.join("file.txt"), "b").unwrap();
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
    assert_eq!(conflicts, Some(vec![String::from("file.txt")]));
}

#[test]
fn check_overwrite_conflict_move_same_file_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let file = src.join("file.txt");
    std::fs::write(&file, "a").unwrap();
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
fn check_overwrite_conflict_move_overwrite_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("file.txt"), "a").unwrap();
    std::fs::write(dest.join("file.txt"), "b").unwrap();
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
fn check_overwrite_conflict_delete_no_conflict() {
    let state = AppState {
        pending_action: Some(PendingAction::Delete {
            paths: vec![PathBuf::from("/tmp/nonexistent")],
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}
