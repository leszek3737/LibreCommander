use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::app::panel_ops::update_panel_read_errors;
use crate::app::types::{AppState, PanelState};
use crate::debug_log;
use crate::fs::reader;
use crate::fs::watcher::{WatchEvent, Watcher};

const COOLDOWN: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct WatcherSyncState {
    pub last_synced: Option<(PathBuf, PathBuf)>,
    pub failed_cooldown: Option<(Instant, PathBuf, PathBuf)>,
}

pub fn sync_watcher_paths(
    watcher: &mut Option<Watcher>,
    state: &AppState,
    sync_state: &mut WatcherSyncState,
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };

    if let Some((l, r)) = sync_state.last_synced.as_ref()
        && l == &state.left_panel.path
        && r == &state.right_panel.path
    {
        return;
    }

    if let Some((deadline, fl, fr)) = &sync_state.failed_cooldown
        && Instant::now() < *deadline
        && fl == &state.left_panel.path
        && fr == &state.right_panel.path
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
        sync_state.last_synced = None;
        sync_state.failed_cooldown = Some((Instant::now() + COOLDOWN, left, right));
    } else {
        sync_state.last_synced = Some((left, right));
        sync_state.failed_cooldown = None;
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
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.last_error = Some(err.to_string());
            panel.recalculate_selection_stats();
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
mod tests;
