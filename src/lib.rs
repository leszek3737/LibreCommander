//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.
//!
//! Modules remain `pub` because the binary crate (`main.rs`) accesses them
//! directly via `use lc::{app, fs, menu, ops, ui}` — they are separate crates
//! so `pub(crate)` would break all 172+ module-path references.

pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;
