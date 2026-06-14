// `unwrap_used`/`expect_used` are allowed for this whole module because it is a
// `#[cfg(test)]` `mod tests` (AGENTS.md permits the allow only on test modules).
// The unwraps are all on `TempDir`/filesystem setup or on building a known-valid
// `FileEntry`, where a failure means the test environment is broken and
// panicking with a backtrace is the right outcome.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::app::types::{
    ArchiveCreateDetails, ArchiveExtractDetails, ConfirmDetails, DialogKind, FileKind, InputAction,
    InputState, InteractionState, OverwriteConfirmDetails, PendingAction, PropertiesDetails,
    TextInput, TransferAction, UiState,
};

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

fn archive_input_col(width: u16, cursor: u16) -> u16 {
    let dialog_width = (width * 50 / 100).max(30).min(width);
    (width.saturating_sub(dialog_width)) / 2 + 2 + cursor
}

fn archive_input_row(height: u16, offset: u16) -> u16 {
    let dialog_height = (height * 40 / 100).max(5).min(height);
    (height.saturating_sub(dialog_height)) / 2 + offset
}

fn text_input(text: &str, cursor: usize) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(text.to_string());
    input.set_cursor(cursor);
    input
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

/// A regular-file `FileEntry` at `/{name}`. Built via the production
/// `FileEntry::builder()` rather than a hand-rolled struct literal so it tracks
/// the real field set.
///
/// (The richer `TestEntry` builder lives behind `#[cfg(test)]` in the *library*
/// crate and is not reachable from these binary-side unit tests, so the plain
/// `FileEntry::builder()` is used here.)
fn mk_entry(name: &str) -> crate::app::types::FileEntry {
    crate::app::types::FileEntry::builder()
        .name(name)
        .path(std::path::PathBuf::from(format!("/{name}")))
        .is_dir(false)
        .size(0)
        .build()
        .expect("valid test file entry")
}

#[test]
fn mouse_input_dialog_outside_preserves_text() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Name:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        input: InputState {
            dialog_input: {
                let mut ti = TextInput::new();
                ti.set_text("draft".to_string());
                ti.set_cursor(5);
                ti
            },
            ..Default::default()
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
    assert_eq!(state.input.dialog_input.text(), "draft");
    assert_eq!(state.input.dialog_input.cursor(), 5);
}

#[test]
fn mouse_input_dialog_inside_consumes_click() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Name:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        input: InputState {
            dialog_input: {
                let mut ti = TextInput::new();
                ti.set_text("draft".to_string());
                ti
            },
            ..Default::default()
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
    assert_eq!(state.input.dialog_input.text(), "draft");
}

#[test]
fn mouse_archive_extract_input_click_positions_cursor() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::ArchiveExtract(Box::new(
            ArchiveExtractDetails {
                source: std::path::PathBuf::from("archive.zip"),
                entries: Vec::new(),
                dest_input: text_input("target-dir", 10),
            },
        ))),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(
        &mut state,
        &mut running_job,
        &mp(
            archive_input_col(100, 3),
            archive_input_row(40, ARCHIVE_EXTRACT_INPUT_ROW_OFFSET),
            100,
            40,
        ),
    );

    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::ArchiveExtract(..))
    ));
    if let AppMode::Dialog(DialogKind::ArchiveExtract(details)) = state.mode {
        assert_eq!(details.dest_input.text(), "target-dir");
        assert_eq!(details.dest_input.cursor(), 3);
    }
}

#[test]
fn mouse_archive_create_input_click_positions_cursor() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::ArchiveCreate(Box::new(ArchiveCreateDetails {
            sources: vec![std::path::PathBuf::from("file.txt")],
            dest_input: text_input("archive.zip", 11),
        }))),
        ..Default::default()
    };
    let mut running_job = None;
    let outcomes = handle_mouse_dialog(
        &mut state,
        &mut running_job,
        &mp(
            archive_input_col(100, 7),
            archive_input_row(40, ARCHIVE_CREATE_INPUT_ROW_OFFSET),
            100,
            40,
        ),
    );

    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::ArchiveCreate(..))
    ));
    if let AppMode::Dialog(DialogKind::ArchiveCreate(details)) = state.mode {
        assert_eq!(details.dest_input.text(), "archive.zip");
        assert_eq!(details.dest_input.cursor(), 7);
    }
}

#[test]
fn mouse_function_bar_zero_width_does_not_panic() {
    let mut state = AppState::default();
    let outcomes = handle_mouse_function_bar(&mut state, &mp(0, 0, 0, 1));
    assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
}

/// One parameterized passive-dialog case: a label, the initial state, the click
/// position (chosen to miss any action button), and a matcher asserting the
/// dialog mode survived the click.
type PassiveDialogCase = (&'static str, AppState, MousePosition, fn(&AppMode) -> bool);

/// A non-actionable click on each of these dialogs must be `Consumed` and must
/// leave the dialog mode unchanged. Parameterized over the six passive dialog
/// kinds (the former six near-identical `*_does_not_dismiss` / `_handled` /
/// `_is_consumed` tests) so the shared assertion lives in one place.
fn passive_dialog_cases() -> Vec<PassiveDialogCase> {
    vec![
        (
            "error",
            AppState {
                mode: AppMode::Dialog(DialogKind::Error("error".to_string())),
                ..Default::default()
            },
            mp(1, 1, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::Error(_))),
        ),
        (
            "properties",
            AppState {
                mode: AppMode::Dialog(DialogKind::Properties(Box::new(PropertiesDetails {
                    name: "file.txt".to_string(),
                    size: 0,
                    mtime: std::time::SystemTime::UNIX_EPOCH,
                    permissions: 0o644,
                    owner: String::new(),
                    group: String::new(),
                    kind: FileKind::from_metadata_flags(false, false),
                }))),
                ..Default::default()
            },
            mp(1, 1, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::Properties(..))),
        ),
        (
            "help",
            AppState {
                mode: AppMode::Dialog(DialogKind::Help {
                    message: "help".to_string(),
                    scroll_offset: 0,
                }),
                ..Default::default()
            },
            mp(1, 1, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::Help { .. })),
        ),
        (
            "confirm",
            AppState {
                mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                    "Confirm", "Run?",
                ))),
                input: InputState {
                    dialog_selection: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            // Outside the button row: click must not trigger the action.
            mp(79, 23, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::Confirm(_))),
        ),
        (
            "overwrite",
            AppState {
                mode: AppMode::Dialog(DialogKind::OverwriteConfirm(Box::new(
                    OverwriteConfirmDetails {
                        conflicting: vec![],
                    },
                ))),
                input: InputState {
                    dialog_selection: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            mp(1, 1, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::OverwriteConfirm(..))),
        ),
        (
            "progress",
            AppState {
                mode: AppMode::Dialog(DialogKind::Progress {
                    message: "Copying".to_string(),
                    progress_fraction: 0.5,
                    cancellable: true,
                }),
                ..Default::default()
            },
            mp(40, 21, 80, 24),
            |m| matches!(m, AppMode::Dialog(DialogKind::Progress { .. })),
        ),
    ]
}

#[test]
fn passive_dialog_click_is_consumed_and_preserves_mode() {
    for (label, mut state, pos, mode_ok) in passive_dialog_cases() {
        let mut running_job = None;
        let outcome = handle_mouse_dialog(&mut state, &mut running_job, &pos);
        assert!(
            matches!(outcome, Some(MouseOutcome::Consumed)),
            "{label}: expected Consumed, got {outcome:?}"
        );
        assert!(
            mode_ok(&state.mode),
            "{label}: dialog mode was not preserved"
        );
    }
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
        interaction: InteractionState {
            drag_anchor_index: Some(5),
            ..Default::default()
        },
        ..Default::default()
    };

    handle_mouse_up(&mut state);

    assert!(state.interaction.drag_anchor_index.is_none());
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
    left_panel.set_entries(entries.clone());
    let mut right_panel = crate::app::types::PanelState::new(std::path::PathBuf::from("/"));
    right_panel.set_entries(entries);
    let mut state = AppState {
        left_panel,
        right_panel,
        interaction: InteractionState {
            drag_anchor_index: Some(0),
            ..Default::default()
        },
        ..Default::default()
    };

    // anchor = index 0; click at row 5 maps to filtered index 3
    // (list starts at row 2, so relative row 3). The drag selects the inclusive
    // range 0..=3, i.e. entries a, b, c, d — and must leave e untouched.
    handle_mouse_drag(&mut state, &mp(1, 5, 80, 24));

    let selected: Vec<&str> = state
        .left_panel
        .selected_entries()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(selected, ["a", "b", "c", "d"], "wrong entries selected");
    assert_eq!(state.left_panel.cursor, 3, "cursor should land on drag end");
    // The right panel shares identical entries but is not the active panel, so
    // the drag must not touch its selection.
    assert_eq!(state.right_panel.selected_entries().count(), 0);
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
        input: InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![dirs.src.join("clash.txt")],
                dest: dirs.dest,
                overwrite: false,
            })),
            ..Default::default()
        },
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
        AppMode::Dialog(DialogKind::OverwriteConfirm(..))
    ));
    assert!(state.ui.pending_action.is_some());
}

#[test]
fn mouse_confirm_click_without_conflict_starts_action() {
    let dirs = setup_copy_dirs();
    std::fs::write(dirs.src.join("unique.txt"), b"data").unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        ))),
        input: InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![dirs.src.join("unique.txt")],
                dest: dirs.dest,
                overwrite: false,
            })),
            ..Default::default()
        },
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
        input: InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: UiState {
            status_message: Some("Queued".to_string()),
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![dirs.src.join("unique.txt")],
                dest: dirs.dest,
                overwrite: false,
            })),
            ..Default::default()
        },
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

    assert_eq!(state.ui.status_message.as_deref(), Some("Queued"));
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
        input: InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![first_src_dir.join("first.txt")],
                dest: dest_dir.clone(),
                overwrite: false,
            })),
            ..Default::default()
        },
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
    state.ui.status_message = Some("Queued".to_string());
    state.ui.pending_action = Some(PendingAction::Copy(TransferAction {
        sources: vec![second_src_dir.join("second.txt")],
        dest: dest_dir,
        overwrite: false,
    }));

    let outcome = handle_confirm_click(
        &mut state,
        &mut running_job,
        &mp(30, btn_row, width, height),
    );
    assert!(matches!(outcome, Some(MouseOutcome::Consumed)));

    assert_eq!(
        state.ui.status_message.as_deref(),
        Some("Another job is already running")
    );
}
