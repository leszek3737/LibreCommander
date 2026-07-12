use std::fmt::Write as _;

use crossterm::event::{KeyCode, KeyModifiers};

use lc::app::user_menu::MenuSource;
use lc::app::{config, dir_tree, types::*, user_menu};
use lc::menu::{MenuAction, menu_action_at};
use lc::ops;

use super::directory_tree::set_tree_diagnostic_status;
use crate::app::panel_ops::{current_visible_height, rebuild_visible_entries, with_menu_panel};

const TREE_INITIAL_EXPAND_DEPTH: usize = 2;

/// Mask for the permission bits (`setuid`/`setgid`/`sticky` + rwx triplets)
/// extracted from a raw `st_mode` value when prefilling the chmod dialog.
const PERMISSION_MASK: u32 = 0o7777;

/// Status-bar labels for the two listing layouts.
const LISTING_MODE_LONG_LABEL: &str = "Long";
const LISTING_MODE_BRIEF_LABEL: &str = "Brief";

/// Status-bar labels for a toggled boolean flag (e.g. permission column).
const FLAG_ON_LABEL: &str = "ON";
const FLAG_OFF_LABEL: &str = "OFF";

/// Returns the listing layout that toggling `mode` switches to.
///
/// Models the binary `Long`/`Brief` switch as a total function over the type,
/// so the compiler enforces exhaustiveness if a third variant is ever added.
/// (Lives here rather than as a `ListingMode` method because that enum is owned
/// by `app::types::sorting`; promote to an inherent method when touching it.)
fn toggle_listing_mode(mode: ListingMode) -> ListingMode {
    match mode {
        ListingMode::Long => ListingMode::Brief,
        ListingMode::Brief => ListingMode::Long,
    }
}

/// Human-readable status-bar label for a listing layout.
fn listing_mode_label(mode: ListingMode) -> &'static str {
    match mode {
        ListingMode::Long => LISTING_MODE_LONG_LABEL,
        ListingMode::Brief => LISTING_MODE_BRIEF_LABEL,
    }
}

/// Enters a list-picker mode with a freshly reset selection cursor.
fn enter_picker(state: &mut AppState, kind: PickerKind) {
    state.ui.picker_selected = 0;
    state.mode = AppMode::ListPicker(kind);
}

/// Name of the panel's current entry, cloned if one is selected.
fn current_entry_name(panel: &PanelState) -> Option<String> {
    panel.current_entry().map(|e| e.name.clone())
}

pub fn execute_menu_action(state: &mut AppState) -> Option<(KeyCode, KeyModifiers, bool)> {
    let action = menu_action_at(state.ui.menu_selected, state.ui.menu_item_selected)?;
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
                state.ui.status_message =
                    Some("Path added to hotlist (run Save Setup to persist)".to_string());
            });
            None
        }
        MenuAction::Quit => {
            state.request_quit();
            None
        }
        MenuAction::SaveSetup => {
            match config::save_setup(state) {
                Ok(path) => {
                    state.ui.status_message = Some(format!("Setup saved to {}", path.display()));
                }
                Err(err) => {
                    state.ui.status_message = Some(format!("Save setup failed: {err}"));
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
                let new_mode = toggle_listing_mode(panel.listing_mode());
                panel.set_listing_mode(new_mode);
                let label = listing_mode_label(new_mode);
                state.ui.status_message = Some(format!("Layout changed to {label}"));
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
                state.input.dialog_input.set_text_at_end(
                    state
                        .active_panel()
                        .filter()
                        .unwrap_or_default()
                        .to_string(),
                );
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
                state.ui.status_message = Some("Panel filter reset".to_string());
            });
            None
        }
        MenuAction::TogglePermissions => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                let show = !panel.show_permissions();
                panel.set_show_permissions(show);
                let flag_label = if show { FLAG_ON_LABEL } else { FLAG_OFF_LABEL };
                state.ui.status_message = Some(format!("Permissions: {flag_label}"));
            });
            None
        }
        _ => None,
    }
}

/// Depth the directory-tree view initially expands to. Exposed for the
/// background tree builder in the main loop.
pub(crate) const TREE_EXPAND_DEPTH: usize = TREE_INITIAL_EXPAND_DEPTH;

/// Enter the directory-tree view from a completed background build. Called by
/// the main loop when the `bg_load` finishes.
pub(crate) fn apply_tree_build_result(
    state: &mut AppState,
    root: std::path::PathBuf,
    tree: dir_tree::TreeBuildResult,
) {
    state.tree.root = root;
    state.tree.entries = tree.entries;
    state.tree.selected = 0;
    state.tree.scroll = 0;
    state.mode = AppMode::DirectoryTree;
    set_tree_diagnostic_status(&mut state.ui.status_message, &tree.diagnostics);
}

fn execute_nav_action(
    state: &mut AppState,
    action: MenuAction,
) -> Option<(KeyCode, KeyModifiers, bool)> {
    match action {
        MenuAction::DirectoryTree => {
            // The recursive tree build can be slow on a wide/NFS tree, so it runs
            // off the event thread. Capture the target panel's path here (still
            // under `with_menu_panel`, which restores the original active panel
            // because the mode is not yet a dialog), then show a loading dialog;
            // the main loop builds the tree and enters DirectoryTree on completion.
            with_menu_panel(state, |state| {
                let path = state.active_panel().path().to_path_buf();
                let show_hidden = state.active_panel().show_hidden();
                state.ui.pending_tree_build = Some((path, show_hidden));
            });
            state.mode = AppMode::Dialog(DialogKind::progress(
                "Building tree...".to_string(),
                0.0,
                true,
            ));
            None
        }
        MenuAction::FindFile => {
            state.input.dialog_input.clear();
            state.mode = AppMode::Dialog(DialogKind::Input {
                prompt: "Find file:".to_string(),
                action: InputAction::FindFile,
            });
            None
        }
        MenuAction::CompareDirs => {
            enter_picker(state, PickerKind::CompareMode);
            None
        }
        MenuAction::History => {
            enter_picker(state, PickerKind::History);
            None
        }
        MenuAction::DirectoryHotlist => {
            enter_picker(state, PickerKind::Hotlist);
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
                let entry_name = current_entry_name(state.active_panel());
                if let Some(name) = entry_name
                    && name != ".."
                {
                    state.input.dialog_input.set_text_at_end(name);
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
                    state
                        .input
                        .dialog_input
                        .set_text_at_end(format!("{:o}", permissions & PERMISSION_MASK));
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
    let current_file = current_entry_name(state.active_panel()).unwrap_or_default();
    match user_menu::load_menu_with_warnings(&panel_dir, &current_file) {
        Ok(loaded) if loaded.entries.is_empty() => {
            let message = loaded.warnings.iter().fold(
                "No matching menu entries found.".to_string(),
                |mut acc, warning| {
                    // Infallible: writing into a String never errors.
                    let _ = write!(acc, "\nLine {}: {}", warning.line, warning.message);
                    acc
                },
            );
            state.mode = AppMode::Dialog(DialogKind::Error(message));
        }
        Ok(loaded) => {
            let mut parts: Vec<String> = loaded
                .warnings
                .iter()
                .map(|w| format!("User menu warning: Line {}: {}", w.line, w.message))
                .collect();
            if loaded.source == MenuSource::Local {
                parts.push("Local .mc.menu loaded — commands require confirmation".to_string());
            }
            if !parts.is_empty() {
                state.ui.status_message = Some(parts.join(" | "));
            }
            state.ui.user_menu_source = loaded.source;
            state.user_menu_set(loaded.entries);
            state.ui.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::UserMenu);
        }
        Err(msg) => {
            state.mode = AppMode::Dialog(DialogKind::Error(msg));
        }
    }
}
