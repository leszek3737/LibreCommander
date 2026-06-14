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

// Up/Down do NOT wrap; on empty and single-item lists the cursor stays clamped
// at 0 in both directions (no wrap-around to the other end).
#[test]
fn picker_navigation_clamps_at_bounds_empty_and_single() {
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

/// Number of options the `ArchiveMenu` picker exposes, derived at runtime
/// instead of hard-coded. The option list lives in a private `const ITEMS`
/// inside the production handler (and a twin in `render.rs`); neither is
/// reachable from tests. Rather than baking in a magic count (brittle the
/// moment an option is added/removed), we lean on the navigation contract:
/// `End` clamps the cursor to the last index, so `last_index + 1` is the
/// number of options. Adding a third archive option makes this return 3
/// automatically with no test edits.
fn archive_menu_option_count() -> usize {
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);
    pickers::handle_list_picker(&mut state, KeyCode::End);
    state.ui.picker_selected + 1
}

#[test]
fn archive_menu_picker_navigate_bounds() {
    let count = archive_menu_option_count();
    assert!(count >= 2, "expected at least Extract + Create options");
    let last = count - 1;
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);

    // Walk Down past the end: cursor advances one per key, then clamps at `last`.
    for expected in 1..=last {
        pickers::handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, expected);
    }
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, last, "Down clamps at last index");

    // Walk Up past the start: cursor retreats one per key, then clamps at 0.
    for expected in (0..last).rev() {
        pickers::handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.ui.picker_selected, expected);
    }
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0, "Up clamps at first index");
}

#[test]
fn archive_menu_picker_home_end() {
    let last = archive_menu_option_count() - 1;
    let mut state = picker_at(PickerKind::ArchiveMenu, 0);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.ui.picker_selected, last);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.ui.picker_selected, 0);
}

// Happy path: Enter on a hotlist picker performs the real selection action,
// not merely closing the picker. The selected entry (index 1 of two real
// directories) drives `navigate_to_hotlist`, which changes the active panel's
// path and reports the `cd to ...` status. Exercising two distinct temp dirs
// proves the cursor index actually selects the right entry.
#[test]
fn hotlist_picker_enter_navigates_to_selected_directory() {
    let dir0 = tempfile::tempdir().unwrap();
    let dir1 = tempfile::tempdir().unwrap();
    let path0 = dir0.path().to_path_buf();
    let path1 = dir1.path().to_path_buf();

    let mut state = AppState {
        left_panel: PanelState::new(path0.clone()),
        mode: AppMode::ListPicker(PickerKind::Hotlist),
        ..Default::default()
    };
    state.ui.directory_hotlist = vec![path0, path1.clone()];
    // Cursor on the second entry: Enter must navigate there, not to entry 0.
    state.ui.picker_selected = 1;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::Normal, "Enter closes the picker");
    assert_eq!(
        state.active_panel().path(),
        path1.as_path(),
        "active panel navigated to the selected hotlist entry"
    );
    assert_eq!(
        state.ui.status_message,
        Some(format!("cd to {}", path1.display())),
        "selection triggered the navigation action"
    );
}

// PageUp/PageDown are not navigation keys for list pickers: `handle_nav_key`
// only maps Up/Down/Home/End, so these fall through as no-ops and leave the
// cursor untouched (they neither page nor clamp). This pins that contract so a
// future addition of paging would be a deliberate, test-visible change.
#[test]
fn picker_page_up_down_are_noops() {
    let mut state = with_history(picker_at(PickerKind::History, 1), &["a", "b", "c"]);

    pickers::handle_list_picker(&mut state, KeyCode::PageDown);
    assert_eq!(state.ui.picker_selected, 1, "PageDown does not move cursor");

    pickers::handle_list_picker(&mut state, KeyCode::PageUp);
    assert_eq!(state.ui.picker_selected, 1, "PageUp does not move cursor");
}

// A printable character that is not a picker hotkey is a no-op: non-filtering
// pickers (History here) ignore typed chars entirely — no cursor move, no mode
// change, no status. Only Hotlist's 'a'/'d' are meaningful hotkeys elsewhere.
#[test]
fn picker_typed_char_is_noop_when_not_a_hotkey() {
    let mut state = with_history(picker_at(PickerKind::History, 1), &["a", "b", "c"]);

    pickers::handle_list_picker(&mut state, KeyCode::Char('x'));

    assert_eq!(state.ui.picker_selected, 1, "char does not move cursor");
    assert_eq!(
        state.mode,
        AppMode::ListPicker(PickerKind::History),
        "char does not close the picker"
    );
    assert_eq!(state.ui.status_message, None, "char produces no status");
}

// In the hotlist picker, 'a'/'d' are hotkeys but any other char is inert: it
// must not add/remove entries, move the cursor, or close the picker.
#[test]
fn hotlist_picker_unmapped_char_is_noop() {
    let mut state = hotlist_state(&["/a", "/b", "/c"]);
    state.ui.picker_selected = 1;

    pickers::handle_list_picker(&mut state, KeyCode::Char('z'));

    assert_eq!(
        state.hotlist().len(),
        3,
        "unmapped char does not mutate list"
    );
    assert_eq!(
        state.ui.picker_selected, 1,
        "unmapped char does not move cursor"
    );
    assert_eq!(
        state.mode,
        AppMode::ListPicker(PickerKind::Hotlist),
        "unmapped char does not close the picker"
    );
}
