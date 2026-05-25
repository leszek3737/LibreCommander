use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};

use lc::app::types::{AppMode, AppState, InputAction};
use lc::app::{panel_ops, shell};
use lc::ui::viewer;

use crate::{
    file_name_str, resume_terminal_stdout, suspend_terminal_stdout, terminal_state_file_path,
};

pub(crate) fn handle_function_keys<B: ratatui::backend::Backend>(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    terminal: &mut ratatui::Terminal<B>,
) {
    match key {
        KeyCode::F(1) => {
            state.mode = AppMode::Dialog(lc::app::types::DialogKind::Help {
                message: lc::app::keymap::build_help_message().to_string(),
                scroll_offset: 0,
            });
        }
        KeyCode::F(2) => {
            super::menu_actions::open_user_menu(state);
        }
        KeyCode::F(3) => {
            if let Some(entry) = state.active_panel().current_entry()
                && !entry.is_dir()
            {
                let path = entry.path.clone();
                *viewer_loader = Some(viewer::ViewerState::open_background(path));
                state.prev_mode = None;
                state.mode = AppMode::Viewing;
            }
        }
        KeyCode::F(4) => {
            launch_editor(state, terminal);
        }
        KeyCode::F(5) => {
            confirm_file_transfer(state, "Copy Confirm", "Copy", |sources, dest| {
                lc::app::types::PendingAction::Copy {
                    sources,
                    dest,
                    overwrite: false,
                }
            });
        }
        KeyCode::F(6) => {
            confirm_file_transfer(state, "Move Confirm", "Move", |sources, dest| {
                lc::app::types::PendingAction::Move {
                    sources,
                    dest,
                    overwrite: false,
                }
            });
        }
        KeyCode::F(7) => {
            state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
                prompt: "Create directory:".to_string(),
                action: InputAction::CreateDirectory,
            });
            state.dialog_input.clear();
        }
        KeyCode::F(8) => {
            confirm_delete(state);
        }
        KeyCode::F(9) => {
            state.prev_mode = Some(state.mode.clone());
            state.mode = AppMode::Menu;
            state.menu_item_selected = 0;
        }
        KeyCode::F(10) => {
            state.should_quit = true;
        }
        KeyCode::F(11) => {
            let entry_name = state.active_panel().current_entry().map(|e| e.name.clone());
            if let Some(name) = entry_name
                && name != ".."
            {
                state.dialog_input.text = name;
                state.dialog_input.cursor_end();
                state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
                    prompt: "Rename to:".to_string(),
                    action: InputAction::Rename,
                });
            }
        }
        _ => {}
    }
}

pub(crate) fn launch_editor<B: ratatui::backend::Backend>(
    state: &mut AppState,
    terminal: &mut ratatui::Terminal<B>,
) {
    let entry_info = state
        .active_panel()
        .current_entry()
        .map(|e| (e.is_dir(), e.path.clone()));
    if let Some((is_dir, path)) = entry_info
        && !is_dir
    {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        if let Err(e) = suspend_terminal_stdout() {
            state.status_message = Some(format!("Terminal suspend failed: {e}"));
            return;
        }
        let terminal_state_file = terminal_state_file_path();
        if let Some(parent) = terminal_state_file.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            lc::debug_log!("failed to create terminal state dir: {e}");
        }
        if let Err(e) = std::fs::write(&terminal_state_file, "alternate_screen") {
            lc::debug_log!("failed to write terminal state file: {e}");
        }
        let parts: Vec<String> = shlex::split(&editor).unwrap_or_else(|| {
            let fallback: Vec<String> = editor.split_whitespace().map(String::from).collect();
            if fallback.is_empty() {
                vec!["vi".to_string()]
            } else {
                fallback
            }
        });
        let cmd = parts.first().map_or("vi", |s| s.as_str());
        let status = std::process::Command::new(cmd)
            .args(parts.get(1..).unwrap_or_default())
            .arg(&path)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        let resume_result = resume_terminal_stdout();
        if let Err(e) = std::fs::remove_file(&terminal_state_file) {
            lc::debug_log!("failed to remove terminal state file: {e}");
        }
        match (status, resume_result) {
            (Err(e), _) => state.status_message = Some(format!("Editor error: {e}")),
            (Ok(s), Err(e)) if !s.success() => {
                state.status_message = Some(format!(
                    "Editor exited with status: {s}; terminal restore failed: {e}"
                ));
            }
            (_, Err(e)) => {
                state.status_message = Some(format!("Terminal restore failed after editor: {e}"));
            }
            (Ok(s), _) if !s.success() => {
                state.status_message = Some(format!("Editor exited with status: {s}"));
            }
            (Ok(_), Ok(_)) => {}
        }
        if let Err(e) = terminal.clear() {
            lc::debug_log!("terminal.clear() failed after editor: {e}");
        }
        panel_ops::refresh_active(state);
    }
}

pub(crate) fn confirm_file_transfer(
    state: &mut AppState,
    label: &str,
    verb: &str,
    make_pending: impl FnOnce(Vec<PathBuf>, PathBuf) -> lc::app::types::PendingAction,
) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let dest_dir = state.inactive_panel().path.clone();
    let file_names = panel_ops::file_names_from_paths(&paths);
    let msg = if paths.len() == 1 {
        let name = file_name_str(&paths[0]).unwrap_or_default();
        format!("{verb} '{name}' to '{}' ?", dest_dir.display())
    } else {
        format!(
            "{verb} {} entries to '{}' ?",
            paths.len(),
            dest_dir.display()
        )
    }
    .replace(" ?", "?");
    state.dialog_selection = 0;
    state.mode = AppMode::Dialog(lc::app::types::DialogKind::Confirm(
        lc::app::types::ConfirmDetails::with_files(label, &msg, file_names),
    ));
    state.pending_action = Some(make_pending(paths, dest_dir));
}

pub(crate) fn confirm_delete(state: &mut AppState) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let file_names = panel_ops::file_names_from_paths(&paths);
    let msg = if paths.len() == 1 {
        let name = file_name_str(&paths[0]).unwrap_or_default();
        format!("Delete '{name}'?")
    } else {
        format!("Delete {} entries?", paths.len())
    };
    state.dialog_selection = 0;
    state.mode = AppMode::Dialog(lc::app::types::DialogKind::Confirm(
        lc::app::types::ConfirmDetails::with_files("Delete Confirm", &msg, file_names),
    ));
    state.pending_action = Some(lc::app::types::PendingAction::Delete { paths });
}

pub(crate) fn handle_navigation_keys(
    state: &mut AppState,
    key: KeyCode,
    modifiers: KeyModifiers,
    visible: usize,
) {
    match key {
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            if panel.listing.entries.is_empty() {
                return;
            }
            panel.toggle_selection_at(panel.cursor);
            panel.move_cursor_up(visible);
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            if panel.listing.entries.is_empty() {
                return;
            }
            panel.toggle_selection_at(panel.cursor);
            panel.move_cursor_down(visible);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let panel = state.active_panel_mut();
            panel.move_cursor_up(visible);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let panel = state.active_panel_mut();
            panel.move_cursor_down(visible);
        }
        KeyCode::Home => {
            let p = state.active_panel_mut();
            p.cursor = 0;
            p.scroll_offset = 0;
        }
        KeyCode::End => {
            let len = state.active_panel().listing.entries.len();
            if len > 0 {
                let p = state.active_panel_mut();
                p.cursor = len - 1;
                p.ensure_cursor_visible(visible);
            }
        }
        KeyCode::PageUp => {
            let p = state.active_panel_mut();
            p.cursor = p.cursor.saturating_sub(visible);
            p.scroll_offset = p.scroll_offset.saturating_sub(visible);
        }
        KeyCode::PageDown => {
            let len = state.active_panel().listing.entries.len();
            let p = state.active_panel_mut();
            p.cursor = (p.cursor + visible).min(len.saturating_sub(1));
            p.scroll_offset = (p.scroll_offset + visible).min(len.saturating_sub(visible));
        }
        KeyCode::Tab => {
            state.active_panel = state.active_panel.toggle();
            let p = state.active_panel_mut();
            let max = p.listing.entries.len().saturating_sub(1);
            p.cursor = p.cursor.min(max);
            p.ensure_cursor_visible(visible);
        }
        KeyCode::Insert => {
            let panel = state.active_panel_mut();
            if panel.listing.entries.is_empty() {
                return;
            }
            panel.toggle_selection();
            if panel.cursor < panel.listing.entries.len() - 1 {
                panel.move_cursor_down(visible);
            }
        }
        _ => {}
    }
}

pub(crate) fn reposition_cursor_to_entry(
    state: &mut AppState,
    prev_dir_name: Option<&str>,
    visible: usize,
) {
    if let Some(name) = prev_dir_name
        && let Some(idx) = state
            .active_panel()
            .listing
            .entries
            .iter()
            .position(|e| e.name == name)
    {
        let p = state.active_panel_mut();
        p.cursor = idx;
        p.ensure_cursor_visible(visible);
    }
}

pub(crate) fn handle_enter_key(state: &mut AppState, visible: usize) {
    let entry_info = state.active_panel().current_entry().and_then(|e| {
        if e.is_dir() {
            Some((e.path.clone(), e.name == ".."))
        } else {
            None
        }
    });
    if let Some((path, is_dotdot)) = entry_info {
        let prev_dir_name = if is_dotdot {
            file_name_str(&state.active_panel().path)
        } else {
            None
        };
        let p = state.active_panel_mut();
        p.history.push(p.path.clone());
        p.set_path(path);
        p.cursor = 0;
        p.scroll_offset = 0;
        panel_ops::refresh_active(state);
        reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
    }
}

pub(crate) fn handle_ctrl_keys(state: &mut AppState, key: KeyCode, terminal_height: u16) {
    match key {
        KeyCode::Char('u') => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = state.active_panel.toggle();
        }
        KeyCode::Char('s') => {
            let panel = state.active_panel_mut();
            if panel.listing.unfiltered_entries.is_empty() {
                panel.listing.set_unfiltered(panel.listing.entries.clone());
            }
            state.mode = AppMode::Search;
            state.search_query.clear();
            state.search_cursor = 0;
        }
        KeyCode::Char('h') => {
            let visible = panel_ops::panel_visible_height(terminal_height);
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            panel_ops::rebuild_visible_entries(p, visible);
        }
        KeyCode::Char('r') => {
            panel_ops::refresh_active(state);
        }
        KeyCode::Char('o') => {
            if let Err(e) = shell::toggle_external_view(state, panel_ops::refresh_both) {
                state.status_message = Some(format!("External view error: {e}"));
            }
        }
        _ => {}
    }
}

pub(crate) fn handle_alt_keys(state: &mut AppState, key: KeyCode, visible: usize) {
    match key {
        KeyCode::Enter => {
            if let Some(entry) = state.active_panel().current_entry()
                && entry.name != ".."
            {
                state.mode = AppMode::Dialog(lc::app::types::DialogKind::Properties {
                    name: entry.name.clone(),
                    size: entry.size(),
                    mtime: entry.mtime(),
                    permissions: entry.mode_bits(),
                    owner: entry.owner.clone(),
                    group: entry.group.clone(),
                    is_dir: entry.is_dir(),
                    is_symlink: entry.is_symlink(),
                });
            }
        }
        KeyCode::Backspace => {
            let prev_dir_name = file_name_str(&state.active_panel().path);
            let panel = state.active_panel_mut();
            if let Some(prev_path) = panel.history.pop()
                && prev_path.is_dir()
            {
                panel.set_path(prev_path.clone());
                panel.cursor = 0;
                panel.scroll_offset = 0;
                panel_ops::refresh_active(state);
                reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
                state.status_message = Some(format!("cd to {}", prev_path.display()));
            }
        }
        KeyCode::Char(c) if ('1'..='9').contains(&c) => {
            panel_ops::navigate_to_hotlist(state, (c as usize) - ('1' as usize));
        }
        KeyCode::Char('c') => {
            state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
                prompt: "Quick cd:".to_string(),
                action: InputAction::QuickCd,
            });
            state.dialog_input.text = state.active_panel().path.display().to_string();
            state.dialog_input.cursor_end();
        }
        KeyCode::Char('x' | 'X') => state.enter_command_line_mode(),
        _ => {}
    }
}

pub(crate) fn selected_or_current_paths(state: &AppState) -> Vec<PathBuf> {
    let panel = state.active_panel();

    let current_entry_fallback = || {
        panel
            .current_entry()
            .filter(|entry| entry.name != "..")
            .map(|entry| vec![entry.path.clone()])
            .unwrap_or_default()
    };

    if panel.selected_count == 0 {
        return current_entry_fallback();
    }

    let selected: Vec<PathBuf> = panel
        .selected_entries()
        .into_iter()
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.path.clone())
        .collect();

    if selected.is_empty() {
        return current_entry_fallback();
    }

    selected
}
