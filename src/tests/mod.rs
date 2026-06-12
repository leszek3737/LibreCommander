#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Integration tests for Libre Commander.
//!
//! Covers `AppState` harness, key bindings, search mode, dialogs, file viewer,
//! file operations (copy/move/delete/overwrite), pickers, menus, and selection.

// File comparison tests
mod compare;
// Dialog input and rendering tests
mod dialogs;
// Shared test helpers and AppState harness
mod helpers;
// Directory history navigation tests
mod history;
// Key binding configuration tests
mod keybinds;
// Raw key event dispatch tests
mod keyevents;
// Menu bar tests
mod menu;
// Miscellaneous integration tests
mod misc;
// File overwrite confirmation tests
mod overwrite;
// File/folder picker tests
mod pickers;
// Search mode tests
mod search;
// File selection (mark/unmark) tests
mod selection;
// User-defined menu tests
mod user_menu;
// Internal viewer tests
mod viewer;
