//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.

pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;

pub use app::types::{
    ActivePanel, AppMode, AppState, CompareMode, DialogKind, FileCategory, FileEntry, FileSize,
    ListingMode, PanelState, PendingAction, SortMode, SortOptions, ViewMode,
};
pub use menu::MenuAction;
pub use ops::search::{FileSearch, SearchOutcome, TruncationReason};
