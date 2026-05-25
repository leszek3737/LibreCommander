use std::path::PathBuf;

use crossterm::event::KeyCode;

use lc::app::user_menu::MenuSource;
use lc::app::{types::*, user_menu};
use lc::ops;

use crate::app::panel_ops::refresh_active;

fn handle_history_picker(state: &mut AppState, key: KeyCode, len: usize) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Home if len > 0 => {
            state.picker_selected = 0;
        }
        KeyCode::End if len > 0 => {
            state.picker_selected = len - 1;
        }
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
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Home if len > 0 => {
            state.picker_selected = 0;
        }
        KeyCode::End if len > 0 => {
            state.picker_selected = len - 1;
        }
        KeyCode::Enter => {
            if let Some(path) = state.hotlist().get(state.picker_selected).cloned() {
                if path.is_dir() {
                    state.active_panel_mut().set_path(path);
                    state.active_panel_mut().cursor = 0;
                    state.active_panel_mut().scroll_offset = 0;
                    refresh_active(state);
                } else {
                    state.status_message =
                        Some("Hotlist entry is no longer a valid directory".to_string());
                }
                state.mode = AppMode::Normal;
            } else {
                state.mode = AppMode::Normal;
            }
        }
        KeyCode::Char('a') => {
            let cur = state.active_panel().path.clone();
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
    const MODES: [CompareMode; 3] = [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
    let len = MODES.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Home => {
            state.picker_selected = 0;
        }
        KeyCode::End => {
            state.picker_selected = len - 1;
        }
        KeyCode::Enter => {
            let chosen = MODES[state.picker_selected.min(len - 1)];
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
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Home if len > 0 => {
            state.picker_selected = 0;
        }
        KeyCode::End if len > 0 => {
            state.picker_selected = len - 1;
        }
        KeyCode::Enter => {
            let idx = state.picker_selected.min(len.saturating_sub(1));
            state.mode = AppMode::Normal;
            if let Some(entry) = state.user_menu_entries.get(idx).cloned() {
                let active_dir = state.active_panel().path.clone();
                let other_dir = state.inactive_panel().path.clone();
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
    }
}

pub(crate) fn compare_directories(state: &mut AppState, mode: CompareMode) {
    let left_entries = if state.left_panel.listing.unfiltered_entries.is_empty() {
        &state.left_panel.listing.entries
    } else {
        &state.left_panel.listing.unfiltered_entries
    };
    let right_entries = if state.right_panel.listing.unfiltered_entries.is_empty() {
        &state.right_panel.listing.entries
    } else {
        &state.right_panel.listing.unfiltered_entries
    };
    let report = ops::compare_entries(left_entries, right_entries, mode);
    ops::apply_compare_to_panels(&mut state.left_panel, &mut state.right_panel, &report);

    let mode_name = match mode {
        CompareMode::Quick => "Quick",
        CompareMode::Size => "Size",
        CompareMode::Thorough => "Thorough",
    };
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

        // Can't go up from 0
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);

        // Can go down
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        // Can't go past end
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
        // Can't go past 2 (3 modes)
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 2);

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 1);
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
        // Can't go below 0
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
