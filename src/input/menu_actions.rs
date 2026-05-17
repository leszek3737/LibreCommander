use crossterm::event::{KeyCode, KeyModifiers};

use lc::app::user_menu::MenuSource;
use lc::app::{config, dir_tree, types::*, user_menu};
use lc::menu::{MenuAction, menu_action_at};
use lc::ops;

use super::directory_tree::set_tree_diagnostic_status;
use crate::app::panel_ops::{refresh_active, with_menu_panel};

#[allow(clippy::too_many_lines)]
pub fn execute_menu_action(state: &mut AppState) -> Option<(KeyCode, KeyModifiers, bool)> {
    let action = menu_action_at(state.menu_selected, state.menu_item_selected)?;
    match action {
        MenuAction::ToggleListingMode => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.listing_mode = match panel.listing_mode {
                    ListingMode::Long => ListingMode::Brief,
                    ListingMode::Brief => ListingMode::Long,
                };
                state.status_message = Some(format!("Layout changed to {:?}", panel.listing_mode));
            });
            None
        }
        MenuAction::CycleSortOrder => {
            with_menu_panel(state, |state| {
                let p = state.active_panel_mut();
                p.sort_mode = ops::cycle_sort_mode(p.sort_mode);
                refresh_active(state);
            });
            None
        }
        MenuAction::OpenFilter => {
            with_menu_panel(state, |state| {
                state.dialog_input = state.active_panel().filter.clone().unwrap_or_default();
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(DialogKind::Input {
                    prompt: "Filter:".to_string(),
                    default_text: state.dialog_input.clone(),
                    action: InputAction::Filter,
                });
            });
            None
        }
        MenuAction::RefreshPanel => Some((KeyCode::Char('r'), KeyModifiers::CONTROL, true)),
        MenuAction::ResetPanelFilter => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.filter = None;
                refresh_active(state);
                state.status_message = Some("Panel filter reset".to_string());
            });
            None
        }
        MenuAction::ToggleHiddenFiles => Some((KeyCode::Char('h'), KeyModifiers::CONTROL, true)),
        MenuAction::TogglePermissions => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.show_permissions = !panel.show_permissions;
                state.status_message = Some(format!(
                    "Permissions: {}",
                    if panel.show_permissions { "ON" } else { "OFF" }
                ));
            });
            None
        }
        MenuAction::DirectoryTree => {
            with_menu_panel(state, |state| {
                let path = state.active_panel().path.clone();
                let show_hidden = state.active_panel().show_hidden;
                let tree = dir_tree::build_tree_with_diagnostics(&path, 2, show_hidden);
                state.tree_root = path;
                state.tree_entries = tree.entries;
                state.tree_selected = 0;
                state.tree_scroll = 0;
                state.mode = AppMode::DirectoryTree;
                set_tree_diagnostic_status(&mut state.status_message, &tree.diagnostics);
            });
            None
        }
        MenuAction::FindFile => {
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            state.mode = AppMode::Dialog(DialogKind::Input {
                prompt: "Find file:".to_string(),
                default_text: String::new(),
                action: InputAction::FindFile,
            });
            None
        }
        MenuAction::SwapPanels => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        MenuAction::SwitchPanels => {
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        MenuAction::CompareDirs => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::CompareMode);
            None
        }
        MenuAction::History => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::History);
            None
        }
        MenuAction::DirectoryHotlist => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::Hotlist);
            None
        }
        MenuAction::SaveCurrentPathToHotlist => {
            with_menu_panel(state, |state| {
                if !state
                    .directory_hotlist
                    .iter()
                    .any(|p| p == &state.active_panel().path)
                {
                    state
                        .directory_hotlist
                        .push(state.active_panel().path.clone());
                }
                state.status_message =
                    Some("Path added to hotlist (run Save Setup to persist)".to_string());
            });
            None
        }
        MenuAction::ViewFile => Some((KeyCode::F(3), KeyModifiers::NONE, false)),
        MenuAction::EditFile => Some((KeyCode::F(4), KeyModifiers::NONE, false)),
        MenuAction::Copy => Some((KeyCode::F(5), KeyModifiers::NONE, false)),
        MenuAction::Move => Some((KeyCode::F(6), KeyModifiers::NONE, false)),
        MenuAction::MakeDirectory => Some((KeyCode::F(7), KeyModifiers::NONE, false)),
        MenuAction::Delete => Some((KeyCode::F(8), KeyModifiers::NONE, false)),
        MenuAction::Rename => {
            with_menu_panel(state, |state| {
                let entry_name = state.active_panel().current_entry().map(|e| e.name.clone());
                if let Some(name) = entry_name
                    && name != ".."
                {
                    state.dialog_input = name.clone();
                    state.dialog_cursor_pos = state.dialog_input.chars().count();
                    state.mode = AppMode::Dialog(DialogKind::Input {
                        prompt: "Rename to:".to_string(),
                        default_text: name,
                        action: InputAction::Rename,
                    });
                }
            });
            None
        }
        MenuAction::Chmod => {
            with_menu_panel(state, |state| {
                let entry_info = state
                    .active_panel()
                    .current_entry()
                    .map(|e| (e.name.clone(), e.mode_bits()));
                if let Some((name, permissions)) = entry_info
                    && name != ".."
                {
                    state.dialog_input = format!("{:o}", permissions & 0o7777);
                    state.dialog_cursor_pos = state.dialog_input.chars().count();
                    state.mode = AppMode::Dialog(DialogKind::Input {
                        prompt: "Chmod (octal):".to_string(),
                        default_text: state.dialog_input.clone(),
                        action: InputAction::Chmod,
                    });
                }
            });
            None
        }
        MenuAction::Quit => {
            state.should_quit = true;
            None
        }
        MenuAction::SaveSetup => {
            match config::save_setup(state) {
                Ok(path) => {
                    state.status_message = Some(format!("Setup saved to {}", path.display()));
                }
                Err(err) => {
                    state.status_message = Some(format!("Save setup failed: {err}"));
                }
            }
            None
        }
        MenuAction::OpenUserMenu => {
            open_user_menu(state);
            None
        }
        MenuAction::CommandLine => {
            state.enter_command_line_mode();
            None
        }
        _ => None,
    }
}

pub fn open_user_menu(state: &mut AppState) {
    let panel_dir = state.active_panel().path.clone();
    let current_file = state
        .active_panel()
        .current_entry()
        .map(|e| e.name.clone())
        .unwrap_or_default();
    match user_menu::load_menu_with_warnings(&panel_dir, &current_file) {
        Ok(loaded) if loaded.entries.is_empty() => {
            let message = if loaded.warnings.is_empty() {
                "No matching menu entries found.".to_string()
            } else {
                format!(
                    "No matching menu entries found.\n{}",
                    loaded
                        .warnings
                        .iter()
                        .map(|warning| format!("Line {}: {}", warning.line, warning.message))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            state.mode = AppMode::Dialog(DialogKind::Error(message));
        }
        Ok(loaded) => {
            if let Some(warning) = loaded.warnings.first() {
                state.status_message = Some(format!(
                    "User menu warning: Line {}: {}",
                    warning.line, warning.message
                ));
            }
            if loaded.source == MenuSource::Local {
                state.status_message =
                    Some("Local .mc.menu loaded — commands require confirmation".to_string());
            }
            state.user_menu_source = loaded.source;
            state.user_menu_entries = loaded.entries;
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::UserMenu);
        }
        Err(msg) => {
            state.mode = AppMode::Dialog(DialogKind::Error(msg));
        }
    }
}
