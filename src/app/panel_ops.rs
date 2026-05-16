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
    match reader::read_directory(&panel.path) {
        Ok((entries, errors)) => {
            update_panel_read_errors(panel, &errors);
            let current_name = current_panel_entry_name(panel);
            let saved = selected_panel_paths(panel);
            let new_unfiltered = entries;
            let new_filtered = filtered_sorted_entries(
                &new_unfiltered,
                panel.filter.as_deref(),
                panel.sort_mode,
                panel.sort_options,
                panel.show_hidden,
            );
            panel.unfiltered_entries = new_unfiltered;
            panel.path_index.clear();
            panel.entries = new_filtered;
            restore_panel_selection(panel, &saved);
            panel.recalculate_selection_stats();
            restore_panel_cursor(panel, current_name.as_deref());
            panel.ensure_cursor_visible(visible_height);
        }
        Err(e) => {
            panel.unfiltered_entries.clear();
            panel.entries.clear();
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.last_error = Some(e.to_string());
            panel.recalculate_selection_stats();
        }
    }
}

fn update_panel_read_errors(panel: &mut PanelState, errors: &[io::Error]) {
    if errors.is_empty() {
        panel.last_error = None;
    } else {
        let error_summary = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        panel.last_error = Some(format!(
            "{} file(s) failed to read: {error_summary}",
            errors.len()
        ));
    }
}

fn current_panel_entry_name(panel: &PanelState) -> Option<String> {
    panel
        .entries
        .get(panel.cursor)
        .filter(|e| e.name != "..")
        .map(|e| e.name.clone())
}

fn selected_panel_paths(panel: &PanelState) -> HashSet<PathBuf> {
    panel
        .entries
        .iter()
        .filter(|e| e.selected)
        .map(|e| e.path.clone())
        .collect()
}

pub fn filtered_sorted_entries(
    entries: &[reader::FileEntry],
    filter: Option<&str>,
    sort_mode: SortMode,
    sort_options: SortOptions,
    show_hidden: bool,
) -> Vec<reader::FileEntry> {
    let mut sort_entries: Vec<reader::FileEntry> = entries
        .iter()
        .filter(|e| {
            if e.name == ".." {
                true
            } else if !show_hidden && e.cha.is_hidden() {
                false
            } else if let Some(filter) = filter {
                ops::FileSearch::matches_pattern(&e.name, filter, false)
            } else {
                true
            }
        })
        .cloned()
        .collect();
    ops::sort_entries(&mut sort_entries, sort_mode, sort_options);
    sort_entries
}

fn restore_panel_selection(panel: &mut PanelState, saved: &HashSet<PathBuf>) {
    for entry in &mut panel.entries {
        entry.selected = saved.contains(&entry.path);
    }
}

fn restore_panel_cursor(panel: &mut PanelState, current_name: Option<&str>) {
    if let Some(name) = current_name
        && let Some(pos) = panel.entries.iter().position(|e| e.name == name)
    {
        panel.cursor = pos;
    }
    if panel.cursor >= panel.entries.len() && !panel.entries.is_empty() {
        panel.cursor = panel.entries.len() - 1;
    }
}

const FALLBACK_VISIBLE_HEIGHT: usize = 18;

fn current_visible_height() -> usize {
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

const MENU_ITEM_LEFT_PANEL: usize = 0;
const MENU_ITEM_RIGHT_PANEL: usize = 4;

pub fn with_menu_panel<T>(state: &mut AppState, f: impl FnOnce(&mut AppState) -> T) -> T {
    let original = state.active_panel;
    match state.menu_selected {
        MENU_ITEM_LEFT_PANEL => set_active_panel(state, ActivePanel::Left),
        MENU_ITEM_RIGHT_PANEL => set_active_panel(state, ActivePanel::Right),
        _ => {}
    }
    let result = f(state);
    if matches!(state.mode, AppMode::Dialog(_)) {
        state.menu_restore_panel = Some(original);
    } else {
        set_active_panel(state, original);
    }
    result
}

pub fn panel_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(LAYOUT_OVERHEAD_ROWS) as usize
}

pub fn navigate_to_hotlist(state: &mut AppState, index: usize) {
    if index >= state.directory_hotlist.len() {
        state.status_message = Some(format!(
            "Hotlist index {} out of range (0..{})",
            index,
            state.directory_hotlist.len()
        ));
        return;
    }
    let path = &state.directory_hotlist[index];
    if !path.is_dir() {
        state.status_message = Some(format!("{} is not a directory", path.display()));
        return;
    }
    let display = path.display().to_string();
    let path = path.clone();
    state.active_panel_mut().path = path;
    state.active_panel_mut().cursor = 0;
    state.active_panel_mut().scroll_offset = 0;
    refresh_active(state);
    state.status_message = Some(format!("cd to {display}"));
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
