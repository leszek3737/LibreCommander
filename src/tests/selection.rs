use super::helpers::{TestEntry, dispatch_key, test_path, test_terminal};
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
    let panel = match panel {
        ActivePanel::Left => &state.left_panel,
        ActivePanel::Right => &state.right_panel,
    };
    let selected: Vec<bool> = panel.listing.filtered().map(|e| e.selected).collect();
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(
            selected[i], exp,
            "expected filtered entry [{i}].selected = {exp}"
        );
    }
    // Enforce the sync invariant documented above: `selected_count` must equal
    // the number of entries whose `selected` flag is set across the whole
    // backing store (not just the filtered view), matching `selected_count`'s
    // own source. This is what the doc-comment promises but no test previously
    // verified.
    let actual_count = panel.selected_entries().count();
    assert_eq!(
        panel.selected_count(),
        actual_count,
        "selected_count out of sync with per-entry selected flags"
    );
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

// Two plain file entries `a.txt` / `b.txt`, the most common fixture in this
// module. Cuts the repeated two-line `set_entries(vec![...])` boilerplate.
fn test_files_ab() -> Vec<FileEntry> {
    vec![
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
    ]
}

// Helper for selected_or_current_paths tests. Loads `entries` into the given
// panel, makes it active, and asserts the resolved paths. Covering both panels
// guards against the resolver hard-coding `left_panel`.
fn check_selected_paths_on(
    panel: ActivePanel,
    entries: Vec<FileEntry>,
    cursor: usize,
    expected: Vec<&str>,
) {
    let mut state = AppState::new();
    state.active_panel = panel;
    let target = match panel {
        ActivePanel::Left => &mut state.left_panel,
        ActivePanel::Right => &mut state.right_panel,
    };
    target.set_entries(entries);
    target.cursor = cursor;

    let paths = selected_or_current_paths(&state);
    let expected: Vec<PathBuf> = expected.into_iter().map(test_path).collect();
    assert_eq!(paths, expected);
}

// Backwards-compatible wrapper: existing cases target the left panel.
fn check_selected_paths(entries: Vec<FileEntry>, cursor: usize, expected: Vec<&str>) {
    check_selected_paths_on(ActivePanel::Left, entries, cursor, expected);
}

#[test]
fn shift_down_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(test_files_ab());

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
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
        entry("c.txt").file(30).build(),
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
        entry("a.txt").file(10).selected().build(),
        entry("b.txt").file(20).build(),
        entry("c.txt").file(30).build(),
        entry("d.txt").file(40).build(),
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
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).build(),
        entry("c.txt").file(30).build(),
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
            entry("file_a.txt").file(100).build(),
            entry("file_b.txt").file(100).build(),
        ],
        1,
        vec!["file_b.txt"],
    );
}

#[test]
fn selected_or_current_paths_uses_selection_when_present() {
    check_selected_paths(
        vec![
            entry("file_a.txt").file(100).selected().build(),
            entry("file_b.txt").file(100).build(),
            entry("file_c.txt").file(100).selected().build(),
        ],
        1,
        vec!["file_a.txt", "file_c.txt"],
    );
}

#[test]
fn selected_or_current_paths_skips_dotdot() {
    check_selected_paths(vec![entry("..").file(100).selected().build()], 0, vec![]);
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
            entry("..").file(100).build(),
            entry("file_a.txt").file(100).build(),
        ],
        1,
        vec!["file_a.txt"],
    );
}

#[test]
fn selected_or_current_paths_dotdot_current_returns_empty() {
    check_selected_paths(vec![entry("..").file(100).build()], 0, vec![]);
}

#[test]
fn selected_or_current_paths_all_dotdot_selected_fallback() {
    check_selected_paths(
        vec![
            entry("..").file(100).selected().build(),
            entry("file_a.txt").file(100).build(),
        ],
        1,
        vec!["file_a.txt"],
    );
}

#[test]
fn selected_or_current_paths_right_panel_fallback_to_cursor() {
    check_selected_paths_on(
        ActivePanel::Right,
        vec![
            entry("file_a.txt").file(100).build(),
            entry("file_b.txt").file(100).build(),
        ],
        1,
        vec!["file_b.txt"],
    );
}

#[test]
fn selected_or_current_paths_right_panel_uses_selection_when_present() {
    check_selected_paths_on(
        ActivePanel::Right,
        vec![
            entry("file_a.txt").file(100).selected().build(),
            entry("file_b.txt").file(100).build(),
            entry("file_c.txt").file(100).selected().build(),
        ],
        1,
        vec!["file_a.txt", "file_c.txt"],
    );
}

// The 2nd argument to `reposition_cursor_to_entry(state, name, visible)` is
// `visible`: the number of rows the panel can display at once. After moving the
// cursor onto the matched entry it is forwarded to `ensure_cursor_visible` so
// the viewport scrolls to keep the new cursor on screen. The value (20 here)
// only affects scroll_offset, not the resolved cursor index these tests assert.
#[test]
fn reposition_cursor_finds_matching_name() {
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        entry("a").build(),
        entry("b").build(),
        entry("c").build(),
    ]);
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("b"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_no_match_leaves_cursor() {
    let mut state = AppState::new();
    state
        .left_panel
        .set_entries(vec![entry("a").build(), entry("b").build()]);
    state.left_panel.cursor = 1;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("z"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_none_name_unchanged() {
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![entry("a").build()]);
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
    state.left_panel.set_entries(test_files_ab());

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
        entry("a.txt").file(10).build(),
        entry("b.txt").file(20).selected().build(),
    ]);
    state.left_panel.cursor = 1;

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert!(
        !state
            .left_panel
            .listing
            .filtered_get(1)
            .expect("entry 1 exists")
            .selected
    );
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn insert_on_last_entry_cursor_stays_selection_toggled() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(test_files_ab());
    state.left_panel.cursor = 1;

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert_selections(&state, ActivePanel::Left, &[false, true]);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn insert_on_dotdot_skips_toggle_and_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(vec![
        entry("..").file(100).build(),
        entry("a.txt").file(10).build(),
    ]);

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert_selections(&state, ActivePanel::Left, &[false, false]);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn insert_on_dotdot_only_entry_no_toggle_cursor_stays() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state
        .left_panel
        .set_entries(vec![entry("..").file(100).build()]);

    dispatch_key(
        &mut state,
        KeyCode::Insert,
        KeyModifiers::NONE,
        &mut terminal,
    );

    assert_selections(&state, ActivePanel::Left, &[false]);
    assert_eq!(state.left_panel.cursor, 0);
}

#[test]
fn shift_wraparound_down_on_last_and_up_on_first() {
    let mut terminal = test_terminal();
    let mut state = AppState::new();
    state.left_panel.set_entries(test_files_ab());

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
        entry("..").file(100).build(),
        entry("a.txt").file(10).build(),
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
        entry("x.txt").file(10).build(),
        entry("y.txt").file(20).build(),
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
