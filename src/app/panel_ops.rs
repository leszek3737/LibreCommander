use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use crate::app::types::*;
use crate::fs::reader;
use crate::fs::watcher::Watcher;
use crate::ops;
use crate::ui::LAYOUT_OVERHEAD_ROWS;

pub fn file_names_from_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            p.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| p.clone())
        })
        .collect()
}

pub fn sync_watcher_job_state(
    watcher: &Option<Watcher>,
    job_running: bool,
    watcher_paused: &mut bool,
) -> bool {
    let Some(watcher) = watcher.as_ref() else {
        return false;
    };

    if job_running && !*watcher_paused {
        watcher.pause();
        *watcher_paused = true;
        false
    } else if !job_running && *watcher_paused {
        watcher.resume();
        *watcher_paused = false;
        true
    } else {
        false
    }
}

pub fn refresh_panel(panel: &mut PanelState, visible_height: usize) {
    match reader::read_directory(panel.path()) {
        Ok((entries, errors)) => {
            update_panel_read_errors(panel, &errors);
            let current_name = current_panel_entry_name(panel);
            let saved = selected_panel_paths(panel);
            let new_unfiltered = entries;
            let new_filtered = filtered_sorted_entries(
                &new_unfiltered,
                panel.filter(),
                panel.sort_mode(),
                *panel.sort_options(),
                panel.show_hidden(),
            );
            let mut sorted_unfiltered = new_unfiltered;
            ops::sort_entries(
                &mut sorted_unfiltered,
                panel.sort_mode(),
                *panel.sort_options(),
            );
            // Both listing stores receive pre-sorted data: `set_unfiltered` takes
            // the sorted backing store, and `set_filtered` takes the sorted
            // filtered slice and maps each entry back to its backing slot by path.
            // Ordering is the caller's responsibility; the listing never reorders.
            panel.listing.set_unfiltered(sorted_unfiltered);
            panel.listing.set_filtered(&new_filtered);
            restore_panel_selection(panel, &saved);
            finalize_view(panel, current_name.as_deref(), visible_height);
        }
        Err(e) => {
            panel.listing.clear();
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.set_last_error(Some(e.to_string()));
            panel.recalculate_selection_stats();
        }
    }
}

pub(crate) fn update_panel_read_errors(panel: &mut PanelState, errors: &[io::Error]) {
    if errors.is_empty() {
        panel.set_last_error(None);
    } else {
        // Fold straight into one String (no intermediate Vec<String> alloc).
        let error_summary = errors.iter().fold(String::new(), |mut acc, e| {
            if !acc.is_empty() {
                acc.push_str("; ");
            }
            acc.push_str(&e.to_string());
            acc
        });
        panel.set_last_error(Some(format!(
            "{} file(s) failed to read: {error_summary}",
            errors.len()
        )));
    }
}

fn current_panel_entry_name(panel: &PanelState) -> Option<String> {
    panel
        .listing
        .filtered_get(panel.cursor)
        .filter(|e| e.name != "..")
        .map(|e| e.name.clone())
}

fn selected_panel_paths(panel: &PanelState) -> HashSet<PathBuf> {
    panel.selected_entries().map(|e| e.path.clone()).collect()
}

pub fn filtered_sorted_entries(
    entries: &[reader::FileEntry],
    filter: Option<&str>,
    sort_mode: SortMode,
    sort_options: SortOptions,
    show_hidden: bool,
) -> Vec<reader::FileEntry> {
    // PERF FOLLOW-UP: `CompiledPattern` is rebuilt on every call (once per
    // filtered_sorted_entries). It cannot be cached on `PanelState` yet because
    // `CompiledPattern` derives none of `Debug`/`Clone`/`PartialEq` that
    // `PanelState` requires; caching needs those derives added in
    // ops/search/pattern.rs first (cross-module, separate change).
    let compiled = filter.map(|f| ops::CompiledPattern::new(f, false));
    // PERF FOLLOW-UP: `.cloned()` copies each (potentially large) FileEntry into
    // the filtered Vec. A `Vec<&FileEntry>` / index view would avoid the copy,
    // but `ops::sort_entries` takes `&mut [FileEntry]` (owned), so a borrow-only
    // view needs a reference-sorting variant in `ops` first (cross-module).
    let mut filtered_entries: Vec<reader::FileEntry> = entries
        .iter()
        .filter(|e| entry_matches_panel(e, compiled.as_ref(), show_hidden))
        .cloned()
        .collect();
    ops::sort_entries(&mut filtered_entries, sort_mode, sort_options);
    filtered_entries
}

pub fn rebuild_visible_entries(panel: &mut PanelState, visible_height: usize) {
    let current_name = current_panel_entry_name(panel);
    let filtered = filtered_sorted_entries(
        panel.listing.unfiltered(),
        panel.filter(),
        panel.sort_mode(),
        *panel.sort_options(),
        panel.show_hidden(),
    );
    panel.listing.set_filtered(&filtered);
    finalize_view(panel, current_name.as_deref(), visible_height);
}

pub(crate) fn entry_matches_panel(
    entry: &reader::FileEntry,
    compiled_filter: Option<&ops::CompiledPattern>,
    show_hidden: bool,
) -> bool {
    entry.name == ".."
        || ((show_hidden || !entry.cha.is_hidden())
            && compiled_filter.is_none_or(|pat| pat.matches(&entry.name)))
}

fn restore_selection_for(entries: &mut [reader::FileEntry], saved: &HashSet<PathBuf>) {
    for entry in entries {
        entry.selected = saved.contains(&entry.path);
    }
}

fn restore_panel_selection(panel: &mut PanelState, saved: &HashSet<PathBuf>) {
    // Single backing store now owns selection; the filtered view borrows it.
    restore_selection_for(panel.listing.unfiltered_mut(), saved);
}

/// Shared post-rebuild steps for both the full refresh and the filter-only
/// rebuild: recompute selection stats, re-anchor the cursor on the previously
/// focused entry name (if still visible) and clamp it into the viewport.
fn finalize_view(panel: &mut PanelState, current_name: Option<&str>, visible_height: usize) {
    panel.recalculate_selection_stats();
    restore_panel_cursor(panel, current_name);
    panel.ensure_cursor_visible(visible_height);
}

fn restore_panel_cursor(panel: &mut PanelState, current_name: Option<&str>) {
    if let Some(name) = current_name
        && let Some(pos) = panel.listing.filtered().position(|e| e.name == name)
    {
        panel.cursor = pos;
    }
    if panel.cursor >= panel.listing.filtered_len() {
        panel.cursor = panel.listing.filtered_len().saturating_sub(1);
    }
}

// Usable panel rows for a standard 24-row terminal (24 − LAYOUT_OVERHEAD_ROWS = 18).
// Used when crossterm::terminal::size() fails (e.g. piped stdout, no tty).
const FALLBACK_VISIBLE_HEIGHT: usize = 18;

pub fn current_visible_height() -> usize {
    crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(FALLBACK_VISIBLE_HEIGHT)
}

pub fn refresh_active(state: &mut AppState) {
    let visible = current_visible_height();
    match state.active_panel {
        ActivePanel::Left => refresh_panel(&mut state.left_panel, visible),
        ActivePanel::Right => refresh_panel(&mut state.right_panel, visible),
    }
}

pub fn refresh_both(state: &mut AppState) {
    let visible = current_visible_height();
    refresh_panel(&mut state.left_panel, visible);
    refresh_panel(&mut state.right_panel, visible);
}

pub fn set_active_panel(state: &mut AppState, panel: ActivePanel) {
    state.active_panel = panel;
}

// Indices into the top menu bar (`crate::menu::MENUS`):
// 0:Left 1:File 2:Command 3:Options 4:Right. The "Left"/"Right" menus drive the
// panel of the same name, so their selection maps onto the matching `ActivePanel`.
const MENU_ITEM_LEFT_PANEL: usize = 0;
const MENU_ITEM_RIGHT_PANEL: usize = 4;

pub fn with_menu_panel<T>(state: &mut AppState, f: impl FnOnce(&mut AppState) -> T) -> T {
    let original = state.active_panel;
    match state.ui.menu_selected {
        MENU_ITEM_LEFT_PANEL => set_active_panel(state, ActivePanel::Left),
        MENU_ITEM_RIGHT_PANEL => set_active_panel(state, ActivePanel::Right),
        _ => {}
    }
    let result = f(state);
    if matches!(state.mode, AppMode::Dialog(_)) {
        state.ui.menu_restore_panel = Some(original);
    } else {
        set_active_panel(state, original);
    }
    result
}

pub fn panel_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(LAYOUT_OVERHEAD_ROWS) as usize
}

pub fn navigate_to_hotlist(state: &mut AppState, index: usize) {
    let path = match state.hotlist().get(index) {
        Some(p) => p.clone(),
        None => {
            let len = state.hotlist().len();
            state.set_status(format!("Hotlist index {} out of range (0..{})", index, len));
            return;
        }
    };
    if !path.is_dir() {
        state.set_status(format!("{} is not a directory", path.display()));
        return;
    }
    let display = path.display().to_string();
    let panel = state.active_panel_mut();
    panel.push_history(panel.path().to_path_buf());
    panel.set_path(path);
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.set_filter(None);
    refresh_active(state);
    state.set_status(format!("cd to {display}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_visible_height() {
        assert_eq!(panel_visible_height(24), 18);
        assert_eq!(panel_visible_height(10), 4);
        assert_eq!(panel_visible_height(0), 0);
        assert_eq!(panel_visible_height(3), 0);
    }

    #[test]
    fn test_file_names_from_paths() {
        let paths = vec![
            PathBuf::from("/tmp/a.txt"),
            PathBuf::from("/home/user/b.rs"),
            PathBuf::from("/"),
        ];
        let names = file_names_from_paths(&paths);
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], PathBuf::from("a.txt"));
        assert_eq!(names[1], PathBuf::from("b.rs"));
        assert_eq!(names[2], PathBuf::from("/"));
    }

    #[test]
    fn test_file_names_from_paths_empty() {
        let paths: Vec<PathBuf> = vec![];
        let names = file_names_from_paths(&paths);
        assert!(names.is_empty());
    }

    #[test]
    fn test_set_active_panel() {
        let mut state = AppState::default();
        set_active_panel(&mut state, ActivePanel::Right);
        assert_eq!(state.active_panel, ActivePanel::Right);
        set_active_panel(&mut state, ActivePanel::Left);
        assert_eq!(state.active_panel, ActivePanel::Left);
    }
}
