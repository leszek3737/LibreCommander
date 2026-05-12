#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use crate::input::{command_line, dialogs, directory_tree, pickers};
use app::types::{ActivePanel, CompareMode, FileEntry, PickerKind};
use crossterm::event::KeyEvent;
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(80, 24)).unwrap()
}

fn make_test_entry(name: &str, size: u64, selected: bool) -> FileEntry {
    let mut cha = crate::fs::cha::Cha::dummy_dir();
    cha.mode = crate::fs::cha::ChaMode::new(0o100644);
    cha.len = size;
    cha.mtime = Some(std::time::SystemTime::now());
    cha.btime = Some(std::time::UNIX_EPOCH);
    FileEntry::builder()
        .name(name)
        .path(PathBuf::from(format!("/tmp/{name}")))
        .cha(cha)
        .selected(selected)
        .build()
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
fn search_enter_keeps_current_filter() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();
    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "alpha".to_string(),
        ..Default::default()
    };
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.entries = vec![make_test_entry("alpha.txt", 1, false)];
    state.left_panel.unfiltered_entries = vec![
        make_test_entry("alpha.txt", 1, false),
        make_test_entry("beta.txt", 2, false),
    ];
    state.left_panel.filter = Some("alpha".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
    assert_eq!(state.left_panel.filter.as_deref(), Some("alpha"));
    assert!(
        state
            .left_panel
            .entries
            .iter()
            .any(|entry| entry.name == "alpha.txt")
    );
    assert!(
        state
            .left_panel
            .entries
            .iter()
            .all(|entry| entry.name == ".." || entry.name.contains("alpha"))
    );
}

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

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    assert_eq!(state.mode, AppMode::Normal);
    assert!(state.left_panel.show_hidden);
}

#[test]
fn menu_rename_opens_input_dialog_with_current_name() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries.push(
        app::types::FileEntry::builder()
            .name("old.txt")
            .path(std::env::temp_dir().join("old.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    );
    state.mode = AppMode::Menu;
    state.menu_selected = 1;
    state.menu_item_selected = 7;

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    assert_eq!(state.dialog_input, "old.txt");
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
fn parse_octal_mode_accepts_valid_input() {
    assert_eq!(dialogs::parse_octal_mode("755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode("0644"), Some(0o644));
    assert_eq!(dialogs::parse_octal_mode("bad"), None);
}

#[test]
fn compare_directories_reports_summary() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        app::types::FileEntry::builder()
            .name("a.txt")
            .path(std::env::temp_dir().join("a.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    ];
    state.right_panel.entries = vec![
        app::types::FileEntry::builder()
            .name("b.txt")
            .path(std::env::temp_dir().join("b.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 1\nDiffering:       0"
            )
        ))
    );
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

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn menu_hotlist_opens_picker() {
    let mut terminal = test_terminal();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 2,
        menu_item_selected: 6,
        ..Default::default()
    };
    state.directory_hotlist.push(std::env::temp_dir());

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn shift_down_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert!(state.left_panel.entries[0].selected);
    assert!(!state.left_panel.entries[1].selected);
}

#[test]
fn shift_up_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
        make_test_entry("c.txt", 30, false),
    ];
    state.left_panel.cursor = 2;

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Up,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert!(!state.left_panel.entries[0].selected);
    assert!(!state.left_panel.entries[1].selected);
    assert!(state.left_panel.entries[2].selected);
}

#[test]
fn shift_selection_preserves_unrelated_entries() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, true),
        make_test_entry("b.txt", 20, false),
        make_test_entry("c.txt", 30, false),
        make_test_entry("d.txt", 40, false),
    ];
    state.left_panel.cursor = 2;
    state.left_panel.recalculate_selection_stats();

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.entries[0].selected);
    assert!(!state.left_panel.entries[1].selected);
    assert!(state.left_panel.entries[2].selected);
    assert!(!state.left_panel.entries[3].selected);
}

#[test]
fn shift_arrow_then_shift_arrow_toggles_two() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
        make_test_entry("c.txt", 30, false),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );
    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.entries[0].selected);
    assert!(state.left_panel.entries[1].selected);
    assert!(!state.left_panel.entries[2].selected);
    assert_eq!(state.left_panel.cursor, 2);
}

#[test]
fn command_line_up_loads_last_history_entry() {
    let mut state = AppState::default();
    state.command_history.push_back("git status".to_string());

    command_line::handle_command_line(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!(state.command_line, "git status");
}

fn test_file_entry(name: &str, path: PathBuf) -> app::types::FileEntry {
    app::types::FileEntry::builder()
        .name(name)
        .path(path)
        .cha(crate::fs::cha::Cha::dummy_dir())
        .build()
}

#[test]
fn compare_directories_marks_unique_entries_selected() {
    let mut state = AppState::default();
    let tmp = std::env::temp_dir();
    state.left_panel.entries = vec![
        test_file_entry("same.txt", tmp.join("same.txt")),
        test_file_entry("left.txt", tmp.join("left.txt")),
    ];
    state.right_panel.entries = vec![
        test_file_entry("same.txt", tmp.join("same.txt")),
        test_file_entry("right.txt", tmp.join("right.txt")),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(!state.left_panel.entries[0].selected);
    assert!(state.left_panel.entries[1].selected);
    assert!(!state.right_panel.entries[0].selected);
    assert!(state.right_panel.entries[1].selected);
}

fn make_entry(name: &str, selected: bool) -> FileEntry {
    let mut cha = crate::fs::cha::Cha::dummy_dir();
    cha.mode = crate::fs::cha::ChaMode::new(0o100644);
    cha.len = 100;
    cha.mtime = Some(UNIX_EPOCH + Duration::from_secs(0));
    cha.btime = Some(std::time::SystemTime::UNIX_EPOCH);
    FileEntry::builder()
        .name(name)
        .path(PathBuf::from(format!("/tmp/{}", name)))
        .cha(cha)
        .owner("user")
        .group("group")
        .selected(selected)
        .build()
}

#[test]
fn test_selected_or_current_paths_fallback_to_cursor() {
    // No entries are selected → should return the cursor entry
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.entries = vec![
        make_entry("file_a.txt", false),
        make_entry("file_b.txt", false),
    ];
    state.left_panel.cursor = 1;

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/tmp/file_b.txt"));
}

#[test]
fn test_selected_or_current_paths_uses_selection_when_present() {
    // Two entries selected → returns both, ignoring cursor position
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.entries = vec![
        make_entry("file_a.txt", true),
        make_entry("file_b.txt", false),
        make_entry("file_c.txt", true),
    ];
    state.left_panel.cursor = 1; // cursor on unselected file_b

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&PathBuf::from("/tmp/file_a.txt")));
    assert!(paths.contains(&PathBuf::from("/tmp/file_c.txt")));
}

#[test]
fn test_selected_or_current_paths_skips_dotdot() {
    // ".." selected → should not appear in results; cursor is on ".."  → empty
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    let mut dotdot = make_entry("..", false);
    dotdot.name = "..".to_string();
    dotdot.selected = true;
    state.left_panel.entries = vec![dotdot];
    state.left_panel.cursor = 0;

    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn test_selected_or_current_paths_empty_panel() {
    let state = AppState::new();
    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
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
            })
            .collect(),
        ..Default::default()
    };

    directory_tree::handle_directory_tree(&mut state, &mut None, KeyCode::PageDown, 12);

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
            })
            .collect(),
        tree_selected: 25,
        tree_scroll: 25,
        ..Default::default()
    };

    directory_tree::handle_directory_tree(&mut state, &mut None, KeyCode::PageUp, 12);

    assert_eq!(state.tree_selected, 16);
    assert_eq!(state.tree_scroll, 16);
}

#[test]
fn history_dedup_consecutive() {
    let mut state = AppState::default();
    state.left_panel.path = std::env::temp_dir();
    state.command_history.push_back("echo hi".to_string());
    // Simulate push logic (same as run_shell_command but without executing)
    let cmd = "echo hi";
    if state.command_history.back().is_none_or(|l| l != cmd) {
        state.command_history.push_back(cmd.to_string());
    }
    assert_eq!(state.command_history.len(), 1);
    assert_eq!(state.command_history[0], "echo hi");
}

#[test]
fn history_dedup_different_commands() {
    let mut state = AppState::default();
    state.command_history.push_back("echo hi".to_string());
    let cmd = "ls -la";
    if state.command_history.back().is_none_or(|l| l != cmd) {
        state.command_history.push_back(cmd.to_string());
    }
    assert_eq!(state.command_history.len(), 2);
}

#[test]
fn history_cap_at_100() {
    let mut state = AppState::default();
    for i in 0..101 {
        let cmd = format!("cmd_{}", i);
        if state
            .command_history
            .back()
            .is_none_or(|l| l.as_str() != cmd.as_str())
        {
            state.command_history.push_back(cmd);
            if state.command_history.len() > shell::MAX_HISTORY {
                state.command_history.pop_front();
            }
        }
    }
    assert_eq!(state.command_history.len(), 100);
    assert_eq!(state.command_history[0], "cmd_1");
    assert_eq!(state.command_history[99], "cmd_100");
}

#[test]
fn history_picker_enter_loads_command_line() {
    let mut state = AppState::default();
    state.command_history.push_back("git status".to_string());
    state.command_history.push_back("git log".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.command_line, "git log");
}

#[test]
fn history_picker_esc_cancels() {
    let mut state = AppState::default();
    state.command_history.push_back("ls".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn history_picker_navigate_up_down() {
    let mut state = AppState::default();
    state.command_history.push_back("cmd1".to_string());
    state.command_history.push_back("cmd2".to_string());
    state.command_history.push_back("cmd3".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn hotlist_picker_add_current_dir() {
    let mut state = AppState::default();
    let tmp = std::env::temp_dir();
    state.left_panel.path = tmp.clone();
    state.directory_hotlist.clear();
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert!(state.directory_hotlist.contains(&tmp));
}

#[test]
fn hotlist_picker_add_dedup() {
    let mut state = AppState::default();
    let tmp = std::env::temp_dir();
    state.left_panel.path = tmp.clone();
    state.directory_hotlist = vec![tmp.clone()];
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert_eq!(
        state
            .directory_hotlist
            .iter()
            .filter(|p| *p == &tmp)
            .count(),
        1
    );
}

#[test]
fn hotlist_picker_delete_entry() {
    let mut state = AppState {
        directory_hotlist: vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ],
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));

    assert_eq!(state.directory_hotlist.len(), 2);
    assert!(!state.directory_hotlist.contains(&PathBuf::from("/b")));
}

#[test]
fn hotlist_picker_delete_adjusts_cursor() {
    let mut state = AppState {
        directory_hotlist: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));

    assert_eq!(state.directory_hotlist.len(), 1);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn hotlist_persistence_roundtrip() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let tmp_dir = std::env::temp_dir();
    let state = AppState {
        directory_hotlist: vec![tmp_dir, PathBuf::from("/usr")],
        ..Default::default()
    };

    // Serialize and deserialize manually via PersistedSetup
    let hotlist_strs: Vec<String> = state
        .directory_hotlist
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let content = format!(
        "version = 1\nactive_panel = \"left\"\nhotlist = {:?}\n\
        [left]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n\
        [right]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n",
        hotlist_strs
    );

    // Write to a temp file, then read back via toml
    let mut f = NamedTempFile::new().unwrap();
    write!(f, "{}", content).unwrap();
    let read_back = std::fs::read_to_string(f.path()).unwrap();
    let parsed: app::config::PersistedSetup = toml::from_str(&read_back).unwrap();

    let loaded: Vec<PathBuf> = parsed.hotlist.iter().map(PathBuf::from).collect();
    assert_eq!(loaded, state.directory_hotlist);
}

#[test]
fn user_menu_picker_esc_closes() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: vec![app::user_menu::MenuEntry {
            hotkey: 'A',
            title: "Archive".to_string(),
            command: "tar czf a.tgz".to_string(),
            condition: None,
        }],
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn user_menu_picker_navigate_and_select() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: vec![
            app::user_menu::MenuEntry {
                hotkey: 'A',
                title: "Archive".to_string(),
                command: "echo archive".to_string(),
                condition: None,
            },
            app::user_menu::MenuEntry {
                hotkey: 'B',
                title: "Build".to_string(),
                command: "echo build".to_string(),
                condition: None,
            },
        ],
        ..Default::default()
    };

    // Navigate down
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    // Navigate up
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn user_menu_file_menu_no_menu_file_shows_error() {
    // Point the panel at a temp dir with no .mc.menu file
    let tmp = std::env::temp_dir();
    let mut terminal = test_terminal();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };
    state.left_panel.path = tmp;

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    // Should show an error dialog since no menu file exists
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
}

#[test]
fn user_menu_file_menu_with_entries_opens_picker() {
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let menu_path = tmp.path().join(".mc.menu");
    let mut f = std::fs::File::create(&menu_path).unwrap();
    write!(
        f,
        "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n"
    )
    .unwrap();

    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();

    handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
    assert_eq!(state.picker_selected, 0);
    assert_eq!(state.user_menu_entries.len(), 2);
    assert_eq!(state.user_menu_entries[0].hotkey, 'A');
    assert_eq!(state.user_menu_entries[1].hotkey, 'B');
}

#[test]
fn f2_loads_user_menu_file_with_entries() {
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let menu_path = tmp.path().join(".mc.menu");
    let mut f = std::fs::File::create(&menu_path).unwrap();
    write!(
        f,
        "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n"
    )
    .unwrap();

    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::F(2),
        KeyModifiers::NONE,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
    assert_eq!(state.user_menu_entries.len(), 2);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn f2_no_user_menu_file_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::F(2),
        KeyModifiers::NONE,
        24,
        &mut terminal,
    );

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
}

#[test]
fn compare_mode_picker_maps_index_to_mode() {
    // picker_selected 0 => Quick, 1 => Size, 2 => Thorough
    const MODES: [CompareMode; 3] = [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
    assert_eq!(MODES[0], CompareMode::Quick);
    assert_eq!(MODES[1], CompareMode::Size);
    assert_eq!(MODES[2], CompareMode::Thorough);
}

#[test]
fn compare_mode_picker_esc_cancels() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::CompareMode),
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn compare_mode_picker_enter_runs_quick_by_default() {
    let mut state = AppState::default();
    let tmp = std::env::temp_dir();
    state.left_panel.entries = vec![test_file_entry("a.txt", tmp.join("a.txt"))];
    state.mode = AppMode::ListPicker(PickerKind::CompareMode);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
            )
        ))
    );
}

#[test]
fn compare_mode_picker_navigate_and_select_thorough() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::CompareMode),
        picker_selected: 0,
        ..Default::default()
    };
    state.left_panel.entries = vec![{
        let mut cha = crate::fs::cha::Cha::dummy_dir();
        cha.len = 42;
        cha.mtime = Some(std::time::SystemTime::now());
        app::types::FileEntry::builder()
            .name("x.txt")
            .path(std::env::temp_dir().join("x.txt"))
            .cha(cha)
            .build()
    }];

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 2);

    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Thorough):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
            )
        ))
    );
}

#[test]
fn ctrl_alt_s_starts_search_mode() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
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
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.show_hidden = false;
    state.left_panel.cursor = 3;
    state.left_panel.scroll_offset = 2;

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Char('h'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.show_hidden);
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.left_panel.scroll_offset, 0);
}

#[test]
fn ctrl_alt_r_refreshes() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("existing.txt"), b"data").unwrap();
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.entries = vec![];
    assert!(state.left_panel.entries.is_empty());

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Char('r'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
        24,
        &mut terminal,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(
        state
            .left_panel
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
    state.left_panel.path = PathBuf::from("/tmp/left");
    state.right_panel.path = PathBuf::from("/tmp/right");
    state.active_panel = ActivePanel::Left;

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
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
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
    ];

    handle_normal_mode(
        &mut state,
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
    state.left_panel.entries = vec![make_test_entry("a.txt", 10, false)];
    state.left_panel.cursor = 0;

    handle_normal_mode(
        &mut state,
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
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
    ];

    handle_normal_mode(
        &mut state,
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
    state.left_panel.entries = vec![
        make_test_entry("a.txt", 10, false),
        make_test_entry("b.txt", 20, false),
    ];
    state.left_panel.cursor = 1;

    handle_normal_mode(
        &mut state,
        &mut None,
        KeyCode::Char('k'),
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 0);
}

fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
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
    let viewer_state: Option<viewer::ViewerState> = None;

    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
    assert!(rendered.contains("Test Error"));
    assert!(rendered.contains("Message"));
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
    let viewer_state: Option<viewer::ViewerState> = None;

    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state))
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
    let viewer_state: Option<viewer::ViewerState> = None;

    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
    assert!(rendered.contains("Command History"));
    assert!(rendered.contains("echo hello"));
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
    let viewer_state: Option<viewer::ViewerState> = None;

    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer_to_string(buffer);
    assert!(rendered.contains("TEST HELP"));
}

#[test]
fn check_overwrite_no_conflicts_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("new.txt"), b"hello").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("new.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_one_conflict_returns_some() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("clash.txt"), b"src").unwrap();
    std::fs::write(dest.join("clash.txt"), b"dest").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("clash.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["clash.txt"]);
}

#[test]
fn check_overwrite_all_conflicts_returns_all_names() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    std::fs::write(src.join("b.txt"), b"b").unwrap();
    std::fs::write(dest.join("a.txt"), b"a").unwrap();
    std::fs::write(dest.join("b.txt"), b"b").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("a.txt"), src.join("b.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.contains(&"a.txt".to_string()));
    assert!(conflicts.contains(&"b.txt".to_string()));
}

#[test]
fn check_overwrite_source_equals_dest_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("same.txt");
    std::fs::write(&file, b"data").unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![file],
            dest: tmp.path().to_path_buf(),
            overwrite: false,
        }),
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_broken_symlink_at_dest_is_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("link.txt"), b"src").unwrap();

    #[cfg(unix)]
    {
        let link_path = dest.join("link.txt");
        std::os::unix::fs::symlink("/nonexistent/broken", &link_path).unwrap();
    }

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("link.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    #[cfg(unix)]
    {
        let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
        assert_eq!(conflicts, vec!["link.txt"]);
    }
}
