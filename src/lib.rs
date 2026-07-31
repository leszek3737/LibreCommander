//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.

// The `lc` binary and integration tests consume this library as an external
// crate, reaching items through their module path (e.g. `lc::app::types`).
pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;
