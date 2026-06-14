/// Static descriptive keymap table for lc (Midnight Commander compatible).
///
/// Each binding records the app mode, key combo, semantic action, and
/// human-readable description. The `find_duplicate_keys()` helper
/// validates that no key appears twice within the same mode.
use std::fmt::Write;
use std::sync::OnceLock;

#[cfg(test)]
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub mode: &'static str,
    pub key: &'static str,
    pub action: &'static str,
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
        action: "Help",
        description: "Show help dialog",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F2",
        action: "UserMenu",
        description: "Open user menu",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F3",
        action: "View",
        description: "View file in internal viewer",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F4",
        action: "Edit",
        description: "Edit file in external editor",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F5",
        action: "Copy",
        description: "Copy selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F6",
        action: "Move",
        description: "Move/rename selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F7",
        action: "Mkdir/ArchiveExtract",
        description: "Create directory or extract archive",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F8",
        action: "Delete",
        description: "Delete selected files",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F9",
        action: "Menu",
        description: "Open left bottom menu",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F10",
        action: "Quit",
        description: "Quit the application",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F11",
        action: "Rename",
        description: "Rename file or directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Tab",
        action: "SwitchPanel",
        description: "Switch active panel",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Insert",
        action: "ToggleSelect",
        description: "Toggle selection and move down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Up",
        action: "CursorUp",
        description: "Move cursor up",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Down",
        action: "CursorDown",
        description: "Move cursor down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "k",
        action: "CursorUp",
        description: "Move cursor up (vi)",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "j",
        action: "CursorDown",
        description: "Move cursor down (vi)",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Home",
        action: "Top",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "End",
        action: "Bottom",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Shift+Up",
        action: "ExtendSelection",
        description: "Extend selection upward",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Shift+Down",
        action: "ExtendSelection",
        description: "Extend selection downward",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Enter",
        action: "OpenDir",
        description: "Open directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+Enter",
        action: "Properties",
        description: "Show file properties",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+U",
        action: "SwapPanels",
        description: "Swap left and right panels",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+1..9",
        action: "Hotlist",
        description: "Navigate to hotlist directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+Backspace",
        action: "GoBack",
        description: "Previous directory in history",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+C",
        action: "QuickCd",
        description: "Quick change directory",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+S",
        action: "Search",
        description: "Start incremental search/filter",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+H",
        action: "ToggleHidden",
        description: "Toggle hidden files visibility",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+R",
        action: "Refresh",
        description: "Refresh panel contents",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Ctrl+O",
        action: "ExternalView",
        description: "Toggle external panel view",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "Alt+X",
        action: "CommandLine",
        description: "Open command line",
    },
    KeyBinding {
        mode: MODE_NORMAL,
        key: "F12",
        action: "ArchiveMenu",
        description: "Archive operations menu",
    },
    // ── Viewer ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Esc",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "F3",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "F10",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "q",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Up",
        action: "ScrollUp",
        description: "Scroll up one line",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Down",
        action: "ScrollDown",
        description: "Scroll down one line",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "k",
        action: "ScrollUp",
        description: "Scroll up one line (vi)",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "j",
        action: "ScrollDown",
        description: "Scroll down one line (vi)",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Home",
        action: "Top",
        description: "Go to beginning of file",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "End",
        action: "Bottom",
        description: "Go to end of file",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Left",
        action: "ScrollLeft",
        description: "Scroll left",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "Right",
        action: "ScrollRight",
        description: "Scroll right",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "l",
        action: "ToggleLineNum",
        description: "Toggle line numbers",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "w",
        action: "ToggleWrap",
        description: "Toggle line wrapping",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "h",
        action: "ToggleHex",
        description: "Toggle hex mode",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "n",
        action: "NextMatch",
        description: "Next search match",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "N",
        action: "PrevMatch",
        description: "Previous search match",
    },
    KeyBinding {
        mode: MODE_VIEWING,
        key: "/",
        action: "Search",
        description: "Open search dialog",
    },
    // ── CommandLine ──────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Esc",
        action: "Cancel",
        description: "Cancel command line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Enter",
        action: "Execute",
        description: "Execute shell command",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Up",
        action: "HistoryPrev",
        description: "Previous command in history",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Down",
        action: "HistoryNext",
        description: "Next command in history",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+A",
        action: "CursorHome",
        description: "Move cursor to beginning of line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+E",
        action: "CursorEnd",
        description: "Move cursor to end of line",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+U",
        action: "ClearToStart",
        description: "Clear line before cursor",
    },
    KeyBinding {
        mode: MODE_COMMAND_LINE,
        key: "Ctrl+W",
        action: "DeleteWordBack",
        description: "Delete word before cursor",
    },
    // ── Search ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Esc",
        action: "Cancel",
        description: "Cancel search and restore",
    },
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Enter",
        action: "Accept",
        description: "Accept current search filter",
    },
    KeyBinding {
        mode: MODE_SEARCH,
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    // ── Menu ─────────────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_MENU,
        key: "Esc",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "F9",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "F10",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Left",
        action: "PrevCategory",
        description: "Previous menu category",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Right",
        action: "NextCategory",
        description: "Next menu category",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Up",
        action: "PrevItem",
        description: "Select previous menu item",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Down",
        action: "NextItem",
        description: "Select next menu item",
    },
    KeyBinding {
        mode: MODE_MENU,
        key: "Enter",
        action: "Execute",
        description: "Execute selected menu action",
    },
    // ── Dialog/Confirm ───────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "y",
        action: "Confirm",
        description: "Confirm action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Y",
        action: "Confirm",
        description: "Confirm action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "n",
        action: "Cancel",
        description: "Cancel action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "N",
        action: "Cancel",
        description: "Cancel action",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Enter",
        action: "Confirm",
        description: "Confirm or cancel based on selection",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Esc",
        action: "Cancel",
        description: "Cancel dialog",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Left",
        action: "ToggleButton",
        description: "Toggle Yes/No button",
    },
    KeyBinding {
        mode: MODE_DIALOG_CONFIRM,
        key: "Right",
        action: "ToggleButton",
        description: "Toggle Yes/No button",
    },
    // ── Dialog/Input ─────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Enter",
        action: "Submit",
        description: "Submit input",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Esc",
        action: "Cancel",
        description: "Cancel input",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Delete",
        action: "DeleteCharFwd",
        description: "Delete character at cursor",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Left",
        action: "CursorLeft",
        description: "Move cursor left",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Right",
        action: "CursorRight",
        description: "Move cursor right",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "Home",
        action: "CursorHome",
        description: "Move cursor to start",
    },
    KeyBinding {
        mode: MODE_DIALOG_INPUT,
        key: "End",
        action: "CursorEnd",
        description: "Move cursor to end",
    },
    // ── ListPicker ───────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Esc",
        action: "Cancel",
        description: "Close picker",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Up",
        action: "PrevItem",
        description: "Select previous item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Down",
        action: "NextItem",
        description: "Select next item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Enter",
        action: "Select",
        description: "Confirm selection",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "Home",
        action: "Top",
        description: "Go to first item",
    },
    KeyBinding {
        mode: MODE_LIST_PICKER,
        key: "End",
        action: "Bottom",
        description: "Go to last item",
    },
    // ── DirectoryTree ────────────────────────────────────────────────────────
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Esc",
        action: "Close",
        description: "Close directory tree",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Up",
        action: "Prev",
        description: "Select previous entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Down",
        action: "Next",
        description: "Select next entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "k",
        action: "Prev",
        description: "Select previous entry (vi)",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "j",
        action: "Next",
        description: "Select next entry (vi)",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Home",
        action: "Top",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "End",
        action: "Bottom",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "Enter",
        action: "ToggleExpand",
        description: "Toggle expand dir or open file",
    },
    KeyBinding {
        mode: MODE_DIRECTORY_TREE,
        key: "c",
        action: "CdToDir",
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
                !b.action.is_empty(),
                "Binding #{i} ({}, {}) has empty action",
                b.mode,
                b.key
            );
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
                | DialogKind::CopyMove(_)
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
