use crossterm::event::{KeyCode, KeyModifiers};

const TREE_INITIAL_EXPAND_DEPTH: usize = 2;

use lc::app::user_menu::MenuSource;
use lc::app::{config, dir_tree, types::*, user_menu};
use lc::menu::{MenuAction, menu_action_at};
use lc::ops;

use super::directory_tree::set_tree_diagnostic_status;
use crate::app::panel_ops::{current_visible_height, rebuild_visible_entries, with_menu_panel};

pub fn execute_menu_action(state: &mut AppState) -> Option<(KeyCode, KeyModifiers, bool)> {
    let action = menu_action_at(state.menu_selected, state.menu_item_selected)?;
    match action {
        MenuAction::ToggleListingMode
        | MenuAction::CycleSortOrder
        | MenuAction::OpenFilter
        | MenuAction::ResetPanelFilter
        | MenuAction::TogglePermissions => execute_panel_config_action(state, action),
        MenuAction::DirectoryTree
        | MenuAction::FindFile
        | MenuAction::CompareDirs
        | MenuAction::History
        | MenuAction::DirectoryHotlist
        | MenuAction::CommandLine => execute_nav_action(state, action),
        MenuAction::Rename | MenuAction::Chmod => execute_dialog_action(state, action),
        MenuAction::ViewFile => Some((KeyCode::F(3), KeyModifiers::NONE, false)),
        MenuAction::EditFile => Some((KeyCode::F(4), KeyModifiers::NONE, false)),
        MenuAction::Copy => Some((KeyCode::F(5), KeyModifiers::NONE, false)),
        MenuAction::Move => Some((KeyCode::F(6), KeyModifiers::NONE, false)),
        MenuAction::MakeDirectory => Some((KeyCode::F(7), KeyModifiers::NONE, false)),
        MenuAction::Delete => Some((KeyCode::F(8), KeyModifiers::NONE, false)),
        MenuAction::RefreshPanel => Some((KeyCode::Char('r'), KeyModifiers::CONTROL, true)),
        MenuAction::ToggleHiddenFiles => Some((KeyCode::Char('h'), KeyModifiers::CONTROL, true)),
        MenuAction::SwapPanels => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = state.active_panel.toggle();
            None
        }
        MenuAction::SwitchPanels => {
            state.active_panel = state.active_panel.toggle();
            None
        }
        MenuAction::SaveCurrentPathToHotlist => {
            with_menu_panel(state, |state| {
                if !state
                    .hotlist()
                    .iter()
                    .any(|p| p == state.active_panel().path())
                {
                    state.hotlist_push(state.active_panel().path().to_path_buf());
                }
                state.status_message =
                    Some("Path added to hotlist (run Save Setup to persist)".to_string());
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
        _ => None,
    }
}

fn execute_panel_config_action(
    state: &mut AppState,
    action: MenuAction,
) -> Option<(KeyCode, KeyModifiers, bool)> {
    match action {
        MenuAction::ToggleListingMode => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                let new_mode = match panel.listing_mode() {
                    ListingMode::Long => ListingMode::Brief,
                    ListingMode::Brief => ListingMode::Long,
                };
                panel.set_listing_mode(new_mode);
                let label = match new_mode {
                    ListingMode::Long => "Long",
                    ListingMode::Brief => "Brief",
                };
                state.status_message = Some(format!("Layout changed to {label}"));
            });
            None
        }
        MenuAction::CycleSortOrder => {
            with_menu_panel(state, |state| {
                let p = state.active_panel_mut();
                p.set_sort_mode(ops::cycle_sort_mode(p.sort_mode()));
                rebuild_visible_entries(p, current_visible_height());
            });
            None
        }
        MenuAction::OpenFilter => {
            with_menu_panel(state, |state| {
                state.dialog_input.text = state
                    .active_panel()
                    .filter()
                    .unwrap_or_default()
                    .to_string();
                state.dialog_input.cursor_end();
                state.mode = AppMode::Dialog(DialogKind::Input {
                    prompt: "Filter:".to_string(),
                    action: InputAction::Filter,
                });
            });
            None
        }
        MenuAction::ResetPanelFilter => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.set_filter(None);
                rebuild_visible_entries(panel, current_visible_height());
                state.status_message = Some("Panel filter reset".to_string());
            });
            None
        }
        MenuAction::TogglePermissions => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                let show = !panel.show_permissions();
                panel.set_show_permissions(show);
                state.status_message =
                    Some(format!("Permissions: {}", if show { "ON" } else { "OFF" }));
            });
            None
        }
        _ => None,
    }
}

fn execute_nav_action(
    state: &mut AppState,
    action: MenuAction,
) -> Option<(KeyCode, KeyModifiers, bool)> {
    match action {
        MenuAction::DirectoryTree => {
            with_menu_panel(state, |state| {
                let path = state.active_panel().path().to_path_buf();
                let show_hidden = state.active_panel().show_hidden();
                let tree = dir_tree::build_tree_with_diagnostics(
                    &path,
                    TREE_INITIAL_EXPAND_DEPTH,
                    show_hidden,
                );
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
            state.mode = AppMode::Dialog(DialogKind::Input {
                prompt: "Find file:".to_string(),
                action: InputAction::FindFile,
            });
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
        MenuAction::CommandLine => {
            state.enter_command_line_mode();
            None
        }
        _ => None,
    }
}

fn execute_dialog_action(
    state: &mut AppState,
    action: MenuAction,
) -> Option<(KeyCode, KeyModifiers, bool)> {
    match action {
        MenuAction::Rename => {
            with_menu_panel(state, |state| {
                let entry_name = state.active_panel().current_entry().map(|e| e.name.clone());
                if let Some(name) = entry_name
                    && name != ".."
                {
                    state.dialog_input.text = name;
                    state.dialog_input.cursor_end();
                    state.mode = AppMode::Dialog(DialogKind::Input {
                        prompt: "Rename to:".to_string(),
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
                    state.dialog_input.text = format!("{:o}", permissions & 0o7777);
                    state.dialog_input.cursor_end();
                    state.mode = AppMode::Dialog(DialogKind::Input {
                        prompt: "Chmod (octal):".to_string(),
                        action: InputAction::Chmod,
                    });
                }
            });
            None
        }
        _ => None,
    }
}

pub fn open_user_menu(state: &mut AppState) {
    let panel_dir = state.active_panel().path().to_path_buf();
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
            let mut status_parts: String = String::new();
            for warning in &loaded.warnings {
                if !status_parts.is_empty() {
                    status_parts.push_str(" | ");
                }
                status_parts.push_str(&format!(
                    "User menu warning: Line {}: {}",
                    warning.line, warning.message
                ));
            }
            if loaded.source == MenuSource::Local {
                if !status_parts.is_empty() {
                    status_parts.push_str(" | ");
                }
                status_parts.push_str("Local .mc.menu loaded — commands require confirmation");
            }
            if !status_parts.is_empty() {
                state.status_message = Some(status_parts);
            }
            state.user_menu_source = loaded.source;
            state.user_menu_set(loaded.entries);
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::UserMenu);
        }
        Err(msg) => {
            state.mode = AppMode::Dialog(DialogKind::Error(msg));
        }
    }
}
