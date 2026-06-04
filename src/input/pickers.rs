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

fn handle_history_picker(state: &mut AppState, key: KeyCode, len: usize) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up => move_cursor(len, &mut state.picker_selected, MoveDirection::Up),
        KeyCode::Down => move_cursor(len, &mut state.picker_selected, MoveDirection::Down),
        KeyCode::Home => move_cursor(len, &mut state.picker_selected, MoveDirection::Home),
        KeyCode::End => move_cursor(len, &mut state.picker_selected, MoveDirection::End),
        KeyCode::Enter => {
            if state.picker_selected >= len {
                state.mode = AppMode::Normal;
                return;
            }
            let idx = len.saturating_sub(1).saturating_sub(state.picker_selected);
            if let Some(cmd) = state.command_history.get(idx).cloned() {
                state.command_line.text = cmd;
                state.command_line.cursor_end();
                state.mode = AppMode::CommandLine;
            } else {
                state.mode = AppMode::Normal;
            }
        }
        _ => {}
    }
}

fn handle_hotlist_picker(state: &mut AppState, key: KeyCode, len: usize) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up => move_cursor(len, &mut state.picker_selected, MoveDirection::Up),
        KeyCode::Down => move_cursor(len, &mut state.picker_selected, MoveDirection::Down),
        KeyCode::Home => move_cursor(len, &mut state.picker_selected, MoveDirection::Home),
        KeyCode::End => move_cursor(len, &mut state.picker_selected, MoveDirection::End),
        KeyCode::Enter => {
            let idx = state.picker_selected;
            state.mode = AppMode::Normal;
            navigate_to_hotlist(state, idx);
        }
        KeyCode::Char('a') => {
            let cur = state.active_panel().path().to_path_buf();
            if state.hotlist().iter().any(|p| p == &cur) {
                state.status_message = Some("Directory already in hotlist".to_string());
            } else {
                state.hotlist_push(cur);
                state.status_message = Some("Added current directory to hotlist".to_string());
            }
        }
        KeyCode::Char('d') if state.picker_selected < state.hotlist().len() => {
            state.hotlist_remove(state.picker_selected);
            if state.picker_selected > 0 && state.picker_selected >= state.hotlist().len() {
                state.picker_selected -= 1;
            }
        }
        _ => {}
    }
}

fn handle_compare_mode_picker(state: &mut AppState, key: KeyCode) {
    let modes = CompareMode::ALL;
    let len = modes.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up => move_cursor(len, &mut state.picker_selected, MoveDirection::Up),
        KeyCode::Down => move_cursor(len, &mut state.picker_selected, MoveDirection::Down),
        KeyCode::Home => move_cursor(len, &mut state.picker_selected, MoveDirection::Home),
        KeyCode::End => move_cursor(len, &mut state.picker_selected, MoveDirection::End),
        KeyCode::Enter => {
            let chosen = modes[state.picker_selected.min(len.saturating_sub(1))];
            state.mode = AppMode::Normal;
            compare_directories(state, chosen);
        }
        _ => {}
    }
}

fn handle_user_menu_picker(state: &mut AppState, key: KeyCode) {
    let len = state.user_menu_entries.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up => move_cursor(len, &mut state.picker_selected, MoveDirection::Up),
        KeyCode::Down => move_cursor(len, &mut state.picker_selected, MoveDirection::Down),
        KeyCode::Home => move_cursor(len, &mut state.picker_selected, MoveDirection::Home),
        KeyCode::End => move_cursor(len, &mut state.picker_selected, MoveDirection::End),
        KeyCode::Enter => {
            let idx = state.picker_selected.min(len.saturating_sub(1));
            state.mode = AppMode::Normal;
            if let Some(entry) = state.user_menu_entries.get(idx).cloned() {
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
                    .into_iter()
                    .filter(|e| e.name != "..")
                    .map(|e| e.path.clone())
                    .collect();
                let ctx = user_menu::SubstContext {
                    current_file: std::path::Path::new(&current_file),
                    active_dir: &active_dir,
                    other_dir: &other_dir,
                    tagged: &tagged,
                };
                let cmd = match user_menu::apply_substitutions(&entry.command, &ctx) {
                    Ok(c) => c,
                    Err(e) => {
                        state.status_message = Some(e);
                        return;
                    }
                };
                if state.user_menu_source == MenuSource::Local {
                    state.pending_menu_command = Some(cmd);
                    state.dialog_selection = 0;
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
        }
        _ => {}
    }
}

fn handle_archive_menu_picker(state: &mut AppState, key: KeyCode) {
    const ITEMS: [&str; 2] = ["Extract Archive", "Create Archive"];
    let len = ITEMS.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up => move_cursor(len, &mut state.picker_selected, MoveDirection::Up),
        KeyCode::Down => move_cursor(len, &mut state.picker_selected, MoveDirection::Down),
        KeyCode::Home => move_cursor(len, &mut state.picker_selected, MoveDirection::Home),
        KeyCode::End => move_cursor(len, &mut state.picker_selected, MoveDirection::End),
        KeyCode::Enter => {
            let choice = state.picker_selected;
            state.mode = AppMode::Normal;
            match choice {
                0 => {
                    if let Some(entry) = state.active_panel().current_entry() {
                        if entry.name != ".." && file_type::is_archive(&entry.name) {
                            super::normal::show_archive_dialog(state);
                        } else {
                            state.status_message =
                                Some("Cursor is not on an archive file".to_string());
                        }
                    }
                }
                1 => {
                    let paths = super::normal::selected_or_current_paths(state);
                    if paths.is_empty() {
                        state.status_message = Some("No files selected".to_string());
                    } else {
                        show_create_dialog(state, paths);
                    }
                }
                _ => {}
            }
        }
        _ => {}
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
            handle_history_picker(state, key, state.command_history.len());
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

fn effective_entries(panel: &PanelState) -> &[FileEntry] {
    if panel.listing.unfiltered_entries.is_empty() {
        &panel.listing.entries
    } else {
        &panel.listing.unfiltered_entries
    }
}

pub(crate) fn compare_directories(state: &mut AppState, mode: CompareMode) {
    let left_entries = effective_entries(&state.left_panel);
    let right_entries = effective_entries(&state.right_panel);
    let report = ops::compare_entries(left_entries, right_entries, mode);
    ops::apply_compare_to_panels(&mut state.left_panel, &mut state.right_panel, &report);

    let mode_name = mode.label();
    state.status_message = None;
    state.dialog_selection = 0;
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
            picker_selected: 0,
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn history_picker_navigate_bounds() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::History),
            picker_selected: 0,
            ..Default::default()
        };
        state.command_history.push_back("cmd1".to_string());
        state.command_history.push_back("cmd2".to_string());

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);
    }

    #[test]
    fn hotlist_picker_empty_hotlist_enter() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            directory_hotlist: vec![],
            picker_selected: 0,
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn compare_mode_picker_navigate() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            picker_selected: 0,
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 2);
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 2);

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 1);
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn user_menu_picker_empty_list() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            user_menu_entries: vec![],
            picker_selected: 0,
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 0);
        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn hotlist_picker_delete_empty_after_delete() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            directory_hotlist: vec![PathBuf::from("/only")],
            picker_selected: 0,
            ..Default::default()
        };
        handle_list_picker(&mut state, KeyCode::Char('d'));
        assert!(state.directory_hotlist.is_empty());
        assert_eq!(state.picker_selected, 0);
    }
}
