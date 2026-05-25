use crate::input::pickers;
use crate::*;
use app::config::{PersistedPanel, PersistedSetup};
use app::types::PickerKind;
use std::path::PathBuf;

#[test]
fn hotlist_picker_add_current_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.directory_hotlist.clear();
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert!(state.directory_hotlist.contains(&tmp.path().to_path_buf()));
}

#[test]
fn hotlist_picker_add_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
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
        hotlist: Some(hotlist.clone()),
    };

    let serialized = toml::to_string(&setup).unwrap();
    let deserialized: PersistedSetup = toml::from_str(&serialized).unwrap();

    assert_eq!(deserialized.hotlist, Some(hotlist));
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
fn picker_wrap_empty_list_does_nothing() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        picker_selected: 0,
        ..Default::default()
    };
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
fn empty_hotlist_does_not_open_picker() {
    let mut state = AppState::default();
    state.directory_hotlist.clear();
    state.mode = AppMode::ListPicker(PickerKind::Hotlist);
    state.picker_selected = 0;
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
}
