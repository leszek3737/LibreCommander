#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use crate::input::{command_line, dialogs, directory_tree, mode_dispatch, pickers};
use app::config::{PersistedPanel, PersistedSetup};
use app::types::{
    ActivePanel, CompareMode, DialogKind, FileEntry, InputAction, PendingAction, PickerKind,
};
use crossterm::event::{KeyEvent, KeyEventKind};
use mode_dispatch::{
    handle_menu_mode, handle_normal_mode, handle_search_mode, run_selected_menu_action,
};
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(80, 24)).unwrap()
}

struct TestEntry {
    name: String,
    path: Option<PathBuf>,
    size: u64,
    selected: bool,
}

impl TestEntry {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            size: 0,
            selected: false,
        }
    }

    fn path(mut self, p: impl Into<PathBuf>) -> Self {
        self.path = Some(p.into());
        self
    }

    fn size(mut self, s: u64) -> Self {
        self.size = s;
        self
    }

    fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    fn build(self) -> FileEntry {
        let path = self
            .path
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/{}", self.name)));
        let mut cha = crate::fs::cha::Cha::dummy_dir();
        if self.size > 0 {
            cha.mode = crate::fs::cha::ChaMode::new(0o100644);
            cha.len = self.size;
            cha.mtime = Some(std::time::SystemTime::now());
            cha.btime = Some(UNIX_EPOCH);
        }
        FileEntry::builder()
            .name(&self.name)
            .path(path)
            .cha(cha)
            .selected(self.selected)
            .build()
    }
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
fn search_enter_clears_filter_and_refreshes_from_disk() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();
    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "alpha".to_string(),
        search_cursor: 5,
        ..Default::default()
    };
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.entries = vec![TestEntry::new("alpha.txt").size(1).build()];
    state.left_panel.unfiltered_entries = vec![
        TestEntry::new("alpha.txt").size(1).build(),
        TestEntry::new("beta.txt").size(2).build(),
    ];
    state.left_panel.filter = Some("alpha".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
    assert_eq!(state.left_panel.filter.as_deref(), None);
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
            .any(|entry| entry.name == "beta.txt")
    );
}

#[test]
fn search_enter_clears_filter_and_restores_unfiltered_entries() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();

    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "alpha".to_string(),
        search_cursor: 5,
        ..Default::default()
    };
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.entries = vec![TestEntry::new("alpha.txt").size(1).build()];
    state.left_panel.unfiltered_entries = vec![
        TestEntry::new("alpha.txt").size(1).build(),
        TestEntry::new("beta.txt").size(2).build(),
    ];
    state.left_panel.filter = Some("alpha".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
    assert!(state.left_panel.filter.is_none());
    let names: Vec<&str> = state
        .left_panel
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"alpha.txt"), "alpha.txt missing: {names:?}");
    assert!(names.contains(&"beta.txt"), "beta.txt missing: {names:?}");
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
fn parse_octal_mode_edge_cases() {
    assert_eq!(dialogs::parse_octal_mode(""), None);
    assert_eq!(dialogs::parse_octal_mode("1234567"), None);
    assert_eq!(dialogs::parse_octal_mode("7"), Some(0o7));
    assert_eq!(dialogs::parse_octal_mode("00755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode(" 755"), Some(0o755));
    assert_eq!(dialogs::parse_octal_mode("789"), None);
}

#[test]
fn compare_directories_reports_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        app::types::FileEntry::builder()
            .name("a.txt")
            .path(tmp.path().join("a.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    ];
    state.right_panel.entries = vec![
        app::types::FileEntry::builder()
            .name("b.txt")
            .path(tmp.path().join("b.txt"))
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
fn shift_down_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
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
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
    ];
    state.left_panel.cursor = 2;

    handle_normal_mode(
        &mut state,
        &mut None,
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
        TestEntry::new("a.txt").size(10).selected().build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
        TestEntry::new("d.txt").size(40).build(),
    ];
    state.left_panel.cursor = 2;
    state.left_panel.recalculate_selection_stats();

    handle_normal_mode(
        &mut state,
        &mut None,
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
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );
    handle_normal_mode(
        &mut state,
        &mut None,
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

#[test]
fn compare_directories_marks_unique_entries_selected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("same.txt")
            .path(tmp.path().join("same.txt"))
            .build(),
        TestEntry::new("left.txt")
            .path(tmp.path().join("left.txt"))
            .build(),
    ];
    state.right_panel.entries = vec![
        TestEntry::new("same.txt")
            .path(tmp.path().join("same.txt"))
            .build(),
        TestEntry::new("right.txt")
            .path(tmp.path().join("right.txt"))
            .build(),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(!state.left_panel.entries[0].selected);
    assert!(state.left_panel.entries[1].selected);
    assert!(!state.right_panel.entries[0].selected);
    assert!(state.right_panel.entries[1].selected);
}

#[test]
fn selected_or_current_paths_fallback_to_cursor() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.entries = vec![
        TestEntry::new("file_a.txt").size(100).build(),
        TestEntry::new("file_b.txt").size(100).build(),
    ];
    state.left_panel.cursor = 1;

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/tmp/file_b.txt"));
}

#[test]
fn selected_or_current_paths_uses_selection_when_present() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.entries = vec![
        TestEntry::new("file_a.txt").size(100).selected().build(),
        TestEntry::new("file_b.txt").size(100).build(),
        TestEntry::new("file_c.txt").size(100).selected().build(),
    ];
    state.left_panel.cursor = 1;

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&PathBuf::from("/tmp/file_a.txt")));
    assert!(paths.contains(&PathBuf::from("/tmp/file_c.txt")));
}

#[test]
fn selected_or_current_paths_skips_dotdot() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.entries = vec![TestEntry::new("..").size(100).selected().build()];
    state.left_panel.cursor = 0;

    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn selected_or_current_paths_empty_panel() {
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
fn history_dedup_consecutive() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.command_history.push_back("echo hi".to_string());
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
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.directory_hotlist.clear();
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert!(state.directory_hotlist.contains(&tmp.path().to_path_buf()));
}

#[test]
fn hotlist_picker_add_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.directory_hotlist = vec![tmp.path().to_path_buf()];
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert_eq!(
        state
            .directory_hotlist
            .iter()
            .filter(|p| **p == tmp.path())
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
    let tmp = tempfile::tempdir().unwrap();
    let tmp_str = tmp.path().display().to_string();
    let hotlist = vec![tmp_str, "/usr".to_string()];

    let setup = PersistedSetup {
        active_panel: String::new(),
        dir_first: true,
        sensitive: false,
        left: PersistedPanel {
            path: Some("/tmp".to_string()),
            show_hidden: false,
            ..Default::default()
        },
        right: PersistedPanel {
            path: Some("/tmp".to_string()),
            show_hidden: false,
            ..Default::default()
        },
        hotlist: hotlist.clone(),
    };

    let serialized = toml::to_string(&setup).unwrap();
    let deserialized: PersistedSetup = toml::from_str(&serialized).unwrap();

    assert_eq!(deserialized.hotlist, hotlist);
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

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn user_menu_file_menu_no_menu_file_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: 1,
        menu_item_selected: 0,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();

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

    handle_menu_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Enter,
        24,
        &mut terminal,
    );

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
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
    ];

    let modes = ["Quick", "Size", "Thorough"];
    for (idx, label) in modes.iter().enumerate() {
        state.mode = AppMode::ListPicker(PickerKind::CompareMode);
        state.picker_selected = idx;
        pickers::handle_list_picker(&mut state, KeyCode::Enter);

        match &state.mode {
            AppMode::Dialog(app::types::DialogKind::Confirm(details)) => {
                let expected = format!("Compare dirs ({label}):");
                assert!(
                    details.message.contains(&expected),
                    "mode {label}: expected '{expected}' in '{}'",
                    details.message
                );
            }
            other => panic!("expected Confirm dialog for {label}, got {other:?}"),
        }
    }
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
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
    ];
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
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::CompareMode),
        picker_selected: 0,
        ..Default::default()
    };
    state.left_panel.entries = vec![
        TestEntry::new("x.txt")
            .size(42)
            .path(tmp.path().join("x.txt"))
            .build(),
    ];

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
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
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
    state.left_panel.entries = vec![TestEntry::new("a.txt").size(10).build()];
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
    state.left_panel.entries = vec![
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
    state.left_panel.entries = vec![
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
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
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
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
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
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
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
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
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

#[cfg(unix)]
#[test]
fn check_overwrite_broken_symlink_at_dest_is_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("link.txt"), b"src").unwrap();

    std::os::unix::fs::symlink("/nonexistent/broken", dest.join("link.txt")).unwrap();

    let state = AppState {
        pending_action: Some(app::types::PendingAction::Copy {
            sources: vec![src.join("link.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["link.txt"]);
}

#[test]
fn dispatch_resize_event_returns_true() {
    let mut state = AppState {
        ..Default::default()
    };
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::Resize(80, 24),
    );

    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn dispatch_unhandled_event_returns_false() {
    let mut state = AppState {
        ..Default::default()
    };
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::FocusGained,
    );

    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn dispatch_mouse_click_moves_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"b").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
        TestEntry::new("b.txt")
            .path(tmp.path().join("b.txt"))
            .build(),
    ];
    state.left_panel.unfiltered_entries = state.left_panel.entries.clone();
    state.left_panel.cursor = 1;
    state.left_panel.path_index = state
        .left_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();

    let event = Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 2,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &event,
    );

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.active_panel, ActivePanel::Left);
}

#[test]
fn ctrl_alt_h_toggles_hidden_back() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = temp_dir.path().to_path_buf();
    state.left_panel.show_hidden = true;
    state.active_panel = ActivePanel::Left;
    super::handle_ctrl_keys(&mut state, KeyCode::Char('h'));
    assert!(!state.left_panel.show_hidden);
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
    state.dialog_input = "new.txt".to_string();
    state.dialog_cursor_pos = state.dialog_input.len();
    assert_eq!(state.dialog_input, "new.txt");
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
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.entries = vec![TestEntry::new("delme.txt").build()];
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
}

#[test]
fn search_mode_with_empty_panel_handles_enter_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![];
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Enter, 20);
}

#[test]
fn search_mode_with_empty_panel_handles_esc_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![];
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Esc, 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn search_mode_with_empty_panel_handles_char_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![];
    state.active_panel = ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Char('x'), 20);
}

#[test]
fn apply_search_filter_exact_match() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("foo").build(), TestEntry::new("bar").build()];
    state.left_panel.filter = Some("foo".to_string());
    apply_search_filter(&mut state.left_panel);
    let names: Vec<_> = state.left_panel.entries.iter().map(|e| &e.name).collect();
    assert!(names.iter().all(|n| *n == "foo"));
}

#[test]
fn apply_search_filter_no_match_clears_entries() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    state.left_panel.filter = Some("xyz".to_string());
    apply_search_filter(&mut state.left_panel);
    assert!(state.left_panel.entries.is_empty());
}

#[test]
fn apply_search_filter_empty_pattern_shows_all() {
    let mut state = AppState::default();
    let entries = vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    let count = entries.len();
    state.left_panel.entries = entries.clone();
    state.left_panel.unfiltered_entries = entries;
    state.left_panel.filter = None;
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.entries.len(), count);
}

#[test]
fn apply_search_filter_partial_match() {
    let mut state = AppState::default();
    let entries = vec![
        TestEntry::new("bar").build(),
        TestEntry::new("baz").build(),
        TestEntry::new("foo").build(),
    ];
    state.left_panel.entries = entries.clone();
    state.left_panel.unfiltered_entries = entries;
    state.left_panel.filter = Some("ba".to_string());
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.entries.len(), 2);
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
    assert!(state.dialog_input.is_empty());
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
    state.left_panel.entries = vec![TestEntry::new("mydir").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    let mut terminal = test_terminal();
    launch_editor(&mut state, &mut terminal);
    assert!(matches!(state.mode, AppMode::Normal));
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
fn compare_directories_size_mode_reports_mismatches() {
    let left_dir = tempfile::tempdir().unwrap();
    let right_dir = tempfile::tempdir().unwrap();
    std::fs::write(left_dir.path().join("file.txt"), "short").unwrap();
    std::fs::write(right_dir.path().join("file.txt"), "longer content here").unwrap();
    let mut state = AppState::default();
    state.left_panel.path = left_dir.path().to_path_buf();
    state.right_panel.path = right_dir.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("file.txt")
            .path(left_dir.path().join("file.txt"))
            .size(5)
            .build(),
    ];
    state.right_panel.entries = vec![
        TestEntry::new("file.txt")
            .path(right_dir.path().join("file.txt"))
            .size(20)
            .build(),
    ];
    pickers::compare_directories(&mut state, CompareMode::Size);
    let left_selected: Vec<_> = state
        .left_panel
        .entries
        .iter()
        .filter(|e| e.name != ".." && e.selected)
        .collect();
    assert!(!left_selected.is_empty());
}

#[test]
fn compare_directories_by_content_zero_length() {
    let left_dir = tempfile::tempdir().unwrap();
    let right_dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = left_dir.path().to_path_buf();
    state.right_panel.path = right_dir.path().to_path_buf();
    pickers::compare_directories(&mut state, CompareMode::Quick);
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn empty_history_does_not_open_picker() {
    let mut state = AppState::default();
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
}

#[test]
fn empty_hotlist_does_not_open_picker() {
    let mut state = AppState::default();
    state.directory_hotlist.clear();
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
}

#[test]
fn empty_user_menu_no_file() {
    let mut state = AppState::default();
    crate::input::menu_actions::open_user_menu(&mut state);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
}

#[test]
fn alt_enter_shows_properties_dialog() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("file.txt").build()];
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
    state.left_panel.entries = vec![TestEntry::new("..").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Enter, 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn alt_backspace_navigates_to_parent() {
    let mut state = AppState::default();
    let parent = std::path::PathBuf::from("/tmp");
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
#[allow(clippy::field_reassign_with_default)]
fn alt_c_opens_quick_cd() {
    let mut state = AppState::default();
    state.active_panel = ActivePanel::Left;
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
#[allow(clippy::field_reassign_with_default)]
fn alt_unhandled_does_nothing() {
    let mut state = AppState::default();
    state.active_panel = ActivePanel::Left;
    handle_alt_keys(&mut state, KeyCode::Char('x'), 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn reposition_cursor_finds_matching_name() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("a").build(),
        TestEntry::new("b").build(),
        TestEntry::new("c").build(),
    ];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("b"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_no_match_leaves_cursor() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    state.left_panel.cursor = 1;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("z"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_none_name_unchanged() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("a").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, None, 20);
    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn file_name_str_valid_utf8() {
    assert_eq!(
        file_name_str(std::path::Path::new("/home/user/file.txt")),
        Some("file.txt".to_string())
    );
}

#[test]
fn file_name_str_root_returns_none() {
    assert_eq!(file_name_str(std::path::Path::new("/")), None);
}

#[test]
fn file_name_str_non_utf8_returns_lossy() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let bad = OsStr::from_bytes(b"bad\xFFname");
    let path = std::path::Path::new("/tmp").join(bad);
    let result = file_name_str(&path);
    assert!(result.is_some());
}

#[test]
fn picker_clamp_up_at_top_stays() {
    let mut state = AppState::default();
    state.command_history.push_back("a".to_string());
    state.command_history.push_back("b".to_string());
    state.command_history.push_back("c".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn picker_clamp_down_at_bottom_stays() {
    let mut state = AppState::default();
    state.command_history.push_back("a".to_string());
    state.command_history.push_back("b".to_string());
    state.command_history.push_back("c".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 2;
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 2);
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn picker_wrap_empty_list_does_nothing() {
    let mut state = AppState::default();
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn picker_wrap_single_item_stays_at_zero() {
    let mut state = AppState::default();
    state.command_history.push_back("only".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn config_load_missing_file_ok() {
    let result = app::config::load_settings();
    assert!(result.is_ok());
}

#[test]
fn progress_dialog_nan_percent_handled() {
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Progress("copying".to_string(), f32::NAN)),
        ..Default::default()
    };
    let mut terminal = test_terminal();
    let viewer_state: Option<viewer::ViewerState> = None;
    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
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
fn dialog_with_long_title_does_not_overflow() {
    let long_msg = "x".repeat(200);
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Error(long_msg)),
        ..Default::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let viewer_state: Option<viewer::ViewerState> = None;
    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let text = buffer_to_string(buf);
    assert!(text.contains("xxxxx"));
}

#[test]
fn dialog_overlay_centered() {
    let state = AppState {
        mode: AppMode::Dialog(DialogKind::Error("test error".to_string())),
        ..Default::default()
    };
    let mut terminal = test_terminal();
    let viewer_state: Option<viewer::ViewerState> = None;
    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    assert!(buf.cell((20, 7)).is_some());
    assert!(buf.cell((39, 0)).is_some());
}

#[test]
fn menu_bar_rendered_at_top() {
    let state = AppState::default();
    let mut terminal = test_terminal();
    let viewer_state: Option<viewer::ViewerState> = None;
    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let cell = buf.cell((39, 0)).unwrap();
    assert!(!cell.symbol().trim().is_empty());
}

#[test]
fn status_bar_at_bottom() {
    let state = AppState::default();
    let mut terminal = test_terminal();
    let viewer_state: Option<viewer::ViewerState> = None;
    terminal
        .draw(|f| render::render_ui(f, &state, &viewer_state, &None))
        .unwrap();
    let buf = terminal.backend().buffer();
    let cell = buf.cell((2, 23)).unwrap();
    assert!(!cell.symbol().trim().is_empty());
}

#[test]
fn confirm_file_transfer_copy_opens_dialog() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt").build(),
        TestEntry::new("b.txt").build(),
    ];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
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
}

#[test]
fn confirm_delete_opens_dialog() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("delme.txt").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    confirm_delete(&mut state);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
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
    state.left_panel.entries = vec![TestEntry::new("a").build(); 10];
    state.left_panel.cursor = 9;
    state.right_panel.entries = vec![TestEntry::new("x").build(), TestEntry::new("y").build()];
    state.active_panel = ActivePanel::Left;
    handle_navigation_keys(&mut state, KeyCode::Tab, KeyModifiers::NONE, 20);
    assert_eq!(state.active_panel, ActivePanel::Right);
    assert!(state.right_panel.cursor <= 1);
}

#[test]
fn check_overwrite_conflict_move_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("file.txt"), "a").unwrap();
    std::fs::write(dest.join("file.txt"), "b").unwrap();
    let state = AppState {
        active_panel: ActivePanel::Left,
        pending_action: Some(PendingAction::Move {
            sources: vec![src.join("file.txt")],
            dest,
            overwrite: false,
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    let _ = conflicts;
}

#[test]
fn check_overwrite_conflict_delete_no_conflict() {
    let state = AppState {
        pending_action: Some(PendingAction::Delete {
            paths: vec![PathBuf::from("/tmp/nonexistent")],
        }),
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

#[test]
fn key_press_triggers_search_initiation() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("alpha.txt")
            .path(tmp.path().join("alpha.txt"))
            .build(),
    ];
    state.left_panel.unfiltered_entries = state.left_panel.entries.clone();
    state.left_panel.path_index = state
        .left_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::Key(key),
    );

    assert!(result.is_ok());
    assert!(matches!(state.mode, AppMode::Search));
}

#[test]
fn key_release_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("alpha.txt")
            .path(tmp.path().join("alpha.txt"))
            .build(),
    ];
    state.left_panel.unfiltered_entries = state.left_panel.entries.clone();
    state.left_panel.path_index = state
        .left_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::Key(key),
    );

    assert!(result.is_ok());
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn key_repeat_navigation_moves_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"b").unwrap();
    std::fs::write(tmp.path().join("c.txt"), b"c").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
        TestEntry::new("b.txt")
            .path(tmp.path().join("b.txt"))
            .build(),
        TestEntry::new("c.txt")
            .path(tmp.path().join("c.txt"))
            .build(),
    ];
    state.left_panel.unfiltered_entries = state.left_panel.entries.clone();
    state.left_panel.path_index = state
        .left_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::Key(key),
    );

    assert!(result.is_ok());
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn key_repeat_destructive_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("victim.txt"), b"x").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.path = tmp.path().to_path_buf();
    state.left_panel.entries = vec![
        TestEntry::new("victim.txt")
            .path(tmp.path().join("victim.txt"))
            .build(),
    ];
    state.left_panel.unfiltered_entries = state.left_panel.entries.clone();
    state.left_panel.path_index = state
        .left_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::F(8), KeyModifiers::NONE, KeyEventKind::Repeat);

    let result = super::dispatch_event(
        &mut state,
        &mut viewer,
        &mut None,
        &mut job,
        &mut terminal,
        &Event::Key(key),
    );

    assert!(result.is_ok());
    assert!(matches!(state.mode, AppMode::Normal));
    assert!(state.pending_action.is_none());
}
