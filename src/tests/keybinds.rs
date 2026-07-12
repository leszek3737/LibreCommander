use super::helpers::{
    TestEntry, VISIBLE_HEIGHT, dispatch_key, dummy_tree_entries, test_path, test_terminal,
};
use crate::input::{command_line, directory_tree};
use crate::{handle_alt_keys, handle_function_keys, handle_navigation_keys, launch_editor};
use crossterm::event::KeyCode;
use crossterm::event::{KeyEvent, KeyModifiers};
use lc::app;
use lc::app::types::{ActivePanel, AppMode, AppState, DialogKind, InputAction};
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;

/// Terminal height used by the directory-tree paging tests.
const TREE_TERM_HEIGHT: u16 = 12;
/// Visible tree rows for `TREE_TERM_HEIGHT` (height minus the tree chrome
/// overhead). One PageDown/PageUp moves the selection by this many rows.
const TREE_PAGE_STEP: usize = (TREE_TERM_HEIGHT - lc::ui::DIR_TREE_OVERHEAD_ROWS) as usize;

fn setup_ctrl_test() -> (Terminal<TestBackend>, AppState) {
    (test_terminal(), AppState::default())
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

#[test]
fn ctrl_s_starts_search_mode() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state.left_panel.set_entries(vec![
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Search);
    assert_eq!(state.input.search_query, "");
}

#[test]
fn ctrl_h_toggles_hidden() {
    let (mut terminal, mut state) = setup_ctrl_test();
    let temp_dir = tempfile::tempdir().unwrap();
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.active_panel = ActivePanel::Left;

    state.left_panel.set_show_hidden(false);
    state.left_panel.cursor = 3;
    state.left_panel.scroll_offset = 2;
    dispatch_key(
        &mut state,
        KeyCode::Char('h'),
        KeyModifiers::CONTROL,
        &mut terminal,
    );
    assert!(state.left_panel.show_hidden());
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.left_panel.scroll_offset, 0);

    dispatch_key(
        &mut state,
        KeyCode::Char('h'),
        KeyModifiers::CONTROL,
        &mut terminal,
    );
    assert!(!state.left_panel.show_hidden());
}

#[test]
fn ctrl_r_refreshes() {
    let (mut terminal, mut state) = setup_ctrl_test();
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("existing.txt"), b"data").unwrap();
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.set_entries(vec![]);
    assert!(state.left_panel.listing.filtered_is_empty());

    dispatch_key(
        &mut state,
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(
        state
            .left_panel
            .listing
            .filtered()
            .any(|e| e.name == "existing.txt"),
        "refresh_active should have loaded directory entries"
    );
}

#[test]
fn ctrl_u_swaps_panels() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state.left_panel.set_path(PathBuf::from("/tmp/left"));
    state.right_panel.set_path(PathBuf::from("/tmp/right"));
    state.active_panel = ActivePanel::Left;

    dispatch_key(
        &mut state,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        &mut terminal,
    );

    assert_eq!(state.left_panel.path(), PathBuf::from("/tmp/right"));
    assert_eq!(state.right_panel.path(), PathBuf::from("/tmp/left"));
    assert_eq!(state.active_panel, ActivePanel::Right);
}

#[test]
fn alt_j_does_not_start_search_mode() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state.left_panel.set_entries(vec![
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Char('j'),
        KeyModifiers::ALT,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.input.search_query, "");
}

#[test]
fn alt_k_does_not_move_cursor() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state
        .left_panel
        .set_entries(vec![entry("a.txt").file(10).build()]);
    state.left_panel.cursor = 0;

    dispatch_key(
        &mut state,
        KeyCode::Char('k'),
        KeyModifiers::ALT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn shift_j_falls_through_to_navigation_down() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state.left_panel.set_entries(vec![
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Char('j'),
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn shift_k_falls_through_to_navigation_up() {
    let (mut terminal, mut state) = setup_ctrl_test();
    state.left_panel.set_entries(vec![
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
    ]);
    state.left_panel.cursor = 1;

    dispatch_key(
        &mut state,
        KeyCode::Char('k'),
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn alt_enter_shows_properties_dialog() {
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("file.txt").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Enter, VISIBLE_HEIGHT);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Properties(..))
    ));
}

#[test]
fn alt_enter_on_dotdot_does_nothing() {
    let mut state = AppState::default();
    state.left_panel.set_entries(vec![entry("..").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Enter, VISIBLE_HEIGHT);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn alt_backspace_navigates_to_parent() {
    let mut state = AppState::default();
    // The handler only navigates to a history entry that exists (`is_dir`), so
    // use the platform temp dir rather than a Unix-only literal like /tmp.
    let parent = std::env::temp_dir();
    state.left_panel.push_history(parent.clone());
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Backspace, VISIBLE_HEIGHT);
    assert_eq!(state.left_panel.path(), parent);
}

#[test]
fn alt_backspace_empty_history_does_nothing() {
    let mut state = AppState::default();
    let orig_path = state.left_panel.path().to_path_buf();
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Backspace, VISIBLE_HEIGHT);
    assert_eq!(state.left_panel.path(), orig_path);
}

#[test]
fn alt_c_opens_quick_cd() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    handle_alt_keys(&mut state, KeyCode::Char('c'), VISIBLE_HEIGHT);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Input {
            action: InputAction::QuickCd,
            ..
        })
    ));
}

#[test]
fn alt_x_opens_command_line() {
    let mut state = AppState::default();
    state
        .input
        .command_line
        .set_text_at_end("draft".to_string());
    state.input.history_index = Some(0);
    state.prev_mode = Some(AppMode::Search);

    handle_alt_keys(&mut state, KeyCode::Char('X'), VISIBLE_HEIGHT);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert!(state.input.command_line.text().is_empty());
    assert_eq!(state.input.command_line.cursor(), 0);
    assert_eq!(state.input.history_index, None);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn alt_unhandled_does_nothing() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    handle_alt_keys(&mut state, KeyCode::Char('y'), VISIBLE_HEIGHT);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn f7_opens_create_directory_dialog() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(&mut state, &mut viewer, KeyCode::F(7), &mut terminal);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::CreateDirectory,
            ..
        })
    ));
    assert!(state.input.dialog_input.text().is_empty());
}

#[test]
fn f9_enters_menu_mode() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(&mut state, &mut viewer, KeyCode::F(9), &mut terminal);
    assert!(matches!(state.mode, AppMode::Menu));
    assert_eq!(state.ui.menu_item_selected, 0);
}

#[test]
fn f10_sets_should_quit() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(&mut state, &mut viewer, KeyCode::F(10), &mut terminal);
    assert!(state.should_quit());
}

#[test]
fn launch_editor_no_current_entry_does_nothing() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();
    launch_editor(&mut state, &mut terminal);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn launch_editor_directory_entry_does_not_launch() {
    let mut state = AppState::default();
    state.left_panel.set_entries(vec![entry("mydir").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    let mut terminal = test_terminal();
    launch_editor(&mut state, &mut terminal);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn tab_switches_panel_left_to_right() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, VISIBLE_HEIGHT);
    assert_eq!(state.active_panel, ActivePanel::Right);
}

#[test]
fn tab_switches_panel_right_to_left() {
    let mut state = AppState {
        active_panel: ActivePanel::Right,
        ..Default::default()
    };
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, VISIBLE_HEIGHT);
    assert_eq!(state.active_panel, ActivePanel::Left);
}

#[test]
fn tab_clamps_cursor() {
    let mut state = AppState::default();
    state.left_panel.set_entries(vec![entry("a").build(); 10]);
    state.left_panel.cursor = 9;
    state
        .right_panel
        .set_entries(vec![entry("x").build(), entry("y").build()]);
    state.right_panel.cursor = 9;
    state.active_panel = ActivePanel::Left;
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, VISIBLE_HEIGHT);
    assert_eq!(state.active_panel, ActivePanel::Right);
    assert_eq!(state.right_panel.cursor, 1);
}

#[test]
fn directory_tree_page_down_uses_terminal_height() {
    let mut state = AppState {
        tree: app::types::TreeState {
            entries: dummy_tree_entries(50),
            ..Default::default()
        },
        ..Default::default()
    };

    {
        let mut no_viewer = None;
        let mut no_loader = None;
        let mut no_image = None;
        let mut no_job = None;
        let mut ctx = crate::input::EventContext {
            state: &mut state,
            viewer_state: &mut no_viewer,
            viewer_loader: &mut no_loader,
            image_preview_loader: &mut no_image,
            running_job: &mut no_job,
            term_size: ratatui::layout::Size::new(80, TREE_TERM_HEIGHT),
        };
        directory_tree::handle_directory_tree(&mut ctx, KeyCode::PageDown);
    }

    // From the top, one PageDown advances by a full visible page.
    assert_eq!(state.tree.selected, TREE_PAGE_STEP);
    assert_eq!(state.tree.scroll, TREE_PAGE_STEP);
}

#[test]
fn directory_tree_page_up_uses_terminal_height() {
    const START: usize = 25;
    let mut state = AppState {
        tree: app::types::TreeState {
            entries: dummy_tree_entries(50),
            selected: START,
            scroll: START,
            ..Default::default()
        },
        ..Default::default()
    };

    {
        let mut no_viewer = None;
        let mut no_loader = None;
        let mut no_image = None;
        let mut no_job = None;
        let mut ctx = crate::input::EventContext {
            state: &mut state,
            viewer_state: &mut no_viewer,
            viewer_loader: &mut no_loader,
            image_preview_loader: &mut no_image,
            running_job: &mut no_job,
            term_size: ratatui::layout::Size::new(80, TREE_TERM_HEIGHT),
        };
        directory_tree::handle_directory_tree(&mut ctx, KeyCode::PageUp);
    }

    // One PageUp retreats by a full visible page.
    assert_eq!(state.tree.selected, START - TREE_PAGE_STEP);
    assert_eq!(state.tree.scroll, START - TREE_PAGE_STEP);
}

#[test]
fn command_line_up_loads_last_history_entry() {
    let mut state = AppState::default();
    state
        .input
        .command_history
        .push_back("git status".to_string());

    command_line::handle_command_line(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!(state.input.command_line.text(), "git status");
}

#[test]
fn f4_no_current_entry_does_not_launch() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(&mut state, &mut viewer, KeyCode::F(4), &mut terminal);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn f5_opens_copy_confirm_dialog() {
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("file.txt").file(10).build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.right_panel.set_path(PathBuf::from("/tmp/dest"));
    let mut viewer = None;
    let mut terminal = test_terminal();

    handle_function_keys(&mut state, &mut viewer, KeyCode::F(5), &mut terminal);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(_))
    ));
    assert!(matches!(
        state.ui.pending_action,
        Some(app::types::PendingAction::Copy(_))
    ));
}

#[test]
fn f6_opens_move_confirm_dialog() {
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("file.txt").file(10).build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    state.right_panel.set_path(PathBuf::from("/tmp/dest"));
    let mut viewer = None;
    let mut terminal = test_terminal();

    handle_function_keys(&mut state, &mut viewer, KeyCode::F(6), &mut terminal);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(_))
    ));
    assert!(matches!(
        state.ui.pending_action,
        Some(app::types::PendingAction::Move(_))
    ));
}

#[test]
fn f5_with_empty_panel_does_nothing() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.right_panel.set_path(PathBuf::from("/tmp/dest"));
    let mut viewer = None;
    let mut terminal = test_terminal();

    handle_function_keys(&mut state, &mut viewer, KeyCode::F(5), &mut terminal);

    assert!(matches!(state.mode, AppMode::Normal));
    assert!(state.ui.pending_action.is_none());
}

#[test]
fn ctrl_down_arrow_falls_through_to_navigation() {
    // Navigation ignores Ctrl on arrow keys: Ctrl+Down behaves like a plain
    // Down and advances the cursor.
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("a").build(), entry("b").build()]);
    state.active_panel = ActivePanel::Left;
    state.left_panel.cursor = 0;

    handle_navigation_keys(
        &mut state,
        KeyCode::Down,
        KeyModifiers::CONTROL,
        VISIBLE_HEIGHT,
    );

    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn alt_up_arrow_falls_through_to_navigation() {
    // Navigation ignores Alt on arrow keys: Alt+Up behaves like a plain Up.
    let mut state = AppState::default();
    state
        .left_panel
        .set_entries(vec![entry("a").build(), entry("b").build()]);
    state.active_panel = ActivePanel::Left;
    state.left_panel.cursor = 1;

    handle_navigation_keys(&mut state, KeyCode::Up, KeyModifiers::ALT, VISIBLE_HEIGHT);

    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn page_down_clamps_to_last_entry() {
    // A panel shorter than one page must clamp the cursor at the final entry
    // rather than overshoot past the end.
    let mut state = AppState::default();
    state.left_panel.set_entries(vec![
        entry("a").build(),
        entry("b").build(),
        entry("c").build(),
    ]);
    state.active_panel = ActivePanel::Left;
    state.left_panel.cursor = 0;

    handle_navigation_keys(
        &mut state,
        KeyCode::PageDown,
        KeyModifiers::NONE,
        VISIBLE_HEIGHT,
    );

    assert_eq!(state.left_panel.cursor, 2);
}
