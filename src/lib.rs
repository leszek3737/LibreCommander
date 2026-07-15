//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.

// Public API surface.
//
// The `lc` binary (`main.rs`, `render*`, `input/*`, integration tests under
// `src/tests/`) consumes this library as an external crate and always reaches
// items through their module path (e.g. `lc::app::types::AppState`,
// `lc::ops::compare::compare_entries`). The crate-root re-exports that used to
// live here were never referenced via `lc::<Symbol>` by any consumer, so they
// were a redundant, inconsistent second surface. The intended public API is the
// set of top-level modules below; navigate into them for concrete items.
pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;
