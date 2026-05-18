use super::helpers::*;
use crate::input::mode_dispatch::handle_menu_mode;
use crate::input::mode_dispatch::handle_normal_mode;
use crate::input::pickers;
use crate::*;
use app::types::PickerKind;

#[test]
fn user_menu_picker_esc_closes() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::UserMenu),
        user_menu_entries: vec![app::user_menu::MenuEntry {
            hotkey: 'A',
            title: "Archive".to_string(),
            command: "tar czf a.tgz".to_string(),
            condition: app::user_menu::CompiledCondition::Always,
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
                condition: app::user_menu::CompiledCondition::Always,
            },
            app::user_menu::MenuEntry {
                hotkey: 'B',
                title: "Build".to_string(),
                command: "echo build".to_string(),
                condition: app::user_menu::CompiledCondition::Always,
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
    state.left_panel.set_path(tmp.path().to_path_buf());

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
    state.left_panel.set_path(tmp.path().to_path_buf());

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
    state.left_panel.set_path(tmp.path().to_path_buf());

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
    state.left_panel.set_path(tmp.path().to_path_buf());

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
fn empty_user_menu_no_file() {
    let mut state = AppState::default();
    crate::input::menu_actions::open_user_menu(&mut state);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
}
