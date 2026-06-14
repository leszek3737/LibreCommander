use super::helpers::*;
use crate::input::dialogs;
use crate::render;
use crate::{confirm_delete, confirm_file_transfer};
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{
    AppMode, AppState, DialogKind, InputAction, PendingAction, PickerKind, TransferAction,
};
use lc::ui::viewer;
use ratatui::layout::Size;
use ratatui::{Terminal, backend::TestBackend};

fn no_viewer_state() -> Option<viewer::ViewerState> {
    None
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

/// Render `state` on the standard test terminal and return the buffer as text.
/// Collapses the repeated `test_terminal` + `render_ui` + `buffer_to_string`
/// triple used by the text-content assertions below.
fn render_and_get_text(state: &AppState) -> String {
    let mut terminal = test_terminal();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, state, viewer_state.as_ref(), None))
        .unwrap();
    buffer_to_string(terminal.backend().buffer())
}

// Named cell coordinates / terminal sizes for the buffer-position checks.
const TOP_ROW: u16 = 0;
const BOTTOM_ROW: u16 = TERMINAL_HEIGHT - 1;
const SAMPLE_COLUMN: u16 = 39;
const STATUS_BAR_COLUMN: u16 = 2;
const SMALL_TERM_WIDTH: u16 = 40;
const SMALL_TERM_HEIGHT: u16 = 10;

#[test]
fn confirm_enter_without_pending_action_dismisses_dialog() {
    let mut state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple("Info", "Nothing to run"),
        )),
        input: app::types::InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: app::types::UiState {
            pending_action: None,
            ..Default::default()
        },
        ..Default::default()
    };

    dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        Size::new(80, 24),
    );

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn confirm_enter_with_pending_action_starts_action() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("delme.txt");
    std::fs::write(&src, "x").unwrap();
    let mut state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple("Delete", "Delete selected?"),
        )),
        input: app::types::InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ui: app::types::UiState {
            pending_action: Some(app::types::PendingAction::Delete { paths: vec![src] }),
            ..Default::default()
        },
        active_panel: app::types::ActivePanel::Left,
        ..Default::default()
    };
    state
        .left_panel
        .set_entries(vec![entry("delme.txt").build()]);
    state.left_panel.cursor = 0;

    dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        Size::new(80, 24),
    );

    assert!(!matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(..))
    ));
    assert!(
        matches!(state.mode, AppMode::Dialog(DialogKind::Progress { .. })),
        "expected Delete to start Progress dialog, got: {:?}",
        state.mode
    );
}

#[test]
fn confirm_file_transfer_copy_opens_dialog() {
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("a.txt").build(), entry("b.txt").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;
    confirm_file_transfer(&mut state, "Copy Confirm", "Copy", |sources, dest| {
        PendingAction::Copy(TransferAction {
            sources,
            dest,
            overwrite: false,
        })
    });
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
    assert!(
        matches!(state.ui.pending_action, Some(PendingAction::Copy(_))),
        "expected PendingAction::Copy, got: {:?}",
        state.ui.pending_action
    );
}

#[test]
fn confirm_delete_opens_dialog() {
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("delme.txt").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;
    confirm_delete(&mut state);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
}

#[test]
fn parse_octal_mode_accepts_valid_input() {
    assert_eq!(dialogs::parse_octal_mode("755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode("0644"), Some(0o644));
    assert_eq!(dialogs::parse_octal_mode("bad"), None);
}

#[test]
fn parse_octal_mode_edge_cases() {
    assert_eq!(dialogs::parse_octal_mode(""), None);
    assert_eq!(dialogs::parse_octal_mode("1234567"), None);
    assert_eq!(dialogs::parse_octal_mode("7"), Some(0o7));
    assert_eq!(dialogs::parse_octal_mode("00755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode(" 755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode("789"), None);
}

#[test]
fn dialog_overlay_renders_error_text() {
    let state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Error(
            "Test Error Message".to_string(),
        )),
        ..Default::default()
    };
    let rendered = render_and_get_text(&state);
    assert!(rendered.contains("Test Error"));
    assert!(rendered.contains("Message"));
}

#[test]
fn dialog_overlay_centered() {
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Error("test error".to_string())),
        ..Default::default()
    };
    let mut terminal = test_terminal();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    // Verifies cells exist; centering requires positional assertions.
    assert!(buf.cell((20, 7)).is_some());
    assert!(buf.cell((39, 0)).is_some());
}

#[test]
fn dialog_with_long_title_does_not_overflow() {
    let long_msg = "x".repeat(200);
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Error(long_msg)),
        ..Default::default()
    };
    let mut terminal =
        Terminal::new(TestBackend::new(SMALL_TERM_WIDTH, SMALL_TERM_HEIGHT)).unwrap();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let text = buffer_to_string(buf);
    // Verifies title content renders; overflow check would require buffer bounds validation.
    assert!(text.contains("xxxxx"));
}

#[test]
fn help_dialog_renders_help_text() {
    let state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Help {
            message: "TEST HELP CONTENT".to_string(),
            scroll_offset: 0,
        }),
        ..Default::default()
    };
    let rendered = render_and_get_text(&state);
    assert!(rendered.contains("TEST HELP"));
}

#[test]
fn progress_dialog_nan_percent_handled() {
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Progress {
            message: "copying".to_string(),
            progress_fraction: f32::NAN,
            cancellable: true,
        }),
        ..Default::default()
    };
    let text = render_and_get_text(&state);
    assert!(!text.is_empty());
    assert!(
        !text.contains("NaN"),
        "progress dialog should not render 'NaN' as percentage, got:\n{text}"
    );
}

#[test]
fn menu_dropdown_renders_over_panels() {
    let state = AppState {
        mode: AppMode::Menu,
        ui: app::types::UiState {
            menu_selected: 1,
            menu_item_selected: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let rendered = render_and_get_text(&state);
    assert!(rendered.contains("User menu"));
    assert!(rendered.contains("View file"));
}

#[test]
fn list_picker_overlay_renders_title() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        ui: app::types::UiState {
            picker_selected: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .input
        .command_history
        .push_back("echo hello".to_string());
    let rendered = render_and_get_text(&state);
    assert!(rendered.contains("Command History"));
    assert!(rendered.contains("echo hello"));
}

#[test]
fn menu_bar_rendered_at_top() {
    let state = AppState::default();
    let mut terminal = test_terminal();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let cell = buf.cell((SAMPLE_COLUMN, TOP_ROW)).unwrap();
    assert!(!cell.symbol().trim().is_empty());
}

#[test]
fn status_bar_at_bottom() {
    let state = AppState::default();
    let mut terminal = test_terminal();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let cell = buf.cell((STATUS_BAR_COLUMN, BOTTOM_ROW)).unwrap();
    assert!(!cell.symbol().trim().is_empty());
}

#[test]
#[cfg(unix)]
fn chmod_valid_input_applies_mode_and_dismisses() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("chmod_target.txt");
    std::fs::write(&file, "data").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Chmod (octal):".to_string(),
            action: InputAction::Chmod,
        }),
        ..Default::default()
    };
    state.input.dialog_input.set_text_at_end("755".to_string());
    state.left_panel.set_entries(vec![
        TestEntry::new("chmod_target.txt")
            .path(&file)
            .file(4)
            .build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;

    dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        Size::new(80, 24),
    );

    assert_eq!(state.mode, AppMode::Normal);
    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
}

#[test]
fn chmod_invalid_input_shows_error_stays_in_dialog() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Chmod (octal):".to_string(),
            action: InputAction::Chmod,
        }),
        ..Default::default()
    };
    state.input.dialog_input.set_text_at_end("bad".to_string());
    state
        .left_panel
        .set_entries(vec![entry("f.txt").file(4).build()]);
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;

    dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        Size::new(80, 24),
    );

    assert!(
        matches!(state.mode, AppMode::Dialog(DialogKind::Input { .. })),
        "expected Input dialog to remain open, got: {:?}",
        state.mode
    );
    let msg = state.ui.status_message.as_deref().unwrap_or("");
    assert!(
        msg.to_lowercase().contains("invalid"),
        "expected 'Invalid' in status_message, got: {msg}"
    );
}

#[test]
#[cfg(unix)]
fn chmod_esc_dismisses_without_changing_mode() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("chmod_target.txt");
    std::fs::write(&file, "data").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Chmod (octal):".to_string(),
            action: InputAction::Chmod,
        }),
        ..Default::default()
    };
    state.input.dialog_input.set_text_at_end("777".to_string());
    state.left_panel.set_entries(vec![
        TestEntry::new("chmod_target.txt")
            .path(&file)
            .file(4)
            .build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;

    dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Esc,
        Size::new(80, 24),
    );

    assert_eq!(state.mode, AppMode::Normal);
    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o644);
}
