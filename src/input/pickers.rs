use std::path::PathBuf;

use crossterm::event::KeyCode;

use lc::app::{file_type, types::*, user_menu, user_menu::MenuSource};
use lc::ops;

use crate::app::panel_ops::{navigate_to_hotlist, refresh_active};

#[derive(Clone, Copy)]
enum MoveDirection {
    Up,
    Down,
    Home,
    End,
}

fn move_cursor(entries_len: usize, selected: &mut usize, direction: MoveDirection) {
    if entries_len == 0 {
        return;
    }
    match direction {
        MoveDirection::Up if *selected > 0 => {
            *selected -= 1;
        }
        MoveDirection::Down if *selected + 1 < entries_len => {
            *selected += 1;
        }
        MoveDirection::Home => {
            *selected = 0;
        }
        MoveDirection::End => {
            *selected = entries_len - 1;
        }
        _ => {}
    }
}

/// Result of the shared picker key dispatch.
enum NavOutcome {
    /// The key was fully handled here (Esc closed the picker, or the cursor
    /// moved). The caller should stop processing.
    Handled,
    /// Not a shared navigation key; the caller handles it (e.g. Enter, hotkeys).
    Passthrough,
}

/// Handle the navigation keys common to every list picker: `Esc` closes the
/// picker, and Up/Down/Home/End move the shared `picker_selected` cursor.
///
/// Centralizing these arms keeps the per-picker handlers focused on their own
/// `Enter`/hotkey behaviour instead of repeating five identical match arms.
fn handle_nav_key(state: &mut AppState, key: KeyCode, len: usize) -> NavOutcome {
    let direction = match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            return NavOutcome::Handled;
        }
        KeyCode::Up => MoveDirection::Up,
        KeyCode::Down => MoveDirection::Down,
        KeyCode::Home => MoveDirection::Home,
        KeyCode::End => MoveDirection::End,
        _ => return NavOutcome::Passthrough,
    };
    move_cursor(len, &mut state.ui.picker_selected, direction);
    NavOutcome::Handled
}

fn handle_history_picker(state: &mut AppState, key: KeyCode, len: usize) {
    if let NavOutcome::Handled = handle_nav_key(state, key, len) {
        return;
    }
    if key == KeyCode::Enter {
        if state.ui.picker_selected >= len {
            state.mode = AppMode::Normal;
            return;
        }
        // History displays most-recent-first; reverse visual index to VecDeque position
        let idx = len - 1 - state.ui.picker_selected;
        if let Some(cmd) = state.input.command_history.get(idx).cloned() {
            state.input.command_line.set_text_at_end(cmd);
            state.mode = AppMode::CommandLine;
        } else {
            state.mode = AppMode::Normal;
        }
    }
}

fn handle_hotlist_picker(state: &mut AppState, key: KeyCode, len: usize) {
    if let NavOutcome::Handled = handle_nav_key(state, key, len) {
        return;
    }
    match key {
        KeyCode::Enter => {
            let idx = state.ui.picker_selected;
            state.mode = AppMode::Normal;
            navigate_to_hotlist(state, idx);
        }
        KeyCode::Char('a') => {
            let cur = state.active_panel().path().to_path_buf();
            if state.hotlist().iter().any(|p| p == &cur) {
                state.set_status("Directory already in hotlist");
            } else {
                state.hotlist_push(cur);
                state.set_status("Added current directory to hotlist");
            }
        }
        KeyCode::Char('d') if state.ui.picker_selected < state.hotlist().len() => {
            state.hotlist_remove(state.ui.picker_selected);
            let hotlist_len = state.hotlist().len();
            if state.ui.picker_selected > 0 && state.ui.picker_selected >= hotlist_len {
                state.ui.picker_selected -= 1;
            }
        }
        _ => {}
    }
}

fn handle_compare_mode_picker(state: &mut AppState, key: KeyCode) {
    let modes = CompareMode::ALL;
    let len = modes.len();
    if let NavOutcome::Handled = handle_nav_key(state, key, len) {
        return;
    }
    if key == KeyCode::Enter {
        let Some(&chosen) = modes.get(state.ui.picker_selected) else {
            state.mode = AppMode::Normal;
            return;
        };
        state.mode = AppMode::Normal;
        compare_directories(state, chosen);
    }
}

fn handle_user_menu_picker(state: &mut AppState, key: KeyCode) {
    let len = state.ui.user_menu_entries.len();
    if let NavOutcome::Handled = handle_nav_key(state, key, len) {
        return;
    }
    if key == KeyCode::Enter {
        activate_user_menu_entry(state, len);
    }
}

/// Run the user-menu entry under the cursor: build the substitution context,
/// expand the command template, then either confirm (local menus) or execute.
fn activate_user_menu_entry(state: &mut AppState, len: usize) {
    let idx = state.ui.picker_selected.min(len.saturating_sub(1));
    state.mode = AppMode::Normal;
    let cmd = match resolve_user_menu_command(state, idx) {
        Some(Ok(cmd)) => cmd,
        Some(Err(err)) => {
            state.ui.status_message = Some(err);
            return;
        }
        None => return,
    };
    dispatch_user_menu_command(state, cmd);
}

/// Expand the substitution template of the entry at `idx` against the current
/// panels. Returns `None` when no entry exists at `idx`, otherwise the
/// expansion result. Only the command template is cloned (not the whole
/// `MenuEntry`), so the compiled condition is never duplicated.
fn resolve_user_menu_command(state: &AppState, idx: usize) -> Option<Result<String, String>> {
    let command = state.ui.user_menu_entries.get(idx)?.command.clone();
    let active_dir = state.active_panel().path().to_path_buf();
    let other_dir = state.inactive_panel().path().to_path_buf();
    let current_file = state
        .active_panel()
        .current_entry()
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let tagged: Vec<PathBuf> = state
        .active_panel()
        .selected_entries()
        .filter(|e| e.name != "..")
        .map(|e| e.path.clone())
        .collect();
    let ctx = user_menu::SubstContext {
        current_file: std::path::Path::new(&current_file),
        active_dir: &active_dir,
        other_dir: &other_dir,
        tagged: &tagged,
    };
    Some(user_menu::apply_substitutions(&command, &ctx))
}

/// Either prompt for trust (local menus, which may carry untrusted commands)
/// or run the expanded command straight away (global menus).
fn dispatch_user_menu_command(state: &mut AppState, cmd: String) {
    if state.ui.user_menu_source == MenuSource::Local {
        state.ui.pending_menu_command = Some(cmd);
        state.input.dialog_selection = 0;
        state.mode = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Trust Local Menu?",
            "This menu comes from the current directory.\n\
                 Running untrusted commands may be dangerous.\n\n\
                 Execute?",
        )));
    } else {
        lc::app::shell::run_shell_command(state, &cmd, true, refresh_active);
    }
}

fn handle_archive_menu_picker(state: &mut AppState, key: KeyCode) {
    const ITEMS: [&str; 2] = ["Extract Archive", "Create Archive"];
    let len = ITEMS.len();
    if let NavOutcome::Handled = handle_nav_key(state, key, len) {
        return;
    }
    if key == KeyCode::Enter {
        let choice = state.ui.picker_selected;
        state.mode = AppMode::Normal;
        match choice {
            0 => {
                // Preserve original behaviour: do nothing when the panel has no
                // current entry; only report when an entry exists but is not an
                // archive.
                if let Some(on_archive) = state
                    .active_panel()
                    .current_entry()
                    .map(|e| e.name != ".." && file_type::is_archive(&e.name))
                {
                    if on_archive {
                        super::normal::show_archive_dialog(state);
                    } else {
                        state.set_status("Cursor is not on an archive file");
                    }
                }
            }
            1 => {
                let paths = super::normal::selected_or_current_paths(state);
                if paths.is_empty() {
                    state.set_status("No files selected");
                } else {
                    show_create_dialog(state, paths);
                }
            }
            _ => {}
        }
    }
}

fn show_create_dialog(state: &mut AppState, sources: Vec<PathBuf>) {
    let dest_input = TextInput::new();
    state.mode = AppMode::Dialog(DialogKind::ArchiveCreate(Box::new(ArchiveCreateDetails {
        sources,
        dest_input,
    })));
}

pub(crate) fn handle_list_picker(state: &mut AppState, key: KeyCode) {
    let kind = if let AppMode::ListPicker(ref k) = state.mode {
        *k
    } else {
        return;
    };

    match kind {
        PickerKind::History => {
            handle_history_picker(state, key, state.input.command_history.len());
        }
        PickerKind::Hotlist => {
            handle_hotlist_picker(state, key, state.hotlist().len());
        }
        PickerKind::CompareMode => {
            handle_compare_mode_picker(state, key);
        }
        PickerKind::UserMenu => {
            handle_user_menu_picker(state, key);
        }
        PickerKind::ArchiveMenu => {
            handle_archive_menu_picker(state, key);
        }
    }
}

/// Entries to feed into a directory comparison: always the full, unfiltered
/// listing. Comparison must reflect the whole directory, not whatever subset a
/// user-applied filter happens to show, so any active filter is ignored here.
fn comparable_entries(panel: &PanelState) -> &[FileEntry] {
    panel.listing.unfiltered()
}

pub(crate) fn compare_directories(state: &mut AppState, mode: CompareMode) {
    let left_entries = comparable_entries(&state.left_panel);
    let right_entries = comparable_entries(&state.right_panel);
    let report = ops::compare_entries(left_entries, right_entries, mode);
    ops::apply_compare_to_panels(&mut state.left_panel, &mut state.right_panel, &report);

    let mode_name = mode.label();
    state.ui.status_message = None;
    state.input.dialog_selection = 0;
    state.mode = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
        "Compare Results",
        &format!(
            "Compare dirs ({mode_name}):\nUnique in left:  {}\nUnique in right: {}\nDiffering:       {}",
            report.unique_left, report.unique_right, report.differing
        ),
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_picker_esc_returns_normal() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::History),
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Esc);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn history_picker_enter_empty_history() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::History),
            ui: UiState {
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn history_picker_navigate_bounds() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::History),
            ui: UiState {
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        state.input.command_history.push_back("cmd1".to_string());
        state.input.command_history.push_back("cmd2".to_string());

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.ui.picker_selected, 0);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 1);
    }

    #[test]
    fn hotlist_picker_empty_hotlist_enter() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            ui: UiState {
                directory_hotlist: vec![],
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn compare_mode_picker_navigate() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            ui: UiState {
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 1);
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 2);
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 2);

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.ui.picker_selected, 1);
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.ui.picker_selected, 0);
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.ui.picker_selected, 0);
    }

    #[test]
    fn user_menu_picker_empty_list() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            ui: UiState {
                user_menu_entries: vec![],
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.ui.picker_selected, 0);
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn hotlist_picker_delete_empty_after_delete() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            ui: UiState {
                directory_hotlist: vec![PathBuf::from("/only")],
                picker_selected: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Char('d'));
        assert!(state.ui.directory_hotlist.is_empty());
        assert_eq!(state.ui.picker_selected, 0);
    }
}
