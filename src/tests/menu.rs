use super::helpers::*;
use crate::input::mode_dispatch::{handle_menu_mode, run_selected_menu_action};
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{ActivePanel, AppMode, AppState, PickerKind};

const _TEST_WIDTH: u16 = 24;
const TEST_HEIGHT: u16 = 24;

fn dispatch_menu(state: &mut AppState, key: KeyCode) {
    let mut terminal = test_terminal();
    handle_menu_mode(state, &mut None, &mut None, key, TEST_HEIGHT, &mut terminal);
}

fn run_menu_action(state: &mut AppState) {
    let mut terminal = test_terminal();
    run_selected_menu_action(state, &mut None, &mut None, TEST_HEIGHT, &mut terminal);
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

#[test]
fn menu_toggle_hidden_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    for initial in [false, true] {
        let mut state = AppState {
            active_panel: ActivePanel::Left,
            ..Default::default()
        };
        state.left_panel.set_path(temp_dir.path().to_path_buf());
        state.left_panel.set_show_hidden(initial);
        state.mode = AppMode::Menu;
        state.menu_selected = 3;
        state.menu_item_selected = 0;

        dispatch_menu(&mut state, KeyCode::Enter);

        assert_eq!(state.left_panel.show_hidden(), !initial);
    }
}

#[test]
fn menu_rename_opens_input_dialog_with_current_name() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries.push(
        TestEntry::new("old.txt")
            .path(tmp.path().join("old.txt"))
            .build(),
    );
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.dialog_input.text(), "old.txt");
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            prompt: _,
            action: app::types::InputAction::Rename,
        })
    ));
}

#[test]
fn menu_rename_confirms_and_renames_file() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.txt");
    std::fs::write(&old_path, "content").unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries =
        vec![TestEntry::new("old.txt").path(&old_path).file(1).build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    dispatch_menu(&mut state, KeyCode::Enter);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::Rename,
            ..
        })
    ));
    state.dialog_input.set_text_at_end("new.txt".to_string());

    crate::input::dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        ratatui::layout::Size::new(80, TEST_HEIGHT),
    );

    assert_eq!(state.mode, AppMode::Normal);
    let new_path = dir.path().join("new.txt");
    assert!(new_path.exists(), "new.txt should exist after rename");
    assert!(
        !old_path.exists(),
        "old.txt should no longer exist after rename"
    );
}

#[test]
fn menu_history_opens_picker() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 2,
        menu_item_selected: 5,
        ..Default::default()
    };
    state.command_history.push_back("ls -la".to_string());

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn menu_hotlist_opens_picker() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 2,
        menu_item_selected: 6,
        ..Default::default()
    };
    state.hotlist_push(tmp.path().to_path_buf());

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn menu_sort_preserves_current_entry_focus() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 0,
        menu_item_selected: 1,
        ..Default::default()
    };
    state.left_panel.listing.entries = vec![entry("zeta.txt").build(), entry("alpha.txt").build()];
    state.left_panel.listing.unfiltered_entries = state.left_panel.listing.entries.clone();
    state.left_panel.cursor = 0;
    state
        .left_panel
        .set_sort_mode(lc::app::types::SortMode::new(
            lc::app::types::SortField::Name,
            lc::app::types::Direction::Desc,
        ));

    run_menu_action(&mut state);

    assert_eq!(
        state.left_panel.sort_mode(),
        lc::app::types::SortMode::new(
            lc::app::types::SortField::NaturalName,
            lc::app::types::Direction::Asc,
        )
    );
    assert_eq!(state.left_panel.listing.entries[0].name, "alpha.txt");
    assert_eq!(state.left_panel.listing.entries[1].name, "zeta.txt");
    assert_eq!(
        state
            .left_panel
            .current_entry()
            .map(|entry| entry.name.as_str()),
        Some("zeta.txt")
    );
}

#[test]
fn menu_reset_filter_preserves_current_entry_focus() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 0,
        menu_item_selected: 4,
        ..Default::default()
    };
    state.left_panel.listing.entries = vec![entry("beta.txt").build()];
    state.left_panel.listing.unfiltered_entries =
        vec![entry("alpha.txt").build(), entry("beta.txt").build()];
    state.left_panel.set_filter(Some("beta".to_string()));

    run_menu_action(&mut state);

    assert_eq!(
        state
            .left_panel
            .current_entry()
            .map(|entry| entry.name.as_str()),
        Some("beta.txt")
    );
}

#[test]
fn run_selected_menu_action_fallback_to_normal() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_item_selected: 99,
        ..Default::default()
    };

    run_menu_action(&mut state);

    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn menu_command_line_clears_stale_prev_mode() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: Some(AppMode::Search),
        menu_selected: 2,
        menu_item_selected: 7,
        ..Default::default()
    };

    run_menu_action(&mut state);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_right_panel_sort_changes_right_panel() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 4,
        menu_item_selected: 1,
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state
        .right_panel
        .set_sort_mode(lc::app::types::SortMode::new(
            lc::app::types::SortField::Name,
            lc::app::types::Direction::Desc,
        ));
    let left_sort_before = state.left_panel.sort_mode();

    run_menu_action(&mut state);

    assert_eq!(
        state.right_panel.sort_mode(),
        lc::app::types::SortMode::new(
            lc::app::types::SortField::NaturalName,
            lc::app::types::Direction::Asc,
        )
    );
    assert_eq!(state.left_panel.sort_mode(), left_sort_before);
}

#[test]
fn menu_right_panel_filter_applies_to_right_panel() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 4,
        menu_item_selected: 2,
        active_panel: ActivePanel::Left,
        ..Default::default()
    };

    dispatch_menu(&mut state, KeyCode::Enter);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::Filter,
            ..
        })
    ));
}

#[test]
fn menu_right_panel_listing_mode_toggles_right() {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 4,
        menu_item_selected: 0,
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    let initial_mode = state.right_panel.listing_mode();

    run_menu_action(&mut state);

    assert_ne!(state.right_panel.listing_mode(), initial_mode);
}

#[test]
fn menu_right_panel_refresh_refreshes_right() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 4,
        menu_item_selected: 3,
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.right_panel.set_path(temp_dir.path().to_path_buf());
    std::fs::write(temp_dir.path().join("test.txt"), "content").unwrap();

    run_menu_action(&mut state);

    assert!(
        state
            .right_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "test.txt")
    );
}

#[test]
fn menu_cancel_from_search_restores_search_mode() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: Some(AppMode::Search),
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Search);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_from_normal_returns_to_normal() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: Some(AppMode::Normal),
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_with_no_prev_mode_defaults_to_normal() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: None,
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_with_f9_restores_prev_mode() {
    let mut state = AppState {
        mode: AppMode::Menu,
        prev_mode: Some(AppMode::Viewing),
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };

    dispatch_menu(&mut state, KeyCode::F(9));

    assert_eq!(state.mode, AppMode::Viewing);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_rename_collision_shows_error_message() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.txt");
    let existing_path = dir.path().join("existing.txt");
    std::fs::write(&old_path, "old content").unwrap();
    std::fs::write(&existing_path, "existing content").unwrap();

    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("old.txt").path(&old_path).file(1).build(),
        TestEntry::new("existing.txt")
            .path(&existing_path)
            .file(1)
            .build(),
    ];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    dispatch_menu(&mut state, KeyCode::Enter);

    state
        .dialog_input
        .set_text_at_end("existing.txt".to_string());

    crate::input::dialogs::handle_dialog(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        ratatui::layout::Size::new(80, TEST_HEIGHT),
    );

    assert!(state.status_message.is_some());
    assert_eq!(state.mode, AppMode::Normal);
    assert!(old_path.exists());
    assert!(existing_path.exists());
}
