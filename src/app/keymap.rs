/// Static descriptive keymap table for lc (Midnight Commander compatible).
///
/// Each binding records the app mode, key combo, and human-readable description.
/// The `find_duplicate_keys()` helper validates that no key appears twice within
/// the same mode.
use std::fmt::Write;
use std::sync::OnceLock;

#[cfg(test)]
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub mode: &'static str,
    pub key: &'static str,
    pub description: &'static str,
}

/// Canonical mode-name labels shared by the keymap table, the help output,
/// and (eventually) other modules. Defining them here keeps each string in
/// exactly one place instead of duplicating literals across call sites.
pub const MODE_NORMAL: &str = "Normal";
pub const MODE_VIEWING: &str = "Viewing";
pub const MODE_COMMAND_LINE: &str = "CommandLine";
pub const MODE_SEARCH: &str = "Search";
pub const MODE_MENU: &str = "Menu";
pub const MODE_LIST_PICKER: &str = "ListPicker";
pub const MODE_DIRECTORY_TREE: &str = "DirectoryTree";
pub const MODE_DIALOG_CONFIRM: &str = "Dialog/Confirm";
pub const MODE_DIALOG_INPUT: &str = "Dialog/Input";

/// Static table covering all mc-compatible shortcuts.
pub static KEYBINDINGS: &[KeyBinding] = &[
    // ── Normal ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F1",
        description: "Show help dialog",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F2",
        description: "Open user menu",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F3",
        description: "View file in internal viewer",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F4",
        description: "Edit file in external editor",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F5",
        description: "Copy selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F6",
        description: "Move/rename selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F7",
        description: "Create directory or extract archive",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F8",
        description: "Delete selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F9",
        description: "Open left bottom menu",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F10",
        description: "Quit the application",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F11",
        description: "Rename file or directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Tab",
        description: "Switch active panel",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Insert",
        description: "Toggle selection and move down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Up",
        description: "Move cursor up",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Down",
        description: "Move cursor down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "k",
        description: "Move cursor up (vi)",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "j",
        description: "Move cursor down (vi)",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Home",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "End",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Shift+Up",
        description: "Extend selection upward",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Shift+Down",
        description: "Extend selection downward",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Enter",
        description: "Open directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+Enter",
        description: "Show file properties",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+U",
        description: "Swap left and right panels",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+1..9",
        description: "Navigate to hotlist directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+Backspace",
        description: "Previous directory in history",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+C",
        description: "Quick change directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+S",
        description: "Start incremental search/filter",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+H",
        description: "Toggle hidden files visibility",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+R",
        description: "Refresh panel contents",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+O",
        description: "Toggle external panel view",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+X",
        description: "Open command line",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F12",
        description: "Archive operations menu",
    },
    // ── Viewer ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Esc",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "F3",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "F10",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "q",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Up",
        description: "Scroll up one line",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Down",
        description: "Scroll down one line",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "k",
        description: "Scroll up one line (vi)",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "j",
        description: "Scroll down one line (vi)",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Home",
        description: "Go to beginning of file",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "End",
        description: "Go to end of file",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Left",
        description: "Scroll left",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Right",
        description: "Scroll right",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "l",
        description: "Toggle line numbers",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "w",
        description: "Toggle line wrapping",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "h",
        description: "Toggle hex mode",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "n",
        description: "Next search match",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "N",
        description: "Previous search match",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "/",
        description: "Open search dialog",
    },
    // ── CommandLine ──────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Esc",
        description: "Cancel command line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Enter",
        description: "Execute shell command",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Backspace",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Up",
        description: "Previous command in history",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Down",
        description: "Next command in history",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+A",
        description: "Move cursor to beginning of line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+E",
        description: "Move cursor to end of line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+U",
        description: "Clear line before cursor",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+W",
        description: "Delete word before cursor",
    },
    // ── Search ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Esc",
        description: "Cancel search and restore",
    },
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Enter",
        description: "Accept current search filter",
    },
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Backspace",
        description: "Delete character before cursor",
    },
    // ── Menu ─────────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_MENU,
        key: "Esc",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "F9",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "F10",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Left",
        description: "Previous menu category",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Right",
        description: "Next menu category",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Up",
        description: "Select previous menu item",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Down",
        description: "Select next menu item",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Enter",
        description: "Execute selected menu action",
    },
    // ── Dialog/Confirm ───────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "y",
        description: "Confirm action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Y",
        description: "Confirm action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "n",
        description: "Cancel action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "N",
        description: "Cancel action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Enter",
        description: "Confirm or cancel based on selection",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Esc",
        description: "Cancel dialog",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Left",
        description: "Toggle Yes/No button",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Right",
        description: "Toggle Yes/No button",
    },
    // ── Dialog/Input ─────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Enter",
        description: "Submit input",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Esc",
        description: "Cancel input",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Backspace",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Delete",
        description: "Delete character at cursor",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Left",
        description: "Move cursor left",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Right",
        description: "Move cursor right",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Home",
        description: "Move cursor to start",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "End",
        description: "Move cursor to end",
    },
    // ── ListPicker ───────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Esc",
        description: "Close picker",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Up",
        description: "Select previous item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Down",
        description: "Select next item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Enter",
        description: "Confirm selection",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Home",
        description: "Go to first item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "End",
        description: "Go to last item",
    },
    // ── DirectoryTree ────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Esc",
        description: "Close directory tree",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Up",
        description: "Select previous entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Down",
        description: "Select next entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "k",
        description: "Select previous entry (vi)",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "j",
        description: "Select next entry (vi)",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Home",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "End",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Enter",
        description: "Toggle expand dir or open file",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "c",
        description: "Change to selected directory",
    },
];

/// Returns `(mode, key)` pairs for any key that appears more than once
/// within the same mode. Empty vec means no duplicates.
#[cfg(test)]
fn find_duplicate_keys() -> Vec<(&'static str, &'static str)> {
    let mut seen: HashSet<(&str, &str)> = HashSet::with_capacity(KEYBINDINGS.len());
    // Duplicates are expected to be empty in a valid table, so leave this
    // unsized rather than pre-allocating memory that is almost never used.
    let mut duplicates = Vec::new();

    for binding in KEYBINDINGS {
        let pair = (binding.mode, binding.key);
        if !seen.insert(pair) {
            duplicates.push(pair);
        }
    }

    duplicates
}

/// Build a help message string grouped by mode (for F1 display).
pub fn build_help_message() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        // Rough capacity estimate: ~45 bytes per binding. Each line is a
        // 2-space indent, a key padded to 16 columns, a space, the description,
        // and a newline (~20 bytes of fixed overhead plus the description),
        // amortizing the per-mode header. Slightly over-estimates to minimize
        // reallocations.
        let mut msg = String::with_capacity(KEYBINDINGS.len() * 45);
        let mut current_mode = "";
        for b in KEYBINDINGS {
            if b.mode != current_mode {
                if !msg.is_empty() {
                    msg.push('\n');
                }
                msg.push_str(b.mode);
                msg.push_str(":\n");
                current_mode = b.mode;
            }
            let _ = writeln!(msg, "  {:<16} {}", b.key, b.description);
        }
        msg
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{AppMode, DialogKind, InputAction, PickerKind};

    #[test]
    fn no_duplicate_keys_per_mode() {
        let duplicates = find_duplicate_keys();
        let msg: Vec<String> = duplicates
            .iter()
            .map(|(m, k)| format!("  mode={m} key={k}"))
            .collect();
        assert!(
            duplicates.is_empty(),
            "Duplicate key bindings found:\n{}",
            msg.join("\n")
        );
    }

    #[test]
    fn every_binding_has_non_empty_fields() {
        for (i, b) in KEYBINDINGS.iter().enumerate() {
            assert!(!b.mode.is_empty(), "Binding #{i} has empty mode");
            assert!(!b.key.is_empty(), "Binding #{i} ({}) has empty key", b.mode);
            assert!(
                !b.description.is_empty(),
                "Binding #{i} ({}, {}) has empty description",
                b.mode,
                b.key
            );
        }
    }

    #[test]
    fn all_app_modes_have_keymap_or_documented_fallback() {
        let msg = build_help_message();
        let keymap_modes: HashSet<&str> = KEYBINDINGS.iter().map(|binding| binding.mode).collect();

        for keymap_mode in documented_keymap_modes() {
            assert!(
                keymap_modes.contains(keymap_mode),
                "Keymap missing documented mode {keymap_mode:?}"
            );
            assert!(
                msg.contains(keymap_mode),
                "Help message missing documented mode {keymap_mode:?}"
            );
        }

        for mode in representative_app_modes() {
            let keymap_mode = keymap_coverage_for_mode(&mode);
            assert!(
                keymap_modes.contains(keymap_mode),
                "{mode:?} must have keymap coverage or documented fallback"
            );
        }
    }

    #[test]
    fn build_help_message_is_valid() {
        let msg = build_help_message();
        assert!(!msg.is_empty(), "Help message must not be empty");
        for (i, line) in msg.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            assert!(
                trimmed.len() > 2,
                "Line {i} is suspiciously short: {:?}",
                line
            );
        }
    }

    fn documented_keymap_modes() -> &'static [&'static str] {
        &[
            MODE_NORMAL,
            MODE_VIEWING,
            MODE_COMMAND_LINE,
            MODE_SEARCH,
            MODE_MENU,
            MODE_LIST_PICKER,
            MODE_DIRECTORY_TREE,
            MODE_DIALOG_CONFIRM,
            MODE_DIALOG_INPUT,
        ]
    }

    fn representative_app_modes() -> Vec<AppMode> {
        vec![
            AppMode::Normal,
            AppMode::Viewing,
            AppMode::CommandLine,
            AppMode::Search,
            AppMode::Menu,
            AppMode::ListPicker(PickerKind::History),
            AppMode::DirectoryTree,
            AppMode::Dialog(DialogKind::Input {
                prompt: "Input".to_string(),
                action: InputAction::CreateDirectory,
            }),
        ]
    }

    /// Returns the keymap mode whose bindings drive `mode`, either directly or
    /// via a documented fallback. The match stays exhaustive (no wildcard) so
    /// adding a `DialogKind`/`PickerKind` variant forces a review here.
    fn keymap_coverage_for_mode(mode: &AppMode) -> &'static str {
        match mode {
            AppMode::Normal => MODE_NORMAL,
            AppMode::Viewing => MODE_VIEWING,
            AppMode::CommandLine => MODE_COMMAND_LINE,
            AppMode::Search => MODE_SEARCH,
            AppMode::Menu => MODE_MENU,
            AppMode::ListPicker(
                PickerKind::History
                | PickerKind::Hotlist
                | PickerKind::CompareMode
                | PickerKind::UserMenu
                | PickerKind::ArchiveMenu,
            ) => MODE_LIST_PICKER,
            AppMode::DirectoryTree => MODE_DIRECTORY_TREE,
            AppMode::Dialog(
                DialogKind::Confirm(_)
                | DialogKind::Error(_)
                | DialogKind::Progress { .. }
                | DialogKind::Properties(_)
                | DialogKind::OverwriteConfirm(_),
            ) => MODE_DIALOG_CONFIRM,
            AppMode::Dialog(
                DialogKind::Input { .. }
                | DialogKind::ArchiveExtract(_)
                | DialogKind::ArchiveCreate(_),
            ) => MODE_DIALOG_INPUT,
            AppMode::Dialog(DialogKind::Help { .. }) => MODE_VIEWING,
        }
    }
}
