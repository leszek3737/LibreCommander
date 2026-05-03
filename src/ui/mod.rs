pub mod dialogs;
pub mod dir_tree;
pub mod menu;
pub mod panels;
pub mod theme;
pub mod viewer;

/// Number of rows reserved for UI elements outside the main panels.
/// Accounts for: top menu bar (1), title bars (2), function key bar (1),
/// and command line/status area (2). Used to calculate available height
/// for file panels and viewer.
pub const LAYOUT_OVERHEAD_ROWS: u16 = 6;

/// Number of rows reserved for directory tree panel overhead.
/// Accounts for: panel title/border (1) and column headers (2).
/// The remaining space is used for the actual tree listing.
pub const DIR_TREE_OVERHEAD_ROWS: u16 = 3;
