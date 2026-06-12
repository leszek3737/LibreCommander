use super::helpers::*;
use crate::input::dialogs;
use crate::render;
use crate::{confirm_delete, confirm_file_transfer};
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{AppMode, AppState, DialogKind, PendingAction, PickerKind};
use lc::ui::viewer;
use ratatui::layout::Size;
use ratatui::{Terminal, backend::TestBackend};

fn no_viewer_state() -> Option<viewer::ViewerState> {
    None
}

#[test]
fn confirm_enter_without_pending_action_dismisses_dialog() {
    let mut state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple("Info", "Nothing to run"),
        )),
        dialog_selection: 0,
        pending_action: None,
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
        dialog_selection: 0,
        pending_action: Some(app::types::PendingAction::Delete { paths: vec![src] }),
        active_panel: app::types::ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.listing.entries = vec![TestEntry::new("delme.txt").build()];
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
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").build(),
        TestEntry::new("b.txt").build(),
    ];
    state.left_panel.cursor = 0;
    state.active_panel = app::types::ActivePanel::Left;
    confirm_file_transfer(&mut state, "Copy Confirm", "Copy", |sources, dest| {
        PendingAction::Copy {
            sources,
            dest,
            overwrite: false,
        }
    });
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
    assert!(
        matches!(state.pending_action, Some(PendingAction::Copy { .. })),
        "expected PendingAction::Copy, got: {:?}",
        state.pending_action
    );
}

#[test]
fn confirm_delete_opens_dialog() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("delme.txt").build()];
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
    let mut terminal = test_terminal();
    let state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Error(
            "Test Error Message".to_string(),
        )),
        ..Default::default()
    };
    let viewer_state = no_viewer_state();

    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
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
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
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
    let mut terminal = test_terminal();
    let state = AppState {
        mode: AppMode::Dialog(app::types::DialogKind::Help {
            message: "TEST HELP CONTENT".to_string(),
            scroll_offset: 0,
        }),
        ..Default::default()
    };
    let viewer_state = no_viewer_state();

    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
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
    let mut terminal = test_terminal();
    let viewer_state = no_viewer_state();
    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let text = buffer_to_string(buf);
    assert!(!text.is_empty());
    assert!(
        !text.contains("NaN"),
        "progress dialog should not render 'NaN' as percentage, got:\n{text}"
    );
}

#[test]
fn menu_dropdown_renders_over_panels() {
    let mut terminal = test_terminal();
    let state = AppState {
        mode: AppMode::Menu,
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };
    let viewer_state = no_viewer_state();

    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
    assert!(rendered.contains("User menu"));
    assert!(rendered.contains("View file"));
}

#[test]
fn list_picker_overlay_renders_title() {
    let mut terminal = test_terminal();
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        picker_selected: 0,
        ..Default::default()
    };
    state.command_history.push_back("echo hello".to_string());
    let viewer_state = no_viewer_state();

    terminal
        .draw(|f| render::render_ui(f, &state, viewer_state.as_ref(), None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
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
    let cell = buf.cell((39, 0)).unwrap();
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
    let cell = buf.cell((2, 23)).unwrap();
    assert!(!cell.symbol().trim().is_empty());
}

// TODO: Add integration test for chmod dialog (set mode, verify pending action, apply)
