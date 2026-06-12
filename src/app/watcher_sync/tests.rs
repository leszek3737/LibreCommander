use super::*;
use crate::app::types::SortMode;
use std::fs;
use std::sync::Arc;
use std::sync::mpsc;

const WATCHER_CHANNEL_CAPACITY: usize = 256;
const OVERFLOW_EVENT_COUNT: usize = WATCHER_CHANNEL_CAPACITY + 1;

fn test_panel(path: &Path) -> PanelState {
    let mut panel = PanelState::new(path.to_path_buf());
    panel.listing.unfiltered_entries = vec![parent_entry(path)];
    panel.listing.entries = panel.listing.unfiltered_entries.clone();
    panel.recalculate_selection_stats();
    panel
}

fn parent_entry(path: &Path) -> reader::FileEntry {
    reader::FileEntry::builder()
        .name("..")
        .path(path.parent().unwrap_or(path))
        .is_dir(true)
        .is_executable(true)
        .permissions(0o755)
        .build()
}

fn select_entry_by_name(entries: &mut [reader::FileEntry], name: &str) {
    entries
        .iter_mut()
        .find(|e| e.name == name)
        .expect("entry should exist")
        .selected = true;
}

struct WatcherHarness {
    pub watcher: Option<Watcher>,
    pub state: AppState,
    pub sync_state: WatcherSyncState,
}

impl WatcherHarness {
    fn new() -> Self {
        let (event_tx, _rx) = mpsc::sync_channel(WATCHER_CHANNEL_CAPACITY);
        let watcher = Some(Watcher::new(Arc::new(event_tx)).expect("create watcher"));
        Self {
            watcher,
            state: AppState::new(),
            sync_state: WatcherSyncState::default(),
        }
    }

    fn sync(&mut self) {
        sync_watcher_paths(&mut self.watcher, &self.state, &mut self.sync_state);
    }
}

struct EventHarness {
    pub state: AppState,
    tx: mpsc::SyncSender<WatchEvent>,
    rx: mpsc::Receiver<WatchEvent>,
}

impl EventHarness {
    fn new(dir: &Path) -> Self {
        let (tx, rx) = mpsc::sync_channel(WATCHER_CHANNEL_CAPACITY);
        let mut state = AppState::new();
        state.left_panel = test_panel(dir);
        state.right_panel = test_panel(dir);
        Self { state, tx, rx }
    }

    fn send(&self, event: WatchEvent) {
        self.tx
            .send(event)
            .expect("watcher event receiver should be alive");
    }

    fn poll(&mut self) -> bool {
        poll_watcher_events(&mut self.state, &self.rx)
    }
}

// TODO: many tests repeat the same setup (tempdir + fs::write +
// apply_watcher_upsert_if_matches + rebuild_visible_entries). Extract a helper
// (e.g. `fn build_panel_with_files(dir, &[("name", contents)])`) to cut the
// boilerplate and reduce drift between scenarios.
// TODO: extract an `assert_entries_names_eq(panel, &["..", "alpha.txt", ...])`
// helper to replace the repeated collect-into-Vec<&str> + assert_eq pattern.

#[test]
fn watcher_upsert_adds_visible_entry_sorted_and_updates_stats() {
    let dir = tempfile::tempdir().unwrap();
    let beta = dir.path().join("beta.txt");
    let alpha = dir.path().join("alpha.txt");
    fs::write(&beta, b"beta").unwrap();
    fs::write(&alpha, b"alpha").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &beta));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &alpha));
    rebuild_visible_entries(&mut panel, None);

    let names: Vec<_> = panel
        .listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["..", "alpha.txt", "beta.txt"]);
    assert_eq!(panel.total_size(), 9);
}

#[test]
fn watcher_upsert_respects_filter_and_preserves_selection() {
    let dir = tempfile::tempdir().unwrap();
    let keep = dir.path().join("keep.txt");
    let drop = dir.path().join("drop.log");
    fs::write(&keep, b"old").unwrap();
    fs::write(&drop, b"drop").unwrap();

    let mut panel = test_panel(dir.path());
    panel.set_filter(Some("*.txt".to_string()));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
    rebuild_visible_entries(&mut panel, None);
    select_entry_by_name(&mut panel.listing.entries, "keep.txt");
    panel.sync_unfiltered_selection();

    fs::write(&keep, b"updated").unwrap();
    assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &drop));
    rebuild_visible_entries(&mut panel, None);

    assert_eq!(panel.listing.entries.len(), 2);
    let keep_entry = panel
        .listing
        .entries
        .iter()
        .find(|e| e.name == "keep.txt")
        .unwrap();
    assert!(keep_entry.selected);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 7);
    // total_size aggregates unfiltered_entries, so it includes drop.log (4 bytes)
    // even though it is filtered out of the visible listing.
    assert_eq!(panel.total_size(), 11);
}

#[test]
fn watcher_upsert_hides_hidden_when_hidden_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".secret");
    fs::write(&hidden, b"secret").unwrap();

    let mut panel = test_panel(dir.path());
    panel.set_show_hidden(false);

    assert!(!apply_watcher_upsert_if_matches(&mut panel, &hidden));
    assert_eq!(panel.listing.entries.len(), 1);
    assert_eq!(panel.listing.unfiltered_entries.len(), 1);
}

#[test]
fn watcher_remove_updates_visible_entries_and_clamps_cursor_scroll() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &a));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &b));
    panel.cursor = 2;
    panel.scroll_offset = 2;

    assert!(apply_watcher_remove_if_matches(&mut panel, &b));
    rebuild_visible_entries(&mut panel, None);

    let names: Vec<_> = panel
        .listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["..", "a.txt"]);
    assert_eq!(panel.cursor, 1);
    assert_eq!(panel.scroll_offset, 1);
    assert_eq!(panel.total_size(), 1);
}

#[test]
fn watcher_remove_handles_deleted_child_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gone.txt");
    fs::write(&file, b"gone").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
    fs::remove_file(&file).unwrap();

    assert!(apply_watcher_remove_if_matches(&mut panel, &file));
    rebuild_visible_entries(&mut panel, None);
    assert_eq!(panel.listing.entries.len(), 1);
    assert_eq!(panel.listing.unfiltered_entries.len(), 1);
}

#[test]
fn canonical_desired_paths_normalizes_paths_without_io() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other");

    let desired = canonical_desired_paths(dir.path(), &other);

    assert_eq!(desired.len(), 2);
    assert!(desired.contains(&crate::fs::path::clean_path(dir.path())));
    assert!(desired.contains(&crate::fs::path::clean_path(&other)));
}

#[test]
fn sync_watcher_paths_keeps_existing_panel_when_other_panel_missing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");

    let mut harness = WatcherHarness::new();
    harness.state.left_panel.set_path(dir.path().to_path_buf());
    harness.state.right_panel.set_path(missing);

    harness.sync();

    let watched = harness.watcher.as_ref().unwrap().watched_dirs();
    assert_eq!(watched, vec![dir.path().canonicalize().unwrap()]);
    assert!(
        harness.sync_state.last_synced.is_none(),
        "should not set last_synced when a panel path is missing"
    );
}

#[test]
fn path_parent_matches_keeps_raw_fallback_for_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let panel_path = dir.path().join("missing");
    let child = panel_path.join("file.txt");

    let panel = PanelState::new(panel_path);
    assert!(path_parent_matches(&child, &panel));
}

#[test]
fn watcher_upsert_uses_panel_sort_mode() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("small.txt");
    let big = dir.path().join("big.txt");
    fs::write(&small, b"s").unwrap();
    fs::write(&big, b"larger").unwrap();

    let mut panel = test_panel(dir.path());
    panel.set_sort_mode(SortMode::SizeDesc);
    assert!(apply_watcher_upsert_if_matches(&mut panel, &small));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &big));
    rebuild_visible_entries(&mut panel, None);

    let names: Vec<_> = panel
        .listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["..", "big.txt", "small.txt"]);
}

#[test]
fn watcher_skips_update_when_metadata_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("same.txt");
    fs::write(&file, b"content").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
    rebuild_visible_entries(&mut panel, None);
    assert_eq!(panel.listing.entries.len(), 2);

    assert!(!apply_watcher_upsert_if_matches(&mut panel, &file));
    assert_eq!(panel.listing.entries.len(), 2);
    assert_eq!(panel.listing.unfiltered_entries.len(), 2);
}

#[test]
fn watcher_updates_when_metadata_changes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("change.txt");
    fs::write(&file, b"old").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));

    fs::write(&file, b"new longer content").unwrap();
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
    rebuild_visible_entries(&mut panel, None);

    assert_eq!(panel.listing.entries.len(), 2);
    assert_eq!(panel.total_size(), 18);
}

// Creates OVERFLOW_EVENT_COUNT files on disk, making it slower than typical unit tests.
// The I/O cost is intentional: real paths are needed to verify that exactly
// MAX_WATCHER_EVENTS_PER_POLL events are drained and the overflow event remains in the channel.
#[test]
fn poll_watcher_events_processes_at_most_256_events() {
    let dir = tempfile::tempdir().unwrap();
    // NOTE: this deliberately uses an unbounded mpsc::channel() rather than
    // sync_channel(WATCHER_CHANNEL_CAPACITY). A bounded channel would block the
    // test thread on the 257th send (backpressure) before poll_watcher_events
    // drains anything, deadlocking the test.
    let (tx, rx) = mpsc::channel();
    let mut state = AppState::new();
    state.left_panel = test_panel(dir.path());
    state.right_panel = test_panel(dir.path());

    for idx in 0..OVERFLOW_EVENT_COUNT {
        let file = dir.path().join(format!("file{idx}.txt"));
        fs::write(&file, b"x").unwrap();
        tx.send(WatchEvent::Created(file)).unwrap();
    }

    assert!(poll_watcher_events(&mut state, &rx));

    let left_names: Vec<_> = state
        .left_panel
        .listing
        .unfiltered_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        state.left_panel.listing.unfiltered_entries.len(),
        OVERFLOW_EVENT_COUNT,
    );
    assert!(left_names.contains(&".."));
    assert!(left_names.contains(&"file0.txt"));
    assert!(!left_names.contains(&"file256.txt"));
    assert!(rx.try_recv().is_ok());
}

#[test]
fn full_refresh_preserves_selected_entries() {
    let dir = tempfile::tempdir().unwrap();
    let selected = dir.path().join("selected.txt");
    fs::write(&selected, b"selected").unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &selected));
    rebuild_visible_entries(&mut panel, None);
    select_entry_by_name(&mut panel.listing.entries, "selected.txt");
    panel.sync_unfiltered_selection();

    refresh_panel_from_disk(&mut panel);

    assert!(
        panel
            .listing
            .unfiltered_entries
            .iter()
            .any(|entry| entry.path == selected && entry.selected)
    );
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 8);
}

#[test]
fn overflow_event_triggers_full_refresh_on_both_panels() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = EventHarness::new(dir.path());

    let existing = dir.path().join("existing.txt");
    fs::write(&existing, b"old").unwrap();

    harness.send(WatchEvent::Overflow);
    harness.poll();

    assert!(
        harness
            .state
            .left_panel
            .listing
            .unfiltered_entries
            .iter()
            .any(|e| e.name == "existing.txt"),
        "left panel should have file after Overflow refresh"
    );
    assert!(
        harness
            .state
            .right_panel
            .listing
            .unfiltered_entries
            .iter()
            .any(|e| e.name == "existing.txt"),
        "right panel should have file after Overflow refresh"
    );
}

#[test]
fn deleted_panel_dir_navigates_to_parent_and_refreshes() {
    let parent = tempfile::tempdir().unwrap();
    let child = parent.path().join("child_dir");
    fs::create_dir(&child).unwrap();

    let mut harness = EventHarness::new(parent.path());
    harness.state.left_panel = test_panel(&child);
    let child_canonical = child.canonicalize().unwrap();
    assert_eq!(
        harness
            .state
            .left_panel
            .canonical_path()
            .map(|p| p.to_path_buf()),
        Some(child_canonical.clone())
    );

    fs::remove_dir(&child).unwrap();

    harness.send(WatchEvent::Deleted(child_canonical));

    let dirty = harness.poll();
    assert!(dirty);
    assert_eq!(harness.state.left_panel.path(), parent.path());
    assert_eq!(
        harness
            .state
            .left_panel
            .canonical_path()
            .map(|p| p.to_path_buf()),
        parent.path().canonicalize().ok()
    );
    assert!(
        !harness
            .state
            .left_panel
            .listing
            .unfiltered_entries
            .is_empty(),
        "panel should have refreshed entries from parent"
    );
}

#[test]
fn renamed_panel_dir_updates_path_and_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let old_name = dir.path().join("old_name");
    let new_name = dir.path().join("new_name");
    fs::create_dir(&old_name).unwrap();

    let mut harness = EventHarness::new(dir.path());
    harness.state.left_panel = test_panel(&old_name);

    let marker = old_name.join("marker.txt");
    fs::write(&marker, b"x").unwrap();

    let old_canonical = old_name.canonicalize().unwrap();

    fs::rename(&old_name, &new_name).unwrap();

    harness.send(WatchEvent::Renamed {
        from: old_canonical,
        to: new_name.clone(),
    });

    let dirty = harness.poll();
    assert!(dirty);
    assert_eq!(harness.state.left_panel.path(), new_name);
    // TODO: assert that panel entries were refreshed from new_name (e.g. that
    // marker.txt is present) to verify the rename triggers a full reload, not
    // just a path update.
}

#[test]
fn full_refresh_on_error_clears_entries_and_resets_viewport() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = test_panel(dir.path());
    let file = dir.path().join("file.txt");
    fs::write(&file, b"data").unwrap();
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
    rebuild_visible_entries(&mut panel, None);
    assert!(panel.listing.entries.len() > 1);

    // Hardcoded nonexistent path — test-only fixture to trigger a read error; must not appear in production code.
    // TODO: use a tempdir with revoked permissions instead to avoid cross-test path collision risk.
    panel.set_path(PathBuf::from("/nonexistent_dir_for_test_12345"));
    refresh_panel_from_disk(&mut panel);

    assert!(panel.listing.entries.is_empty());
    assert!(panel.listing.unfiltered_entries.is_empty());
    assert!(panel.listing.path_index.is_empty());
    assert!(panel.listing.unfiltered_dirty);
    assert!(!panel.listing.needs_rebuild);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
    assert!(
        panel.last_error().is_some(),
        "should set last_error on read failure"
    );
}

#[test]
fn full_refresh_recovers_after_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = test_panel(dir.path());
    let file = dir.path().join("recovery.txt");
    fs::write(&file, b"hello").unwrap();

    // Hardcoded nonexistent path — test-only fixture to simulate an unreadable directory; must not appear in production code.
    // TODO: use a tempdir with revoked permissions instead to avoid cross-test path collision risk.
    panel.set_path(PathBuf::from("/nonexistent_for_error_test_xyz"));
    refresh_panel_from_disk(&mut panel);
    assert!(panel.listing.entries.is_empty());

    panel.set_path(dir.path().to_path_buf());
    refresh_panel_from_disk(&mut panel);

    assert!(
        !panel.listing.entries.is_empty(),
        "should have entries after recovery"
    );
    assert!(
        panel.last_error().is_none(),
        "last_error should be cleared on success"
    );
    assert!(
        panel
            .listing
            .unfiltered_entries
            .iter()
            .any(|e| e.name == "recovery.txt"),
        "should contain the file"
    );
}

#[test]
fn sync_watcher_paths_succeeds_with_two_valid_dirs() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let mut harness = WatcherHarness::new();
    harness.state.left_panel = test_panel(dir1.path());
    harness.state.right_panel = test_panel(dir2.path());

    harness.sync();

    assert!(
        harness.sync_state.last_synced.is_some(),
        "should set last_synced on successful sync"
    );
    let synced_paths = harness.sync_state.last_synced.unwrap();
    assert_eq!(synced_paths.0, harness.state.left_panel.path());
    assert_eq!(synced_paths.1, harness.state.right_panel.path());

    let watched = harness.watcher.as_ref().unwrap().watched_dirs();
    assert_eq!(watched.len(), 2, "should watch both panel dirs");
}

#[test]
fn event_is_panel_dir_cached_uses_injected_canonical_without_io() {
    let injected = PathBuf::from("/fake/canonical_that_does_not_exist_on_disk");
    let cache = PanelCache {
        path: PathBuf::from("/some/panel"),
        clean: PathBuf::from("/some/panel"),
        canonical: Some(injected.clone()),
        canonical_clean: None,
    };
    // The cached canonical does not exist on disk; canonicalize() would fail.
    // Returning true proves the function matched via the cache, not via fresh I/O.
    assert!(event_is_panel_dir_cached(&injected, &cache));
    // A child file path should not be treated as the panel directory itself.
    assert!(!event_is_panel_dir_cached(
        Path::new("/some/panel/child.txt"),
        &cache,
    ));
}

#[test]
fn set_path_updates_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    // Hardcoded nonexistent path — test-only fixture to verify canonical_path() is None for missing dirs; must not appear in production code.
    // TODO: use a tempdir (then drop it) instead to avoid cross-test path collision risk.
    let mut panel = PanelState::new(PathBuf::from("/nonexistent"));

    assert!(panel.canonical_path().is_none());
    panel.set_path(dir.path().to_path_buf());
    assert_eq!(panel.path(), dir.path());
    assert_eq!(
        panel.canonical_path(),
        dir.path().canonicalize().ok().as_deref()
    );
}

#[test]
fn deleted_child_file_removes_from_panel() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("to_delete.txt");
    fs::write(&file, b"data").unwrap();

    let mut harness = EventHarness::new(dir.path());
    assert!(apply_watcher_upsert_if_matches(
        &mut harness.state.left_panel,
        &file
    ));
    rebuild_visible_entries(&mut harness.state.left_panel, None);
    assert!(
        harness
            .state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "to_delete.txt")
    );

    fs::remove_file(&file).unwrap();
    harness.send(WatchEvent::Deleted(file.clone()));

    let dirty = harness.poll();
    assert!(dirty);
    assert!(
        !harness
            .state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "to_delete.txt"),
        "deleted file should be removed from entries"
    );
}

#[test]
fn created_child_file_appears_in_panel() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = EventHarness::new(dir.path());

    let new_file = dir.path().join("new_file.txt");
    fs::write(&new_file, b"hello").unwrap();

    harness.send(WatchEvent::Created(new_file));

    let dirty = harness.poll();
    assert!(dirty);
    assert!(
        harness
            .state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "new_file.txt"),
        "created file should appear in entries"
    );
}

#[test]
fn deleted_root_dir_stays_at_root_and_refreshes() {
    let (tx, rx) = mpsc::sync_channel(WATCHER_CHANNEL_CAPACITY);
    let mut state = AppState::new();
    // Hardcoded filesystem root — test-only fixture for the edge case where the panel is at "/";
    // safe because "/" always exists, but this pattern must not appear in production code.
    // TODO: "/" is fine, but the other hardcoded paths above should migrate to tempdir fixtures.
    state.left_panel.set_path(PathBuf::from("/"));

    tx.send(WatchEvent::Deleted(PathBuf::from("/"))).unwrap();

    let dirty = poll_watcher_events(&mut state, &rx);
    assert!(dirty, "should be dirty after root deletion event");
    assert_eq!(
        state.left_panel.path(),
        PathBuf::from("/"),
        "should stay at root since parent() is None"
    );
}

#[test]
fn sync_watcher_paths_does_not_set_last_synced_when_path_missing() {
    // TODO: the three sync_watcher_paths_*missing/cooldown scenarios below share
    // near-identical setup (missing panel dir + sync + assert cooldown). Consider
    // parameterizing them (e.g. a #[test] table or a shared harness builder) to
    // isolate only the differing expectation per case.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent_panel_dir");

    let mut harness = WatcherHarness::new();
    harness.state.left_panel.set_path(missing);
    harness.state.right_panel.set_path(dir.path().to_path_buf());

    harness.sync();

    assert!(
        harness.sync_state.last_synced.is_none(),
        "should not set last_synced when a panel path does not exist"
    );
    assert!(
        harness.sync_state.failed_cooldown.is_some(),
        "should set cooldown on failure"
    );
}

#[test]
fn sync_watcher_paths_cooldown_skips_early_retries() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent_panel_dir");

    let mut harness = WatcherHarness::new();
    harness.state.left_panel.set_path(missing);
    harness.state.right_panel.set_path(dir.path().to_path_buf());

    harness.sync();
    assert!(harness.sync_state.failed_cooldown.is_some());

    let watched_before = harness.watcher.as_ref().unwrap().watched_dirs().len();

    harness.sync();

    let watched_after = harness.watcher.as_ref().unwrap().watched_dirs().len();
    assert_eq!(
        watched_before, watched_after,
        "cooldown should prevent redundant watch attempts"
    );
}

#[test]
fn sync_watcher_paths_cooldown_allows_different_paths() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let missing = dir1.path().join("nonexistent");

    let mut harness = WatcherHarness::new();
    harness.state.left_panel.set_path(missing);
    harness
        .state
        .right_panel
        .set_path(dir1.path().to_path_buf());

    harness.sync();
    assert!(harness.sync_state.failed_cooldown.is_some());

    // Navigate to valid directory — cooldown should NOT block
    harness.state.left_panel.set_path(dir2.path().to_path_buf());
    harness.sync();

    assert!(
        harness.sync_state.last_synced.is_some(),
        "different paths should bypass cooldown"
    );
}

// TODO: missing coverage — add tests for symlink handling (symlinked panel dir,
// symlink targets changing, broken symlinks during upsert/remove).
// TODO: missing coverage — add a test for the race between poll_watcher_events
// draining and concurrent panel navigation (path changes mid-poll) to verify
// stale events do not mutate a panel that has already navigated away.
