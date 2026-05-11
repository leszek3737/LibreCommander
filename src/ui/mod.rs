pub mod dialogs;
pub mod dir_tree;
pub mod menu;
pub mod panels;
pub mod theme;
pub mod viewer;

/// Number of rows reserved for UI elements outside the main panels.
/// Accounts for: top menu bar (1), status bar (1), command line (1),
/// function key bar (1), and borders (2). Used to calculate available
/// height for file panels and viewer.
pub const LAYOUT_OVERHEAD_ROWS: u16 = 6;

/// Number of rows reserved for directory tree panel overhead.
/// Accounts for: top border (1), bottom border (1), and help bar (1).
/// The remaining space is used for the actual tree listing.
pub const DIR_TREE_OVERHEAD_ROWS: u16 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_overhead_rows_matches_fixed_constraints() {
        assert_eq!(
            LAYOUT_OVERHEAD_ROWS, 6,
            "LAYOUT_OVERHEAD_ROWS = menu bar (1) + status bar (1) + command line (1) + function bar (1) + borders (2) = 6"
        );
    }
}
