use crate::input::pickers;
use crossterm::event::KeyCode;
use lc::app::types::{AppMode, AppState, PanelState, PickerKind};
use std::path::PathBuf;

fn hotlist_state(paths: &[&str]) -> AppState {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        ..Default::default()
    };
    state.ui.directory_hotlist = paths.iter().map(PathBuf::from).collect();
    state
}

fn with_history(mut state: AppState, cmds: &[&str]) -> AppState {
    for cmd in cmds {
        state.input.command_history.push_back(cmd.to_string());
    }
    state
}

fn picker_at(kind: PickerKind, selected: usize) -> AppState {
    let mut state = AppState {
        mode: AppMode::ListPicker(kind),
        ..Default::default()
    };
    state.ui.picker_selected = selected;
    state
}

#[test]
fn hotlist_picker_add_and_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    let mut state = AppState {
        left_panel: PanelState::new(tmp_path.clone()),
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        ..Default::default()
    };
    state.ui.directory_hotlist = vec![];

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));
    assert!(state.hotlist().contains(&tmp_path));
    assert_eq!(
        state.ui.status_message,
        Some("Added current directory to hotlist".to_string())
    );

    pickers::handle_list_picker(&mut state, KeyCode::Char('a'));
    assert!(state.hotlist().contains(&tmp_path));
    assert_eq!(
        state.hotlist().iter().filter(|p| **p == tmp_path).count(),
        1
    );
    assert_eq!(
        state.ui.status_message,
        Some("Directory already in hotlist".to_string())
    );
}

#[test]
fn hotlist_picker_delete_middle_and_last() {
    let mut state = hotlist_state(&["/a", "/b", "/c"]);
    state.ui.picker_selected = 1;

    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));
    assert_eq!(state.hotlist().len(), 2);
    assert!(!state.hotlist().contains(&PathBuf::from("/b")));

    state.ui.picker_selected = state.hotlist().len() - 1;
    pickers::handle_list_picker(&mut state, KeyCode::Char('d'));
    assert_eq!(state.hotlist().len(), 1);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn picker_wrap_empty_and_single() {
    let mut empty = picker_at(PickerKind::History, 0);
    pickers::handle_list_picker(&mut empty, KeyCode::Up);
    assert_eq!(empty.ui.picker_selected, 0);
    pickers::handle_list_picker(&mut empty, KeyCode::Down);
    assert_eq!(empty.ui.picker_selected, 0);

    let mut single = with_history(picker_at(PickerKind::History, 0), &["only"]);
    pickers::handle_list_picker(&mut single, KeyCode::Up);
    assert_eq!(single.ui.picker_selected, 0);
    pickers::handle_list_picker(&mut single, KeyCode::Down);
    assert_eq!(single.ui.picker_selected, 0);
}

#[test]
fn picker_three_items_no_wrap_at_bounds() {
    let mut state = with_history(picker_at(PickerKind::History, 0), &["a", "b", "c"]);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);

    state.ui.picker_selected = 2;
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 2);
}

#[test]
fn picker_escape_returns_normal() {
    let mut hist = with_history(picker_at(PickerKind::History, 0), &["cmd"]);
    pickers::handle_list_picker(&mut hist, KeyCode::Esc);
    assert_eq!(hist.mode, AppMode::Normal);

    let mut hot = hotlist_state(&["/a"]);
    pickers::handle_list_picker(&mut hot, KeyCode::Esc);
    assert_eq!(hot.mode, AppMode::Normal);
}

#[test]
fn list_picker_returns_early_if_not_list_picker_mode() {
    let mut state = AppState {
        mode: AppMode::Normal,
        ..Default::default()
    };
    state.ui.status_message = Some("preserved".to_string());
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.ui.status_message, Some("preserved".to_string()));
}

#[test]
fn history_picker_home_end() {
    let mut state = with_history(picker_at(PickerKind::History, 2), &["a", "b", "c"]);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.ui.picker_selected, 0);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.ui.picker_selected, 2);
}

#[test]
fn hotlist_picker_home_end() {
    let mut state = hotlist_state(&["/a", "/b", "/c"]);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.ui.picker_selected, 2);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn archive_menu_picker_esc_returns_normal() {
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);
    pickers::handle_list_picker(&mut state, KeyCode::Esc);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn archive_menu_picker_navigate_bounds() {
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn archive_menu_picker_home_end() {
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.ui.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.ui.picker_selected, 0);
}
