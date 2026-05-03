use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::app::types::{AppState, PanelState};
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

    if last_synced.as_ref() == Some(&(left.clone(), right.clone())) {
        return;
    }

    let desired: HashSet<PathBuf> = [&left, &right]
        .into_iter()
        .filter_map(|path| {
            match path.canonicalize() {
                Ok(canonical) => Some(canonical),
                Err(_) => {
                    // Skip paths that cannot be canonicalized (e.g., deleted, inaccessible)
                    None
                }
            }
        })
        .collect();
    let current: HashSet<PathBuf> = watcher.watched_dirs().into_iter().collect();

    for path in current.difference(&desired) {
        let _ = watcher.unwatch(path);
    }
    for path in desired.difference(&current) {
        let _ = watcher.watch(path);
    }

    *last_synced = Some((left, right));
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

    let Ok(parent) = parent.canonicalize() else {
        // Skip if parent cannot be canonicalized (deleted/inaccessible)
        return false;
    };
    let Ok(panel_path) = panel_path.canonicalize() else {
        // Skip if panel_path cannot be canonicalized (deleted/inaccessible)
        return false;
    };
    parent == panel_path
}

fn apply_watcher_upsert(panel: &mut PanelState, path: &Path) -> bool {
    let Ok(mut entry) = reader::get_single_entry(path) else {
        return false;
    };

    if !panel.show_hidden && entry.is_hidden {
        return apply_watcher_remove(panel, path);
    }

    if let Some(existing) = panel
        .unfiltered_entries
        .iter()
        .find(|existing| existing.path == entry.path)
        .or_else(|| {
            panel
                .entries
                .iter()
                .find(|existing| existing.path == entry.path)
        })
    {
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
        .chain(panel.entries.iter())
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
    sorting::sort_entries(&mut panel.entries, panel.sort_mode);

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
mod tests {
    use super::*;
    use crate::app::types::SortMode;
    use std::fs;

    fn test_panel(path: PathBuf) -> PanelState {
        let mut panel = PanelState::new(path.clone());
        panel.unfiltered_entries = vec![parent_entry(&path)];
        panel.entries = panel.unfiltered_entries.clone();
        panel.recalculate_selection_stats();
        panel
    }

    fn parent_entry(path: &Path) -> reader::FileEntry {
        reader::FileEntry {
            name: "..".to_string(),
            path: path.parent().unwrap_or(path).to_path_buf(),
            is_dir: true,
            is_symlink: false,
            is_executable: true,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0o755,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        }
    }

    #[test]
    fn watcher_upsert_adds_visible_entry_sorted_and_updates_stats() {
        let dir = tempfile::tempdir().unwrap();
        let beta = dir.path().join("beta.txt");
        let alpha = dir.path().join("alpha.txt");
        fs::write(&beta, b"beta").unwrap();
        fs::write(&alpha, b"alpha").unwrap();

        let mut panel = test_panel(dir.path().to_path_buf());
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

        let mut panel = test_panel(dir.path().to_path_buf());
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

        let mut panel = test_panel(dir.path().to_path_buf());
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

        let mut panel = test_panel(dir.path().to_path_buf());
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
    fn watcher_upsert_uses_panel_sort_mode() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.txt");
        let big = dir.path().join("big.txt");
        fs::write(&small, b"s").unwrap();
        fs::write(&big, b"larger").unwrap();

        let mut panel = test_panel(dir.path().to_path_buf());
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
}
