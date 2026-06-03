use super::helpers::*;
use crossterm::event::KeyCode;
use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use lc::app::types::{ActivePanel, AppMode, AppState, DialogKind, InputAction, TextInput};

#[test]
fn dispatch_resize_event_returns_true() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Resize(80, 24));

    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn dispatch_unhandled_event_returns_false() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::FocusGained);

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
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
        TestEntry::new("b.txt")
            .path(tmp.path().join("b.txt"))
            .build(),
    ]);
    state.left_panel.cursor = 1;

    let event = Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    let mut terminal = test_terminal();

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &event);

    assert!(result.is_ok());
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.active_panel, ActivePanel::Left);
}

#[test]
fn key_press_triggers_search_initiation() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(vec![
        TestEntry::new("alpha.txt")
            .path(tmp.path().join("alpha.txt"))
            .build(),
    ]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

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
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(vec![
        TestEntry::new("alpha.txt")
            .path(tmp.path().join("alpha.txt"))
            .build(),
    ]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

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
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
        TestEntry::new("b.txt")
            .path(tmp.path().join("b.txt"))
            .build(),
        TestEntry::new("c.txt")
            .path(tmp.path().join("c.txt"))
            .build(),
    ]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(result.is_ok());
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn key_repeat_text_edit_updates_input_dialog() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Create directory:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        dialog_input: TextInput {
            text: "ab".to_string(),
            cursor: 2,
        },
        ..Default::default()
    };
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Backspace, KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(result.is_ok());
    assert_eq!(state.dialog_input.text, "a");
    assert_eq!(state.dialog_input.cursor, 1);
}

#[test]
fn key_repeat_destructive_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("victim.txt"), b"x").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(vec![
        TestEntry::new("victim.txt")
            .path(tmp.path().join("victim.txt"))
            .build(),
    ]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::F(8), KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult {
        handled: result, ..
    } = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(result.is_ok());
    assert!(matches!(state.mode, AppMode::Normal));
    assert!(state.pending_action.is_none());
}

#[test]
fn dispatch_test_event_exposes_viewer_and_job() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let res = dispatch_test_event(&mut state, &mut terminal, &Event::FocusGained);

    assert!(res.viewer.is_none());
    assert!(res.job.is_none());
}
