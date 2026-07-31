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
    fn layout_overhead_rows_sums_components() {
        let menu_bar = 1;
        let status_bar = 1;
        let command_line = 1;
        let function_bar = 1;
        let borders = 2;
        assert_eq!(
            LAYOUT_OVERHEAD_ROWS,
            menu_bar + status_bar + command_line + function_bar + borders,
        );
    }

    #[test]
    fn dir_tree_overhead_rows_sums_components() {
        let top_border = 1;
        let bottom_border = 1;
        let help_bar = 1;
        assert_eq!(
            DIR_TREE_OVERHEAD_ROWS,
            top_border + bottom_border + help_bar,
        );
    }
}
