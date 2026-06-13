//! LibreCommander (`lc`) — a TUI file manager inspired by Midnight Commander.
//!
//! Built with Ratatui + Crossterm. Single binary, no runtime dependencies.

pub mod app;
pub mod fs;
pub mod menu;
pub mod ops;
pub mod ui;

pub use app::types::{
    ActivePanel, AppMode, AppState, CompareMode, DialogKind, Direction, FileCategory, FileEntry,
    FileSize, ListingMode, PanelState, PendingAction, SortField, SortMode, SortOptions, ViewMode,
};
pub use menu::MenuAction;
pub use ops::compare::{CompareReport, apply_compare_to_panels, compare_entries};
#[cfg(unix)]
pub use ops::file_ops::chmod;
pub use ops::file_ops::{create_directory, rename_entry};
pub use ops::search::{FileSearch, SearchError, SearchErrorKind, SearchOutcome, TruncationReason};
pub use ops::sorting::{cycle_sort_mode, sort_entries};
