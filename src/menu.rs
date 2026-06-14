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

// The "Left" and "Right" top-level menus are identical apart from their title:
// both drive the panel of the same name via `with_menu_panel`. Share a single
// definition of the item labels and their actions so the two entries cannot
// drift out of sync.
const PANEL_MENU_ITEMS: &[&str] = &[
    "Listing mode...",
    "Sort order...",
    "Filter...",
    "Refresh panel",
];

const PANEL_MENU_ACTIONS: &[MenuAction] = &[
    MenuAction::ToggleListingMode,
    MenuAction::CycleSortOrder,
    MenuAction::OpenFilter,
    MenuAction::RefreshPanel,
];

pub const MENUS: [MenuEntry; 5] = [
    MenuEntry {
        title: "Left",
        items: PANEL_MENU_ITEMS,
        actions: PANEL_MENU_ACTIONS,
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
        items: PANEL_MENU_ITEMS,
        actions: PANEL_MENU_ACTIONS,
    },
];

// Compile-time guard: every menu must have exactly as many actions as labels,
// so `menu_action_at` can index `actions` with a validated `items` position.
// NOTE: this only checks the *lengths* line up — it cannot verify that each
// label is semantically paired with the right action; that mapping is the
// author's responsibility and is covered by the unit tests below.
const _: () = const {
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

/// Display width of one rendered title cell: the title plus its padding.
///
/// `const fn`, so `MENU_BAR_TEXT_WIDTH` below and the per-index prefix sums can
/// be folded at compile time instead of re-walking `MENUS` on every frame.
/// Menu titles are ASCII (enforced by `_TITLES_ARE_ASCII`), so byte length
/// equals the unicode display width here.
const fn const_title_width(title: &str) -> u16 {
    (title.len() as u16).saturating_add(MENU_TITLE_PADDING as u16)
}

/// Total rendered width of the whole menu bar (all titles + the separators
/// between them). Precomputed once at compile time from `MENUS`; this is the
/// single source of truth shared by `menu_bar_start_x` and `menu_title_x`.
const MENU_BAR_TEXT_WIDTH: u16 = {
    let mut total: u16 = 0;
    let mut i = 0;
    while i < MENUS.len() {
        if i > 0 {
            total = total.saturating_add(MENU_TITLE_SEPARATOR as u16);
        }
        total = total.saturating_add(const_title_width(MENUS[i].title));
        i += 1;
    }
    total
};

// Guard the ASCII assumption baked into `const_title_width`: if a non-ASCII
// title is ever added, byte length would diverge from the unicode display
// width and the precomputed offsets would be wrong, so fail the build instead.
const _TITLES_ARE_ASCII: () = const {
    let mut i = 0;
    while i < MENUS.len() {
        assert!(MENUS[i].title.is_ascii(), "menu titles must be ASCII");
        i += 1;
    }
};

pub fn menu_bar_text_width() -> u16 {
    MENU_BAR_TEXT_WIDTH
}

pub fn menu_bar_start_x(width: u16) -> u16 {
    width.saturating_sub(MENU_BAR_TEXT_WIDTH) / 2
}

pub fn menu_title_width(title: &str) -> u16 {
    let title_w: u16 = match u16::try_from(UnicodeWidthStr::width(title)) {
        Ok(w) => w,
        Err(_) => {
            // A title wider than u16::MAX columns cannot really happen (titles
            // are short ASCII labels), but if it ever did, silently clamping to
            // 0 used to collapse the title to zero width and mis-position every
            // following menu with no trace. Saturate to the visible maximum and
            // leave a breadcrumb instead.
            crate::debug_log!("menu_title_width: title width overflowed u16, clamping: {title:?}");
            u16::MAX
        }
    };
    title_w.saturating_add(MENU_TITLE_PADDING as u16)
}

/// Horizontal offset of menu `index`'s title relative to `menu_bar_start_x`:
/// the summed width of every preceding title plus the separators between them.
fn menu_prefix_width(index: usize) -> u16 {
    let mut prefix: u16 = 0;
    for entry in MENUS.iter().take(index) {
        prefix = prefix
            .saturating_add(menu_title_width(entry.title))
            .saturating_add(MENU_TITLE_SEPARATOR as u16);
    }
    prefix
}

pub fn menu_title_x(width: u16, index: usize) -> u16 {
    menu_bar_start_x(width).saturating_add(menu_prefix_width(index))
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
