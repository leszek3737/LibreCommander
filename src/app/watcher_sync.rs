use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::app::panel_ops::update_panel_read_errors;
use crate::app::types::{AppState, PanelState};
use crate::debug_log;
use crate::fs::reader;
use crate::fs::watcher::{WatchEvent, Watcher};

pub fn sync_watcher_paths(
    watcher: &mut Option<Watcher>,
    state: &AppState,
    last_synced: &mut Option<(PathBuf, PathBuf)>,
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };

    if let Some((l, r)) = last_synced.as_ref()
        && l == &state.left_panel.path
        && r == &state.right_panel.path
    {
        return;
    }

    let left = state.left_panel.path.clone();
    let right = state.right_panel.path.clone();

    let (desired, all_paths_present) = canonical_desired_paths(&left, &right);
    let current: HashSet<PathBuf> = watcher.watched_dirs().into_iter().collect();

    let mut had_error = false;
    for path in current.difference(&desired) {
        if let Err(err) = watcher.unwatch(path) {
            debug_log!("Watcher unwatch failed for {}: {err}", path.display());
            had_error = true;
        }
    }
    for path in desired.difference(&current) {
        if let Err(err) = watcher.watch(path) {
            debug_log!("Watcher watch failed for {}: {err}", path.display());
            had_error = true;
        }
    }

    if had_error || !all_paths_present {
        *last_synced = None;
    } else {
        *last_synced = Some((left, right));
    }
}

fn canonical_desired_paths(left: &Path, right: &Path) -> (HashSet<PathBuf>, bool) {
    let mut desired = HashSet::new();
    let mut all_paths_present = true;
    for path in [left, right] {
        match path.canonicalize() {
            Ok(path) => {
                desired.insert(path);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                all_paths_present = false;
                debug_log!(
                    "Watcher sync skipped missing path {}: {err}",
                    path.display()
                );
            }
            Err(err) => {
                all_paths_present = false;
                debug_log!(
                    "Watcher sync skipped non-canonicalizable path {}: {err}",
                    path.display()
                );
            }
        }
    }
    (desired, all_paths_present)
}

pub fn poll_watcher_events(state: &mut AppState, receiver: &Receiver<WatchEvent>) -> bool {
    const MAX_WATCHER_EVENTS_PER_POLL: usize = 256;

    let mut dirty = false;
    let mut left_needs_full_refresh = false;
    let mut right_needs_full_refresh = false;

    for _ in 0..MAX_WATCHER_EVENTS_PER_POLL {
        let Ok(event) = receiver.try_recv() else {
            break;
        };
        match event {
            WatchEvent::Created(path) | WatchEvent::Modified(path) => {
                if event_is_panel_dir(&path, &state.left_panel) {
                    left_needs_full_refresh = true;
                    dirty = true;
                }
                if event_is_panel_dir(&path, &state.right_panel) {
                    right_needs_full_refresh = true;
                    dirty = true;
                }
                dirty |= apply_watcher_upsert_if_matches(&mut state.left_panel, &path);
                dirty |= apply_watcher_upsert_if_matches(&mut state.right_panel, &path);
            }
            WatchEvent::Deleted(path) => {
                if event_is_panel_dir(&path, &state.left_panel) {
                    if let Some(parent) = state.left_panel.path.parent().map(Path::to_path_buf) {
                        state.left_panel.set_path(parent);
                    }
                    left_needs_full_refresh = true;
                    dirty = true;
                }
                if event_is_panel_dir(&path, &state.right_panel) {
                    if let Some(parent) = state.right_panel.path.parent().map(Path::to_path_buf) {
                        state.right_panel.set_path(parent);
                    }
                    right_needs_full_refresh = true;
                    dirty = true;
                }
                dirty |= apply_watcher_remove_if_matches(&mut state.left_panel, &path);
                dirty |= apply_watcher_remove_if_matches(&mut state.right_panel, &path);
            }
            WatchEvent::Renamed { from, to } => {
                if event_is_panel_dir(&from, &state.left_panel) {
                    state.left_panel.set_path(to.clone());
                    left_needs_full_refresh = true;
                    dirty = true;
                }
                if event_is_panel_dir(&from, &state.right_panel) {
                    state.right_panel.set_path(to.clone());
                    right_needs_full_refresh = true;
                    dirty = true;
                }
                if event_is_panel_dir(&to, &state.left_panel)
                    || event_is_panel_dir(&to, &state.right_panel)
                {
                    dirty = true;
                }
                dirty |= apply_watcher_remove_if_matches(&mut state.left_panel, &from);
                dirty |= apply_watcher_remove_if_matches(&mut state.right_panel, &from);
                dirty |= apply_watcher_upsert_if_matches(&mut state.left_panel, &to);
                dirty |= apply_watcher_upsert_if_matches(&mut state.right_panel, &to);
            }
            WatchEvent::Overflow => {
                left_needs_full_refresh = true;
                right_needs_full_refresh = true;
                dirty = true;
            }
        }
    }

    if left_needs_full_refresh {
        full_refresh_panel(&mut state.left_panel);
    }
    if right_needs_full_refresh {
        full_refresh_panel(&mut state.right_panel);
    }

    if state.left_panel.listing.needs_rebuild {
        state.left_panel.listing.needs_rebuild = false;
        rebuild_visible_entries(&mut state.left_panel, None);
        dirty = true;
    }
    if state.right_panel.listing.needs_rebuild {
        state.right_panel.listing.needs_rebuild = false;
        rebuild_visible_entries(&mut state.right_panel, None);
        dirty = true;
    }

    dirty
}

pub fn apply_watcher_upsert_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, &panel.path, panel.canonical_path.as_deref()) {
        return false;
    }

    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_upsert(panel, &path)
}

pub fn apply_watcher_remove_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, &panel.path, panel.canonical_path.as_deref()) {
        return false;
    }

    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_remove(panel, &path)
}

fn panel_event_path(panel: &PanelState, path: &Path) -> Option<PathBuf> {
    path.file_name().map(|name| panel.path.join(name))
}

fn event_is_panel_dir(path: &Path, panel: &PanelState) -> bool {
    if path == panel.path {
        return true;
    }

    if path.parent() == Some(panel.path.as_path()) {
        return false;
    }

    let panel_canonical = match &panel.canonical_path {
        Some(c) => c.clone(),
        None => match panel.path.canonicalize() {
            Ok(c) => c,
            Err(_) => return false,
        },
    };

    if path == panel_canonical {
        return true;
    }

    path.canonicalize().is_ok_and(|p| p == panel_canonical)
}

fn path_parent_matches(path: &Path, panel_path: &Path, panel_canonical: Option<&Path>) -> bool {
    if path.file_name().is_none() {
        return false;
    }

    let Some(parent) = path.parent() else {
        return false;
    };

    if parent == panel_path {
        return true;
    }

    if panel_canonical == Some(parent) {
        return true;
    }

    let parent_clean = crate::fs::path::clean_path(parent);
    let panel_clean = crate::fs::path::clean_path(panel_path);

    if parent_clean == panel_clean {
        return true;
    }

    if let Some(canonical) = panel_canonical {
        let canonical_clean = crate::fs::path::clean_path(canonical);
        if parent_clean == canonical_clean {
            return true;
        }
    }

    false
}

fn apply_watcher_upsert(panel: &mut PanelState, path: &Path) -> bool {
    let Ok(mut entry) = reader::get_file_info(path) else {
        return false;
    };

    if !panel.show_hidden && entry.is_hidden() {
        return apply_watcher_remove(panel, path);
    }

    reader::ensure_path_index(panel);
    let existing = panel
        .listing
        .path_index
        .get(&entry.path)
        .and_then(|&idx| panel.listing.unfiltered_entries.get(idx));
    if let Some(existing) = existing {
        if existing.cha.hits(&entry.cha) {
            return false;
        }
        entry.selected = existing.selected;
    }

    reader::upsert_entry(panel, entry);
    panel.listing.mark_dirty();
    true
}

fn apply_watcher_remove(panel: &mut PanelState, path: &Path) -> bool {
    reader::ensure_path_index(panel);
    let existed = panel.listing.path_index.contains_key(path);
    if existed {
        reader::remove_entry(panel, path);
        panel.listing.mark_dirty();
    }

    existed
}

fn full_refresh_panel(panel: &mut PanelState) {
    let current_name = panel
        .listing
        .entries
        .get(panel.cursor)
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.name.clone());
    let saved: HashSet<PathBuf> = panel
        .selected_entries()
        .into_iter()
        .map(|e| e.path.clone())
        .collect();

    match reader::read_directory(&panel.path) {
        Ok((entries, errors)) => {
            update_panel_read_errors(panel, &errors);
            panel.listing.set_unfiltered(entries);
            panel.canonical_path = panel.path.canonicalize().ok();
            for entry in &mut panel.listing.unfiltered_entries {
                entry.selected = saved.contains(&entry.path);
            }
            rebuild_visible_entries(panel, current_name.as_deref());
        }
        Err(err) => {
            panel.listing.clear();
            // Do NOT set needs_full_refresh here — that would cause
            // poll_watcher_events to retry on every tick, creating an
            // infinite loop when the directory is permanently gone.
            // Retry happens naturally when a new watcher event arrives
            // for this panel directory (Created/Modified triggers
            // left_needs_full_refresh/right_needs_full_refresh).
            panel.last_error = Some(err.to_string());
        }
    }
}

fn rebuild_visible_entries(panel: &mut PanelState, preferred_name: Option<&str>) {
    let current_name = panel
        .listing
        .entries
        .get(panel.cursor)
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.name.clone())
        .or_else(|| preferred_name.map(str::to_string));

    crate::app::panel_ops::rebuild_visible_entries(
        panel,
        crate::app::panel_ops::current_visible_height(),
    );

    if let Some(name) = current_name.as_deref()
        && let Some(pos) = panel
            .listing
            .entries
            .iter()
            .position(|entry| entry.name == name)
    {
        panel.cursor = pos;
    }

    if panel.listing.entries.is_empty() {
        panel.cursor = 0;
        panel.scroll_offset = 0;
    } else if panel.cursor >= panel.listing.entries.len() {
        panel.cursor = panel.listing.entries.len() - 1;
    }

    let max_scroll = panel.listing.entries.len().saturating_sub(1);
    if panel.scroll_offset > max_scroll {
        panel.scroll_offset = max_scroll;
    }
    if panel.scroll_offset > panel.cursor {
        panel.scroll_offset = panel.cursor;
    }
    panel.recalculate_selection_stats();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::app::types::SortMode;
    use std::fs;
    use std::sync::Arc;
    use std::sync::mpsc;

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
        assert_eq!(panel.total_size, 9);
    }

    #[test]
    fn watcher_upsert_respects_filter_and_preserves_selection() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("keep.txt");
        let drop = dir.path().join("drop.log");
        fs::write(&keep, b"old").unwrap();
        fs::write(&drop, b"drop").unwrap();

        let mut panel = test_panel(dir.path());
        panel.filter = Some("*.txt".to_string());
        assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
        rebuild_visible_entries(&mut panel, None);
        panel.listing.entries[1].selected = true;
        panel.sync_unfiltered_selection();

        fs::write(&keep, b"updated").unwrap();
        assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
        assert!(apply_watcher_upsert_if_matches(&mut panel, &drop));
        rebuild_visible_entries(&mut panel, None);

        assert_eq!(panel.listing.entries.len(), 2);
        assert_eq!(panel.listing.entries[1].name, "keep.txt");
        assert!(panel.listing.entries[1].selected);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 7);
        assert_eq!(panel.total_size, 11);
    }

    #[test]
    fn watcher_upsert_hides_hidden_when_hidden_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let hidden = dir.path().join(".secret");
        fs::write(&hidden, b"secret").unwrap();

        let mut panel = test_panel(dir.path());
        panel.show_hidden = false;

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
        assert_eq!(panel.total_size, 1);
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
    fn canonical_desired_paths_skips_missing_panel_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        let (desired, all_paths_present) = canonical_desired_paths(dir.path(), &missing);

        assert_eq!(desired.len(), 1);
        assert!(desired.contains(&dir.path().canonicalize().unwrap()));
        assert!(!all_paths_present);
    }

    #[test]
    fn sync_watcher_paths_keeps_existing_panel_when_other_panel_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let mut watcher = Some(Watcher::new(Arc::new(event_tx)).expect("create watcher"));
        let mut state = AppState::new();
        state.left_panel.set_path(dir.path().to_path_buf());
        state.right_panel.set_path(missing);
        let mut last_synced = None;

        sync_watcher_paths(&mut watcher, &state, &mut last_synced);

        let watched = watcher.as_ref().unwrap().watched_dirs();
        assert_eq!(watched, vec![dir.path().canonicalize().unwrap()]);
        assert!(
            last_synced.is_none(),
            "should not set last_synced when a panel path is missing"
        );
    }

    #[test]
    fn path_parent_matches_keeps_raw_fallback_for_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let panel_path = dir.path().join("missing");
        let child = panel_path.join("file.txt");

        assert!(path_parent_matches(&child, &panel_path, None));
    }

    #[test]
    fn watcher_upsert_uses_panel_sort_mode() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.txt");
        let big = dir.path().join("big.txt");
        fs::write(&small, b"s").unwrap();
        fs::write(&big, b"larger").unwrap();

        let mut panel = test_panel(dir.path());
        panel.sort_mode = SortMode::SizeDesc;
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
        assert_eq!(panel.total_size, 18);
    }

    #[test]
    fn poll_watcher_events_processes_at_most_256_events() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut state = AppState::new();
        state.left_panel = test_panel(dir.path());
        state.right_panel = test_panel(dir.path());

        for idx in 0..257 {
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
        assert_eq!(state.left_panel.listing.unfiltered_entries.len(), 257);
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
        panel.listing.entries[1].selected = true;
        panel.sync_unfiltered_selection();

        full_refresh_panel(&mut panel);

        assert!(
            panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.path == selected && entry.selected)
        );
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 8);
    }

    #[test]
    fn overflow_event_triggers_full_refresh_on_both_panels() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(256);

        let mut state = AppState::new();
        state.left_panel = test_panel(dir.path());
        state.right_panel = test_panel(dir.path());

        let existing = dir.path().join("existing.txt");
        fs::write(&existing, b"old").unwrap();

        tx.send(WatchEvent::Overflow).unwrap();

        poll_watcher_events(&mut state, &rx);

        assert!(
            state
                .left_panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|e| e.name == "existing.txt"),
            "left panel should have file after Overflow refresh"
        );
        assert!(
            state
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

        let (tx, rx) = mpsc::sync_channel(256);
        let mut state = AppState::new();
        state.left_panel = test_panel(&child);
        let child_canonical = child.canonicalize().unwrap();
        assert_eq!(
            state.left_panel.canonical_path,
            Some(child_canonical.clone())
        );

        fs::remove_dir(&child).unwrap();

        tx.send(WatchEvent::Deleted(child_canonical)).unwrap();

        let dirty = poll_watcher_events(&mut state, &rx);
        assert!(dirty);
        assert_eq!(state.left_panel.path, *parent.path());
        assert_eq!(
            state.left_panel.canonical_path,
            parent.path().canonicalize().ok()
        );
        assert!(
            !state.left_panel.listing.unfiltered_entries.is_empty(),
            "panel should have refreshed entries from parent"
        );
    }

    #[test]
    fn renamed_panel_dir_updates_path_and_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let old_name = dir.path().join("old_name");
        let new_name = dir.path().join("new_name");
        fs::create_dir(&old_name).unwrap();

        let (tx, rx) = mpsc::sync_channel(256);
        let mut state = AppState::new();
        state.left_panel = test_panel(&old_name);

        let marker = old_name.join("marker.txt");
        fs::write(&marker, b"x").unwrap();

        let old_canonical = old_name.canonicalize().unwrap();

        fs::rename(&old_name, &new_name).unwrap();

        tx.send(WatchEvent::Renamed {
            from: old_canonical,
            to: new_name.clone(),
        })
        .unwrap();

        let dirty = poll_watcher_events(&mut state, &rx);
        assert!(dirty);
        assert_eq!(state.left_panel.path, new_name);
    }

    #[test]
    fn full_refresh_on_error_clears_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut panel = test_panel(dir.path());
        let file = dir.path().join("file.txt");
        fs::write(&file, b"data").unwrap();
        assert!(apply_watcher_upsert_if_matches(&mut panel, &file));
        rebuild_visible_entries(&mut panel, None);
        assert!(panel.listing.entries.len() > 1);

        panel.set_path(PathBuf::from("/nonexistent_dir_for_test_12345"));
        full_refresh_panel(&mut panel);

        assert!(panel.listing.entries.is_empty());
        assert!(panel.listing.unfiltered_entries.is_empty());
        assert!(panel.listing.path_index.is_empty());
        assert!(panel.listing.unfiltered_dirty);
        assert!(!panel.listing.needs_rebuild);
        assert!(
            panel.last_error.is_some(),
            "should set last_error on read failure"
        );
    }

    #[test]
    fn full_refresh_recovers_after_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut panel = test_panel(dir.path());
        let file = dir.path().join("recovery.txt");
        fs::write(&file, b"hello").unwrap();

        panel.set_path(PathBuf::from("/nonexistent_for_error_test_xyz"));
        full_refresh_panel(&mut panel);
        assert!(panel.listing.entries.is_empty());

        panel.set_path(dir.path().to_path_buf());
        full_refresh_panel(&mut panel);

        assert!(
            !panel.listing.entries.is_empty(),
            "should have entries after recovery"
        );
        assert!(
            panel.last_error.is_none(),
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
    fn sync_watcher_paths_retries_on_watch_error() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let (event_tx, _rx) = mpsc::sync_channel(256);
        let mut watcher = Some(Watcher::new(Arc::new(event_tx)).expect("create watcher"));

        let mut state = AppState::new();
        state.left_panel = test_panel(dir1.path());
        state.right_panel = test_panel(dir2.path());

        let mut last_synced: Option<(PathBuf, PathBuf)> = None;

        sync_watcher_paths(&mut watcher, &state, &mut last_synced);

        assert!(
            last_synced.is_some(),
            "should set last_synced on successful sync"
        );
        let synced_paths = last_synced.unwrap();
        assert_eq!(synced_paths.0, state.left_panel.path);
        assert_eq!(synced_paths.1, state.right_panel.path);

        let watched = watcher.as_ref().unwrap().watched_dirs();
        assert_eq!(watched.len(), 2, "should watch both panel dirs");
    }

    #[test]
    fn sync_watcher_paths_sets_last_synced_when_all_paths_valid() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let (event_tx, _rx) = mpsc::sync_channel(256);
        let mut watcher = Some(Watcher::new(Arc::new(event_tx)).expect("create watcher"));

        let mut state = AppState::new();
        state.left_panel = test_panel(dir1.path());
        state.right_panel = test_panel(dir2.path());

        let mut last_synced: Option<(PathBuf, PathBuf)> = None;

        sync_watcher_paths(&mut watcher, &state, &mut last_synced);

        assert!(
            last_synced.is_some(),
            "should set last_synced when both paths exist and watches succeed"
        );
        let synced = last_synced.unwrap();
        assert_eq!(synced.0, state.left_panel.path);
        assert_eq!(synced.1, state.right_panel.path);
    }

    #[test]
    fn event_is_panel_dir_uses_cached_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let panel = test_panel(dir.path());
        let canonical = panel.canonical_path.clone().unwrap();

        assert!(event_is_panel_dir(&canonical, &panel));

        let file = dir.path().join("child.txt");
        assert!(!event_is_panel_dir(&file, &panel));
    }

    #[test]
    fn set_path_updates_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut panel = PanelState::new(PathBuf::from("/nonexistent"));

        assert!(panel.canonical_path.is_none());
        panel.set_path(dir.path().to_path_buf());
        assert_eq!(panel.path, dir.path());
        assert_eq!(panel.canonical_path, dir.path().canonicalize().ok());
    }

    #[test]
    fn deleted_child_file_removes_from_panel() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("to_delete.txt");
        fs::write(&file, b"data").unwrap();

        let (tx, rx) = mpsc::sync_channel(256);
        let mut state = AppState::new();
        state.left_panel = test_panel(dir.path());
        assert!(apply_watcher_upsert_if_matches(
            &mut state.left_panel,
            &file
        ));
        rebuild_visible_entries(&mut state.left_panel, None);
        assert!(
            state
                .left_panel
                .listing
                .entries
                .iter()
                .any(|e| e.name == "to_delete.txt")
        );

        fs::remove_file(&file).unwrap();
        tx.send(WatchEvent::Deleted(file.clone())).unwrap();

        let dirty = poll_watcher_events(&mut state, &rx);
        assert!(dirty);
        assert!(
            !state
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
        let (tx, rx) = mpsc::sync_channel(256);
        let mut state = AppState::new();
        state.left_panel = test_panel(dir.path());

        let new_file = dir.path().join("new_file.txt");
        fs::write(&new_file, b"hello").unwrap();

        tx.send(WatchEvent::Created(new_file)).unwrap();

        let dirty = poll_watcher_events(&mut state, &rx);
        assert!(dirty);
        assert!(
            state
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
        let (tx, rx) = mpsc::sync_channel(256);
        let mut state = AppState::new();
        // Set panel path to "/" — parent() returns None
        state.left_panel.set_path(PathBuf::from("/"));

        tx.send(WatchEvent::Deleted(PathBuf::from("/"))).unwrap();

        let dirty = poll_watcher_events(&mut state, &rx);
        assert!(dirty, "should be dirty after root deletion event");
        assert_eq!(
            state.left_panel.path,
            PathBuf::from("/"),
            "should stay at root since parent() is None"
        );
    }

    #[test]
    fn sync_watcher_paths_does_not_set_last_synced_when_path_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nonexistent_panel_dir");
        let (event_tx, _rx) = mpsc::sync_channel(256);
        let mut watcher = Some(Watcher::new(Arc::new(event_tx)).expect("create watcher"));

        let mut state = AppState::new();
        state.left_panel.set_path(missing);
        state.right_panel.set_path(dir.path().to_path_buf());

        let mut last_synced: Option<(PathBuf, PathBuf)> = None;

        sync_watcher_paths(&mut watcher, &state, &mut last_synced);

        assert!(
            last_synced.is_none(),
            "should not set last_synced when a panel path does not exist"
        );
    }
}
