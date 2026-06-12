use std::collections::VecDeque;
use std::path::PathBuf;

use super::modes::{AppMode, PendingAction};
use super::panel::{ActivePanel, PanelState};
use super::text_input::TextInput;
use crate::app::dir_tree::TreeEntry;
use crate::app::user_menu::{MenuEntry, MenuSource};
use crate::debug_log;
use crate::ui::theme::ColorPalette;

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,
    pub mode: AppMode,
    /// Mode saved before entering a temporary mode (e.g. CommandLine).
    /// Used by `restore_prev_mode()` to return to the previous mode.
    pub prev_mode: Option<AppMode>,
    pub should_quit: bool,
    pub dialog_input: TextInput,
    pub dialog_selection: usize,
    pub pending_action: Option<PendingAction>,
    pub command_line: TextInput,
    pub command_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub command_draft: String,
    pub search_query: String,
    pub search_cursor: usize,
    pub status_message: Option<String>,
    pub menu_selected: usize,
    pub menu_item_selected: usize,
    pub picker_selected: usize,
    pub user_menu_entries: Vec<MenuEntry>,
    pub user_menu_source: MenuSource,
    pub cached_hotlist_strings: Vec<String>,
    pub cached_user_menu_strings: Vec<String>,
    pub pending_menu_command: Option<String>,
    pub menu_restore_panel: Option<ActivePanel>,
    pub directory_hotlist: Vec<PathBuf>,
    pub tree_root: PathBuf,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub tree_scroll: usize,
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_position: Option<(u16, u16)>,
    pub drag_anchor_index: Option<usize>,
    pub theme_colors: ColorPalette,
    pub viewer_spinner_frame: u64,
    pub viewer_spinner_last_tick: Option<std::time::Instant>,
}

impl AppState {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|e| {
            debug_log!("current_dir failed: {e}, falling back to temp_dir");
            std::env::temp_dir()
        });

        Self {
            left_panel: PanelState::new(current_dir.clone()),
            right_panel: PanelState::new(current_dir.clone()),
            active_panel: ActivePanel::Left,
            mode: AppMode::Normal,
            prev_mode: None,
            should_quit: false,
            dialog_input: TextInput::default(),
            dialog_selection: 0,
            pending_action: None,
            command_line: TextInput::default(),
            command_history: VecDeque::new(),
            history_index: None,
            command_draft: String::new(),
            search_query: String::new(),
            search_cursor: 0,
            status_message: None,
            menu_selected: 0,
            menu_item_selected: 0,
            picker_selected: 0,
            user_menu_entries: Vec::new(),
            user_menu_source: MenuSource::Global,
            cached_hotlist_strings: vec![current_dir.display().to_string()],
            cached_user_menu_strings: Vec::new(),
            pending_menu_command: None,
            menu_restore_panel: None,
            directory_hotlist: vec![current_dir],
            tree_root: PathBuf::new(),
            tree_entries: Vec::new(),
            tree_selected: 0,
            tree_scroll: 0,
            last_click_time: None,
            last_click_position: None,
            drag_anchor_index: None,
            theme_colors: crate::ui::theme::DEFAULT_COLORS,
            viewer_spinner_frame: 0,
            viewer_spinner_last_tick: None,
        }
    }

    // TODO: active_panel/inactive_panel/panel and their _mut counterparts
    //       duplicate the same match arms. Consider a helper or macro to deduplicate.
    pub fn active_panel(&self) -> &PanelState {
        match self.active_panel {
            ActivePanel::Left => &self.left_panel,
            ActivePanel::Right => &self.right_panel,
        }
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        match self.active_panel {
            ActivePanel::Left => &mut self.left_panel,
            ActivePanel::Right => &mut self.right_panel,
        }
    }

    pub fn inactive_panel(&self) -> &PanelState {
        match self.active_panel {
            ActivePanel::Left => &self.right_panel,
            ActivePanel::Right => &self.left_panel,
        }
    }

    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        match self.active_panel {
            ActivePanel::Left => &mut self.right_panel,
            ActivePanel::Right => &mut self.left_panel,
        }
    }

    pub fn hotlist(&self) -> &[PathBuf] {
        &self.directory_hotlist
    }

    pub fn rebuild_hotlist_cache(&mut self) {
        let n = self.directory_hotlist.len();
        self.cached_hotlist_strings = Vec::with_capacity(n);
        self.cached_hotlist_strings.extend(
            self.directory_hotlist
                .iter()
                .map(|p| p.display().to_string()),
        );
    }

    pub fn rebuild_user_menu_cache(&mut self) {
        let n = self.user_menu_entries.len();
        self.cached_user_menu_strings = Vec::with_capacity(n);
        self.cached_user_menu_strings.extend(
            self.user_menu_entries
                .iter()
                .map(|e| format!("{}  {}", e.hotkey, e.title)),
        );
    }

    pub fn hotlist_push(&mut self, path: PathBuf) {
        if self.directory_hotlist.iter().any(|p| p == &path) {
            return;
        }
        self.directory_hotlist.push(path);
        self.rebuild_hotlist_cache();
    }

    pub fn hotlist_remove(&mut self, index: usize) {
        if index >= self.directory_hotlist.len() {
            return;
        }
        self.directory_hotlist.remove(index);
        self.rebuild_hotlist_cache();
    }

    /// Replace the entire directory hotlist and rebuild the string cache.
    /// Used when loading a persisted hotlist from config.
    pub fn hotlist_set(&mut self, hotlist: Vec<PathBuf>) {
        self.directory_hotlist = hotlist;
        self.rebuild_hotlist_cache();
    }

    /// Replace all user-menu entries and rebuild the display-string cache.
    /// Called after parsing a `.mc.menu` file or switching menu source.
    pub fn user_menu_set(&mut self, entries: Vec<MenuEntry>) {
        self.user_menu_entries = entries;
        self.rebuild_user_menu_cache();
    }

    /// Clears any stale `prev_mode` so it cannot leak into a later
    /// `restore_prev_mode()`. Command-line mode always returns to `Normal`
    /// directly, so there is no previous mode to preserve.
    pub fn enter_command_line_mode(&mut self) {
        self.command_line.clear();
        self.history_index = None;
        self.prev_mode = None;
        self.mode = AppMode::CommandLine;
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn reset_drag_state(&mut self) {
        self.drag_anchor_index = None;
    }

    // TODO: same deduplication opportunity as active_panel/inactive_panel above.
    pub fn panel_mut(&mut self, panel: ActivePanel) -> &mut PanelState {
        match panel {
            ActivePanel::Left => &mut self.left_panel,
            ActivePanel::Right => &mut self.right_panel,
        }
    }

    /// Resolve an explicit panel, falling back to the active panel when `None`.
    pub fn panel_or_active_mut(&mut self, panel: Option<ActivePanel>) -> &mut PanelState {
        match panel {
            Some(p) => self.panel_mut(p),
            None => self.active_panel_mut(),
        }
    }

    pub fn panel(&self, panel: ActivePanel) -> &PanelState {
        match panel {
            ActivePanel::Left => &self.left_panel,
            ActivePanel::Right => &self.right_panel,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn restore_prev_mode(&mut self) {
        self.mode = self.prev_mode.take().unwrap_or(AppMode::Normal);
    }
}
