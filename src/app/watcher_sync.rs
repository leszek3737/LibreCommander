use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::app::types::{AppState, PanelState};
use crate::debug_log;
use crate::fs::reader;
use crate::fs::watcher::{WatchEvent, Watcher};
use crate::ops::{search, sorting};

pub fn sync_watcher_paths(
    watcher: &mut Option<Watcher>,
    state: &AppState,
    last_synced: &mut Option<(PathBuf, PathBuf)>,
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };

    let left = state.left_panel.path.clone();
    let right = state.right_panel.path.clone();

    if let Some((l, r)) = last_synced.as_ref()
        && l == &left
        && r == &right
    {
        return;
    }

    let (desired, all_paths_present) = canonical_desired_paths(&left, &right);
    let current: HashSet<PathBuf> = watcher.watched_dirs().into_iter().collect();

    for path in current.difference(&desired) {
        if let Err(err) = watcher.unwatch(path) {
            debug_log!("Watcher unwatch failed for {}: {err}", path.display());
            // Intentional: continue on error — one failing path shouldn't abort the entire sync.
            // Worst case: a stale watch remains until next poll cycle cleans it up.
            continue;
        }
    }
    for path in desired.difference(&current) {
        if let Err(err) = watcher.watch(path) {
            debug_log!("Watcher watch failed for {}: {err}", path.display());
            // Intentional: continue on error — one failing path shouldn't abort the entire sync.
            // Worst case: a stale watch remains until next poll cycle cleans it up.
            continue;
        }
    }

    let current_after: HashSet<PathBuf> = watcher.watched_dirs().into_iter().collect();
    if all_paths_present && desired.iter().all(|p| current_after.contains(p)) {
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
                debug_log!("Watcher sync skipped path {}: {err}", path.display());
                let mut desired = HashSet::new();
                for fallback_path in [left, right] {
                    desired.insert(fallback_path.to_path_buf());
                }
                return (desired, false);
            }
        }
    }
    (desired, all_paths_present)
}

pub fn poll_watcher_events(state: &mut AppState, receiver: &Receiver<WatchEvent>) -> bool {
    let mut dirty = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            WatchEvent::Created(path) | WatchEvent::Modified(path) => {
                dirty |= apply_watcher_upsert_if_matches(&mut state.left_panel, &path);
                dirty |= apply_watcher_upsert_if_matches(&mut state.right_panel, &path);
            }
            WatchEvent::Deleted(path) => {
                dirty |= apply_watcher_remove_if_matches(&mut state.left_panel, &path);
                dirty |= apply_watcher_remove_if_matches(&mut state.right_panel, &path);
            }
            WatchEvent::Renamed { from, to } => {
                dirty |= apply_watcher_remove_if_matches(&mut state.left_panel, &from);
                dirty |= apply_watcher_remove_if_matches(&mut state.right_panel, &from);
                dirty |= apply_watcher_upsert_if_matches(&mut state.left_panel, &to);
                dirty |= apply_watcher_upsert_if_matches(&mut state.right_panel, &to);
            }
        }
    }

    dirty
}

pub fn apply_watcher_upsert_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, &panel.path) {
        return false;
    }

    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_upsert(panel, &path)
}

pub fn apply_watcher_remove_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, &panel.path) {
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

fn path_parent_matches(path: &Path, panel_path: &Path) -> bool {
    if path.file_name().is_none() {
        return false;
    }

    let Some(parent) = path.parent() else {
        return false;
    };

    if parent == panel_path {
        return true;
    }

    let parent_raw = parent.to_path_buf();
    let panel_path_raw = panel_path.to_path_buf();

    let parent_canonical = parent.canonicalize().ok();
    let panel_canonical = panel_path.canonicalize().ok();

    match (parent_canonical, panel_canonical) {
        (Some(parent), Some(panel_path)) => parent == panel_path,
        (Some(parent), None) => parent == panel_path_raw,
        (None, Some(panel_path)) => parent_raw == panel_path,
        (None, None) => false,
    }
}

fn apply_watcher_upsert(panel: &mut PanelState, path: &Path) -> bool {
    let Ok(mut entry) = reader::get_single_entry(path) else {
        return false;
    };

    if !panel.show_hidden && entry.is_hidden() {
        return apply_watcher_remove(panel, path);
    }

    if let Some(existing) = panel
        .unfiltered_entries
        .iter()
        .find(|existing| existing.path == entry.path)
    {
        if existing.cha.hits(&entry.cha) {
            return false;
        }
        entry.selected = existing.selected;
    }

    reader::upsert_entry(panel, entry);
    rebuild_visible_entries(panel, path.file_name().and_then(|name| name.to_str()));
    true
}

fn apply_watcher_remove(panel: &mut PanelState, path: &Path) -> bool {
    let existed = panel
        .unfiltered_entries
        .iter()
        .any(|entry| entry.path == path);
    if existed {
        reader::remove_entry(panel, path);
        rebuild_visible_entries(panel, path.file_name().and_then(|name| name.to_str()));
    }

    existed
}

fn rebuild_visible_entries(panel: &mut PanelState, preferred_name: Option<&str>) {
    let current_name = panel
        .entries
        .get(panel.cursor)
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.name.clone())
        .or_else(|| preferred_name.map(str::to_string));

    panel.sync_unfiltered_selection();
    panel.entries = panel
        .unfiltered_entries
        .iter()
        .filter(|entry| entry_matches_panel(entry, panel.filter.as_deref()))
        .cloned()
        .collect();
    sorting::sort_entries(&mut panel.entries, panel.sort_mode, panel.sort_options);

    if let Some(name) = current_name.as_deref()
        && let Some(pos) = panel.entries.iter().position(|entry| entry.name == name)
    {
        panel.cursor = pos;
    }

    if panel.entries.is_empty() {
        panel.cursor = 0;
        panel.scroll_offset = 0;
    } else if panel.cursor >= panel.entries.len() {
        panel.cursor = panel.entries.len() - 1;
    }

    let max_scroll = panel.entries.len().saturating_sub(1);
    if panel.scroll_offset > max_scroll {
        panel.scroll_offset = max_scroll;
    }
    if panel.scroll_offset > panel.cursor {
        panel.scroll_offset = panel.cursor;
    }
    panel.ensure_cursor_visible(0);
    panel.recalculate_selection_stats();
}

fn entry_matches_panel(entry: &reader::FileEntry, filter: Option<&str>) -> bool {
    entry.name == ".."
        || filter
            .is_none_or(|filter| search::FileSearch::matches_pattern(&entry.name, filter, false))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::app::types::SortMode;
    use std::fs;
    use std::sync::mpsc;

    fn test_panel(path: &Path) -> PanelState {
        let mut panel = PanelState::new(path.to_path_buf());
        panel.unfiltered_entries = vec![parent_entry(path)];
        panel.entries = panel.unfiltered_entries.clone();
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

        let names: Vec<_> = panel
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
        panel.entries[1].selected = true;
        panel.sync_unfiltered_selection();

        fs::write(&keep, b"updated").unwrap();
        assert!(apply_watcher_upsert_if_matches(&mut panel, &keep));
        assert!(apply_watcher_upsert_if_matches(&mut panel, &drop));

        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.entries[1].name, "keep.txt");
        assert!(panel.entries[1].selected);
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
        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.unfiltered_entries.len(), 1);
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

        let names: Vec<_> = panel
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
        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.unfiltered_entries.len(), 1);
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
        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Some(Watcher::new(event_tx).expect("create watcher"));
        let mut state = AppState::new();
        state.left_panel.path = dir.path().to_path_buf();
        state.right_panel.path = missing;
        let mut last_synced = None;

        sync_watcher_paths(&mut watcher, &state, &mut last_synced);

        let watched = watcher.as_ref().unwrap().watched_dirs();
        assert_eq!(watched, vec![dir.path().canonicalize().unwrap()]);
        assert!(last_synced.is_none());
    }

    #[test]
    fn path_parent_matches_keeps_raw_fallback_for_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let panel_path = dir.path().join("missing");
        let child = panel_path.join("file.txt");

        assert!(path_parent_matches(&child, &panel_path));
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

        let names: Vec<_> = panel
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
        assert_eq!(panel.entries.len(), 2);

        assert!(!apply_watcher_upsert_if_matches(&mut panel, &file));
        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.unfiltered_entries.len(), 2);
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

        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.total_size, 18);
    }
}
