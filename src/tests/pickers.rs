use crate::input::pickers;
use crossterm::event::KeyCode;
use lc::app::types::{AppMode, AppState, PanelState, PickerKind};
use std::path::PathBuf;

#[test]
fn hotlist_picker_add_current_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    let mut state = AppState {
        left_panel: PanelState::new(tmp_path.clone()),
        directory_hotlist: vec![],
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert!(state.hotlist().contains(&tmp_path));
    assert_eq!(
        state.status_message,
        Some("Added current directory to hotlist".to_string())
    );
}

#[test]
fn hotlist_picker_add_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    let mut state = AppState {
        left_panel: PanelState::new(tmp_path.clone()),
        directory_hotlist: vec![tmp_path],
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));

    assert_eq!(
        state
            .hotlist()
            .iter()
            .filter(|p| p.as_path() == tmp.path())
            .count(),
        1
    );
    assert_eq!(
        state.status_message,
        Some("Directory already in hotlist".to_string())
    );
}

#[test]
fn hotlist_picker_delete_entry() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        directory_hotlist: vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ],
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));

    assert_eq!(state.hotlist().len(), 2);
    assert!(!state.hotlist().contains(&PathBuf::from("/b")));
}

#[test]
fn hotlist_picker_delete_adjusts_cursor() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        directory_hotlist: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));

    assert_eq!(state.hotlist().len(), 1);
    assert_eq!(state.picker_selected, 0);
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
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        picker_selected: 0,
        ..Default::default()
    };
    state.command_history.push_back("only".to_string());
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn list_picker_returns_early_if_not_list_picker_mode() {
    let mut state = AppState {
        mode: AppMode::Normal,
        status_message: Some("preserved".to_string()),
        ..Default::default()
    };
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.status_message, Some("preserved".to_string()));
}

#[test]
fn history_picker_home_end() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        picker_selected: 2,
        ..Default::default()
    };
    state.command_history.push_back("a".to_string());
    state.command_history.push_back("b".to_string());
    state.command_history.push_back("c".to_string());

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.picker_selected, 0);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.picker_selected, 2);
}

#[test]
fn hotlist_picker_home_end() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        directory_hotlist: vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ],
        picker_selected: 0,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.picker_selected, 2);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn archive_menu_picker_esc_returns_normal() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::ArchiveMenu),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Esc);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn archive_menu_picker_navigate_bounds() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::ArchiveMenu),
        picker_selected: 0,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn archive_menu_picker_home_end() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::ArchiveMenu),
        picker_selected: 0,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.picker_selected, 0);
}
