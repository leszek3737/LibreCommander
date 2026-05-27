use super::helpers::*;
use crate::input::mode_dispatch::handle_normal_mode;
use crate::*;
use app::types::ActivePanel;
use crossterm::event::KeyModifiers;
use std::path::PathBuf;

#[test]
fn shift_down_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert!(state.left_panel.listing.entries[0].selected);
    assert!(!state.left_panel.listing.entries[1].selected);
}

#[test]
fn shift_up_toggles_current_then_moves() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
    ];
    state.left_panel.cursor = 2;

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Up,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert_eq!(state.left_panel.cursor, 1);
    assert!(!state.left_panel.listing.entries[0].selected);
    assert!(!state.left_panel.listing.entries[1].selected);
    assert!(state.left_panel.listing.entries[2].selected);
}

#[test]
fn shift_selection_preserves_unrelated_entries() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).selected().build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
        TestEntry::new("d.txt").size(40).build(),
    ];
    state.left_panel.cursor = 2;
    state.left_panel.recalculate_selection_stats();

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.listing.entries[0].selected);
    assert!(!state.left_panel.listing.entries[1].selected);
    assert!(state.left_panel.listing.entries[2].selected);
    assert!(!state.left_panel.listing.entries[3].selected);
}

#[test]
fn shift_arrow_then_shift_arrow_toggles_two() {
    let mut terminal = test_terminal();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt").size(10).build(),
        TestEntry::new("b.txt").size(20).build(),
        TestEntry::new("c.txt").size(30).build(),
    ];

    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );
    handle_normal_mode(
        &mut state,
        &mut None,
        &mut None,
        KeyCode::Down,
        KeyModifiers::SHIFT,
        24,
        &mut terminal,
    );

    assert!(state.left_panel.listing.entries[0].selected);
    assert!(state.left_panel.listing.entries[1].selected);
    assert!(!state.left_panel.listing.entries[2].selected);
    assert_eq!(state.left_panel.cursor, 2);
}

#[test]
fn selected_or_current_paths_fallback_to_cursor() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![
        TestEntry::new("file_a.txt").size(100).build(),
        TestEntry::new("file_b.txt").size(100).build(),
    ];
    state.left_panel.cursor = 1;

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/tmp/file_b.txt"));
}

#[test]
fn selected_or_current_paths_uses_selection_when_present() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![
        TestEntry::new("file_a.txt").size(100).selected().build(),
        TestEntry::new("file_b.txt").size(100).build(),
        TestEntry::new("file_c.txt").size(100).selected().build(),
    ];
    state.left_panel.cursor = 1;
    state.left_panel.set_selected_count(2);

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&PathBuf::from("/tmp/file_a.txt")));
    assert!(paths.contains(&PathBuf::from("/tmp/file_c.txt")));
}

#[test]
fn selected_or_current_paths_skips_dotdot() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![TestEntry::new("..").size(100).selected().build()];
    state.left_panel.cursor = 0;

    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn selected_or_current_paths_empty_panel() {
    let state = AppState::new();
    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn selected_or_current_paths_no_selection_returns_current() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![
        TestEntry::new("..").size(100).build(),
        TestEntry::new("file_a.txt").size(100).build(),
    ];
    state.left_panel.cursor = 1;
    state.left_panel.set_selected_count(0);

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/tmp/file_a.txt"));
}

#[test]
fn selected_or_current_paths_dotdot_current_returns_empty() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![TestEntry::new("..").size(100).build()];
    state.left_panel.cursor = 0;
    state.left_panel.set_selected_count(0);

    let paths = selected_or_current_paths(&state);
    assert!(paths.is_empty());
}

#[test]
fn selected_or_current_paths_all_dotdot_selected_fallback() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Left;
    state.left_panel.listing.entries = vec![
        TestEntry::new("..").size(100).selected().build(),
        TestEntry::new("file_a.txt").size(100).build(),
    ];
    state.left_panel.cursor = 1;
    state.left_panel.set_selected_count(1);
    state.left_panel.recalculate_selection_stats();

    let paths = selected_or_current_paths(&state);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/tmp/file_a.txt"));
}

#[test]
fn reposition_cursor_finds_matching_name() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a").build(),
        TestEntry::new("b").build(),
        TestEntry::new("c").build(),
    ];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("b"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_no_match_leaves_cursor() {
    let mut state = AppState::default();
    state.left_panel.listing.entries =
        vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    state.left_panel.cursor = 1;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, Some("z"), 20);
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn reposition_cursor_none_name_unchanged() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("a").build()];
    state.left_panel.cursor = 0;
    state.active_panel = ActivePanel::Left;
    reposition_cursor_to_entry(&mut state, None, 20);
    assert_eq!(state.left_panel.cursor, 0);
}
