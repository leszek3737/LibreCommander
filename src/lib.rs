//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.

pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;

pub use app::config::Settings;
pub use app::keymap::KeyBinding;
pub use app::types::{
    ActivePanel, AppMode, AppState, ConfirmDetails, DialogKind, FileCategory, FileEntry,
    ListingMode, PanelState, PendingAction, SortMode,
};
pub use menu::MenuAction;
