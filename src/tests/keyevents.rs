use super::helpers::*;
use crossterm::event::KeyCode;
use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use lc::app::types::{
    ActivePanel, AppMode, AppState, ConfirmDetails, DialogKind, InputAction, PendingAction,
    TextInput,
};

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
    // Cursor positioning is pure logic over the in-memory entries, so no
    // on-disk files are needed for this test.
    let (_tmp, mut state) = panel_with_files(&["a.txt", "b.txt"]);
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

    // The click is consumed as a side effect (cursor move) but the mouse
    // handler reports it as not producing a follow-up key event, so dispatch
    // returns Ok(false).
    assert_eq!(handled, Ok(false));
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

    assert_eq!(handled, Ok(true));
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

    // Release events are gated out before any handler runs, so dispatch reports
    // the event as unhandled.
    assert_eq!(handled, Ok(false));
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn key_repeat_navigation_moves_cursor() {
    // Navigation is pure cursor math over the in-memory entries; no on-disk
    // files are required.
    let (_tmp, mut state) = panel_with_files(&["a.txt", "b.txt", "c.txt"]);
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    // Repeat is allowed for navigation keys, so the event is handled.
    assert_eq!(handled, Ok(true));
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

    assert_eq!(handled, Ok(true));
    assert_eq!(state.input.dialog_input.text(), "a");
    assert_eq!(state.input.dialog_input.cursor(), 1);
}

#[test]
fn create_directory_trims_surrounding_whitespace() {
    // Leading/trailing whitespace (e.g. from a paste) must be trimmed before the
    // directory is created, so `"  newdir  "` resolves to `newdir`, not a dir
    // named with literal spaces.
    let (tmp, mut state) = panel_with_files(&[]);
    state.mode = AppMode::Dialog(DialogKind::Input {
        prompt: "Create directory:".to_string(),
        action: InputAction::CreateDirectory,
    });
    state.input.dialog_input = {
        let mut ti = TextInput::new();
        ti.set_text("  newdir  ".to_string());
        ti
    };
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert_eq!(handled, Ok(true));
    assert!(
        tmp.path().join("newdir").is_dir(),
        "trimmed `newdir` directory must be created"
    );
    assert!(
        !tmp.path().join("  newdir  ").exists(),
        "untrimmed directory name must NOT be created"
    );
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn key_repeat_destructive_is_ignored() {
    let (tmp, mut state) = panel_with_files(&["victim.txt"]);
    let victim = tmp.path().join("victim.txt");
    std::fs::write(&victim, b"x").unwrap();
    state.left_panel.cursor = 0;
    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::F(8), KeyModifiers::NONE, KeyEventKind::Repeat);

    let DispatchResult { handled, .. } =
        dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    // F8 (delete) is destructive: repeat is gated out, so dispatch reports the
    // event as unhandled and no confirm dialog or pending action is created.
    assert_eq!(handled, Ok(false));
    assert!(matches!(state.mode, AppMode::Normal));
    assert!(state.ui.pending_action.is_none());
    // The victim file must survive an ignored destructive repeat.
    assert!(
        victim.exists(),
        "victim file must not be deleted by an ignored F8 repeat"
    );
}

#[test]
fn dispatch_test_event_exposes_job_on_confirmed_delete() {
    // Confirming a pending Delete spawns a background job, which dispatch
    // surfaces through `DispatchResult.job`. Drive the full key path
    // (Confirm dialog + Enter) so the job handle is produced by the real
    // handler rather than constructed by the test.
    let (tmp, mut state) = panel_with_files(&["victim.txt"]);
    let victim = tmp.path().join("victim.txt");
    std::fs::write(&victim, b"x").unwrap();
    state.left_panel.cursor = 0;
    state.mode = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
        "Delete",
        "Delete selected?",
    )));
    state.ui.pending_action = Some(PendingAction::Delete {
        paths: vec![victim],
    });

    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press);

    let mut res = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert_eq!(res.handled, Ok(true));
    assert!(
        res.job.is_some(),
        "confirmed Delete must produce a running job"
    );
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Progress { .. })
    ));

    // Join the spawned worker so the temp dir outlives the background delete.
    if let Some(mut job) = res.job.take() {
        job.shutdown();
    }
}

#[test]
fn dispatch_test_event_exposes_viewer_loader_on_f3() {
    // F3 on a regular file runs `view_current_entry`, which passes its `!is_dir`
    // guard and calls `open_in_viewer` -> `ViewerState::open_background`. Dispatch
    // surfaces the spawned background loader through `DispatchResult.viewer_loader`.
    // The entry must be a *file* (default `TestEntry` kind is `Directory`, which
    // the guard rejects), so build it with `.file(..)` and back it with a real
    // on-disk file the loader can open.
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("doc.txt");
    std::fs::write(&file, b"hello").unwrap();

    let mut state = AppState {
        active_panel: ActivePanel::Left,
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state
        .left_panel
        .set_entries(vec![TestEntry::new("doc.txt").path(file).file(5).build()]);
    state.left_panel.cursor = 0;

    let mut terminal = test_terminal();
    let key = KeyEvent::new_with_kind(KeyCode::F(3), KeyModifiers::NONE, KeyEventKind::Press);

    let mut res = dispatch_test_event(&mut state, &mut terminal, &Event::Key(key));

    assert!(
        res.viewer_loader.is_some(),
        "F3 on a file must start a background viewer loader"
    );
    assert!(matches!(state.mode, AppMode::Viewing));

    // Drop the loader explicitly: `ViewerLoader`'s `Drop` sets the cancel flag and
    // detaches the worker (no thread leak, no blocking join), so scope exit is the
    // whole cleanup. Drop before `tmp` so the worker is cancelled while the file
    // still exists.
    drop(res.viewer_loader.take());
}

#[test]
fn dispatch_test_event_viewer_and_job_default_none() {
    // A no-op event (FocusGained) leaves both the viewer and job handles unset.
    let mut state = AppState::default();
    let mut terminal = test_terminal();

    let res = dispatch_test_event(&mut state, &mut terminal, &Event::FocusGained);

    assert_eq!(res.handled, Ok(false));
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
