use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

const MENU_TITLE_PADDING: usize = 2;
const MENU_TITLE_SEPARATOR: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    CommandLine,
}

#[derive(Debug, Clone, Copy)]
pub struct MenuEntry {
    pub title: &'static str,
    pub items: &'static [&'static str],
    pub actions: &'static [MenuAction],
}

pub const MENUS: [MenuEntry; 5] = [
    MenuEntry {
        title: "Left",
        items: &[
            "Listing mode...",
            "Sort order...",
            "Filter...",
            "Refresh panel",
        ],
        actions: &[
            MenuAction::ToggleListingMode,
            MenuAction::CycleSortOrder,
            MenuAction::OpenFilter,
            MenuAction::RefreshPanel,
        ],
    },
    MenuEntry {
        title: "File",
        items: &[
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
        actions: &[
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
        ],
    },
    MenuEntry {
        title: "Command",
        items: &[
            "Directory tree",
            "Find file",
            "Swap panels",
            "Switch panels",
            "Compare dirs",
            "History",
            "Directory hotlist",
            "Command line",
        ],
        actions: &[
            MenuAction::DirectoryTree,
            MenuAction::FindFile,
            MenuAction::SwapPanels,
            MenuAction::SwitchPanels,
            MenuAction::CompareDirs,
            MenuAction::History,
            MenuAction::DirectoryHotlist,
            MenuAction::CommandLine,
        ],
    },
    MenuEntry {
        title: "Options",
        items: &[
            "Show hidden files",
            "Show permissions",
            "Add to hotlist",
            "Reset filter",
            "Save setup",
        ],
        actions: &[
            MenuAction::ToggleHiddenFiles,
            MenuAction::TogglePermissions,
            MenuAction::SaveCurrentPathToHotlist,
            MenuAction::ResetPanelFilter,
            MenuAction::SaveSetup,
        ],
    },
    MenuEntry {
        title: "Right",
        items: &[
            "Listing mode...",
            "Sort order...",
            "Filter...",
            "Refresh panel",
        ],
        actions: &[
            MenuAction::ToggleListingMode,
            MenuAction::CycleSortOrder,
            MenuAction::OpenFilter,
            MenuAction::RefreshPanel,
        ],
    },
];

pub const MENU_TITLES: [&str; 5] = [
    MENUS[0].title,
    MENUS[1].title,
    MENUS[2].title,
    MENUS[3].title,
    MENUS[4].title,
];

pub const MENU_ITEMS: [&[&str]; 5] = [
    MENUS[0].items,
    MENUS[1].items,
    MENUS[2].items,
    MENUS[3].items,
    MENUS[4].items,
];

pub const MENU_ACTIONS: [&[MenuAction]; 5] = [
    MENUS[0].actions,
    MENUS[1].actions,
    MENUS[2].actions,
    MENUS[3].actions,
    MENUS[4].actions,
];

const _: () = {
    let mut i = 0;
    while i < MENUS.len() {
        assert!(MENUS[i].items.len() == MENUS[i].actions.len());
        i += 1;
    }
};

pub fn menu_action_at(menu: usize, item: usize) -> Option<MenuAction> {
    MENUS
        .get(menu)
        .and_then(|entry| entry.actions.get(item))
        .copied()
}

pub fn menu_item_count(menu: usize) -> usize {
    MENUS.get(menu).map_or(0, |entry| entry.items.len())
}

pub fn menu_total_count() -> usize {
    MENUS.len()
}

pub fn menu_bar_text_width() -> u16 {
    let titles_width: u16 = MENUS
        .iter()
        .map(|entry| menu_title_width(entry.title))
        .try_fold(0u16, |acc, w| acc.checked_add(w))
        .unwrap_or(u16::MAX);
    let separator_width: u16 = MENUS.len().saturating_sub(1).try_into().unwrap_or(u16::MAX);
    titles_width.saturating_add(separator_width)
}

pub fn menu_bar_start_x(width: u16) -> u16 {
    width.saturating_sub(menu_bar_text_width()) / 2
}

pub fn menu_title_width(title: &str) -> u16 {
    UnicodeWidthStr::width(title) as u16 + MENU_TITLE_PADDING as u16
}

pub fn menu_title_x(width: u16, index: usize) -> u16 {
    let mut x = menu_bar_start_x(width);
    for entry in MENUS.iter().take(index) {
        x = x.saturating_add(menu_title_width(entry.title) + MENU_TITLE_SEPARATOR as u16);
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
        assert_eq!(MENUS[2].items[7], "Command line");
        assert_eq!(menu_action_at(2, 7), Some(MenuAction::CommandLine));
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
