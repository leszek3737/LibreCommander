use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::app::types::{AppState, PanelState};
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

    let left = state.left_panel.path.clone();
    let right = state.right_panel.path.clone();

    if last_synced.as_ref() == Some(&(left.clone(), right.clone())) {
        return;
    }

    let desired: HashSet<PathBuf> = [&left, &right]
        .into_iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
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

    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    let panel_path = panel_path
        .canonicalize()
        .unwrap_or_else(|_| panel_path.to_path_buf());
    parent == panel_path
}

fn apply_watcher_upsert(panel: &mut PanelState, path: &Path) -> bool {
    let Ok(entry) = reader::get_single_entry(path) else {
        return false;
    };

    reader::upsert_entry(panel, entry);
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
    }

    existed
}
