use super::helpers::*;
use crate::input::menu_actions::open_user_menu;
use crate::input::mode_dispatch::handle_menu_mode;
use crate::input::mode_dispatch::handle_normal_mode;
use crate::input::pickers;
use crossterm::event::{KeyCode, KeyModifiers};
use lc::app;
use lc::app::types::PickerKind;
use lc::app::types::{AppMode, AppState, UiState};
use lc::app::user_menu::MenuSource;
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

/// Create a `.mc.menu` file that exists but contains no usable entries (only a
/// comment), exercising the "file present but empty" path distinct from the
/// "no file" path.
fn create_empty_menu_file(dir: &Path) {
    use std::io::Write;
    let menu_path = dir.join(".mc.menu");
    let mut f = std::fs::File::create(&menu_path).unwrap();
    writeln!(f, "# only a comment, no entries").unwrap();
}

fn test_menu_state(tmp: &tempfile::TempDir) -> AppState {
    let mut state = AppState {
        mode: AppMode::Menu,
        ui: UiState {
            menu_selected: FILE_MENU_INDEX,
            menu_item_selected: 0,
            ..Default::default()
        },
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

/// Build an [`AppState`] in `mode` with the given user-menu `entries` already
/// loaded. Collapses the repeated `AppState { mode, ui: UiState { .. } }`
/// literal used by the list-picker tests below.
fn create_test_state_with_mode(mode: AppMode, entries: Vec<app::user_menu::MenuEntry>) -> AppState {
    AppState {
        mode,
        ui: UiState {
            user_menu_entries: entries,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn user_menu_picker_esc_closes() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        single_menu_entry(),
    );

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn user_menu_picker_navigate() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        test_user_menu_entries(),
    );

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn user_menu_picker_boundary_top_no_wrap() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        test_user_menu_entries(),
    );

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn user_menu_picker_boundary_bottom_no_wrap() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        test_user_menu_entries(),
    );
    state.ui.picker_selected = 1;

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);
}

#[test]
fn user_menu_picker_enter_dismisses() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        vec![app::user_menu::MenuEntry {
            hotkey: 'A',
            title: "Archive".to_string(),
            command: "echo ok".to_string(),
            condition: app::user_menu::CompiledCondition::Always,
        }],
    );
    state.ui.user_menu_source = MenuSource::Local;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(state.ui.user_menu_source, MenuSource::Local);
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
        term_size: ratatui::layout::Size::new(80, 24),
    };
    handle_menu_mode(&mut ctx, KeyCode::Enter, &mut terminal);

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
        term_size: ratatui::layout::Size::new(80, 24),
    };
    handle_menu_mode(&mut ctx, KeyCode::Enter, &mut terminal);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
    assert_eq!(state.ui.picker_selected, 0);
    assert_eq!(state.ui.user_menu_entries.len(), 2);
    assert_eq!(state.ui.user_menu_entries[0].hotkey, 'A');
    assert_eq!(state.ui.user_menu_entries[0].title, "Archive");
    assert_eq!(state.ui.user_menu_entries[0].command, "tar czf a.tgz");
    assert_eq!(state.ui.user_menu_entries[1].hotkey, 'B');
    assert_eq!(state.ui.user_menu_entries[1].title, "Build");
    assert_eq!(state.ui.user_menu_entries[1].command, "cargo build");
}

#[test]
fn f2_loads_user_menu_file_with_entries() {
    let tmp = tempfile::tempdir().unwrap();
    create_menu_file(tmp.path());

    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
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
        term_size: ratatui::layout::Size::new(80, 24),
    };
    handle_normal_mode(&mut ctx, KeyCode::F(2), KeyModifiers::NONE, &mut terminal);

    assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
    assert_eq!(state.ui.user_menu_entries.len(), 2);
    assert_eq!(state.ui.user_menu_entries[0].hotkey, 'A');
    assert_eq!(state.ui.user_menu_entries[0].title, "Archive");
    assert_eq!(state.ui.user_menu_entries[0].command, "tar czf a.tgz");
    assert_eq!(state.ui.user_menu_entries[1].hotkey, 'B');
    assert_eq!(state.ui.user_menu_entries[1].title, "Build");
    assert_eq!(state.ui.user_menu_entries[1].command, "cargo build");
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn f2_no_user_menu_file_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
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
        term_size: ratatui::layout::Size::new(80, 24),
    };
    handle_normal_mode(&mut ctx, KeyCode::F(2), KeyModifiers::NONE, &mut terminal);

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

#[test]
fn empty_user_menu_file_with_no_entries_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    create_empty_menu_file(tmp.path());
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());

    open_user_menu(&mut state);

    // A `.mc.menu` exists but has no entries: stay out of the picker and report
    // the error rather than opening an empty list.
    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Error(_))
    ));
    assert!(state.ui.user_menu_entries.is_empty());
}

#[test]
fn user_menu_picker_enter_out_of_bounds_selection_is_clamped() {
    let mut state = create_test_state_with_mode(
        AppMode::ListPicker(PickerKind::UserMenu),
        single_menu_entry(),
    );
    // Local source routes Enter through the confirmation dialog instead of
    // spawning a shell, keeping the test deterministic and side-effect free.
    state.ui.user_menu_source = MenuSource::Local;
    // Selection points past the single entry (index 5 vs len 1).
    state.ui.picker_selected = 5;

    // Must not panic; the handler clamps `picker_selected` to the last valid
    // entry and acts on it.
    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert!(matches!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(_))
    ));
}
