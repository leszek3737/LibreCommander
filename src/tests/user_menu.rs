use super::helpers::*;
use crate::input::menu_actions::open_user_menu;
use crate::input::mode_dispatch::handle_menu_mode;
use crate::input::mode_dispatch::handle_normal_mode;
use crate::input::pickers;
use crossterm::event::{KeyCode, KeyModifiers};
use lc::app;
use lc::app::types::PickerKind;
use lc::app::types::{AppMode, AppState};
use lc::app::user_menu::MenuSource;
use lc::ui::viewer;
use std::path::Path;

const FILE_MENU_INDEX: usize = 1;

fn create_menu_file(dir: &Path) {
    use std::io::Write;
    let menu_path = dir.join(".mc.menu");
    let mut f = std::fs::File::create(&menu_path).unwrap();
    write!(
        f,
        "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n"
    )
    .unwrap();
}

fn test_menu_state(tmp: &tempfile::TempDir) -> AppState {
    let mut state = AppState {
        mode: AppMode::Menu,
        menu_selected: FILE_MENU_INDEX,
        menu_item_selected: 0,
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state
}

fn test_user_menu_entries() -> Vec<app::user_menu::MenuEntry> {
    vec![
        app::user_menu::MenuEntry {
            hotkey: 'A',
            title: "Archive".to_string(),
            command: "echo archive".to_string(),
            condition: app::user_menu::CompiledCondition::Always,
        },
        app::user_menu::MenuEntry {
            hotkey: 'B',
            title: "Build".to_string(),
            command: "echo build".to_string(),
            condition: app::user_menu::CompiledCondition::Always,
        },
    ]
}

fn single_menu_entry() -> Vec<app::user_menu::MenuEntry> {
    vec![app::user_menu::MenuEntry {
        hotkey: 'A',
        title: "Archive".to_string(),
        command: "tar czf a.tgz".to_string(),
        condition: app::user_menu::CompiledCondition::Always,
    }]
}

fn test_viewer_refs() -> (Option<viewer::ViewerState>, Option<viewer::ViewerLoader>) {
    (None, None)
}

#[test]
fn user_menu_picker_esc_closes() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: single_menu_entry(),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn user_menu_picker_navigate() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: test_user_menu_entries(),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn user_menu_picker_boundary_top_no_wrap() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: test_user_menu_entries(),
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn user_menu_picker_boundary_bottom_no_wrap() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: test_user_menu_entries(),
        ..Default::default()
    };
    state.picker_selected = 1;

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);
}

#[test]
fn user_menu_picker_enter_dismisses() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: vec![app::user_menu::MenuEntry {
            hotkey: 'A',
            title: "Archive".to_string(),
            command: "echo ok".to_string(),
            condition: app::user_menu::CompiledCondition::Always,
        }],
        user_menu_source: MenuSource::Local,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(state.user_menu_source, MenuSource::Local);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(_))
    ));
}

#[test]
fn user_menu_file_menu_no_menu_file_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = test_menu_state(&tmp);
    let (mut no_viewer, mut no_loader) = test_viewer_refs();

    handle_menu_mode(
        &mut state,
        &mut no_viewer,
        &mut no_loader,
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
    let tmp = tempfile::tempdir().unwrap();
    create_menu_file(tmp.path());
    let mut terminal = test_terminal();
    let mut state = test_menu_state(&tmp);
    let (mut no_viewer, mut no_loader) = test_viewer_refs();

    handle_menu_mode(
        &mut state,
        &mut no_viewer,
        &mut no_loader,
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
    let tmp = tempfile::tempdir().unwrap();
    create_menu_file(tmp.path());

    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    let (mut no_viewer, mut no_loader) = test_viewer_refs();

    handle_normal_mode(
        &mut state,
        &mut no_viewer,
        &mut no_loader,
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
    state.left_panel.set_path(tmp.path().to_path_buf());
    let (mut no_viewer, mut no_loader) = test_viewer_refs();

    handle_normal_mode(
        &mut state,
        &mut no_viewer,
        &mut no_loader,
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
fn empty_user_menu_no_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());

    open_user_menu(&mut state);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
}
