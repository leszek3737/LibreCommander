use super::helpers::*;
use crate::input::dialogs;
use crate::*;
use app::types::{ActivePanel, DialogKind, InputAction, TextInput};

#[test]
fn f3_viewer_clears_stale_prev_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("view.txt");
    std::fs::write(&file, b"view").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        prev_mode: Some(AppMode::Search),
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.listing.entries = vec![TestEntry::new("view.txt").path(file).size(4).build()];
    state.left_panel.listing.unfiltered_entries = state.left_panel.listing.entries.clone();
    state.left_panel.cursor = 0;
    state.left_panel.listing.path_index = state
        .left_panel
        .listing
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut loader = None;
    let mut terminal = test_terminal();

    super::super::handle_function_keys(
        &mut state,
        &mut viewer,
        &mut loader,
        KeyCode::F(3),
        &mut terminal,
    );

    assert!(matches!(state.mode, AppMode::Viewing));
    assert!(state.prev_mode.is_none());
}

#[test]
fn viewer_search_esc_keeps_viewer_previous_mode() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Viewer search:".to_string(),
            default_text: "needle".to_string(),
            action: InputAction::ViewerSearch,
        }),
        dialog_input: TextInput {
            text: "needle".to_string(),
            cursor: 6,
        },
        prev_mode: Some(AppMode::Normal),
        ..Default::default()
    };
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;

    dialogs::handle_dialog(
        &mut state,
        &mut viewer,
        &mut job,
        KeyCode::Esc,
        ratatui::layout::Size::new(80, 24),
    );

    assert!(matches!(state.mode, AppMode::Viewing));
    assert_eq!(state.prev_mode, Some(AppMode::Normal));
    assert!(state.dialog_input.text.is_empty());
    assert_eq!(state.dialog_input.cursor, 0);
}
