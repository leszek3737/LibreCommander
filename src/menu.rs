use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

pub const MENU_TITLES: [&str; 5] = ["Left", "File", "Command", "Options", "Right"];

pub const MENU_ITEMS: [&[&str]; 5] = [
    &[
        "Listing mode...",
        "Sort order...",
        "Filter...",
        "Encoding...",
    ],
    &[
        "User menu",
        "View file",
        "Edit file",
        "Copy",
        "Move",
        "Mkdir",
        "Delete",
        "Rename",
        "Chmod",
        "Quit",
    ],
    &[
        "Directory tree",
        "Find file",
        "Swap panels",
        "Switch panels",
        "Compare dirs",
        "History",
        "Directory hotlist",
    ],
    &[
        "Show hidden files",
        "Show permissions",
        "Add to hotlist",
        "Reset filter",
        "Save setup",
    ],
    &[
        "Listing mode...",
        "Sort order...",
        "Filter...",
        "Encoding...",
    ],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ToggleListingMode,
    CycleSortOrder,
    OpenFilter,
    RefreshPanel,
    OpenUserMenu,
    ViewFile,
    EditFile,
    Copy,
    Move,
    MakeDirectory,
    Delete,
    Rename,
    Chmod,
    Quit,
    DirectoryTree,
    FindFile,
    SwapPanels,
    SwitchPanels,
    CompareDirs,
    History,
    DirectoryHotlist,
    SaveCurrentPathToHotlist,
    ResetPanelFilter,
    ToggleHiddenFiles,
    TogglePermissions,
    SaveSetup,
}

const LEFT_RIGHT_MENU_ACTIONS: [MenuAction; 4] = [
    MenuAction::ToggleListingMode,
    MenuAction::CycleSortOrder,
    MenuAction::OpenFilter,
    MenuAction::RefreshPanel,
];

const FILE_MENU_ACTIONS: [MenuAction; 10] = [
    MenuAction::OpenUserMenu,
    MenuAction::ViewFile,
    MenuAction::EditFile,
    MenuAction::Copy,
    MenuAction::Move,
    MenuAction::MakeDirectory,
    MenuAction::Delete,
    MenuAction::Rename,
    MenuAction::Chmod,
    MenuAction::Quit,
];

const COMMAND_MENU_ACTIONS: [MenuAction; 7] = [
    MenuAction::DirectoryTree,
    MenuAction::FindFile,
    MenuAction::SwapPanels,
    MenuAction::SwitchPanels,
    MenuAction::CompareDirs,
    MenuAction::History,
    MenuAction::DirectoryHotlist,
];

const OPTIONS_MENU_ACTIONS: [MenuAction; 5] = [
    MenuAction::ToggleHiddenFiles,
    MenuAction::TogglePermissions,
    MenuAction::SaveCurrentPathToHotlist,
    MenuAction::ResetPanelFilter,
    MenuAction::SaveSetup,
];

const MENU_ACTIONS: [&[MenuAction]; 5] = [
    &LEFT_RIGHT_MENU_ACTIONS,
    &FILE_MENU_ACTIONS,
    &COMMAND_MENU_ACTIONS,
    &OPTIONS_MENU_ACTIONS,
    &LEFT_RIGHT_MENU_ACTIONS,
];

pub fn menu_action_at(menu: usize, item: usize) -> Option<MenuAction> {
    MENU_ACTIONS
        .get(menu)
        .and_then(|actions| actions.get(item))
        .copied()
}

pub fn menu_item_count(menu: usize) -> usize {
    MENU_ACTIONS.get(menu).map_or(0, |actions| actions.len())
}

pub fn menu_total_count() -> usize {
    MENU_ACTIONS.len()
}

pub fn menu_bar_text_width() -> u16 {
    let titles_width: u16 = MENU_TITLES
        .iter()
        .map(|title| menu_title_width(title))
        .try_fold(0u16, |acc, w| acc.checked_add(w))
        .unwrap_or(u16::MAX);
    let separator_width: u16 = MENU_TITLES
        .len()
        .saturating_sub(1)
        .try_into()
        .unwrap_or(u16::MAX);
    titles_width.saturating_add(separator_width)
}

pub fn menu_bar_start_x(width: u16) -> u16 {
    width.saturating_sub(menu_bar_text_width()) / 2
}

pub fn menu_title_width(title: &str) -> u16 {
    UnicodeWidthStr::width(title) as u16 + 2
}

pub fn menu_title_x(width: u16, index: usize) -> u16 {
    let mut x = menu_bar_start_x(width);
    for title in MENU_TITLES.iter().take(index) {
        x = x.saturating_add(menu_title_width(title) + 1);
    }
    x
}

pub fn menu_dropdown_x(menu_bar_area: Rect, selected_menu: usize, dropdown_width: u16) -> u16 {
    let dropdown_x = menu_bar_area.x + menu_title_x(menu_bar_area.width, selected_menu);
    let max_dropdown_x = menu_bar_area
        .x
        .saturating_add(menu_bar_area.width)
        .saturating_sub(dropdown_width);
    dropdown_x.min(max_dropdown_x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_action_at_maps_panel_menus() {
        assert_eq!(menu_action_at(0, 0), Some(MenuAction::ToggleListingMode));
        assert_eq!(menu_action_at(4, 2), Some(MenuAction::OpenFilter));
    }

    #[test]
    fn menu_action_at_maps_file_and_command_menus() {
        assert_eq!(menu_action_at(1, 7), Some(MenuAction::Rename));
        assert_eq!(menu_action_at(2, 4), Some(MenuAction::CompareDirs));
    }

    #[test]
    fn menu_action_at_rejects_out_of_range_items() {
        assert_eq!(menu_action_at(3, 5), None);
        assert_eq!(menu_action_at(5, 0), None);
    }

    #[test]
    fn menu_action_at_maps_options_menu() {
        assert_eq!(menu_action_at(3, 0), Some(MenuAction::ToggleHiddenFiles));
        assert_eq!(menu_action_at(3, 1), Some(MenuAction::TogglePermissions));
        assert_eq!(
            menu_action_at(3, 2),
            Some(MenuAction::SaveCurrentPathToHotlist)
        );
        assert_eq!(menu_action_at(3, 3), Some(MenuAction::ResetPanelFilter));
        assert_eq!(menu_action_at(3, 4), Some(MenuAction::SaveSetup));
    }
}
