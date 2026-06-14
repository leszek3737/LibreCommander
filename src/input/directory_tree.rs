use crossterm::event::KeyCode;

use lc::app::{dir_tree, types::*};
use lc::ui::{DIR_TREE_OVERHEAD_ROWS, viewer};

use super::EventContext;
use crate::app::panel_ops::refresh_active;

pub(crate) fn handle_directory_tree(ctx: &mut EventContext, key: KeyCode) {
    let visible_height = directory_tree_visible_height(ctx.term_size.height);
    let state = &mut *ctx.state;
    let viewer_loader = &mut *ctx.viewer_loader;
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') if state.tree.selected > 0 => {
            state.tree.selected -= 1;
        }
        KeyCode::Down | KeyCode::Char('j')
            if !state.tree.entries.is_empty()
                && state.tree.selected + 1 < state.tree.entries.len() =>
        {
            state.tree.selected += 1;
        }
        KeyCode::Home if !state.tree.entries.is_empty() => {
            state.tree.selected = 0;
            state.tree.scroll = 0;
        }
        KeyCode::End if !state.tree.entries.is_empty() => {
            state.tree.selected = state.tree.entries.len() - 1;
        }
        KeyCode::PageUp => {
            state.tree.selected = state.tree.selected.saturating_sub(visible_height);
            state.tree.scroll = state.tree.scroll.saturating_sub(visible_height);
        }
        KeyCode::PageDown if !state.tree.entries.is_empty() => {
            state.tree.selected =
                (state.tree.selected + visible_height).min(state.tree.entries.len() - 1);
            state.tree.scroll = state
                .tree
                .scroll
                .saturating_add(visible_height)
                .min(state.tree.entries.len().saturating_sub(visible_height));
        }
        KeyCode::Enter => {
            handle_tree_enter(state, viewer_loader);
        }
        KeyCode::Char('c') => {
            handle_tree_cd(state);
        }
        _ => {}
    }

    ensure_selected_visible(state.tree.selected, &mut state.tree.scroll, visible_height);
}

fn ensure_selected_visible(selected: usize, scroll: &mut usize, visible_height: usize) {
    let effective = if selected < *scroll {
        selected
    } else if selected - *scroll >= visible_height {
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

    // Build the message directly into a single buffer instead of allocating an
    // intermediate Vec<String> plus a join() pass (avoids N+1 allocations).
    use std::fmt::Write as _;

    let mut buf = String::from("Directory tree warnings: [");
    for (i, d) in diagnostics.iter().enumerate() {
        if i > 0 {
            buf.push_str("] [");
        }
        // Writing to a String is infallible; the Result is intentionally ignored.
        let _ = write!(buf, "{}: {}", d.path.display(), d.message);
    }
    buf.push(']');
    *status_message = Some(buf);
}

fn handle_tree_enter(state: &mut AppState, viewer_loader: &mut Option<viewer::ViewerLoader>) {
    let selected = state.tree.selected;
    let is_dir = state.tree.entries.get(selected).is_some_and(|e| e.is_dir);

    if is_dir {
        let show_hidden = state.active_panel().show_hidden();
        let diagnostics = dir_tree::toggle_expand_with_diagnostics(
            &mut state.tree.entries,
            selected,
            show_hidden,
        );
        set_tree_diagnostic_status(&mut state.ui.status_message, &diagnostics);
        if state.tree.selected >= state.tree.entries.len() && !state.tree.entries.is_empty() {
            state.tree.selected = state.tree.entries.len() - 1;
        }
    } else if let Some(entry) = state.tree.entries.get(selected) {
        let path = entry.path.clone();
        *viewer_loader = Some(viewer::ViewerState::open_background(path));
        state.prev_mode = Some(std::mem::replace(&mut state.mode, AppMode::Viewing));
    }
}

fn handle_tree_cd(state: &mut AppState) {
    let (entry_is_dir, entry_path) = match state.tree.entries.get(state.tree.selected) {
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
        state.tree.selected = 0;
        state.tree.scroll = 0;
        refresh_active(state);
        state.mode = AppMode::Normal;
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Drive `handle_directory_tree` with a freshly built `EventContext` so the
    /// per-test call-sites stay as terse as the old positional form.
    fn tree_key(state: &mut AppState, key: KeyCode, height: u16) {
        let mut viewer_state = None;
        let mut viewer_loader = None;
        let mut image_preview_loader = None;
        let mut running_job = None;
        let mut ctx = EventContext {
            state,
            viewer_state: &mut viewer_state,
            viewer_loader: &mut viewer_loader,
            image_preview_loader: &mut image_preview_loader,
            running_job: &mut running_job,
            term_size: ratatui::layout::Size::new(80, height),
        };
        handle_directory_tree(&mut ctx, key);
    }

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
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(10);
        tree_key(&mut state, KeyCode::Esc, 24);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn tree_up_at_top_does_nothing() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(10);
        tree_key(&mut state, KeyCode::Up, 24);
        assert_eq!(state.tree.selected, 0);
    }

    #[test]
    fn tree_down_moves() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(10);
        tree_key(&mut state, KeyCode::Down, 24);
        assert_eq!(state.tree.selected, 1);
    }

    #[test]
    fn tree_down_at_end_does_nothing() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(5);
        state.tree.selected = 4;
        tree_key(&mut state, KeyCode::Down, 24);
        assert_eq!(state.tree.selected, 4);
    }

    #[test]
    fn tree_home_resets() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(50);
        state.tree.selected = 25;
        state.tree.scroll = 20;
        tree_key(&mut state, KeyCode::Home, 24);
        assert_eq!(state.tree.selected, 0);
        assert_eq!(state.tree.scroll, 0);
    }

    #[test]
    fn tree_end_goes_to_last() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = make_tree_entries(50);
        tree_key(&mut state, KeyCode::End, 24);
        assert_eq!(state.tree.selected, 49);
    }

    #[test]
    fn tree_empty_entries_doesnt_panic() {
        let mut state = AppState::default();
        state.mode = AppMode::DirectoryTree;
        state.tree.entries = vec![];
        tree_key(&mut state, KeyCode::Down, 24);
        assert_eq!(state.tree.selected, 0);
        tree_key(&mut state, KeyCode::End, 24);
        assert_eq!(state.tree.selected, 0);
        tree_key(&mut state, KeyCode::Enter, 24);
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
