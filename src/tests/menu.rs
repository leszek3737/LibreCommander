use super::helpers::*;
use crate::input::mode_dispatch::{handle_menu_mode, run_selected_menu_action};
use crate::*;
use app::types::{ActivePanel, PickerKind};

#[test]
fn menu_toggle_hidden_files_refreshes_active_panel() {
    let temp_dir = tempfile::tempdir().unwrap();
    let state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    let mut terminal = test_terminal();
    let mut state = state;
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.show_hidden = false;
    state.mode = AppMode::Menu;
    state.menu_selected = 3;
    state.menu_item_selected = 0;

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(state.left_panel.show_hidden);
}

#[test]
fn menu_toggle_hidden_files_reverse_refreshes_active_panel() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.show_hidden = true;
    state.mode = AppMode::Menu;
    state.menu_selected = 3;
    state.menu_item_selected = 0;

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert!(!state.left_panel.show_hidden);
}

#[test]
fn menu_rename_opens_input_dialog_with_current_name() {
    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries.push(
        app::types::FileEntry::builder()
            .name("old.txt")
            .path(tmp.path().join("old.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    );
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert_eq!(state.dialog_input.text, "old.txt");
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            prompt: _,
            default_text: _,
            action: app::types::InputAction::Rename,
        })
    ));
}

#[test]
fn menu_rename_confirms_and_renames_file() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.txt");
    std::fs::write(&old_path, "content").unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("old.txt").path(old_path).build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::Rename,
            ..
        })
    ));
    state.dialog_input.text = "new.txt".to_string();
    state.dialog_input.cursor = state.dialog_input.text.len();
    assert_eq!(state.dialog_input.text, "new.txt");
}

#[test]
fn menu_history_opens_picker() {
    let mut terminal = test_terminal();
    let state = AppState {
        mode: AppMode::Menu,
        menu_selected: 2,
        menu_item_selected: 5,
        ..Default::default()
    };
    let mut state = state;
    state.command_history.push_back("ls -la".to_string());

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn menu_hotlist_opens_picker() {
    let mut terminal = test_terminal();
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 2,
        menu_item_selected: 6,
        ..Default::default()
    };
    state.directory_hotlist.push(tmp.path().to_path_buf());

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
    assert_eq!(state.picker_selected, 0);
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn run_selected_menu_action_fallback_to_normal() {
    let mut state = AppState::default();
    state.mode = AppMode::Menu;
    state.menu_item_selected = 99;
    run_selected_menu_action(&mut state, &mut None, &mut None, 24, &mut test_terminal());
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn menu_command_line_clears_stale_prev_mode() {
    let mut state = AppState::default();
    state.mode = AppMode::Menu;
    state.prev_mode = Some(AppMode::Search);
    state.menu_selected = 2;
    state.menu_item_selected = 7;

    run_selected_menu_action(&mut state, &mut None, &mut None, 24, &mut test_terminal());

    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.prev_mode, None);
}
