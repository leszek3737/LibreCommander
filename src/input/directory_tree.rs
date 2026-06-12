use crossterm::event::KeyCode;

use lc::app::{dir_tree, types::*};
use lc::ui::{DIR_TREE_OVERHEAD_ROWS, viewer};

use crate::app::panel_ops::refresh_active;

pub(crate) fn handle_directory_tree(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
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
        KeyCode::Home if !state.tree_entries.is_empty() => {
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
            handle_tree_enter(state, viewer_loader);
        }
        KeyCode::Char('c') => {
            handle_tree_cd(state);
        }
        _ => {}
    }

    ensure_selected_visible(state.tree_selected, &mut state.tree_scroll, visible_height);
}

fn ensure_selected_visible(selected: usize, scroll: &mut usize, visible_height: usize) {
    let effective = if selected < *scroll {
        selected
    } else if selected >= *scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        *scroll
    };
    *scroll = effective;
}

pub(crate) fn directory_tree_visible_height(terminal_height: u16) -> usize {
    terminal_height
        .saturating_sub(DIR_TREE_OVERHEAD_ROWS)
        .max(1) as usize
}

pub(crate) fn set_tree_diagnostic_status(
    status_message: &mut Option<String>,
    diagnostics: &[dir_tree::TreeDiagnostic],
) {
    if diagnostics.is_empty() {
        *status_message = None;
        return;
    }

    let items: Vec<String> = diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.path.display(), d.message))
        .collect();
    *status_message = Some(format!("Directory tree warnings: [{}]", items.join("] [")));
}

fn handle_tree_enter(state: &mut AppState, viewer_loader: &mut Option<viewer::ViewerLoader>) {
    let selected = state.tree_selected;
    let is_dir = state.tree_entries.get(selected).is_some_and(|e| e.is_dir);

    if is_dir {
        let show_hidden = state.active_panel().show_hidden();
        let diagnostics = dir_tree::toggle_expand_with_diagnostics(
            &mut state.tree_entries,
            selected,
            show_hidden,
        );
        set_tree_diagnostic_status(&mut state.status_message, &diagnostics);
        if state.tree_selected >= state.tree_entries.len() && !state.tree_entries.is_empty() {
            state.tree_selected = state.tree_entries.len() - 1;
        }
    } else if let Some(entry) = state.tree_entries.get(selected) {
        let path = entry.path.clone();
        *viewer_loader = Some(viewer::ViewerState::open_background(path));
        state.prev_mode = Some(std::mem::replace(&mut state.mode, AppMode::Viewing));
    }
}

fn handle_tree_cd(state: &mut AppState) {
    let (entry_is_dir, entry_path) = match state.tree_entries.get(state.tree_selected) {
        Some(e) => (e.is_dir, e.path.clone()),
        None => return,
    };
    let target = if entry_is_dir {
        entry_path
    } else {
        match entry_path.parent() {
            Some(p) => p.to_path_buf(),
            None => entry_path,
        }
    };
    if target.is_dir() {
        let panel = state.active_panel_mut();
        panel.set_path(target);
        panel.cursor = 0;
        panel.scroll_offset = 0;
        state.tree_selected = 0;
        state.tree_scroll = 0;
        refresh_active(state);
        state.mode = AppMode::Normal;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_tree_entries(count: usize) -> Vec<lc::app::dir_tree::TreeEntry> {
        (0..count)
            .map(|i| {
                let name = format!("entry-{i}");
                let name_width = unicode_width::UnicodeWidthStr::width(name.as_str());
                lc::app::dir_tree::TreeEntry {
                    path: PathBuf::from(format!("/tmp/{i}")),
                    depth: 0,
                    is_dir: false,
                    expanded: false,
                    name,
                    name_width,
                    read_error: false,
                }
            })
            .collect()
    }

    #[test]
    fn tree_esc_returns_normal() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(10),
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Esc, 24);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn tree_up_at_top_does_nothing() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(10),
            tree_selected: 0,
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Up, 24);
        assert_eq!(state.tree_selected, 0);
    }

    #[test]
    fn tree_down_moves() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(10),
            tree_selected: 0,
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Down, 24);
        assert_eq!(state.tree_selected, 1);
    }

    #[test]
    fn tree_down_at_end_does_nothing() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(5),
            tree_selected: 4,
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Down, 24);
        assert_eq!(state.tree_selected, 4);
    }

    #[test]
    fn tree_home_resets() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(50),
            tree_selected: 25,
            tree_scroll: 20,
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Home, 24);
        assert_eq!(state.tree_selected, 0);
        assert_eq!(state.tree_scroll, 0);
    }

    #[test]
    fn tree_end_goes_to_last() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: make_tree_entries(50),
            tree_selected: 0,
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::End, 24);
        assert_eq!(state.tree_selected, 49);
    }

    #[test]
    fn tree_empty_entries_doesnt_panic() {
        let mut state = AppState {
            mode: AppMode::DirectoryTree,
            tree_entries: vec![],
            ..Default::default()
        };
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Down, 24);
        assert_eq!(state.tree_selected, 0);
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::End, 24);
        assert_eq!(state.tree_selected, 0);
        handle_directory_tree(&mut state, &mut None, &mut None, KeyCode::Enter, 24);
    }

    #[test]
    fn directory_tree_visible_height_calc() {
        assert_eq!(directory_tree_visible_height(24), 21);
        assert_eq!(directory_tree_visible_height(10), 7);
        assert_eq!(directory_tree_visible_height(0), 1);
    }

    #[test]
    fn set_tree_diagnostic_status_empty() {
        let mut msg = Some("old".to_string());
        set_tree_diagnostic_status(&mut msg, &[]);
        assert_eq!(msg, None);
    }

    #[test]
    fn set_tree_diagnostic_status_single() {
        let mut msg = None;
        let diagnostics = vec![lc::app::dir_tree::TreeDiagnostic {
            path: PathBuf::from("/tmp/bad"),
            message: "permission denied".to_string(),
            ..Default::default()
        }];
        set_tree_diagnostic_status(&mut msg, &diagnostics);
        assert!(
            msg.as_deref()
                .is_some_and(|m| m.contains("permission denied"))
        );
        assert!(msg.as_deref().is_some_and(|m| m.contains("/tmp/bad")));
    }

    #[test]
    fn set_tree_diagnostic_status_multiple() {
        let mut msg = None;
        let diagnostics = vec![
            lc::app::dir_tree::TreeDiagnostic {
                path: PathBuf::from("/tmp/a"),
                message: "err1".to_string(),
                ..Default::default()
            },
            lc::app::dir_tree::TreeDiagnostic {
                path: PathBuf::from("/tmp/b"),
                message: "err2".to_string(),
                ..Default::default()
            },
        ];
        set_tree_diagnostic_status(&mut msg, &diagnostics);
        assert!(msg.is_some(), "expected Some");
        let msg = msg.as_deref().unwrap_or("");
        assert!(msg.contains("err1"));
        assert!(msg.contains("err2"));
    }
}
