use super::helpers::*;
use crate::apply_search_filter;
use crate::input::mode_dispatch::handle_search_mode;
use crossterm::event::{KeyCode, KeyModifiers};
use lc::app;
use lc::app::types::{AppMode, AppState, InputState};

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

/// Seed the left panel with `entries` as both the visible and unfiltered
/// listing — the common "filtered view == full backing store" setup.
fn setup_entries(state: &mut AppState, entries: Vec<lc::app::types::FileEntry>) {
    state.left_panel.listing.set_unfiltered(entries);
    state.left_panel.listing.set_filtered_all();
}

fn setup_temp_files(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        std::fs::write(dir.path().join(name), name.as_bytes()).unwrap();
    }
    dir
}

#[test]
fn search_enter_preserves_current_entry_focus() {
    let temp_dir = setup_temp_files(&["alpha.txt", "beta.txt"]);
    let alpha = temp_dir.path().join("alpha.txt");
    let beta = temp_dir.path().join("beta.txt");
    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "beta".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.listing.set_unfiltered(vec![
        TestEntry::new("alpha.txt").path(&alpha).file(1).build(),
        TestEntry::new("beta.txt").path(&beta).file(1).build(),
    ]);
    state
        .left_panel
        .listing
        .set_filtered(&[TestEntry::new("beta.txt").path(&beta).file(1).build()]);
    state.left_panel.set_filter(Some("beta".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Enter,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

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
    let temp_dir = setup_temp_files(&["fresh.txt"]);
    let stale = temp_dir.path().join("stale.txt");
    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "fresh".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.listing.set_unfiltered(vec![
        TestEntry::new("stale.txt").path(&stale).file(1).build(),
    ]);
    state.left_panel.listing.set_filtered_all();
    state.left_panel.mark_unfiltered_dirty();
    state.left_panel.set_filter(Some("fresh".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Enter,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    assert!(
        state
            .left_panel
            .listing
            .filtered()
            .any(|entry| entry.name == "fresh.txt")
    );
    assert!(
        !state
            .left_panel
            .listing
            .filtered()
            .any(|entry| entry.name == "stale.txt")
    );
}

#[test]
fn search_enter_clears_filter_and_restores_unfiltered_entries() {
    let temp_dir = setup_temp_files(&["alpha.txt", "beta.txt"]);

    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "alpha".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    state.left_panel.listing.set_unfiltered(vec![
        entry("alpha.txt").file(1).build(),
        entry("beta.txt").file(2).build(),
    ]);
    state
        .left_panel
        .listing
        .set_filtered(&[entry("alpha.txt").file(1).build()]);
    state.left_panel.set_filter(Some("alpha".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Enter,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert_eq!(state.input.search_query, "");
    assert!(state.left_panel.filter().is_none());
    assert!(
        state
            .left_panel
            .listing
            .filtered()
            .any(|e| e.name == "alpha.txt"),
        "alpha.txt missing: {:?}",
        state
            .left_panel
            .listing
            .filtered()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
    assert!(
        state
            .left_panel
            .listing
            .filtered()
            .any(|e| e.name == "beta.txt"),
        "beta.txt missing: {:?}",
        state
            .left_panel
            .listing
            .filtered()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn search_mode_with_empty_panel_handles_enter_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(
        &mut state,
        KeyCode::Enter,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn search_mode_with_empty_panel_handles_esc_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(
        &mut state,
        KeyCode::Esc,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn search_mode_with_empty_panel_handles_char_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(tmp.path().to_path_buf());
    state.active_panel = app::types::ActivePanel::Left;
    state.mode = AppMode::Search;
    handle_search_mode(
        &mut state,
        KeyCode::Char('x'),
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );
    assert_eq!(state.input.search_query, "x");
}

#[test]
fn apply_search_filter_exact_match() {
    let mut state = AppState::default();
    let entries = vec![entry("foo").build(), entry("bar").build()];
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(Some("foo".to_string()));
    apply_search_filter(&mut state.left_panel);
    assert!(state.left_panel.listing.filtered().all(|e| e.name == "foo"));
}

#[test]
fn apply_search_filter_no_match_clears_entries() {
    let mut state = AppState::default();
    let entries = vec![entry("a").build(), entry("b").build()];
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(Some("xyz".to_string()));
    apply_search_filter(&mut state.left_panel);
    assert!(state.left_panel.listing.filtered_is_empty());
}

#[test]
fn apply_search_filter_empty_pattern_shows_all() {
    let mut state = AppState::default();
    let entries = vec![entry("a").build(), entry("b").build()];
    let count = entries.len();
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(None);
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.listing.filtered_len(), count);
}

#[test]
fn apply_search_filter_partial_match() {
    let mut state = AppState::default();
    let entries = vec![
        entry("bar").build(),
        entry("baz").build(),
        entry("foo").build(),
    ];
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(Some("ba".to_string()));
    apply_search_filter(&mut state.left_panel);
    let names: Vec<&str> = state
        .left_panel
        .listing
        .filtered()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["bar", "baz"],
        "only the 'ba'-prefixed entries match"
    );
}

#[test]
fn apply_search_filter_unicode_cjk() {
    let mut state = AppState::default();
    let entries = vec![
        entry("文件.txt").build(),
        entry("テスト.txt").build(),
        entry("alpha.txt").build(),
    ];
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(Some("文件".to_string()));
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.listing.filtered_len(), 1);
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(0)
            .expect("one filtered entry")
            .name,
        "文件.txt"
    );
}

#[test]
fn apply_search_filter_unicode_emoji() {
    let mut state = AppState::default();
    let entries = vec![
        entry("🎉party.txt").build(),
        entry("📁folder").build(),
        entry("normal.txt").build(),
    ];
    setup_entries(&mut state, entries);
    state.left_panel.set_filter(Some("🎉".to_string()));
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.listing.filtered_len(), 1);
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(0)
            .expect("one filtered entry")
            .name,
        "🎉party.txt"
    );
}

#[test]
fn apply_search_filter_unicode_combining_chars() {
    let mut state = AppState::default();
    let decomposed = "cafe\u{0301}.txt";
    let precomposed = "caf\u{00E9}.txt";
    let entries = vec![entry(decomposed).build(), entry(precomposed).build()];
    setup_entries(&mut state, entries);
    state
        .left_panel
        .set_filter(Some("cafe\u{0301}".to_string()));
    apply_search_filter(&mut state.left_panel);
    assert_eq!(state.left_panel.listing.filtered_len(), 1);
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(0)
            .expect("one filtered entry")
            .name,
        decomposed
    );
}

#[test]
fn search_esc_restores_entries_documents_cursor() {
    let mut state = AppState::default();
    let alpha = test_path("alpha.txt");
    let beta = test_path("beta.txt");
    let gamma = test_path("gamma.txt");
    let entries = vec![
        TestEntry::new("alpha.txt").path(&alpha).build(),
        TestEntry::new("beta.txt").path(&beta).build(),
        TestEntry::new("gamma.txt").path(&gamma).build(),
    ];
    state.left_panel.listing.set_unfiltered(entries.clone());
    state
        .left_panel
        .listing
        .set_filtered(&[TestEntry::new("beta.txt").path(&beta).build()]);
    state.left_panel.cursor = 0;
    state.left_panel.set_filter(Some("beta".to_string()));
    state.input.search_query = "beta".to_string();
    state.input.search_cursor = 4;
    state.mode = AppMode::Search;

    handle_search_mode(
        &mut state,
        KeyCode::Esc,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(state.left_panel.filter().is_none());
    assert!(
        state.input.search_query.is_empty(),
        "Esc must clear the search query"
    );
    assert_eq!(
        state.input.search_cursor, 0,
        "Esc must reset the search cursor"
    );
    assert_eq!(state.left_panel.listing.filtered_len(), entries.len());
    assert_eq!(
        state
            .left_panel
            .listing
            .filtered_get(1)
            .expect("second filtered entry")
            .name,
        "beta.txt"
    );
    assert_eq!(state.left_panel.cursor, 1);
}

#[test]
fn search_backspace_shortens_query_and_cursor() {
    let temp_dir = setup_temp_files(&["alpha.txt", "beta.txt"]);
    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "alp".to_string(),
            search_cursor: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    setup_entries(
        &mut state,
        vec![
            entry("alpha.txt").file(1).build(),
            entry("beta.txt").file(1).build(),
        ],
    );
    state.left_panel.set_filter(Some("alp".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Backspace,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    assert_eq!(state.input.search_query, "al");
    assert_eq!(
        state.input.search_cursor, 2,
        "cursor follows the shortened query"
    );
    assert_eq!(
        state.mode,
        AppMode::Search,
        "non-empty query stays in search"
    );
    assert_eq!(
        state.left_panel.filter().map(str::to_owned),
        Some("al".to_string())
    );
}

#[test]
fn search_backspace_to_empty_clears_search() {
    let temp_dir = setup_temp_files(&["alpha.txt"]);
    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "a".to_string(),
            search_cursor: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    setup_entries(&mut state, vec![entry("alpha.txt").file(1).build()]);
    state.left_panel.set_filter(Some("a".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Backspace,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    assert_eq!(
        state.mode,
        AppMode::Normal,
        "emptying the query exits search"
    );
    assert!(state.input.search_query.is_empty());
    assert_eq!(state.input.search_cursor, 0);
    assert!(state.left_panel.filter().is_none());
}

#[test]
fn search_clear_resets_scroll_offset() {
    let temp_dir = setup_temp_files(&["a.txt", "b.txt", "c.txt"]);
    let mut state = AppState {
        mode: AppMode::Search,
        input: InputState {
            search_query: "a".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    state.left_panel.set_path(temp_dir.path().to_path_buf());
    setup_entries(
        &mut state,
        vec![
            entry("a.txt").file(1).build(),
            entry("b.txt").file(1).build(),
            entry("c.txt").file(1).build(),
        ],
    );
    state.left_panel.scroll_offset = 10;
    state.left_panel.set_filter(Some("a".to_string()));

    handle_search_mode(
        &mut state,
        KeyCode::Esc,
        KeyModifiers::NONE,
        TERMINAL_HEIGHT,
    );

    // The pre-clear scroll (10) was past the end; clearing must pull it back
    // into bounds and never leave it ahead of the cursor.
    let panel = &state.left_panel;
    assert!(
        panel.scroll_offset < panel.listing.filtered_len(),
        "scroll_offset {} must be within the {}-entry listing",
        panel.scroll_offset,
        panel.listing.filtered_len()
    );
    assert!(
        panel.scroll_offset <= panel.cursor,
        "scroll_offset {} must not run ahead of cursor {}",
        panel.scroll_offset,
        panel.cursor
    );
}
