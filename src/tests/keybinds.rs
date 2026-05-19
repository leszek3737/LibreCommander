use super::helpers::*;
use crate::input::mode_dispatch::handle_normal_mode;
use crate::input::{command_line, directory_tree};
use crate::*;
use app::types::{ActivePanel, DialogKind, InputAction};
use crossterm::event::{KeyEvent, KeyModifiers};
use std::path::PathBuf;

#[test]
fn ctrl_alt_s_starts_search_mode() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Search);
    assert_eq!(state.search_query, "");
}

#[test]
fn ctrl_alt_h_toggles_hidden() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    let temp_dir = tempfile::tempdir().unwrap();
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.show_hidden = false;
    state.left_panel.cursor = 3;
    state.left_panel.scroll_offset = 2;

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('h'),
        KeyModifiers::CONTROL,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.show_hidden);
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.left_panel.scroll_offset, 0);
}

#[test]
fn ctrl_alt_h_toggles_hidden_back() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.show_hidden = true;
    state.active_panel = ActivePanel::Left;
    super::super::handle_ctrl_keys(&mut state, KeyCode::Char('h'), 24);
    assert!(!state.left_panel.show_hidden);
}

#[test]
fn ctrl_alt_r_refreshes() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("existing.txt"), b"data").unwrap();
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.listing.entries = vec![];
    assert!(state.left_panel.listing.entries.is_empty());

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(
        state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "existing.txt"),
        "refresh_active should have loaded directory entries"
    );
}

#[test]
fn ctrl_alt_u_swaps_panels() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.set_path(PathBuf::from("/tmp/left"));
    state.right_panel.set_path(PathBuf::from("/tmp/right"));
    state.active_panel = ActivePanel::Left;

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.path, PathBuf::from("/tmp/right"));
    assert_eq!(state.right_panel.path, PathBuf::from("/tmp/left"));
    assert_eq!(state.active_panel, ActivePanel::Right);
}

#[test]
fn alt_j_does_not_start_search_mode() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('j'),
        KeyModifiers::ALT,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
}

#[test]
fn alt_k_does_not_move_cursor() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("a.txt").size(10).build()];
    state.left_panel.cursor = 0;

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('k'),
        KeyModifiers::ALT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn shift_j_falls_through_to_navigation_down() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('j'),
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn shift_k_falls_through_to_navigation_up() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];
    state.left_panel.cursor = 1;

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Char('k'),
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn alt_enter_shows_properties_dialog() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("file.txt").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Enter, 20);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Properties { .. })
    ));
}

#[test]
fn alt_enter_on_dotdot_does_nothing() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("..").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Enter, 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn alt_backspace_navigates_to_parent() {
    let mut state = AppState::default();
    let parent = PathBuf::from("/tmp");
    state.left_panel.history.push(parent.clone());
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Backspace, 20);
    assert_eq!(state.left_panel.path, parent);
}

#[test]
fn alt_backspace_empty_history_does_nothing() {
    let mut state = AppState::default();
    let orig_path = state.left_panel.path.clone();
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Backspace, 20);
    assert_eq!(state.left_panel.path, orig_path);
}

#[test]
fn alt_c_opens_quick_cd() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    handle_alt_keys(&mut state, KeyCode::Char('c'), 20);
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
    state.command_line.text = "draft".to_string();
    state.command_line.cursor = state.command_line.text.len();
    state.history_index = Some(0);
    state.prev_mode = Some(AppMode::Search);

    handle_alt_keys(&mut state, KeyCode::Char('X'), 20);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert!(state.command_line.text.is_empty());
    assert_eq!(state.command_line.cursor, 0);
    assert_eq!(state.history_index, None);
    assert_eq!(state.prev_mode, None);
}

#[test]
fn alt_unhandled_does_nothing() {
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    handle_alt_keys(&mut state, KeyCode::Char('y'), 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn f7_opens_create_directory_dialog() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(
        &mut state,
        &mut viewer,
        &mut None,
        KeyCode::F(7),
        &mut terminal,
    );
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Input {
            action: app::types::InputAction::CreateDirectory,
            ..
        })
    ));
    assert!(state.dialog_input.text.is_empty());
}

#[test]
fn f9_enters_menu_mode() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(
        &mut state,
        &mut viewer,
        &mut None,
        KeyCode::F(9),
        &mut terminal,
    );
    assert!(matches!(state.mode, AppMode::Menu));
    assert_eq!(state.menu_item_selected, 0);
}

#[test]
fn f10_sets_should_quit() {
    let mut state = AppState::default();
    let mut viewer = None;
    let mut terminal = test_terminal();
    handle_function_keys(
        &mut state,
        &mut viewer,
        &mut None,
        KeyCode::F(10),
        &mut terminal,
    );
    assert!(state.should_quit);
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
    state.left_panel.listing.entries = vec![TestEntry::new("mydir").build()];
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
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, 20);
    assert_eq!(state.active_panel, ActivePanel::Right);
}

#[test]
fn tab_switches_panel_right_to_left() {
    let mut state = AppState {
        active_panel: ActivePanel::Right,
        ..Default::default()
    };
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, 20);
    assert_eq!(state.active_panel, ActivePanel::Left);
}

#[test]
fn tab_clamps_cursor() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("a").build(); 10];
    state.left_panel.cursor = 9;
    state.right_panel.listing.entries =
        vec![TestEntry::new("x").build(), TestEntry::new("y").build()];
    state.active_panel = ActivePanel::Left;
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, 20);
    assert_eq!(state.active_panel, ActivePanel::Right);
    assert!(state.right_panel.cursor <= 1);
}

#[test]
fn directory_tree_page_down_uses_terminal_height() {
    let mut state = AppState {
        tree_entries: (0..50)
            .map(|i| app::dir_tree::TreeEntry {
                path: PathBuf::from(format!("/tmp/{i}")),
                depth: 0,
                is_dir: false,
                expanded: false,
                name: format!("entry-{i}"),
                read_error: false,
            })
            .collect(),
        ..Default::default()
    };

    directory_tree::handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::PageDown, 12);

    assert_eq!(state.tree_selected, 9);
    assert_eq!(state.tree_scroll, 9);
}

#[test]
fn directory_tree_page_up_uses_terminal_height() {
    let mut state = AppState {
        tree_entries: (0..50)
            .map(|i| app::dir_tree::TreeEntry {
                path: PathBuf::from(format!("/tmp/{i}")),
                depth: 0,
                is_dir: false,
                expanded: false,
                name: format!("entry-{i}"),
                read_error: false,
            })
            .collect(),
        tree_selected: 25,
        tree_scroll: 25,
        ..Default::default()
    };

    directory_tree::handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::PageUp, 12);

    assert_eq!(state.tree_selected, 16);
    assert_eq!(state.tree_scroll, 16);
}

#[test]
fn command_line_up_loads_last_history_entry() {
    let mut state = AppState::default();
    state.command_history.push_back("git status".to_string());

    command_line::handle_command_line(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!(state.command_line.text, "git status");
}
