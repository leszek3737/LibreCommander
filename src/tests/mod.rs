#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Integration tests for Libre Commander.
//!
//! These run as a separate test crate against the public `lc` API plus a small
//! amount of `cfg(test)` glue, driving real `AppState` through the input
//! dispatch and rendering paths. Each submodule owns one behavioural area:
//!
//! - `helpers` — shared harness (`dispatch_test_event`, `dispatch_key`,
//!   `dialog_key`, `TestEntry`, buffer assertions). Other modules build on it.
//! - `compare` — `compare_directories` op: summary counts and unique-entry marking.
//! - `dialogs` — dialog input handling and rendered-buffer assertions.
//! - `history` — command-line history: dedup, capacity cap, picker load/cancel.
//! - `keybinds` — keymap config parsing and key-to-action resolution.
//! - `keyevents` — raw `crossterm` event dispatch (resize/focus/mouse/key-kind).
//! - `menu` — F9 menu bar: navigation and menu-action dispatch.
//! - `misc` — environment/XDG path resolution and other cross-cutting cases.
//! - `overwrite` — copy/move overwrite confirmation flow.
//! - `pickers` — hotlist and history pickers: add/dedup/delete/wrap navigation.
//! - `search` — incremental search mode.
//! - `selection` — mark/unmark of entries and shift-select movement.
//! - `user_menu` — user-defined (config-driven) menu entries.
//! - `viewer` — internal file viewer and image-preview loaders.

// Background-load offload: archive-listing and tree-build result application.
mod background_load;
// `compare_directories` op: summary counts and unique-entry selection marking.
mod compare;
// Dialog input handling plus rendered-buffer assertions.
mod dialogs;
// Shared test harness: dispatch helpers, `TestEntry` builder, buffer utilities.
mod helpers;
// Command history: dedup, capacity cap, and history-picker load/cancel.
mod history;
// Keymap config parsing and key-to-action resolution.
mod keybinds;
// Raw crossterm event dispatch: resize, focus, mouse, and key-event kinds.
mod keyevents;
// F9 menu bar: navigation and menu-action dispatch.
mod menu;
// Cross-cutting cases: environment/XDG path resolution and other one-offs.
mod misc;
// Copy/move overwrite confirmation flow.
mod overwrite;
// Hotlist and history pickers: add, dedup, delete, and wrap-around navigation.
mod pickers;
// Incremental search mode.
mod search;
// Entry mark/unmark and shift-select cursor movement.
mod selection;
// User-defined (config-driven) menu entries.
mod user_menu;
// Internal file viewer and image-preview loader behaviour.
mod viewer;
