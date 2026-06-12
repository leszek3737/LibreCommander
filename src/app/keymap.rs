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

/// Static table covering all mc-compatible shortcuts.
pub static KEYBINDINGS: &[KeyBinding] = &[
    // ── Normal ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: "Normal",
        key: "F1",
        action: "Help",
        description: "Show help dialog",
    },
    KeyBinding {
        mode: "Normal",
        key: "F2",
        action: "UserMenu",
        description: "Open user menu",
    },
    KeyBinding {
        mode: "Normal",
        key: "F3",
        action: "View",
        description: "View file in internal viewer",
    },
    KeyBinding {
        mode: "Normal",
        key: "F4",
        action: "Edit",
        description: "Edit file in external editor",
    },
    KeyBinding {
        mode: "Normal",
        key: "F5",
        action: "Copy",
        description: "Copy selected files",
    },
    KeyBinding {
        mode: "Normal",
        key: "F6",
        action: "Move",
        description: "Move/rename selected files",
    },
    KeyBinding {
        mode: "Normal",
        key: "F7",
        action: "Mkdir/ArchiveExtract",
        description: "Create directory or extract archive",
    },
    KeyBinding {
        mode: "Normal",
        key: "F8",
        action: "Delete",
        description: "Delete selected files",
    },
    KeyBinding {
        mode: "Normal",
        key: "F9",
        action: "Menu",
        description: "Open left bottom menu",
    },
    KeyBinding {
        mode: "Normal",
        key: "F10",
        action: "Quit",
        description: "Quit the application",
    },
    KeyBinding {
        mode: "Normal",
        key: "F11",
        action: "Rename",
        description: "Rename file or directory",
    },
    KeyBinding {
        mode: "Normal",
        key: "Tab",
        action: "SwitchPanel",
        description: "Switch active panel",
    },
    KeyBinding {
        mode: "Normal",
        key: "Insert",
        action: "ToggleSelect",
        description: "Toggle selection and move down",
    },
    KeyBinding {
        mode: "Normal",
        key: "Up",
        action: "CursorUp",
        description: "Move cursor up",
    },
    KeyBinding {
        mode: "Normal",
        key: "Down",
        action: "CursorDown",
        description: "Move cursor down",
    },
    KeyBinding {
        mode: "Normal",
        key: "k",
        action: "CursorUp",
        description: "Move cursor up (vi)",
    },
    KeyBinding {
        mode: "Normal",
        key: "j",
        action: "CursorDown",
        description: "Move cursor down (vi)",
    },
    KeyBinding {
        mode: "Normal",
        key: "Home",
        action: "Top",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: "Normal",
        key: "End",
        action: "Bottom",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: "Normal",
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: "Normal",
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: "Normal",
        key: "Shift+Up",
        action: "ExtendSelection",
        description: "Extend selection upward",
    },
    KeyBinding {
        mode: "Normal",
        key: "Shift+Down",
        action: "ExtendSelection",
        description: "Extend selection downward",
    },
    KeyBinding {
        mode: "Normal",
        key: "Enter",
        action: "OpenDir",
        description: "Open directory",
    },
    KeyBinding {
        mode: "Normal",
        key: "Alt+Enter",
        action: "Properties",
        description: "Show file properties",
    },
    KeyBinding {
        mode: "Normal",
        key: "Ctrl+U",
        action: "SwapPanels",
        description: "Swap left and right panels",
    },
    KeyBinding {
        mode: "Normal",
        key: "Alt+1..9",
        action: "Hotlist",
        description: "Navigate to hotlist directory",
    },
    KeyBinding {
        mode: "Normal",
        key: "Alt+Backspace",
        action: "GoBack",
        description: "Previous directory in history",
    },
    KeyBinding {
        mode: "Normal",
        key: "Alt+C",
        action: "QuickCd",
        description: "Quick change directory",
    },
    KeyBinding {
        mode: "Normal",
        key: "Ctrl+S",
        action: "Search",
        description: "Start incremental search/filter",
    },
    KeyBinding {
        mode: "Normal",
        key: "Ctrl+H",
        action: "ToggleHidden",
        description: "Toggle hidden files visibility",
    },
    KeyBinding {
        mode: "Normal",
        key: "Ctrl+R",
        action: "Refresh",
        description: "Refresh panel contents",
    },
    KeyBinding {
        mode: "Normal",
        key: "Ctrl+O",
        action: "ExternalView",
        description: "Toggle external panel view",
    },
    KeyBinding {
        mode: "Normal",
        key: "Alt+X",
        action: "CommandLine",
        description: "Open command line",
    },
    KeyBinding {
        mode: "Normal",
        key: "F12",
        action: "ArchiveMenu",
        description: "Archive operations menu",
    },
    // ── Viewer ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: "Viewing",
        key: "Esc",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: "Viewing",
        key: "F3",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: "Viewing",
        key: "F10",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: "Viewing",
        key: "q",
        action: "Close",
        description: "Close viewer",
    },
    KeyBinding {
        mode: "Viewing",
        key: "Up",
        action: "ScrollUp",
        description: "Scroll up one line",
    },
    KeyBinding {
        mode: "Viewing",
        key: "Down",
        action: "ScrollDown",
        description: "Scroll down one line",
    },
    KeyBinding {
        mode: "Viewing",
        key: "k",
        action: "ScrollUp",
        description: "Scroll up one line (vi)",
    },
    KeyBinding {
        mode: "Viewing",
        key: "j",
        action: "ScrollDown",
        description: "Scroll down one line (vi)",
    },
    KeyBinding {
        mode: "Viewing",
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: "Viewing",
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: "Viewing",
        key: "Home",
        action: "Top",
        description: "Go to beginning of file",
    },
    KeyBinding {
        mode: "Viewing",
        key: "End",
        action: "Bottom",
        description: "Go to end of file",
    },
    KeyBinding {
        mode: "Viewing",
        key: "Left",
        action: "ScrollLeft",
        description: "Scroll left",
    },
    KeyBinding {
        mode: "Viewing",
        key: "Right",
        action: "ScrollRight",
        description: "Scroll right",
    },
    KeyBinding {
        mode: "Viewing",
        key: "l",
        action: "ToggleLineNum",
        description: "Toggle line numbers",
    },
    KeyBinding {
        mode: "Viewing",
        key: "w",
        action: "ToggleWrap",
        description: "Toggle line wrapping",
    },
    KeyBinding {
        mode: "Viewing",
        key: "h",
        action: "ToggleHex",
        description: "Toggle hex mode",
    },
    KeyBinding {
        mode: "Viewing",
        key: "n",
        action: "NextMatch",
        description: "Next search match",
    },
    KeyBinding {
        mode: "Viewing",
        key: "N",
        action: "PrevMatch",
        description: "Previous search match",
    },
    KeyBinding {
        mode: "Viewing",
        key: "/",
        action: "Search",
        description: "Open search dialog",
    },
    // ── CommandLine ──────────────────────────────────────────────────────────
    KeyBinding {
        mode: "CommandLine",
        key: "Esc",
        action: "Cancel",
        description: "Cancel command line",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Enter",
        action: "Execute",
        description: "Execute shell command",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Up",
        action: "HistoryPrev",
        description: "Previous command in history",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Down",
        action: "HistoryNext",
        description: "Next command in history",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Ctrl+A",
        action: "CursorHome",
        description: "Move cursor to beginning of line",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Ctrl+E",
        action: "CursorEnd",
        description: "Move cursor to end of line",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Ctrl+U",
        action: "ClearToStart",
        description: "Clear line before cursor",
    },
    KeyBinding {
        mode: "CommandLine",
        key: "Ctrl+W",
        action: "DeleteWordBack",
        description: "Delete word before cursor",
    },
    // ── Search ───────────────────────────────────────────────────────────────
    KeyBinding {
        mode: "Search",
        key: "Esc",
        action: "Cancel",
        description: "Cancel search and restore",
    },
    KeyBinding {
        mode: "Search",
        key: "Enter",
        action: "Accept",
        description: "Accept current search filter",
    },
    KeyBinding {
        mode: "Search",
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    // ── Menu ─────────────────────────────────────────────────────────────────
    KeyBinding {
        mode: "Menu",
        key: "Esc",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: "Menu",
        key: "F9",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: "Menu",
        key: "F10",
        action: "Close",
        description: "Close menu",
    },
    KeyBinding {
        mode: "Menu",
        key: "Left",
        action: "PrevCategory",
        description: "Previous menu category",
    },
    KeyBinding {
        mode: "Menu",
        key: "Right",
        action: "NextCategory",
        description: "Next menu category",
    },
    KeyBinding {
        mode: "Menu",
        key: "Up",
        action: "PrevItem",
        description: "Select previous menu item",
    },
    KeyBinding {
        mode: "Menu",
        key: "Down",
        action: "NextItem",
        description: "Select next menu item",
    },
    KeyBinding {
        mode: "Menu",
        key: "Enter",
        action: "Execute",
        description: "Execute selected menu action",
    },
    // ── Dialog/Confirm ───────────────────────────────────────────────────────
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "y",
        action: "Confirm",
        description: "Confirm action",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "Y",
        action: "Confirm",
        description: "Confirm action",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "n",
        action: "Cancel",
        description: "Cancel action",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "N",
        action: "Cancel",
        description: "Cancel action",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "Enter",
        action: "Confirm",
        description: "Confirm or cancel based on selection",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "Esc",
        action: "Cancel",
        description: "Cancel dialog",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "Left",
        action: "ToggleButton",
        description: "Toggle Yes/No button",
    },
    KeyBinding {
        mode: "Dialog/Confirm",
        key: "Right",
        action: "ToggleButton",
        description: "Toggle Yes/No button",
    },
    // ── Dialog/Input ─────────────────────────────────────────────────────────
    KeyBinding {
        mode: "Dialog/Input",
        key: "Enter",
        action: "Submit",
        description: "Submit input",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Esc",
        action: "Cancel",
        description: "Cancel input",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Backspace",
        action: "DeleteChar",
        description: "Delete character before cursor",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Delete",
        action: "DeleteCharFwd",
        description: "Delete character at cursor",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Left",
        action: "CursorLeft",
        description: "Move cursor left",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Right",
        action: "CursorRight",
        description: "Move cursor right",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "Home",
        action: "CursorHome",
        description: "Move cursor to start",
    },
    KeyBinding {
        mode: "Dialog/Input",
        key: "End",
        action: "CursorEnd",
        description: "Move cursor to end",
    },
    // ── ListPicker ───────────────────────────────────────────────────────────
    KeyBinding {
        mode: "ListPicker",
        key: "Esc",
        action: "Cancel",
        description: "Close picker",
    },
    KeyBinding {
        mode: "ListPicker",
        key: "Up",
        action: "PrevItem",
        description: "Select previous item",
    },
    KeyBinding {
        mode: "ListPicker",
        key: "Down",
        action: "NextItem",
        description: "Select next item",
    },
    KeyBinding {
        mode: "ListPicker",
        key: "Enter",
        action: "Select",
        description: "Confirm selection",
    },
    KeyBinding {
        mode: "ListPicker",
        key: "Home",
        action: "Top",
        description: "Go to first item",
    },
    KeyBinding {
        mode: "ListPicker",
        key: "End",
        action: "Bottom",
        description: "Go to last item",
    },
    // ── DirectoryTree ────────────────────────────────────────────────────────
    KeyBinding {
        mode: "DirectoryTree",
        key: "Esc",
        action: "Close",
        description: "Close directory tree",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "Up",
        action: "Prev",
        description: "Select previous entry",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "Down",
        action: "Next",
        description: "Select next entry",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "k",
        action: "Prev",
        description: "Select previous entry (vi)",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "j",
        action: "Next",
        description: "Select next entry (vi)",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "Home",
        action: "Top",
        description: "Go to first entry",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "End",
        action: "Bottom",
        description: "Go to last entry",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "PageUp",
        action: "PageUp",
        description: "Page up",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "PageDown",
        action: "PageDown",
        description: "Page down",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "Enter",
        action: "ToggleExpand",
        description: "Toggle expand dir or open file",
    },
    KeyBinding {
        mode: "DirectoryTree",
        key: "c",
        action: "CdToDir",
        description: "Change to selected directory",
    },
];

/// Returns `(mode, key)` pairs for any key that appears more than once
/// within the same mode. Empty vec means no duplicates.
#[cfg(test)]
fn find_duplicate_keys() -> Vec<(&'static str, &'static str)> {
    let mut seen: HashSet<(&str, &str)> = HashSet::new();
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
        // Rough capacity estimate: ~40 bytes per binding (mode header overhead,
        // indent, padded key, description). Conservative to minimize reallocations.
        let mut msg = String::with_capacity(KEYBINDINGS.len() * 40);
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

    // TODO: this test is O(n²) due to HashSet collection and iteration;
    // consider a single-pass approach or accept the overhead for now.
    #[test]
    fn all_modes_are_non_empty() {
        let modes: HashSet<&str> = KEYBINDINGS.iter().map(|b| b.mode).collect();
        assert!(!modes.is_empty(), "No modes present in KEYBINDINGS table");
    }

    #[test]
    fn every_binding_has_non_empty_fields() {
        for (i, b) in KEYBINDINGS.iter().enumerate() {
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
    fn build_help_message_contains_all_modes() {
        let msg = build_help_message();
        let mut modes: Vec<&str> = KEYBINDINGS.iter().map(|b| b.mode).collect();
        modes.sort();
        modes.dedup();
        for mode in &modes {
            assert!(msg.contains(mode), "Help message missing mode '{}'", mode);
        }
    }

    #[test]
    fn build_help_message_is_non_empty() {
        let msg = build_help_message();
        assert!(!msg.is_empty(), "Help message must not be empty");
    }
}
