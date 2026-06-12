use std::collections::HashMap;
use std::collections::HashSet;
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
        && l == state.left_panel.path()
        && r == state.right_panel.path()
    {
        return;
    }

    if let Some((deadline, fl, fr)) = &sync_state.failed_cooldown
        && Instant::now() < *deadline
        && fl == state.left_panel.path()
        && fr == state.right_panel.path()
    {
        return;
    }

    let left = state.left_panel.path().to_path_buf();
    let right = state.right_panel.path().to_path_buf();

    let desired = canonical_desired_paths(&left, &right);
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

    if had_error {
        sync_state.last_synced = None;
        sync_state.failed_cooldown = Some((Instant::now() + COOLDOWN, left, right));
    } else {
        sync_state.last_synced = Some((left, right));
        sync_state.failed_cooldown = None;
    }
}

fn canonical_desired_paths(left: &Path, right: &Path) -> HashSet<PathBuf> {
    let mut desired = HashSet::with_capacity(2);
    for path in [left, right] {
        desired.insert(crate::fs::path::clean_path(path));
    }
    desired
}

struct PanelCache {
    path: PathBuf,
    clean: PathBuf,
    canonical: Option<PathBuf>,
    canonical_clean: Option<PathBuf>,
}

impl PanelCache {
    fn from_panel(panel: &PanelState) -> Self {
        let clean = crate::fs::path::clean_path(panel.path());
        let canonical = panel.canonical_path().map(Path::to_path_buf);
        let canonical_clean = canonical.as_ref().map(|c| crate::fs::path::clean_path(c));
        Self {
            path: panel.path().to_path_buf(),
            clean,
            canonical,
            canonical_clean,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DedupKind {
    Upsert,
    Remove,
}

struct AccumulatedChanges {
    file_events: HashMap<PathBuf, DedupKind>,
    left_dir_event: Option<DirEvent>,
    right_dir_event: Option<DirEvent>,
    overflow: bool,
}

enum DirEvent {
    Delete,
    Rename { to: PathBuf },
    Touch,
}

fn drain_events(receiver: &Receiver<WatchEvent>) -> Vec<WatchEvent> {
    const MAX_WATCHER_EVENTS_PER_POLL: usize = 256;
    let mut events = Vec::with_capacity(MAX_WATCHER_EVENTS_PER_POLL);
    for _ in 0..MAX_WATCHER_EVENTS_PER_POLL {
        let Ok(event) = receiver.try_recv() else {
            break;
        };
        events.push(event);
    }
    events
}

fn accumulate_changes(
    events: Vec<WatchEvent>,
    left_cache: &PanelCache,
    right_cache: &PanelCache,
) -> AccumulatedChanges {
    let mut changes = AccumulatedChanges {
        file_events: HashMap::new(),
        left_dir_event: None,
        right_dir_event: None,
        overflow: false,
    };

    for event in events {
        match event {
            WatchEvent::Created(path) | WatchEvent::Modified(path) => {
                if event_is_panel_dir_cached(&path, left_cache) {
                    changes.left_dir_event = Some(DirEvent::Touch);
                }
                if event_is_panel_dir_cached(&path, right_cache) {
                    changes.right_dir_event = Some(DirEvent::Touch);
                }
                if path.file_name().is_some() {
                    changes.file_events.insert(path, DedupKind::Upsert);
                }
            }
            WatchEvent::Deleted(path) => {
                if event_is_panel_dir_cached(&path, left_cache) {
                    changes.left_dir_event = Some(DirEvent::Delete);
                }
                if event_is_panel_dir_cached(&path, right_cache) {
                    changes.right_dir_event = Some(DirEvent::Delete);
                }
                if path.file_name().is_some() {
                    changes.file_events.insert(path, DedupKind::Remove);
                }
            }
            WatchEvent::Renamed { from, to } => {
                if event_is_panel_dir_cached(&from, left_cache) {
                    changes.left_dir_event = Some(DirEvent::Rename { to: to.clone() });
                }
                if event_is_panel_dir_cached(&from, right_cache) {
                    changes.right_dir_event = Some(DirEvent::Rename { to: to.clone() });
                }
                if event_is_panel_dir_cached(&to, left_cache) && changes.left_dir_event.is_none() {
                    changes.left_dir_event = Some(DirEvent::Touch);
                }
                if event_is_panel_dir_cached(&to, right_cache) && changes.right_dir_event.is_none()
                {
                    changes.right_dir_event = Some(DirEvent::Touch);
                }
                if from.file_name().is_some() {
                    changes.file_events.insert(from, DedupKind::Remove);
                }
                if to.file_name().is_some() {
                    changes.file_events.insert(to, DedupKind::Upsert);
                }
            }
            WatchEvent::Overflow => {
                changes.overflow = true;
            }
        }
    }

    changes
}

fn apply_panel_changes(
    state: &mut AppState,
    changes: &AccumulatedChanges,
    left_cache: &PanelCache,
    right_cache: &PanelCache,
) -> bool {
    let mut dirty = !changes.file_events.is_empty()
        || changes.left_dir_event.is_some()
        || changes.right_dir_event.is_some()
        || changes.overflow;

    apply_dir_event(
        &mut state.left_panel,
        &changes.left_dir_event,
        changes.overflow,
    );
    apply_dir_event(
        &mut state.right_panel,
        &changes.right_dir_event,
        changes.overflow,
    );

    let left_needs_full_refresh = changes.left_dir_event.is_some() || changes.overflow;
    let right_needs_full_refresh = changes.right_dir_event.is_some() || changes.overflow;

    let left_refreshed = apply_file_events(&mut state.left_panel, &changes.file_events, left_cache);
    let right_refreshed =
        apply_file_events(&mut state.right_panel, &changes.file_events, right_cache);

    if left_needs_full_refresh || right_needs_full_refresh {
        full_refresh_panels(
            &mut state.left_panel,
            &mut state.right_panel,
            left_needs_full_refresh,
            right_needs_full_refresh,
        );
    }

    dirty |= left_refreshed || right_refreshed;

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

fn apply_dir_event(panel: &mut PanelState, dir_event: &Option<DirEvent>, overflow: bool) {
    if overflow {
        return;
    }
    let Some(dir_event) = dir_event else {
        return;
    };
    match dir_event {
        DirEvent::Delete => {
            if let Some(parent) = panel.path().parent().map(Path::to_path_buf) {
                panel.set_path(parent);
            }
        }
        DirEvent::Rename { to } => {
            panel.set_path(to.clone());
        }
        DirEvent::Touch => {}
    }
}

fn apply_file_events(
    panel: &mut PanelState,
    file_events: &HashMap<PathBuf, DedupKind>,
    cache: &PanelCache,
) -> bool {
    let mut dirty = false;
    for (path, kind) in file_events {
        dirty |= match kind {
            DedupKind::Upsert => apply_upsert_cached(panel, path, cache),
            DedupKind::Remove => apply_remove_cached(panel, path, cache),
        };
    }
    dirty
}

fn apply_upsert_cached(panel: &mut PanelState, path: &Path, cache: &PanelCache) -> bool {
    if !path_parent_matches_cached(path, cache) {
        return false;
    }
    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_upsert(panel, &path)
}

fn apply_remove_cached(panel: &mut PanelState, path: &Path, cache: &PanelCache) -> bool {
    if !path_parent_matches_cached(path, cache) {
        return false;
    }
    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_remove(panel, &path)
}

fn full_refresh_panels(
    left: &mut PanelState,
    right: &mut PanelState,
    left_needs: bool,
    right_needs: bool,
) {
    let same_dir = left.path() == right.path();
    if same_dir && left_needs && right_needs {
        match reader::read_directory(left.path()) {
            Ok((entries, errors)) => {
                apply_shared_read_result(left, &entries, &errors);
                apply_shared_read_result(right, &entries, &errors);
            }
            Err(err) => {
                apply_read_error(left, &err);
                apply_read_error(right, &err);
            }
        }
    } else {
        if left_needs {
            refresh_panel_from_disk(left);
        }
        if right_needs {
            refresh_panel_from_disk(right);
        }
    }
}

fn apply_shared_read_result(
    panel: &mut PanelState,
    entries: &[crate::app::types::FileEntry],
    errors: &[std::io::Error],
) {
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

    update_panel_read_errors(panel, errors);
    // NOTE: clone needed because entries is &[FileEntry] shared with the other panel.
    panel.listing.set_unfiltered(entries.to_vec());
    panel.set_canonical_path(panel.path().canonicalize().ok());
    for entry in &mut panel.listing.unfiltered_entries {
        entry.selected = saved.contains(&entry.path);
    }
    rebuild_visible_entries(panel, current_name.as_deref());
}

fn apply_read_error(panel: &mut PanelState, err: &std::io::Error) {
    panel.listing.clear();
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.set_last_error(Some(err.to_string()));
    panel.recalculate_selection_stats();
}

pub fn poll_watcher_events(state: &mut AppState, receiver: &Receiver<WatchEvent>) -> bool {
    let events = drain_events(receiver);
    if events.is_empty() {
        return false;
    }

    let left_cache = PanelCache::from_panel(&state.left_panel);
    let right_cache = PanelCache::from_panel(&state.right_panel);
    let changes = accumulate_changes(events, &left_cache, &right_cache);
    apply_panel_changes(state, &changes, &left_cache, &right_cache)
}

pub fn apply_watcher_upsert_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, panel) {
        return false;
    }

    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_upsert(panel, &path)
}

pub fn apply_watcher_remove_if_matches(panel: &mut PanelState, path: &Path) -> bool {
    if !path_parent_matches(path, panel) {
        return false;
    }

    let Some(path) = panel_event_path(panel, path) else {
        return false;
    };
    apply_watcher_remove(panel, &path)
}

fn panel_event_path(panel: &PanelState, path: &Path) -> Option<PathBuf> {
    path.file_name().map(|name| panel.path().join(name))
}

fn path_matches_any(candidate: &Path, cache: &PanelCache) -> bool {
    if candidate == cache.path {
        return true;
    }
    let candidate_clean = crate::fs::path::clean_path(candidate);
    if candidate_clean == cache.clean {
        return true;
    }
    if let Some(ref c) = cache.canonical
        && candidate_clean == c.as_path()
    {
        return true;
    }
    if let Some(ref canonical_clean) = cache.canonical_clean
        && candidate_clean == *canonical_clean
    {
        return true;
    }
    false
}

fn event_is_panel_dir_cached(path: &Path, cache: &PanelCache) -> bool {
    if path.parent() == Some(cache.path.as_path()) {
        return false;
    }
    path_matches_any(path, cache)
}

fn path_parent_matches_cached(path: &Path, cache: &PanelCache) -> bool {
    if path.file_name().is_none() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    path_matches_any(parent, cache)
}

fn path_parent_matches(path: &Path, panel: &PanelState) -> bool {
    let cache = PanelCache::from_panel(panel);
    path_parent_matches_cached(path, &cache)
}

fn apply_watcher_upsert(panel: &mut PanelState, path: &Path) -> bool {
    let Ok(mut entry) = reader::get_file_info(path) else {
        return false;
    };

    if !panel.show_hidden() && entry.is_hidden() {
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
    panel.mark_dirty();
    true
}

fn apply_watcher_remove(panel: &mut PanelState, path: &Path) -> bool {
    reader::ensure_path_index(panel);
    let existed = panel.listing.path_index.contains_key(path);
    if existed {
        reader::remove_entry(panel, path);
        panel.mark_dirty();
    }

    existed
}

pub(crate) fn refresh_panel_from_disk(panel: &mut PanelState) {
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

    match reader::read_directory(panel.path()) {
        Ok((entries, errors)) => {
            update_panel_read_errors(panel, &errors);
            panel.listing.set_unfiltered(entries);
            panel.set_canonical_path(panel.path().canonicalize().ok());
            for entry in &mut panel.listing.unfiltered_entries {
                entry.selected = saved.contains(&entry.path);
            }
            rebuild_visible_entries(panel, current_name.as_deref());
        }
        Err(err) => {
            panel.listing.clear();
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.set_last_error(Some(err.to_string()));
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
