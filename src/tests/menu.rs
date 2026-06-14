use super::helpers::*;
use crate::input::mode_dispatch::{handle_menu_mode, run_selected_menu_action};
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{ActivePanel, AppMode, AppState, PickerKind, UiState};

const TEST_HEIGHT: u16 = 24;

// Named `(menu, item)` coordinates into the top menu bar (`lc::menu::MENUS`).
// Top-level order is 0:Left 1:File 2:Command 3:Options 4:Right; the second index
// is the position of the item within that menu's `items`/`actions` slice. These
// replace the bare numeric literals the tests used to set on `menu_selected` /
// `menu_item_selected`, so each test reads as the action it actually exercises.
const LEFT_SORT_ORDER: (usize, usize) = (0, 1);
const LEFT_RESET_FILTER: (usize, usize) = (0, 4);
const FILE_USER_MENU: (usize, usize) = (1, 0);
const FILE_RENAME: (usize, usize) = (1, 7);
const COMMAND_HISTORY: (usize, usize) = (2, 5);
const COMMAND_HOTLIST: (usize, usize) = (2, 6);
const COMMAND_COMMAND_LINE: (usize, usize) = (2, 7);
const OPTIONS_SHOW_HIDDEN: (usize, usize) = (3, 0);
const RIGHT_LISTING_MODE: (usize, usize) = (4, 0);
const RIGHT_SORT_ORDER: (usize, usize) = (4, 1);
const RIGHT_FILTER: (usize, usize) = (4, 2);
const RIGHT_REFRESH: (usize, usize) = (4, 3);

/// Build an `AppState` parked in `Menu` mode with the given menu/item selected.
/// Collapses the `AppState { mode: Menu, ui: UiState { .. }, .. }` boilerplate
/// repeated across these tests into one factory.
fn menu_state((menu, item): (usize, usize)) -> AppState {
    AppState {
        mode: AppMode::Menu,
        ui: UiState {
            menu_selected: menu,
            menu_item_selected: item,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Like `menu_state`, but also records the mode the menu was opened from
/// (`prev_mode`) so cancel/restore behaviour can be exercised.
fn menu_state_from((menu, item): (usize, usize), prev_mode: AppMode) -> AppState {
    AppState {
        prev_mode: Some(prev_mode),
        ..menu_state((menu, item))
    }
}

fn dispatch_menu(state: &mut AppState, key: KeyCode) {
    let mut terminal = test_terminal();
    let mut viewer_state = None;
    let mut viewer_loader = None;
    let mut image_preview_loader = None;
    let mut running_job = None;
    let mut ctx = crate::input::EventContext {
        state,
        viewer_state: &mut viewer_state,
        viewer_loader: &mut viewer_loader,
        image_preview_loader: &mut image_preview_loader,
        running_job: &mut running_job,
        term_size: ratatui::layout::Size::new(80, TEST_HEIGHT),
    };
    handle_menu_mode(&mut ctx, key, &mut terminal);
}

fn run_menu_action(state: &mut AppState) {
    let mut terminal = test_terminal();
    let mut viewer_state = None;
    let mut viewer_loader = None;
    let mut image_preview_loader = None;
    let mut running_job = None;
    let mut ctx = crate::input::EventContext {
        state,
        viewer_state: &mut viewer_state,
        viewer_loader: &mut viewer_loader,
        image_preview_loader: &mut image_preview_loader,
        running_job: &mut running_job,
        term_size: ratatui::layout::Size::new(80, TEST_HEIGHT),
    };
    run_selected_menu_action(&mut ctx, &mut terminal);
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
        (state.ui.menu_selected, state.ui.menu_item_selected) = OPTIONS_SHOW_HIDDEN;

        dispatch_menu(&mut state, KeyCode::Enter);

        assert_eq!(state.left_panel.show_hidden(), !initial);
    }
}

#[test]
fn menu_rename_opens_input_dialog_with_current_name() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_entries(vec![
        TestEntry::new("old.txt")
            .path(tmp.path().join("old.txt"))
            .build(),
    ]);
    state.mode = AppMode::Menu;
    (state.ui.menu_selected, state.ui.menu_item_selected) = FILE_RENAME;

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.input.dialog_input.text(), "old.txt");
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
    state.left_panel.set_entries(vec![
        TestEntry::new("old.txt").path(&old_path).file(1).build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Menu;
    (state.ui.menu_selected, state.ui.menu_item_selected) = FILE_RENAME;

    dispatch_menu(&mut state, KeyCode::Enter);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::Rename,
            ..
        })
    ));
    state
        .input
        .dialog_input
        .set_text_at_end("new.txt".to_string());

    dialog_key(
        &mut state,
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
    let mut state = menu_state(COMMAND_HISTORY);
    state.input.command_history.push_back("ls -la".to_string());

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn menu_hotlist_opens_picker() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = menu_state(COMMAND_HOTLIST);
    state.hotlist_push(tmp.path().to_path_buf());

    dispatch_menu(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn menu_sort_preserves_current_entry_focus() {
    let mut state = menu_state(LEFT_SORT_ORDER);
    state
        .left_panel
        .set_entries(vec![entry("zeta.txt").build(), entry("alpha.txt").build()]);
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
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(0)
            .expect("entry 0")
            .name,
        "alpha.txt"
    );
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(1)
            .expect("entry 1")
            .name,
        "zeta.txt"
    );
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
    let mut state = menu_state(LEFT_RESET_FILTER);
    state
        .left_panel
        .set_entries(vec![entry("alpha.txt").build(), entry("beta.txt").build()]);
    state
        .left_panel
        .listing
        .set_filtered(&[entry("beta.txt").build()]);
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

/// `99` is a deliberately out-of-range item index: no menu has that many items,
/// so `menu_action_at` returns `None` and the action runner must fall back to
/// `Normal` instead of panicking or indexing past the actions slice.
const ITEM_OUT_OF_RANGE: usize = 99;

#[test]
fn run_selected_menu_action_fallback_to_normal() {
    let mut state = menu_state((0, ITEM_OUT_OF_RANGE));

    run_menu_action(&mut state);

    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn menu_command_line_clears_stale_prev_mode() {
    let mut state = menu_state_from(COMMAND_COMMAND_LINE, AppMode::Search);

    run_menu_action(&mut state);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_right_panel_sort_changes_right_panel() {
    let mut state = menu_state(RIGHT_SORT_ORDER);
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
    let mut state = menu_state(RIGHT_FILTER);

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
    let mut state = menu_state(RIGHT_LISTING_MODE);
    let initial_mode = state.right_panel.listing_mode();

    run_menu_action(&mut state);

    assert_ne!(state.right_panel.listing_mode(), initial_mode);
}

#[test]
fn menu_right_panel_refresh_refreshes_right() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut state = menu_state(RIGHT_REFRESH);
    state.right_panel.set_path(temp_dir.path().to_path_buf());
    std::fs::write(temp_dir.path().join("test.txt"), "content").unwrap();

    run_menu_action(&mut state);

    assert!(
        state
            .right_panel
            .listing
            .filtered()
            .any(|e| e.name == "test.txt")
    );
}

#[test]
fn menu_cancel_from_search_restores_search_mode() {
    let mut state = menu_state_from(FILE_USER_MENU, AppMode::Search);

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Search);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_from_normal_returns_to_normal() {
    let mut state = menu_state_from(FILE_USER_MENU, AppMode::Normal);

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_with_no_prev_mode_defaults_to_normal() {
    let mut state = menu_state(FILE_USER_MENU);

    dispatch_menu(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn menu_cancel_with_f9_restores_prev_mode() {
    let mut state = menu_state_from(FILE_USER_MENU, AppMode::Viewing);

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
    state.left_panel.set_entries(vec![
        TestEntry::new("old.txt").path(&old_path).file(1).build(),
        TestEntry::new("existing.txt")
            .path(&existing_path)
            .file(1)
            .build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Menu;
    (state.ui.menu_selected, state.ui.menu_item_selected) = FILE_RENAME;

    dispatch_menu(&mut state, KeyCode::Enter);

    state
        .input
        .dialog_input
        .set_text_at_end("existing.txt".to_string());

    dialog_key(
        &mut state,
        KeyCode::Enter,
        ratatui::layout::Size::new(80, TEST_HEIGHT),
    );

    assert!(state.ui.status_message.is_some());
    assert_eq!(state.mode, AppMode::Normal);
    assert!(old_path.exists());
    assert!(existing_path.exists());
}
