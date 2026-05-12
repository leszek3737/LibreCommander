use crossterm::event::KeyCode;

use lc::app::{dir_tree, types::*};
use lc::ui::{DIR_TREE_OVERHEAD_ROWS, viewer};

use crate::app::panel_ops::refresh_active;

pub(crate) fn handle_directory_tree(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
) {
    let visible_height = directory_tree_visible_height(terminal_height);
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') if state.tree_selected > 0 => {
            state.tree_selected -= 1;
        }
        KeyCode::Down | KeyCode::Char('j')
            if !state.tree_entries.is_empty()
                && state.tree_selected + 1 < state.tree_entries.len() =>
        {
            state.tree_selected += 1;
        }
        KeyCode::Home => {
            state.tree_selected = 0;
            state.tree_scroll = 0;
        }
        KeyCode::End if !state.tree_entries.is_empty() => {
            state.tree_selected = state.tree_entries.len() - 1;
        }
        KeyCode::PageUp => {
            state.tree_selected = state.tree_selected.saturating_sub(visible_height);
            state.tree_scroll = state.tree_scroll.saturating_sub(visible_height);
        }
        KeyCode::PageDown if !state.tree_entries.is_empty() => {
            state.tree_selected =
                (state.tree_selected + visible_height).min(state.tree_entries.len() - 1);
            state.tree_scroll = state
                .tree_scroll
                .saturating_add(visible_height)
                .min(state.tree_entries.len().saturating_sub(visible_height));
        }
        KeyCode::Enter => {
            let selected = state.tree_selected;
            let is_dir = state.tree_entries.get(selected).is_some_and(|e| e.is_dir);
            let is_file = state.tree_entries.get(selected).is_some_and(|e| !e.is_dir);

            if is_dir {
                let show_hidden = state.active_panel().show_hidden;
                let diagnostics = dir_tree::toggle_expand_with_diagnostics(
                    &mut state.tree_entries,
                    selected,
                    show_hidden,
                );
                set_tree_diagnostic_status(&mut state.status_message, &diagnostics);
                if state.tree_selected >= state.tree_entries.len() && !state.tree_entries.is_empty()
                {
                    state.tree_selected = state.tree_entries.len() - 1;
                }
            } else if is_file {
                let path = state.tree_entries[selected].path.clone();
                if let Ok(vs) = viewer::ViewerState::open(&path) {
                    *viewer_state = Some(vs);
                    state.prev_mode = Some(state.mode.clone());
                    state.mode = AppMode::Viewing;
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(entry) = state.tree_entries.get(state.tree_selected) {
                let target = if entry.is_dir {
                    entry.path.clone()
                } else {
                    entry
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default()
                };
                if !target.as_os_str().is_empty() && target.is_dir() {
                    state.active_panel_mut().path = target;
                    state.active_panel_mut().cursor = 0;
                    state.active_panel_mut().scroll_offset = 0;
                    refresh_active(state);
                    state.mode = AppMode::Normal;
                }
            }
        }
        _ => {}
    }

    let selected = state.tree_selected;
    let scroll = state.tree_scroll;
    let effective = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };
    state.tree_scroll = effective;
}

pub(crate) fn directory_tree_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(DIR_TREE_OVERHEAD_ROWS) as usize
}

pub(crate) fn set_tree_diagnostic_status(
    status_message: &mut Option<String>,
    diagnostics: &[dir_tree::TreeDiagnostic],
) {
    if diagnostics.is_empty() {
        return;
    }

    let first = &diagnostics[0];
    *status_message = Some(format!(
        "Directory tree warning: {}: {}{}",
        first.path.display(),
        first.message,
        if diagnostics.len() > 1 {
            format!(", {} more", diagnostics.len() - 1)
        } else {
            String::new()
        }
    ));
}
