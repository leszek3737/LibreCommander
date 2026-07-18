use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use super::modes::{AppMode, PendingAction};
use super::panel::{ActivePanel, PanelState};
use super::text_input::TextInput;
use crate::app::dir_tree::TreeEntry;
use crate::app::user_menu::{MenuEntry, MenuSource};
use crate::debug_log;
use crate::ui::theme::ColorPalette;

/// Text-entry and editing state shared across the dialog, command-line and
/// search surfaces. Grouped out of [`AppState`] to shrink the former god
/// object and keep editing concerns together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputState {
    pub dialog_input: TextInput,
    pub dialog_selection: usize,
    pub command_line: TextInput,
    pub command_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub command_draft: String,
    pub search_query: String,
    pub search_cursor: usize,
}

/// Transient presentation state: status line, menus, pickers, the directory
/// hotlist, user-menu data, the deferred action awaiting confirmation, and the
/// viewer spinner animation.
///
/// `Default` is implemented by hand because [`MenuSource`] has no `Default`.
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub status_message: Option<String>,
    pub menu_selected: usize,
    pub menu_item_selected: usize,
    // NOTE: `menu_selected` / `menu_item_selected` / `picker_selected` are bare
    // `usize` indices (primitive obsession). A `MenuIndex` / `PickerIndex`
    // newtype was considered but is not worth the churn: these indices flow
    // through ~130 call sites and many slice operations that expect `usize`.
    // Follow-up: introduce index newtypes once the call sites settle.
    pub picker_selected: usize,
    pub user_menu_entries: Vec<MenuEntry>,
    pub user_menu_source: MenuSource,
    pub cached_hotlist_strings: Vec<String>,
    pub cached_user_menu_strings: Vec<String>,
    pub cached_history_strings: Vec<String>,
    pub pending_menu_command: Option<String>,
    /// Hotlist index awaiting a "Remove from hotlist?" confirmation, if any.
    pub pending_hotlist_delete: Option<usize>,
    /// Archive `(source, dest)` awaiting a background listing, if any. The main
    /// loop picks this up, spawns the read off the event thread, and shows a
    /// loading dialog until it completes (see `bg_load`).
    pub pending_archive_list: Option<(PathBuf, String)>,
    /// Directory-tree `(root, show_hidden)` awaiting a background build, if any.
    pub pending_tree_build: Option<(PathBuf, bool)>,
    pub menu_restore_panel: Option<ActivePanel>,
    pub directory_hotlist: Vec<PathBuf>,
    pub pending_action: Option<PendingAction>,
    pub viewer_spinner_frame: u64,
    pub viewer_spinner_last_tick: Option<Instant>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            status_message: None,
            menu_selected: 0,
            menu_item_selected: 0,
            picker_selected: 0,
            user_menu_entries: Vec::new(),
            user_menu_source: MenuSource::Global,
            cached_hotlist_strings: Vec::new(),
            cached_user_menu_strings: Vec::new(),
            cached_history_strings: Vec::new(),
            pending_menu_command: None,
            pending_hotlist_delete: None,
            pending_archive_list: None,
            pending_tree_build: None,
            menu_restore_panel: None,
            directory_hotlist: Vec::new(),
            pending_action: None,
            viewer_spinner_frame: 0,
            viewer_spinner_last_tick: None,
        }
    }
}

/// Directory-tree browser view state (the `DirectoryTree` mode).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeState {
    pub root: PathBuf,
    pub entries: Vec<TreeEntry>,
    pub selected: usize,
    pub scroll: usize,
}

/// Pointer-interaction state used for double-click detection and drag
/// selection in the panels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InteractionState {
    /// Last single click as `(timestamp, (col, row))`. Reset to `None` once a
    /// double-click fires or the mouse button is released.
    ///
    /// Merged from the former separate `last_click_time` / `last_click_position`
    /// fields so the timestamp and position can never drift out of sync.
    pub last_click: Option<(Instant, (u16, u16))>,
    pub drag_anchor_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    // --- Core ---
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,
    pub mode: AppMode,
    /// Mode saved before entering a temporary mode (e.g. CommandLine).
    /// Used by `restore_prev_mode()` to return to the previous mode.
    pub prev_mode: Option<AppMode>,
    pub should_quit: bool,
    pub theme_colors: ColorPalette,

    // --- Aggregates ---
    pub input: InputState,
    pub ui: UiState,
    pub tree: TreeState,
    pub interaction: InteractionState,
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
            theme_colors: crate::ui::theme::DEFAULT_COLORS,
            input: InputState::default(),
            ui: UiState {
                cached_hotlist_strings: vec![current_dir.display().to_string()],
                directory_hotlist: vec![current_dir],
                ..UiState::default()
            },
            tree: TreeState::default(),
            interaction: InteractionState::default(),
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panel(self.active_panel)
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        let which = self.active_panel;
        self.panel_mut(which)
    }

    pub fn inactive_panel(&self) -> &PanelState {
        self.panel(self.active_panel.toggle())
    }

    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        let which = self.active_panel.toggle();
        self.panel_mut(which)
    }

    /// Switch keyboard focus to an explicit panel.
    pub fn set_active_panel(&mut self, panel: ActivePanel) {
        self.active_panel = panel;
    }

    /// Toggle keyboard focus between the left and right panel.
    pub fn toggle_active_panel(&mut self) {
        self.active_panel = self.active_panel.toggle();
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Request application shutdown. One-way by design: there is no public way
    /// to clear the flag, so a quit request can never be silently undone.
    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn hotlist(&self) -> &[PathBuf] {
        &self.ui.directory_hotlist
    }

    /// Rebuild a display-string cache from a source slice. Shared by the
    /// hotlist and user-menu caches, which differ only in how each entry is
    /// rendered to a `String`.
    fn rebuild_string_cache<T>(
        source: &[T],
        cache: &mut Vec<String>,
        render: impl Fn(&T) -> String,
    ) {
        cache.clear();
        cache.reserve(source.len());
        cache.extend(source.iter().map(render));
    }

    pub fn rebuild_hotlist_cache(&mut self) {
        Self::rebuild_string_cache(
            &self.ui.directory_hotlist,
            &mut self.ui.cached_hotlist_strings,
            |p| p.display().to_string(),
        );
    }

    pub fn rebuild_user_menu_cache(&mut self) {
        Self::rebuild_string_cache(
            &self.ui.user_menu_entries,
            &mut self.ui.cached_user_menu_strings,
            |e| format!("{}  {}", e.hotkey, e.title),
        );
    }

    pub fn rebuild_history_cache(&mut self) {
        self.ui.cached_history_strings.clear();
        self.ui
            .cached_history_strings
            .reserve(self.input.command_history.len());
        self.ui
            .cached_history_strings
            .extend(self.input.command_history.iter().rev().cloned());
    }

    pub fn hotlist_push(&mut self, path: PathBuf) {
        if self.ui.directory_hotlist.iter().any(|p| p == &path) {
            return;
        }
        self.ui.directory_hotlist.push(path);
        self.rebuild_hotlist_cache();
    }

    pub fn hotlist_remove(&mut self, index: usize) {
        if index >= self.ui.directory_hotlist.len() {
            return;
        }
        self.ui.directory_hotlist.remove(index);
        self.rebuild_hotlist_cache();
    }

    /// Replace the entire directory hotlist and rebuild the string cache.
    /// Used when loading a persisted hotlist from config.
    pub fn hotlist_set(&mut self, hotlist: Vec<PathBuf>) {
        self.ui.directory_hotlist = hotlist;
        self.rebuild_hotlist_cache();
    }

    /// Replace all user-menu entries and rebuild the display-string cache.
    /// Called after parsing a `.mc.menu` file or switching menu source.
    pub fn user_menu_set(&mut self, entries: Vec<MenuEntry>) {
        self.ui.user_menu_entries = entries;
        self.rebuild_user_menu_cache();
    }

    /// Clears any stale `prev_mode` so it cannot leak into a later
    /// `restore_prev_mode()`. Command-line mode always returns to `Normal`
    /// directly, so there is no previous mode to preserve.
    pub fn enter_command_line_mode(&mut self) {
        self.input.command_line.clear();
        self.input.history_index = None;
        self.prev_mode = None;
        self.mode = AppMode::CommandLine;
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.ui.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.ui.status_message = None;
    }

    pub fn reset_drag_state(&mut self) {
        self.interaction.drag_anchor_index = None;
    }

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
