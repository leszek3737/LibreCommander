use super::*;
use crate::app::types::{Direction, ListingState, SortField, SortMode};
use std::fs;
use std::sync::Arc;
use std::sync::mpsc;

const WATCHER_CHANNEL_CAPACITY: usize = 256;
const OVERFLOW_EVENT_COUNT: usize = WATCHER_CHANNEL_CAPACITY + 1;

fn test_panel(path: &Path) -> PanelState {
    let mut panel = PanelState::new(path.to_path_buf());
    panel.set_entries(vec![parent_entry(path)]);
    panel.listing.force_state(ListingState::Clean);
    panel
}

fn parent_entry(path: &Path) -> reader::FileEntry {
    use crate::app::types::test_helpers::TestEntry;
    let mut e = TestEntry::new("..")
        .path(path.parent().unwrap_or(path))
        .permissions(0o755)
        .build();
    e.cha.set_executable(true);
    e
}

fn select_entry_by_name(entries: &mut [reader::FileEntry], name: &str) {
    entries
        .iter_mut()
        .find(|e| e.name == name)
        .expect("entry should exist")
        .selected = true;
}

fn build_panel_with_files(dir: &Path, files: &[(&str, &[u8])]) -> PanelState {
    let mut panel = test_panel(dir);
    for (name, contents) in files {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        assert!(apply_watcher_upsert_if_matches(&mut panel, &path));
    }
    rebuild(&mut panel);
    panel
}

fn assert_entry_names_eq(panel: &PanelState, expected: &[&str]) {
    let names: Vec<_> = panel.listing.filtered().map(|e| e.name.as_str()).collect();
    assert_eq!(names, expected);
}

fn assert_has_entry(entries: &[reader::FileEntry], name: &str) {
    assert!(
        entries.iter().any(|e| e.name == name),
        "expected entry `{name}`"
    );
}

fn assert_no_entry(entries: &[reader::FileEntry], name: &str) {
    assert!(
        !entries.iter().any(|e| e.name == name),
        "did not expect entry `{name}`"
    );
}

fn assert_visible_has_entry(panel: &PanelState, name: &str) {
    assert!(
        panel.listing.filtered().any(|e| e.name == name),
        "expected visible entry `{name}`"
    );
}

fn assert_visible_no_entry(panel: &PanelState, name: &str) {
    assert!(
        !panel.listing.filtered().any(|e| e.name == name),
        "did not expect visible entry `{name}`"
    );
}

fn assert_entry_counts(panel: &PanelState, visible: usize, unfiltered: usize) {
    assert_eq!(panel.listing.filtered_len(), visible, "visible entry count");
    assert_eq!(
        panel.listing.unfiltered().len(),
        unfiltered,
        "unfiltered entry count"
    );
}

fn rebuild(panel: &mut PanelState) {
    rebuild_visible_entries(panel, None);
}

fn pin_cooldown_far_future(sync_state: &mut WatcherSyncState) {
    // Mock time: keep the cooldown deadline far enough ahead that it stays
    // active regardless of wall-clock delays on slow machines, while preserving
    // the panel paths captured by the real failed sync.
    if let Some((_, fl, fr)) = sync_state.failed_cooldown.take() {
        sync_state.failed_cooldown = Some((Instant::now() + Duration::from_secs(3600), fl, fr));
    }
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

fn setup_cooldown_harness() -> (tempfile::TempDir, WatcherHarness) {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent_panel_dir");
    let mut harness = WatcherHarness::new();
    harness.state.left_panel.set_path(missing);
    harness.state.right_panel.set_path(dir.path().to_path_buf());
    harness.sync();
    pin_cooldown_far_future(&mut harness.sync_state);
    (dir, harness)
}

#[test]
fn watcher_upsert_adds_visible_entry_sorted_and_updates_stats() {
    let dir = tempfile::tempdir().unwrap();
    let panel = build_panel_with_files(
        dir.path(),
        &[("beta.txt", b"beta"), ("alpha.txt", b"alpha")],
    );

    assert_entry_names_eq(&panel, &["..", "alpha.txt", "beta.txt"]);
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
    rebuild(&mut panel);
    select_entry_by_name(panel.listing.unfiltered_mut(), "keep.txt");
    panel.recalculate_selection_stats();

    fs::write(&keep, b"updated").unwrap();
    assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &drop));
    rebuild(&mut panel);

    assert_eq!(panel.listing.filtered_len(), 2);
    let keep_entry = panel
        .listing
        .filtered()
        .find(|e| e.name == "keep.txt")
        .unwrap();
    assert!(keep_entry.selected);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 7);
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
    assert_entry_counts(&panel, 1, 1);
}

#[test]
fn watcher_remove_updates_visible_entries_and_clamps_cursor_scroll() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = build_panel_with_files(dir.path(), &[("a.txt", b"a"), ("b.txt", b"b")]);
    panel.cursor = 2;
    panel.scroll_offset = 2;

    let b_path = dir.path().join("b.txt");
    assert!(apply_watcher_remove_if_matches(&mut panel, &b_path));
    rebuild(&mut panel);

    assert_entry_names_eq(&panel, &["..", "a.txt"]);
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
    rebuild(&mut panel);
    assert_entry_counts(&panel, 1, 1);
}

#[test]
fn canonical_desired_paths_matches_watch_key_representation() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other"); // does not exist

    let desired = canonical_desired_paths(dir.path(), &other);

    assert_eq!(desired.len(), 2);
    // An existing directory is canonicalized, matching the key `Watcher::watch`
    // stores, so a symlinked panel dir is not perpetually unwatched+rewatched.
    assert!(desired.contains(&dir.path().canonicalize().unwrap()));
    // A non-existent path falls back to the lexical clean path.
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
    let cache = PanelCache::from_panel(&panel);
    assert!(path_parent_matches_cached(&child, &cache));
}

#[test]
fn watcher_upsert_uses_panel_sort_mode() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("small.txt");
    let big = dir.path().join("big.txt");
    fs::write(&small, b"s").unwrap();
    fs::write(&big, b"larger").unwrap();

    let mut panel = test_panel(dir.path());
    panel.set_sort_mode(SortMode::new(SortField::Size, Direction::Desc));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &small));
    assert!(apply_watcher_upsert_if_matches(&mut panel, &big));
    rebuild(&mut panel);

    assert_entry_names_eq(&panel, &["..", "big.txt", "small.txt"]);
}

#[test]
fn watcher_skips_update_when_metadata_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = build_panel_with_files(dir.path(), &[("same.txt", b"content")]);
    assert_eq!(panel.listing.filtered_len(), 2);

    let file = dir.path().join("same.txt");
    assert!(!apply_watcher_upsert_if_matches(&mut panel, &file));
    assert_entry_counts(&panel, 2, 2);
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
    rebuild(&mut panel);

    assert_eq!(panel.listing.filtered_len(), 2);
    assert_eq!(panel.total_size(), 18);
}

#[test]
fn poll_watcher_events_processes_at_most_256_events() {
    let dir = tempfile::tempdir().unwrap();
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

    let entries = state.left_panel.listing.unfiltered();
    assert_eq!(entries.len(), OVERFLOW_EVENT_COUNT);
    assert_has_entry(entries, "..");
    assert_has_entry(entries, "file0.txt");
    assert_no_entry(entries, "file256.txt");
    assert!(rx.try_recv().is_ok());
}

#[test]
fn full_refresh_preserves_selected_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = build_panel_with_files(dir.path(), &[("selected.txt", b"selected")]);
    select_entry_by_name(panel.listing.unfiltered_mut(), "selected.txt");
    panel.recalculate_selection_stats();

    let selected = dir.path().join("selected.txt");
    refresh_panel_from_disk(&mut panel);

    assert!(
        panel
            .listing
            .unfiltered()
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

    assert_has_entry(
        harness.state.left_panel.listing.unfiltered(),
        "existing.txt",
    );
    assert_has_entry(
        harness.state.right_panel.listing.unfiltered(),
        "existing.txt",
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
        !harness.state.left_panel.listing.unfiltered().is_empty(),
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
    assert_has_entry(harness.state.left_panel.listing.unfiltered(), "marker.txt");
}

#[test]
fn full_refresh_on_error_clears_entries_and_resets_viewport() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = test_panel(dir.path());
    let file = dir.path().join("file.txt");
    fs::write(&file, b"data").unwrap();
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
    rebuild(&mut panel);
    assert!(panel.listing.filtered_len() > 1);

    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_path_buf();
    drop(gone);
    panel.set_path(gone_path);
    refresh_panel_from_disk(&mut panel);

    assert!(panel.listing.filtered_is_empty());
    assert!(panel.listing.unfiltered().is_empty());
    assert_eq!(panel.listing.state(), ListingState::NeedsFullRead);
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

    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_path_buf();
    drop(gone);
    panel.set_path(gone_path);
    refresh_panel_from_disk(&mut panel);
    assert!(panel.listing.filtered_is_empty());

    panel.set_path(dir.path().to_path_buf());
    refresh_panel_from_disk(&mut panel);

    assert!(
        !panel.listing.filtered_is_empty(),
        "should have entries after recovery"
    );
    assert!(
        panel.last_error().is_none(),
        "last_error should be cleared on success"
    );
    assert_has_entry(panel.listing.unfiltered(), "recovery.txt");
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
    assert!(event_is_panel_dir_cached(&injected, &mut None, &cache));
    assert!(!event_is_panel_dir_cached(
        Path::new("/some/panel/child.txt"),
        &mut None,
        &cache,
    ));
}

#[test]
fn set_path_updates_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_path_buf();
    drop(gone);
    let mut panel = PanelState::new(gone_path);

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
    rebuild(&mut harness.state.left_panel);
    assert_visible_has_entry(&harness.state.left_panel, "to_delete.txt");

    fs::remove_file(&file).unwrap();
    harness.send(WatchEvent::Deleted(file.clone()));

    let dirty = harness.poll();
    assert!(dirty);
    assert_visible_no_entry(&harness.state.left_panel, "to_delete.txt");
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
    assert_visible_has_entry(&harness.state.left_panel, "new_file.txt");
}

#[test]
fn deleted_root_dir_stays_at_root_and_refreshes() {
    let (tx, rx) = mpsc::sync_channel(WATCHER_CHANNEL_CAPACITY);
    let mut state = AppState::new();
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
    let (_dir, harness) = setup_cooldown_harness();

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
    let (_dir, mut harness) = setup_cooldown_harness();
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

    harness.state.left_panel.set_path(dir2.path().to_path_buf());
    harness.sync();

    assert!(
        harness.sync_state.last_synced.is_some(),
        "different paths should bypass cooldown"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_panel_dir_tracks_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    let link_dir = dir.path().join("link");
    symlink(&real_dir, &link_dir).unwrap();

    let file_via_link = link_dir.join("inside.txt");
    fs::write(&file_via_link, b"data").unwrap();

    let mut panel = test_panel(&link_dir);
    assert!(apply_watcher_upsert_if_matches(&mut panel, &file_via_link));
    rebuild(&mut panel);

    assert_entry_names_eq(&panel, &["..", "inside.txt"]);
}

#[cfg(unix)]
#[test]
fn symlink_target_change_detected() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target_a = dir.path().join("target_a.txt");
    let target_b = dir.path().join("target_b.txt");
    fs::write(&target_a, b"aaa").unwrap();
    fs::write(&target_b, b"bbb").unwrap();

    let link = dir.path().join("link.txt");
    symlink(&target_a, &link).unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &link));
    rebuild(&mut panel);
    assert_visible_has_entry(&panel, "link.txt");

    fs::remove_file(&link).unwrap();
    symlink(&target_b, &link).unwrap();
    apply_watcher_upsert_if_matches(&mut panel, &link);
    rebuild(&mut panel);

    assert_visible_has_entry(&panel, "link.txt");
}

#[cfg(unix)]
#[test]
fn broken_symlink_handled_gracefully() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let dangling = dir.path().join("dangling.txt");
    symlink(dir.path().join("nonexistent_target"), &dangling).unwrap();

    let mut panel = test_panel(dir.path());
    assert!(apply_watcher_upsert_if_matches(&mut panel, &dangling));
    rebuild(&mut panel);

    assert_visible_has_entry(&panel, "dangling.txt");
}

#[test]
fn stale_events_ignored_after_path_change() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let file_a = dir_a.path().join("stale.txt");
    fs::write(&file_a, b"stale").unwrap();

    let (tx, rx) = mpsc::sync_channel(WATCHER_CHANNEL_CAPACITY);
    let mut state = AppState::new();
    state.left_panel = test_panel(dir_a.path());
    state.right_panel = test_panel(dir_a.path());

    tx.send(WatchEvent::Created(file_a)).unwrap();

    state.left_panel.set_path(dir_b.path().to_path_buf());
    state.right_panel.set_path(dir_b.path().to_path_buf());

    poll_watcher_events(&mut state, &rx);
    assert_visible_no_entry(&state.left_panel, "stale.txt");
    assert_visible_no_entry(&state.right_panel, "stale.txt");
}
