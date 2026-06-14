use super::helpers::*;
use crossterm::event::KeyCode;
use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use lc::app::types::{ActivePanel, AppMode, AppState, DialogKind, InputAction, TextInput};

fn panel_with_files(names: &[&str]) -> (tempfile::TempDir, AppState) {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.set_entries(
        names
            .iter()
            .map(|n| TestEntry::new(*n).path(tmp.path().join(n)).build())
            .collect(),
    );
    (tmp, state)
}

#[test]
fn dispatch_resize_event_returns_true() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Resize(80, 24));

    assert_eq!(handled, Ok(true));
}

#[test]
fn dispatch_unhandled_event_returns_false() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::FocusGained);

    assert_eq!(handled, Ok(false));
}

#[test]
fn dispatch_mouse_click_moves_cursor() {
    let (tmp, mut state) = panel_with_files(&["a.txt", "b.txt"]);
    std::fs::write(tmp.path().join("a.txt"), b"a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"b").unwrap();
    state.left_panel.cursor = 1;

    // row=2, col=2 maps to first visible entry (index 0)
    let event = Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    let mut terminal = test_terminal();

    let DispatchResult { handled, .. } = dispatch_test_event(&mut state, &mut terminal, &event);

    assert!(handled.is_ok());
    assert_eq!(state.left_panel.cursor, 0);
    assert_eq!(state.active_panel, ActivePanel::Left);
}

#[test]
fn key_press_triggers_search_initiation() {
    let (_tmp, mut state) = panel_with_files(&["alpha.txt"]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Press);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(handled.is_ok());
    assert!(matches!(state.mode, AppMode::Search));
}

#[test]
fn key_release_is_ignored() {
    let (_tmp, mut state) = panel_with_files(&["alpha.txt"]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(handled.is_ok());
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn key_repeat_navigation_moves_cursor() {
    let (tmp, mut state) = panel_with_files(&["a.txt", "b.txt", "c.txt"]);
    std::fs::write(tmp.path().join("a.txt"), b"a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"b").unwrap();
    std::fs::write(tmp.path().join("c.txt"), b"c").unwrap();
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(handled.is_ok());
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn key_repeat_text_edit_updates_input_dialog() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Create directory:".to_string(),
            action: InputAction::CreateDirectory,
        }),
        ..Default::default()
    };
    state.input.dialog_input = {
        let mut ti = TextInput::new();
        ti.set_text("ab".to_string());
        ti.set_cursor(2);
        ti
    };
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Backspace, KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(handled.is_ok());
    assert_eq!(state.input.dialog_input.text(), "a");
    assert_eq!(state.input.dialog_input.cursor(), 1);
}

#[test]
fn key_repeat_destructive_is_ignored() {
    let (tmp, mut state) = panel_with_files(&["victim.txt"]);
    std::fs::write(tmp.path().join("victim.txt"), b"x").unwrap();
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::F(8), KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(handled.is_ok());
    assert!(matches!(state.mode, AppMode::Normal));
    assert!(state.ui.pending_action.is_none());
}

#[test]
fn dispatch_test_event_exposes_viewer_and_job() {
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let res = dispatch_test_event(&mut state, &mut terminal, &Event::FocusGained);

    assert!(res.viewer.is_none());
    assert!(res.job.is_none());
}

#[test]
fn non_printable_keys_do_not_trigger_search() {
    for code in [KeyCode::F(1), KeyCode::Esc] {
        let (_tmp, mut state) = panel_with_files(&["alpha.txt"]);
        let mut terminal = test_terminal();
        let key = KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press);
        let DispatchResult { handled, .. } =
            dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

        assert!(handled.is_ok());
        assert!(
            !matches!(state.mode, AppMode::Search),
            "{code:?} should not trigger search mode"
        );
    }
}
