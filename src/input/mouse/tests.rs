#![allow(clippy::unwrap_used)]

use super::*;
use crate::app::types::{ConfirmDetails, InputAction, PendingAction, TextInput};

fn mp(col: u16, row: u16, width: u16, height: u16) -> MousePosition {
    MousePosition {
        col,
        row,
        width,
        height,
    }
}

fn confirm_btn_row(height: u16) -> u16 {
    let dialog_height = height * 40 / 100;
    let dialog_y = (height.saturating_sub(dialog_height)) / 2;
    dialog_y + dialog_height.saturating_sub(2)
}

struct TestDirs {
    _tmp: tempfile::TempDir,
    src: std::path::PathBuf,
    dest: std::path::PathBuf,
}

fn setup_copy_dirs() -> TestDirs {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    TestDirs {
        _tmp: tmp,
        src,
        dest,
    }
}

fn mk_entry(name: &str) -> crate::app::types::FileEntry {
    use std::sync::Arc;
    crate::app::types::FileEntry {
        name: name.to_string(),
        path: std::path::PathBuf::from(format!("/{}", name)),
        cha: crate::fs::cha::Cha {
            kind: crate::fs::cha::ChaKind::empty(),
            mode: crate::fs::cha::ChaMode::new(0o100644),
            len: 0,
            mtime: None,
            btime: None,
            ctime: None,
            atime: None,
            uid: 0,
            gid: 0,
            dev: 0,
            nlink: 0,
        },
        owner: Arc::from(""),
        group: Arc::from(""),
        selected: false,
        mime_type: None,
        time_str: String::new(),
        size_str: String::new(),
        name_width: unicode_width::UnicodeWidthStr::width(name),
        size_width: 0,
        time_width: 0,
        category: crate::app::types::FileCategory::Other,
        sanitized_name: None,
    }
}

#[test]
fn mouse_input_dialog_outside_preserves_text() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Name:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        dialog_input: TextInput {
            text: "draft".to_string(),
            cursor: 5,
        },
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(0, 0, 100, 40));
    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Input { .. })
    ));
    assert_eq!(state.dialog_input.text, "draft");
    assert_eq!(state.dialog_input.cursor, 5);
}

#[test]
fn mouse_input_dialog_inside_consumes_click() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Name:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        dialog_input: TextInput {
            text: "draft".to_string(),
            cursor: 0,
        },
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(50, 20, 100, 40));
    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Input { .. })
    ));
    assert_eq!(state.dialog_input.text, "draft");
}

#[test]
fn mouse_function_bar_zero_width_does_not_panic() {
    let mut state = AppState::default();
    let outcomes = handle_mouse_function_bar(&mut state, &mp(0, 0, 0, 1));
    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
}

#[test]
fn mouse_error_dialog_click_does_not_dismiss() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Error("error".to_string())),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(1, 1, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(state.mode, AppMode::Dialog(DialogKind::Error(_))));
}

#[test]
fn mouse_properties_dialog_click_does_not_dismiss() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Properties {
            name: "file.txt".to_string(),
            size: 0,
            mtime: std::time::SystemTime::UNIX_EPOCH,
            permissions: 0o644,
            owner: String::new(),
            group: String::new(),
            is_dir: false,
            is_symlink: false,
        }),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(1, 1, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Properties { .. })
    ));
}

#[test]
fn mouse_help_dialog_click_does_not_dismiss() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Help {
            message: "help".to_string(),
            scroll_offset: 0,
        }),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(1, 1, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Help { .. })
    ));
}

#[test]
fn mouse_confirm_dialog_keeps_existing_behavior() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Confirm", "Run?",
        ))),
        dialog_selection: 1,
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(79, 23, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
}

#[test]
fn mouse_overwrite_confirm_dialog_handled() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::OverwriteConfirm {
            conflicting: vec![],
        }),
        dialog_selection: 0,
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(1, 1, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::OverwriteConfirm { .. })
    ));
}

#[test]
fn mouse_progress_click_is_consumed() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Progress {
            message: "Copying".to_string(),
            progress_fraction: 0.5,
            cancellable: true,
        }),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(&mut state, &mut running_job, &mp(40, 21, 80, 24));
    assert!(outcomes.is_some());
    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
}

#[test]
fn mouse_scroll_handles_help_dialog() {
    let long_text = (0..200)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Help {
            message: long_text,
            scroll_offset: 0,
        }),
        ..Default::default()
    };

    handle_mouse_scroll(
        &mut state,
        &mut None,
        MouseEventKind::ScrollDown,
        &mp(0, 0, 80, 40),
    );

    assert!(
        matches!(&state.mode, AppMode::Dialog(DialogKind::Help { scroll_offset, .. }) if *scroll_offset > 0),
        "expected Help dialog with scroll_offset > 0"
    );
}

#[test]
fn mouse_up_clears_drag_anchor() {
    let mut state = AppState {
        drag_anchor_index: Some(5),
        ..Default::default()
    };

    handle_mouse_up(&mut state);

    assert!(state.drag_anchor_index.is_none());
}

#[test]
fn drag_select_range() {
    let entries = vec![
        mk_entry("a"),
        mk_entry("b"),
        mk_entry("c"),
        mk_entry("d"),
        mk_entry("e"),
    ];
    let mut left_panel = crate::app::types::PanelState::new(std::path::PathBuf::from("/"));
    left_panel.listing.entries = entries.clone();
    let mut right_panel = crate::app::types::PanelState::new(std::path::PathBuf::from("/"));
    right_panel.listing.entries = entries;
    let mut state = AppState {
        left_panel,
        right_panel,
        drag_anchor_index: Some(0),
        ..Default::default()
    };

    handle_mouse_drag(&mut state, &mp(1, 5, 80, 24));

    let selected: Vec<_> = state
        .left_panel
        .listing
        .entries
        .iter()
        .filter(|e| e.selected)
        .collect();
    assert_eq!(selected.len(), 4);
}

#[test]
fn handle_right_click_in_dialog_emits_esc() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple("Title", "Body"))),
        ..Default::default()
    };

    let outcome = handle_right_down(&mut state, &mp(40, 10, 80, 24));
    assert!(matches!(
        outcome,
        Some(MouseOutcome::NormalKey(KeyCode::Esc))
    ));
}

#[test]
fn handle_right_click_in_menu_emits_esc() {
    let mut state = AppState {
        mode: AppMode::Menu,
        ..Default::default()
    };

    let outcome = handle_right_down(&mut state, &mp(40, 10, 80, 24));
    assert!(matches!(
        outcome,
        Some(MouseOutcome::NormalKey(KeyCode::Esc))
    ));
}

#[test]
fn mouse_menu_dropdown_outside_restores_previous_mode() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: Some(AppMode::Search),
        ..Default::default()
    };

    let outcome = handle_mouse_menu_dropdown(&mut state, &mp(79, 23, 80, 24));

    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));
    assert!(matches!(state.mode, AppMode::Search));
    assert!(state.prev_mode.is_none());
}

#[test]
fn handle_right_click_in_panel_emits_esc() {
    let mut state = AppState::default();

    let outcome = handle_right_down(&mut state, &mp(10, 10, 80, 24));
    assert!(matches!(
        outcome,
        Some(MouseOutcome::NormalKey(KeyCode::Esc))
    ));
}

#[test]
fn handle_middle_click_in_panel_emits_f5() {
    let mut state = AppState::default();

    let outcome = handle_middle_down(&mut state, &mp(10, 10, 80, 24));
    assert!(matches!(
        outcome,
        Some(MouseOutcome::NormalKey(KeyCode::F(5)))
    ));
}

#[test]
fn handle_middle_click_in_dialog_consumed() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Error("err".to_string())),
        ..Default::default()
    };

    let outcome = handle_middle_down(&mut state, &mp(40, 10, 80, 24));
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));
}

#[test]
fn mouse_confirm_click_with_overwrite_conflict_shows_overwrite_dialog() {
    let dirs = setup_copy_dirs();
    std::fs::write(dirs.src.join("clash.txt"), b"src").unwrap();
    std::fs::write(dirs.dest.join("clash.txt"), b"dest").unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        ))),
        dialog_selection: 0,
        pending_action: Some(PendingAction::Copy {
            sources: vec![dirs.src.join("clash.txt")],
            dest: dirs.dest,
            overwrite: false,
        }),
        ..Default::default()
    };
    let mut running_job = None;

    let height: u16 = 24;
    let width: u16 = 80;
    let btn_row = confirm_btn_row(height);

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::OverwriteConfirm { .. })
    ));
    assert!(state.pending_action.is_some());
}

#[test]
fn mouse_confirm_click_without_conflict_starts_action() {
    let dirs = setup_copy_dirs();
    std::fs::write(dirs.src.join("unique.txt"), b"data").unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        ))),
        dialog_selection: 0,
        pending_action: Some(PendingAction::Copy {
            sources: vec![dirs.src.join("unique.txt")],
            dest: dirs.dest,
            overwrite: false,
        }),
        ..Default::default()
    };
    let mut running_job = None;

    let height: u16 = 24;
    let width: u16 = 80;
    let btn_row = confirm_btn_row(height);

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Progress { .. })
    ));
}

#[test]
fn mouse_confirm_click_preserves_status_message() {
    let dirs = setup_copy_dirs();
    std::fs::write(dirs.src.join("unique.txt"), b"data").unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        ))),
        dialog_selection: 0,
        status_message: Some("Queued".to_string()),
        pending_action: Some(PendingAction::Copy {
            sources: vec![dirs.src.join("unique.txt")],
            dest: dirs.dest,
            overwrite: false,
        }),
        ..Default::default()
    };
    let mut running_job = None;

    let height: u16 = 24;
    let width: u16 = 80;
    let btn_row = confirm_btn_row(height);

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    assert_eq!(state.status_message.as_deref(), Some("Queued"));
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Progress { .. })
    ));
}

#[test]
fn mouse_confirm_click_keeps_new_status_message() {
    let tmp = tempfile::tempdir().unwrap();
    let first_src_dir = tmp.path().join("first-src");
    let second_src_dir = tmp.path().join("second-src");
    let dest_dir = tmp.path().join("dest");
    std::fs::create_dir_all(&first_src_dir).unwrap();
    std::fs::create_dir_all(&second_src_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();
    std::fs::write(first_src_dir.join("first.txt"), b"data").unwrap();
    std::fs::write(second_src_dir.join("second.txt"), b"data").unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        ))),
        dialog_selection: 0,
        pending_action: Some(PendingAction::Copy {
            sources: vec![first_src_dir.join("first.txt")],
            dest: dest_dir.clone(),
            overwrite: false,
        }),
        ..Default::default()
    };
    let mut running_job = None;

    let height: u16 = 24;
    let width: u16 = 80;
    let btn_row = confirm_btn_row(height);

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    state.mode = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
        "Copy", "Proceed?",
    )));
    state.status_message = Some("Queued".to_string());
    state.pending_action = Some(PendingAction::Copy {
        sources: vec![second_src_dir.join("second.txt")],
        dest: dest_dir,
        overwrite: false,
    });

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    assert_eq!(
        state.status_message.as_deref(),
        Some("Another job is already running")
    );
}
