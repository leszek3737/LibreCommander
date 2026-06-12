use super::helpers::{TestEntry, dispatch_key, test_terminal};
use crate::{reposition_cursor_to_entry, selected_or_current_paths};
use crossterm::event::{KeyCode, KeyModifiers};
use lc::app::types::ActivePanel;
use lc::app::types::AppState;
use lc::app::types::FileEntry;
use std::path::PathBuf;

// Selection contract:
// `PanelState::set_entries()` calls `recalculate_selection_stats()` internally, so
// `entry.selected` is the authoritative source of truth and `selected_count` is always
// in sync after `set_entries()`. No manual `recalculate_selection_stats()` or
// `set_selected_count()` calls are needed after `set_entries()`.

fn assert_selections(state: &AppState, panel: ActivePanel, expected: &[bool]) {
    let entries = match panel {
        ActivePanel::Left => &state.left_panel.listing.entries,
        ActivePanel::Right => &state.right_panel.listing.entries,
    };
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(
            entries[i].selected, exp,
            "expected entries[{i}].selected = {exp}"
        );
    }
}

// Helper for selected_or_current_paths tests.
// TestEntry::build() defaults path to `std::env::temp_dir()/name`, so expected paths
// use `std::env::temp_dir()` to stay portable across platforms.
fn check_selected_paths(entries: Vec<FileEntry>, cursor: usize, expected: Vec<&str>) {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.set_entries(entries);
    state.left_panel.cursor = cursor;

    let paths = selected_or_current_paths(&state);
    let expected: Vec<PathBuf> = expected
        .into_iter()
        .map(|name| std::env::temp_dir().join(name))
        .collect();
    assert_eq!(paths, expected);
}

#[test]
fn shift_down_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert_selections(&state, ActivePanel::Left, &[true, false]);
}

#[test]
fn shift_up_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).build(),
        TestEntry::new("c.txt").file(30).build(),
    ]);
    state.left_panel.cursor = 2;

    dispatch_key(&mut state, KeyCode::Up, KeyModifiers::SHIFT, &mut terminal);

    assert_eq!(state.left_panel.cursor, 1);
    assert_selections(&state, ActivePanel::Left, &[false, false, true]);
}

#[test]
fn shift_selection_preserves_unrelated_entries() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).selected().build(),
        TestEntry::new("b.txt").file(20).build(),
        TestEntry::new("c.txt").file(30).build(),
        TestEntry::new("d.txt").file(40).build(),
    ]);
    state.left_panel.cursor = 2;

    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_selections(&state, ActivePanel::Left, &[true, false, true, false]);
}

#[test]
fn shift_arrow_then_shift_arrow_toggles_two() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).build(),
        TestEntry::new("c.txt").file(30).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );
    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 2);
    assert_selections(&state, ActivePanel::Left, &[true, true, false]);
}

#[test]
fn selected_or_current_paths_fallback_to_cursor() {
    check_selected_paths(
        vec![
            TestEntry::new("file_a.txt").file(100).build(),
            TestEntry::new("file_b.txt").file(100).build(),
        ],
        1,
        vec!["file_b.txt"],
    );
}

#[test]
fn selected_or_current_paths_uses_selection_when_present() {
    check_selected_paths(
        vec![
            TestEntry::new("file_a.txt").file(100).selected().build(),
            TestEntry::new("file_b.txt").file(100).build(),
            TestEntry::new("file_c.txt").file(100).selected().build(),
        ],
        1,
        vec!["file_a.txt", "file_c.txt"],
    );
}

#[test]
fn selected_or_current_paths_skips_dotdot() {
    check_selected_paths(
        vec![TestEntry::new("..").file(100).selected().build()],
        0,
        vec![],
    );
}

#[test]
fn selected_or_current_paths_empty_panel() {
    let state = AppState::new();
    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn selected_or_current_paths_no_selection_returns_current() {
    check_selected_paths(
        vec![
            TestEntry::new("..").file(100).build(),
            TestEntry::new("file_a.txt").file(100).build(),
        ],
        1,
        vec!["file_a.txt"],
    );
}

#[test]
fn selected_or_current_paths_dotdot_current_returns_empty() {
    check_selected_paths(vec![TestEntry::new("..").file(100).build()], 0, vec![]);
}

#[test]
fn selected_or_current_paths_all_dotdot_selected_fallback() {
    check_selected_paths(
        vec![
            TestEntry::new("..").file(100).selected().build(),
            TestEntry::new("file_a.txt").file(100).build(),
        ],
        1,
        vec!["file_a.txt"],
    );
}

#[test]
fn reposition_cursor_finds_matching_name() {
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a").build(),
        TestEntry::new("b").build(),
        TestEntry::new("c").build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("b"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_no_match_leaves_cursor() {
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a").build(),
        TestEntry::new("b").build(),
    ]);
    state.left_panel.cursor = 1;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("z"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_none_name_unchanged() {
    let mut state = AppState::new();
    state
        .left_panel
        .set_entries(vec![TestEntry::new("a").build()]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, None, 20);
    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn reposition_cursor_empty_list_no_panic() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("x"), 20);
}

#[test]
fn insert_toggles_current_on_then_moves_down() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert_selections(&state, ActivePanel::Left, &[true, false]);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn insert_toggles_current_off_stays_on_last() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).selected().build(),
    ]);
    state.left_panel.cursor = 1;

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert!(!state.left_panel.listing.entries[1].selected);
    assert_eq!(state.left_panel.cursor, 1);
}

// TODO: insert_on_last_entry — cursor should stay, selection toggled
// TODO: insert_on_dotdot — should skip toggle (like shift-arrow on "..")

#[test]
fn shift_wraparound_down_on_last_and_up_on_first() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("a.txt").file(10).build(),
        TestEntry::new("b.txt").file(20).build(),
    ]);

    state.left_panel.cursor = 1;
    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );
    assert_eq!(state.left_panel.cursor, 0);
    assert_selections(&state, ActivePanel::Left, &[false, true]);

    dispatch_key(&mut state, KeyCode::Up, KeyModifiers::SHIFT, &mut terminal);
    assert_eq!(state.left_panel.cursor, 1);
    assert_selections(&state, ActivePanel::Left, &[true, true]);
}

#[test]
fn shift_down_on_dotdot_skips_toggle_and_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        TestEntry::new("..").file(100).build(),
        TestEntry::new("a.txt").file(10).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert_selections(&state, ActivePanel::Left, &[false, false]);
}

#[test]
fn shift_selection_on_right_panel() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Right;
    state.right_panel.set_entries(vec![
        TestEntry::new("x.txt").file(10).build(),
        TestEntry::new("y.txt").file(20).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        &mut terminal,
    );

    assert_eq!(state.right_panel.cursor, 1);
    assert_selections(&state, ActivePanel::Right, &[true, false]);
}
