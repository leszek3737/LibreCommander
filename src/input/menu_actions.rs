use crossterm::event::KeyCode;

use lc::app::{config, dir_tree, types::*, user_menu};
use lc::menu::{MenuAction, menu_action_at};
use lc::ops::sorting;

use super::super::{refresh_active, set_tree_diagnostic_status, with_menu_panel};

pub fn execute_menu_action(state: &mut AppState) -> Option<KeyCode> {
    match menu_action_at(state.menu_selected, state.menu_item_selected) {
        Some(MenuAction::ToggleListingMode) => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.listing_mode = match panel.listing_mode {
                    ListingMode::Long => ListingMode::Brief,
                    ListingMode::Brief => ListingMode::Long,
                };
            });
            None
        }
        Some(MenuAction::CycleSortOrder) => {
            with_menu_panel(state, |state| {
                let p = state.active_panel_mut();
                p.sort_mode = sorting::cycle_sort_mode(p.sort_mode);
                refresh_active(state);
            });
            None
        }
        Some(MenuAction::OpenFilter) => {
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
        Some(MenuAction::RefreshPanel) => {
            with_menu_panel(state, refresh_active);
            None
        }
        Some(MenuAction::OpenUserMenu) => {
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
                                .map(|warning| format!(
                                    "Line {}: {}",
                                    warning.line, warning.message
                                ))
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
                    state.user_menu_entries = loaded.entries;
                    state.picker_selected = 0;
                    state.mode = AppMode::ListPicker(PickerKind::UserMenu);
                }
                Err(msg) => {
                    state.mode = AppMode::Dialog(DialogKind::Error(msg));
                }
            }
            None
        }
        Some(MenuAction::ViewFile) => Some(KeyCode::F(3)),
        Some(MenuAction::EditFile) => Some(KeyCode::F(4)),
        Some(MenuAction::Copy) => Some(KeyCode::F(5)),
        Some(MenuAction::Move) => Some(KeyCode::F(6)),
        Some(MenuAction::MakeDirectory) => Some(KeyCode::F(7)),
        Some(MenuAction::Delete) => Some(KeyCode::F(8)),
        Some(MenuAction::Rename) => {
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
            None
        }
        Some(MenuAction::Chmod) => {
            let entry_info = state
                .active_panel()
                .current_entry()
                .map(|e| (e.name.clone(), e.permissions));
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
            None
        }
        Some(MenuAction::Quit) => {
            state.should_quit = true;
            None
        }
        Some(MenuAction::DirectoryTree) => {
            let path = state.active_panel().path.clone();
            let show_hidden = state.active_panel().show_hidden;
            let tree = dir_tree::build_tree_with_diagnostics(&path, 2, show_hidden);
            state.tree_root = path;
            state.tree_entries = tree.entries;
            state.tree_selected = 0;
            state.tree_scroll = 0;
            state.mode = AppMode::DirectoryTree;
            set_tree_diagnostic_status(&mut state.status_message, &tree.diagnostics);
            None
        }
        Some(MenuAction::FindFile) => {
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            state.mode = AppMode::Dialog(DialogKind::Input {
                prompt: "Find file:".to_string(),
                default_text: String::new(),
                action: InputAction::FindFile,
            });
            None
        }
        Some(MenuAction::SwapPanels) => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        Some(MenuAction::SwitchPanels) => {
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        Some(MenuAction::CompareDirs) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::CompareMode);
            None
        }
        Some(MenuAction::History) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::History);
            None
        }
        Some(MenuAction::DirectoryHotlist) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::Hotlist);
            None
        }
        Some(MenuAction::SaveCurrentPathToHotlist) => {
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
                Some("Configuration saved current path into hotlist".to_string());
            None
        }
        Some(MenuAction::ToggleLayoutMode) => {
            let panel = state.active_panel_mut();
            panel.listing_mode = match panel.listing_mode {
                ListingMode::Long => ListingMode::Brief,
                ListingMode::Brief => ListingMode::Long,
            };
            state.status_message = Some(format!("Layout changed to {:?}", panel.listing_mode));
            None
        }
        Some(MenuAction::TogglePanelHidden) => {
            let panel = state.active_panel_mut();
            panel.show_hidden = !panel.show_hidden;
            refresh_active(state);
            state.status_message = Some(format!(
                "Panel options: hidden={}",
                state.active_panel().show_hidden
            ));
            None
        }
        Some(MenuAction::ResetPanelFilter) => {
            let panel = state.active_panel_mut();
            panel.filter = None;
            refresh_active(state);
            state.status_message = Some("Appearance reset active panel filter".to_string());
            None
        }
        Some(MenuAction::ToggleHiddenFiles) => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            refresh_active(state);
            None
        }
        Some(MenuAction::SaveSetup) => {
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
        None => None,
    }
}
