use super::helpers::*;
use crate::input::mode_dispatch::handle_search_mode;
use crate::*;

#[test]
fn search_enter_clears_filter_and_refreshes_from_disk() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();
    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "alpha".to_string(),
        search_cursor: 5,
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.entries = vec![TestEntry::new("alpha.txt").size(1).build()];
    state.left_panel.unfiltered_entries = vec![
        TestEntry::new("alpha.txt").size(1).build(),
        TestEntry::new("beta.txt").size(2).build(),
    ];
    state.left_panel.filter = Some("alpha".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
    assert_eq!(state.left_panel.filter.as_deref(), None);
    assert!(
        state
            .left_panel
            .entries
            .iter()
            .any(|entry| entry.name == "alpha.txt")
    );
    assert!(
        state
            .left_panel
            .entries
            .iter()
            .any(|entry| entry.name == "beta.txt")
    );
}

#[test]
fn search_enter_preserves_current_entry_focus() {
    let temp_dir = tempfile::tempdir().unwrap();
    let alpha = temp_dir.path().join("alpha.txt");
    let beta = temp_dir.path().join("beta.txt");
    std::fs::write(&alpha, b"alpha").unwrap();
    std::fs::write(&beta, b"beta").unwrap();
    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "beta".to_string(),
        search_cursor: 4,
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.entries = vec![TestEntry::new("beta.txt").path(&beta).size(1).build()];
    state.left_panel.unfiltered_entries = vec![
        TestEntry::new("alpha.txt").path(&alpha).size(1).build(),
        TestEntry::new("beta.txt").path(&beta).size(1).build(),
    ];
    state.left_panel.filter = Some("beta".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(
        state
            .left_panel
            .current_entry()
            .map(|entry| entry.name.as_str()),
        Some("beta.txt")
    );
}

#[test]
fn search_enter_refreshes_when_unfiltered_cache_is_dirty() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("fresh.txt"), b"fresh").unwrap();
    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "fresh".to_string(),
        search_cursor: 5,
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.entries = vec![TestEntry::new("stale.txt").size(1).build()];
    state.left_panel.unfiltered_entries = vec![TestEntry::new("stale.txt").size(1).build()];
    state.left_panel.unfiltered_dirty = true;
    state.left_panel.filter = Some("fresh".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert!(
        state
            .left_panel
            .entries
            .iter()
            .any(|entry| entry.name == "fresh.txt")
    );
    assert!(
        !state
            .left_panel
            .entries
            .iter()
            .any(|entry| entry.name == "stale.txt")
    );
}

#[test]
fn search_enter_clears_filter_and_restores_unfiltered_entries() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();

    let mut state = AppState {
        mode: AppMode::Search,
        search_query: "alpha".to_string(),
        search_cursor: 5,
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.entries = vec![TestEntry::new("alpha.txt").size(1).build()];
    state.left_panel.unfiltered_entries = vec![
        TestEntry::new("alpha.txt").size(1).build(),
        TestEntry::new("beta.txt").size(2).build(),
    ];
    state.left_panel.filter = Some("alpha".to_string());

    handle_search_mode(&mut state, KeyCode::Enter, 24);

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.search_query, "");
    assert!(state.left_panel.filter.is_none());
    let names: Vec<&str> = state
        .left_panel
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"alpha.txt"), "alpha.txt missing: {names:?}");
    assert!(names.contains(&"beta.txt"), "beta.txt missing: {names:?}");
}

#[test]
fn search_mode_with_empty_panel_handles_enter_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.entries = vec![];
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Enter, 20);
}

#[test]
fn search_mode_with_empty_panel_handles_esc_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.entries = vec![];
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Esc, 20);
    assert!(matches!(state.mode, AppMode::Normal));
}

#[test]
fn search_mode_with_empty_panel_handles_char_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.left_panel.entries = vec![];
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(&mut state, KeyCode::Char('x'), 20);
}

#[test]
fn apply_search_filter_exact_match() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("foo").build(), TestEntry::new("bar").build()];
    state.left_panel.filter = Some("foo".to_string());
    apply_search_filter(&mut state.left_panel);
    let names: Vec<_> = state.left_panel.entries.iter().map(|e| &e.name).collect();
    assert!(names.iter().all(|n| *n == "foo"));
}

#[test]
fn apply_search_filter_no_match_clears_entries() {
    let mut state = AppState::default();
    state.left_panel.entries = vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    state.left_panel.filter = Some("xyz".to_string());
    apply_search_filter(&mut state.left_panel);
    assert!(state.left_panel.entries.is_empty());
}

#[test]
fn apply_search_filter_empty_pattern_shows_all() {
    let mut state = AppState::default();
    let entries = vec![TestEntry::new("a").build(), TestEntry::new("b").build()];
    let count = entries.len();
    state.left_panel.entries = entries.clone();
    state.left_panel.unfiltered_entries = entries;
    state.left_panel.filter = None;
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.entries.len(), count);
}

#[test]
fn apply_search_filter_partial_match() {
    let mut state = AppState::default();
    let entries = vec![
        TestEntry::new("bar").build(),
        TestEntry::new("baz").build(),
        TestEntry::new("foo").build(),
    ];
    state.left_panel.entries = entries.clone();
    state.left_panel.unfiltered_entries = entries;
    state.left_panel.filter = Some("ba".to_string());
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.entries.len(), 2);
}
