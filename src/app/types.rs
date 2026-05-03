use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::dir_tree::TreeEntry;
use super::user_menu::MenuEntry;

// ============================================================================
// 1b. FileSize newtype
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSize(pub u64);

impl std::fmt::Display for FileSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.0;
        let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;
        while size_f >= 1024.0 && unit_idx < units.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }
        if unit_idx == 0 {
            write!(f, "{} {}", size, units[unit_idx])
        } else {
            write!(f, "{:.1} {}", size_f, units[unit_idx])
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Dir,
    Archive,
    Image,
    Video,
    Audio,
    Document,
    Code,
    Config,
    Executable,
    Symlink,
    Hidden,
    Other,
}

// ============================================================================
// 1c. Free functions for formatting
// ============================================================================

pub fn format_permissions(mode: u32) -> String {
    FileEntry::display_permissions_raw(mode)
}

pub fn format_size(size: u64) -> String {
    FileSize(size).to_string()
}

// ============================================================================
// 1. FileEntry struct definition
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_executable: bool,
    pub size: u64,
    pub modified: SystemTime,
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub selected: bool,
    pub is_hidden: bool,
    pub mime_type: Option<String>,
}

// ============================================================================
// 2. SortMode enum definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    NameAsc,
    NameDesc,
    ExtensionAsc,
    ExtensionDesc,
    SizeAsc,
    SizeDesc,
    ModTimeAsc,
    ModTimeDesc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingMode {
    #[default]
    Long,
    Brief,
}

// ============================================================================
// 3. PanelState struct definition
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub sort_mode: SortMode,
    pub listing_mode: ListingMode,
    pub show_hidden: bool,
    pub filter: Option<String>,
    pub selected_count: usize,
    pub selected_size: u64,
    pub selection_anchor: Option<usize>,
    pub last_error: Option<String>,
    pub history: Vec<PathBuf>,
    pub unfiltered_entries: Vec<FileEntry>,
}

// ============================================================================
// 4. ActivePanel enum definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Left,
    Right,
}

// ============================================================================
// 5. DialogKind enum definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    CreateDirectory,
    Rename,
    Chmod,
    Filter,
    QuickCd,
    FindFile,
    ViewerSearch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogKind {
    Confirm(String),
    Input {
        prompt: String,
        default_text: String,
        action: InputAction,
    },
    Error(String),
    Help(String),          // Help message, exits on any key
    Progress(String, f32), // (message, progress 0.0-1.0)
    CopyMove {
        source: Vec<PathBuf>,
        dest: PathBuf,
        is_move: bool,
    },
    Properties {
        name: String,
        size: u64,
        mtime: SystemTime,
        permissions: u32,
        owner: String,
        group: String,
        is_dir: bool,
        is_symlink: bool,
    },
}

// ============================================================================
// 6. PickerKind enum definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    History,
    Hotlist,
    CompareMode,
    UserMenu,
}

// ============================================================================
// 6b. CompareMode enum definition
// ============================================================================

/// Controls how `compare_directories` determines whether two entries match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareMode {
    /// Match by name only (original behaviour).
    #[default]
    Quick,
    /// Match by name + size (directories: name only).
    Size,
    /// Match by name + size + mtime (directories: name only).
    Thorough,
}

// ============================================================================
// 7. AppMode enum definition
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Viewing,
    CommandLine,
    Dialog(DialogKind),
    Search,
    Menu,
    ListPicker(PickerKind),
    DirectoryTree,
}

// ============================================================================
// 7. AppState substates
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct PanelsState {
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommandState {
    pub command_line: String,
    pub command_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub command_draft: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DialogState {
    pub dialog_input: String,
    pub dialog_cursor_pos: usize,
    pub dialog_selection: usize,
    pub pending_action: Option<PendingAction>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuState {
    pub directory_hotlist: Vec<PathBuf>,
    pub menu_selected: usize,
    pub menu_item_selected: usize,
    pub user_menu_entries: Vec<MenuEntry>,
    pub menu_restore_panel: Option<ActivePanel>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PickerState {
    pub picker_selected: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DirectoryTreeState {
    pub tree_root: PathBuf,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub tree_scroll: usize,
}

impl PanelsState {
    pub fn new(current_dir: PathBuf) -> Self {
        Self {
            left_panel: PanelState::new(current_dir.clone()),
            right_panel: PanelState::new(current_dir),
            active_panel: ActivePanel::Left,
        }
    }
}

impl MenuState {
    pub fn new(initial_hotlist_path: PathBuf) -> Self {
        Self {
            directory_hotlist: vec![initial_hotlist_path],
            ..Self::default()
        }
    }
}

// ============================================================================
// 7b. AppState struct definition
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,
    pub mode: AppMode,
    pub command_line: String,
    pub search_query: String,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub dialog_input: String,
    pub dialog_cursor_pos: usize,
    pub command_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub command_draft: String,
    pub directory_hotlist: Vec<PathBuf>,
    pub menu_selected: usize,
    pub menu_item_selected: usize,
    pub picker_selected: usize,
    pub user_menu_entries: Vec<MenuEntry>,
    pub tree_root: PathBuf,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub tree_scroll: usize,
    pub prev_mode: Option<AppMode>,
    pub menu_restore_panel: Option<ActivePanel>,
    pub dialog_selection: usize,
    pub pending_action: Option<PendingAction>,
    // Mouse support fields
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_position: Option<(u16, u16)>, // (column, row)
}

// ============================================================================
// ViewMode enum
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text { wrap: bool, line_numbers: bool },
    Hex,
}

// ============================================================================
// PendingAction enum
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    Copy {
        sources: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
    },
    Move {
        sources: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
    },
    Delete {
        paths: Vec<std::path::PathBuf>,
    },
}

// ============================================================================
// FileEntry implementation
// ============================================================================

impl FileEntry {
    pub fn category(&self) -> FileCategory {
        use crate::app::file_type as ft;
        if self.is_dir {
            return FileCategory::Dir;
        }
        if self.is_symlink {
            return FileCategory::Symlink;
        }
        if self.is_hidden {
            return FileCategory::Hidden;
        }
        if self.is_executable {
            return FileCategory::Executable;
        }
        if ft::is_source_code(&self.name) {
            return FileCategory::Code;
        }
        if ft::is_config(&self.name) {
            return FileCategory::Config;
        }
        if ft::is_archive(&self.name) {
            return FileCategory::Archive;
        }
        if ft::is_image(&self.name) {
            return FileCategory::Image;
        }
        if ft::is_video(&self.name) {
            return FileCategory::Video;
        }
        if ft::is_audio(&self.name) {
            return FileCategory::Audio;
        }
        if ft::is_document(&self.name) {
            return FileCategory::Document;
        }
        FileCategory::Other
    }

    pub fn display_size(&self) -> String {
        Self::format_size(self.size)
    }

    pub fn format_size(size: u64) -> String {
        const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
        let mut size = size as f64;
        let mut unit_idx = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{:>6} {}", size as u64, UNITS[unit_idx])
        } else {
            format!("{:>6.1} {}", size, UNITS[unit_idx])
        }
    }

    pub fn display_permissions(&self) -> String {
        Self::display_permissions_raw(self.permissions)
    }

    pub fn display_permissions_raw(mode: u32) -> String {
        let mut result = String::with_capacity(9);

        result.push(if mode & 0o400 != 0 { 'r' } else { '-' });
        result.push(if mode & 0o200 != 0 { 'w' } else { '-' });
        result.push(if mode & 0o4000 != 0 {
            if mode & 0o100 != 0 { 's' } else { 'S' }
        } else {
            if mode & 0o100 != 0 { 'x' } else { '-' }
        });

        result.push(if mode & 0o040 != 0 { 'r' } else { '-' });
        result.push(if mode & 0o020 != 0 { 'w' } else { '-' });
        result.push(if mode & 0o2000 != 0 {
            if mode & 0o010 != 0 { 's' } else { 'S' }
        } else {
            if mode & 0o010 != 0 { 'x' } else { '-' }
        });

        result.push(if mode & 0o004 != 0 { 'r' } else { '-' });
        result.push(if mode & 0o002 != 0 { 'w' } else { '-' });
        result.push(if mode & 0o1000 != 0 {
            if mode & 0o001 != 0 { 't' } else { 'T' }
        } else {
            if mode & 0o001 != 0 { 'x' } else { '-' }
        });

        result
    }

    pub fn display_modified(&self) -> String {
        use std::time::UNIX_EPOCH;

        if let Ok(duration) = self.modified.duration_since(UNIX_EPOCH) {
            chrono::DateTime::from_timestamp(
                i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
                0,
            )
            .unwrap_or(chrono::DateTime::UNIX_EPOCH)
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
        } else {
            "Unknown".to_string()
        }
    }
}

// ============================================================================
// PanelState implementation
// ============================================================================

impl PanelState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
            sort_mode: SortMode::default(),
            listing_mode: ListingMode::default(),
            show_hidden: true,
            filter: None,
            selected_count: 0,
            selected_size: 0,
            selection_anchor: None,
            last_error: None,
            history: Vec::new(),
            unfiltered_entries: Vec::new(),
        }
    }

    fn update_selection_stats(&mut self, size: u64, selected: bool) {
        if selected {
            self.selected_count += 1;
            self.selected_size += size;
        } else {
            self.selected_count = self.selected_count.saturating_sub(1);
            self.selected_size = self.selected_size.saturating_sub(size);
        }
    }

    pub fn current_entry(&self) -> Option<&FileEntry> {
        if self.cursor < self.entries.len() {
            Some(&self.entries[self.cursor])
        } else {
            None
        }
    }

    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.cursor) {
            if entry.name == ".." {
                return;
            }
            entry.selected = !entry.selected;
            let size = entry.size;
            let selected = entry.selected;
            let path = entry.path.clone();
            self.update_selection_stats(size, selected);
            self.set_unfiltered_selection(&path, selected);
        }
    }

    pub fn set_selection_at(&mut self, index: usize, selected: bool) {
        if let Some(entry) = self.entries.get_mut(index) {
            if entry.name == ".." || entry.selected == selected {
                return;
            }
            entry.selected = selected;
            let size = entry.size;
            let path = entry.path.clone();
            self.update_selection_stats(size, selected);
            self.set_unfiltered_selection(&path, selected);
        }
    }

    fn set_unfiltered_selection(&mut self, path: &PathBuf, selected: bool) {
        if let Some(ue) = self.unfiltered_entries.iter_mut().find(|e| e.path == *path) {
            ue.selected = selected;
        }
    }

    pub fn sync_unfiltered_selection(&mut self) {
        if self.unfiltered_entries.is_empty() {
            return;
        }

        let selected_by_path: HashMap<&PathBuf, bool> = self
            .entries
            .iter()
            .map(|entry| (&entry.path, entry.selected))
            .collect();
        for entry in &mut self.unfiltered_entries {
            if let Some(selected) = selected_by_path.get(&entry.path) {
                entry.selected = *selected;
            }
        }
    }

    pub fn selected_entries(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| e.selected).collect()
    }

    pub fn clear_selection(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
        for entry in &mut self.unfiltered_entries {
            entry.selected = false;
        }
        self.selected_count = 0;
        self.selected_size = 0;
        self.selection_anchor = None;
    }

    pub fn recalculate_selection_stats(&mut self) {
        self.selected_count = 0;
        self.selected_size = 0;
        let source = if self.unfiltered_entries.is_empty() {
            &self.entries
        } else {
            &self.unfiltered_entries
        };
        for entry in source {
            if entry.selected {
                self.selected_count += 1;
                self.selected_size += entry.size;
            }
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        // Adjust scroll if cursor goes above visible area
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        }
    }

    pub fn move_cursor_down(&mut self, max_height: usize) {
        if self.entries.is_empty() {
            return;
        }

        let max_index = self.entries.len().saturating_sub(1);

        if self.cursor < max_index {
            self.cursor += 1;
        }

        if max_height > 0 && self.cursor >= self.scroll_offset + max_height {
            self.scroll_offset = self.cursor.saturating_sub(max_height) + 1;
        }
    }

    pub fn ensure_cursor_visible(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.scroll_offset > self.cursor {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + visible_height {
            self.scroll_offset = self.cursor.saturating_sub(visible_height) + 1;
        }
    }
}

// ================================================================================
// AppState implementation
// ================================================================================

impl AppState {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let panels = PanelsState::new(current_dir.clone());
        let command = CommandState::default();
        let dialog = DialogState::default();
        let menu = MenuState::new(current_dir);
        let picker = PickerState::default();
        let tree = DirectoryTreeState::default();

        Self {
            left_panel: panels.left_panel,
            right_panel: panels.right_panel,
            active_panel: panels.active_panel,
            mode: AppMode::Normal,
            command_line: command.command_line,
            search_query: String::new(),
            should_quit: false,
            status_message: None,
            dialog_input: dialog.dialog_input,
            dialog_cursor_pos: dialog.dialog_cursor_pos,
            command_history: command.command_history,
            history_index: command.history_index,
            command_draft: command.command_draft,
            directory_hotlist: menu.directory_hotlist,
            menu_selected: menu.menu_selected,
            menu_item_selected: menu.menu_item_selected,
            picker_selected: picker.picker_selected,
            user_menu_entries: menu.user_menu_entries,
            tree_root: tree.tree_root,
            tree_entries: tree.tree_entries,
            tree_selected: tree.tree_selected,
            tree_scroll: tree.tree_scroll,
            prev_mode: None,
            menu_restore_panel: menu.menu_restore_panel,
            dialog_selection: dialog.dialog_selection,
            // Mouse support fields
            last_click_time: None,
            last_click_position: None,
            pending_action: dialog.pending_action,
        }
    }

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
}

// Default implementation for AppState
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    // Helper to create a test FileEntry
    fn create_test_entry(
        name: &str,
        is_dir: bool,
        size: u64,
        permissions: u32,
        is_selected: bool,
    ) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_executable: permissions & 1 != 0,
            size,
            modified: UNIX_EPOCH + Duration::from_secs(1_000_000_000),
            permissions,
            owner: "testuser".to_string(),
            group: "testgroup".to_string(),
            selected: is_selected,
            is_hidden: name.starts_with('.'),
            mime_type: None,
        }
    }

    #[test]
    fn test_file_entry_display_size_bytes() {
        let entry = create_test_entry("test.txt", false, 500, 0o644, false);
        assert_eq!(entry.display_size(), "   500 B");
    }

    #[test]
    fn test_file_entry_display_size_kilobytes() {
        let entry = create_test_entry("test.txt", false, 1500, 0o644, false);
        assert_eq!(entry.display_size(), "   1.5 KB");
    }

    #[test]
    fn test_file_entry_display_size_megabytes() {
        let entry = create_test_entry("test.txt", false, 1_500_000, 0o644, false);
        assert_eq!(entry.display_size(), "   1.4 MB");
    }

    #[test]
    fn test_file_entry_display_size_gigabytes() {
        let entry = create_test_entry("test.txt", false, 1_500_000_000, 0o644, false);
        assert_eq!(entry.display_size(), "   1.4 GB");
    }

    #[test]
    fn test_file_entry_display_size_zero() {
        let entry = create_test_entry("test.txt", false, 0, 0o644, false);
        assert_eq!(entry.display_size(), "     0 B");
    }

    #[test]
    fn test_file_entry_display_permissions() {
        let entry = create_test_entry("test.txt", false, 100, 0o755, false);
        assert_eq!(entry.display_permissions(), "rwxr-xr-x");
    }

    #[test]
    fn test_file_entry_display_permissions_no_exec() {
        let entry = create_test_entry("test.txt", false, 100, 0o644, false);
        assert_eq!(entry.display_permissions(), "rw-r--r--");
    }

    #[test]
    fn test_file_entry_display_permissions_all() {
        let entry = create_test_entry("test.txt", false, 100, 0o777, false);
        assert_eq!(entry.display_permissions(), "rwxrwxrwx");
    }

    #[test]
    fn test_file_entry_display_permissions_none() {
        let entry = create_test_entry("test.txt", false, 100, 0o000, false);
        assert_eq!(entry.display_permissions(), "---------");
    }

    #[test]
    fn test_file_entry_display_modified() {
        let entry = create_test_entry("test.txt", false, 100, 0o644, false);
        let expected = chrono::DateTime::from_timestamp(1_000_000_000, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert_eq!(entry.display_modified(), expected);
    }

    #[test]
    fn test_sort_mode_default() {
        assert_eq!(SortMode::default(), SortMode::NameAsc);
    }

    #[test]
    fn test_panel_state_new() {
        let path = PathBuf::from("/test");
        let panel = PanelState::new(path.clone());
        assert_eq!(panel.path, path);
        assert_eq!(panel.entries.len(), 0);
        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.scroll_offset, 0);
        assert_eq!(panel.sort_mode, SortMode::default());
        assert_eq!(panel.listing_mode, ListingMode::Long);
        assert!(panel.show_hidden);
        assert!(panel.filter.is_none());
    }

    #[test]
    fn test_panel_state_current_entry_none_when_empty() {
        let panel = PanelState::new(PathBuf::from("/test"));
        assert!(panel.current_entry().is_none());
    }

    #[test]
    fn test_panel_state_current_entry_some() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        assert_eq!(panel.current_entry().unwrap().name, "file1.txt");
    }

    #[test]
    fn test_panel_state_current_entry_out_of_bounds() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 5;
        assert!(panel.current_entry().is_none());
    }

    #[test]
    fn test_panel_state_toggle_selection_toggle_on() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.toggle_selection();
        assert!(panel.entries[0].selected);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 100);
    }

    #[test]
    fn test_panel_state_toggle_selection_toggle_off() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));
        panel.cursor = 0;
        assert!(panel.entries[0].selected);
        panel.toggle_selection();
        assert!(!panel.entries[0].selected);
        assert_eq!(panel.selected_count, 0);
        assert_eq!(panel.selected_size, 0);
    }

    #[test]
    fn test_panel_state_set_selection_at_on() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));

        panel.set_selection_at(0, true);

        assert!(panel.entries[0].selected);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 100);
    }

    #[test]
    fn test_panel_state_set_selection_at_off() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));

        panel.set_selection_at(0, false);

        assert!(!panel.entries[0].selected);
        assert_eq!(panel.selected_count, 0);
        assert_eq!(panel.selected_size, 0);
    }

    #[test]
    fn test_panel_state_sync_unfiltered_selection() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("file1.txt", false, 100, 0o644, true),
            create_test_entry("file2.txt", false, 200, 0o644, false),
        ];
        panel.unfiltered_entries = vec![
            create_test_entry("file1.txt", false, 100, 0o644, false),
            create_test_entry("file2.txt", false, 200, 0o644, true),
            create_test_entry("file3.txt", false, 300, 0o644, true),
        ];

        panel.sync_unfiltered_selection();

        assert!(panel.unfiltered_entries[0].selected);
        assert!(!panel.unfiltered_entries[1].selected);
        assert!(panel.unfiltered_entries[2].selected);
    }

    #[test]
    fn test_panel_state_selected_entries() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));
        panel
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel
            .entries
            .push(create_test_entry("file3.txt", false, 300, 0o644, true));

        let selected = panel.selected_entries();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].name, "file1.txt");
        assert_eq!(selected[1].name, "file3.txt");
    }

    #[test]
    fn test_panel_state_move_cursor_up() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel.cursor = 1;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_up_at_top() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_down() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel.cursor = 0;
        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 1);
    }

    #[test]
    fn test_panel_state_move_cursor_down_at_bottom() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel.cursor = 1;
        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 1);
    }

    #[test]
    fn test_panel_state_move_cursor_down_scroll() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 4;
        panel.scroll_offset = 0;
        panel.move_cursor_down(5);
        assert_eq!(panel.cursor, 5);
        // New formula: cursor >= scroll_offset + max_height (5 >= 0 + 5 = 5)
        // scroll_offset = cursor - max_height + 1 = 5 - 5 + 1 = 1
        assert_eq!(panel.scroll_offset, 1);
    }

    #[test]
    fn test_panel_state_move_cursor_down_empty() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.cursor = 0;
        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert_eq!(state.active_panel, ActivePanel::Left);
        assert_eq!(state.mode, AppMode::Normal);
        assert!(!state.should_quit);
        assert!(state.status_message.is_none());
    }

    #[test]
    fn test_app_state_substate_defaults() {
        let current_dir = PathBuf::from("/tmp");
        let panels = PanelsState::new(current_dir.clone());
        let menu = MenuState::new(current_dir.clone());

        assert_eq!(panels.left_panel.path, current_dir.clone());
        assert_eq!(panels.right_panel.path, PathBuf::from("/tmp"));
        assert_eq!(panels.active_panel, ActivePanel::Left);
        assert_eq!(menu.directory_hotlist, vec![PathBuf::from("/tmp")]);
        assert_eq!(DialogState::default().dialog_cursor_pos, 0);
        assert_eq!(PickerState::default().picker_selected, 0);
        assert!(DirectoryTreeState::default().tree_entries.is_empty());
    }

    #[test]
    fn test_app_state_active_panel_left() {
        let state = AppState::new();
        let panel = state.active_panel();
        assert_eq!(panel.path, state.left_panel.path);
    }

    #[test]
    fn test_app_state_active_panel_right() {
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Right;
        let panel = state.active_panel();
        assert_eq!(panel.path, state.right_panel.path);
    }

    #[test]
    fn test_app_state_active_panel_mut_left() {
        let mut state = AppState::new();
        let panel = state.active_panel_mut();
        panel.path = PathBuf::from("/modified");
        assert_eq!(state.left_panel.path, PathBuf::from("/modified"));
    }

    #[test]
    fn test_app_state_active_panel_mut_right() {
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Right;
        let panel = state.active_panel_mut();
        panel.path = PathBuf::from("/modified");
        assert_eq!(state.right_panel.path, PathBuf::from("/modified"));
    }

    #[test]
    fn test_app_state_inactive_panel_left() {
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Right;
        let panel = state.inactive_panel();
        assert_eq!(panel.path, state.left_panel.path);
    }

    #[test]
    fn test_app_state_inactive_panel_right() {
        let state = AppState::new();
        let panel = state.inactive_panel();
        assert_eq!(panel.path, state.right_panel.path);
    }

    #[test]
    fn test_dialog_kind_confirm() {
        let dialog = DialogKind::Confirm("Are you sure?".to_string());
        if let DialogKind::Confirm(msg) = dialog {
            assert_eq!(msg, "Are you sure?");
        } else {
            panic!("Expected Confirm variant");
        }
    }

    #[test]
    fn test_dialog_kind_input() {
        let dialog = DialogKind::Input {
            prompt: "Enter name:".to_string(),
            default_text: "default".to_string(),
            action: InputAction::Rename,
        };
        if let DialogKind::Input {
            prompt,
            default_text,
            action,
        } = dialog
        {
            assert_eq!(prompt, "Enter name:");
            assert_eq!(default_text, "default");
            assert_eq!(action, InputAction::Rename);
        } else {
            panic!("Expected Input variant");
        }
    }

    #[test]
    fn test_dialog_kind_error() {
        let dialog = DialogKind::Error("Error occurred".to_string());
        if let DialogKind::Error(msg) = dialog {
            assert_eq!(msg, "Error occurred");
        } else {
            panic!("Expected Error variant");
        }
    }

    #[test]
    fn test_dialog_kind_progress() {
        let dialog = DialogKind::Progress("Copying...".to_string(), 0.5);
        if let DialogKind::Progress(msg, progress) = dialog {
            assert_eq!(msg, "Copying...");
            assert_eq!(progress, 0.5);
        } else {
            panic!("Expected Progress variant");
        }
    }

    #[test]
    fn test_dialog_kind_copy_move() {
        let sources = vec![PathBuf::from("/source1"), PathBuf::from("/source2")];
        let dest = PathBuf::from("/dest");
        let dialog = DialogKind::CopyMove {
            source: sources.clone(),
            dest: dest.clone(),
            is_move: true,
        };
        if let DialogKind::CopyMove {
            source,
            dest: d,
            is_move,
        } = dialog
        {
            assert_eq!(source, sources);
            assert_eq!(d, dest);
            assert!(is_move);
        } else {
            panic!("Expected CopyMove variant");
        }
    }

    #[test]
    fn test_app_mode_variants() {
        let normal = AppMode::Normal;
        assert_eq!(normal, AppMode::Normal);

        let viewing = AppMode::Viewing;
        assert_eq!(viewing, AppMode::Viewing);

        let cmd_line = AppMode::CommandLine;
        assert_eq!(cmd_line, AppMode::CommandLine);

        let dialog = AppMode::Dialog(DialogKind::Confirm("test".to_string()));
        if let AppMode::Dialog(DialogKind::Confirm(msg)) = &dialog {
            assert_eq!(msg, "test");
        }

        let search = AppMode::Search;
        assert_eq!(search, AppMode::Search);

        let menu = AppMode::Menu;
        assert_eq!(menu, AppMode::Menu);

        let picker = AppMode::ListPicker(PickerKind::History);
        assert_eq!(picker, AppMode::ListPicker(PickerKind::History));
    }

    #[test]
    fn test_active_panel_variants() {
        let left = ActivePanel::Left;
        assert_eq!(left, ActivePanel::Left);

        let right = ActivePanel::Right;
        assert_eq!(right, ActivePanel::Right);
    }

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.active_panel, ActivePanel::Left);
        assert!(!state.should_quit);
    }

    #[test]
    fn test_panel_state_move_cursor_up_scroll_adjust() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 3;
        panel.scroll_offset = 5;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 2);
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_panel_state_move_cursor_up_no_scroll_when_visible() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 5;
        panel.scroll_offset = 3;
        panel.move_cursor_up();
        assert_eq!(panel.cursor, 4);
        assert_eq!(panel.scroll_offset, 3);
    }

    #[test]
    fn test_panel_state_move_cursor_down_scroll_new_formula() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 5;
        panel.scroll_offset = 3;
        // max_height = 4, so visible area is indices 3, 4, 5, 6
        // cursor becomes 6, which equals scroll_offset + max_height (3 + 4 = 7)? No, wait 3 + 4 = 7, cursor is 6
        // Let's test cursor moving beyond visible area
        panel.cursor = 6;
        panel.scroll_offset = 3;
        panel.move_cursor_down(4);
        assert_eq!(panel.cursor, 7);
        // cursor = 7, scroll_offset + max_height = 3 + 4 = 7, so cursor >= visible area
        // scroll_offset = 7 - 4 + 1 = 4
        assert_eq!(panel.scroll_offset, 4);
    }

    #[test]
    fn test_panel_state_move_cursor_down_no_scroll_when_visible() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 3;
        panel.scroll_offset = 0;
        panel.move_cursor_down(5);
        assert_eq!(panel.cursor, 4);
        // scroll_offset should remain 0 since 4 < 0 + 5
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_panel_state_ensure_cursor_visible_below() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 7;
        panel.scroll_offset = 2;
        panel.ensure_cursor_visible(4);
        // cursor = 7, visible area is 2..6, cursor is beyond visible, so scroll_offset = 7 - 4 + 1 = 4
        assert_eq!(panel.scroll_offset, 4);
    }

    #[test]
    fn test_panel_state_ensure_cursor_visible_above() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 2;
        panel.scroll_offset = 5;
        panel.ensure_cursor_visible(4);
        // cursor = 2, scroll_offset = 5, cursor < scroll_offset, so scroll_offset = cursor = 2
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_panel_state_ensure_cursor_visible_already_visible() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 4;
        panel.scroll_offset = 2;
        panel.ensure_cursor_visible(4);
        // cursor = 4, visible area is 2..5 (indices 2, 3, 4), cursor is within
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_panel_state_ensure_cursor_visible_edge_case() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 6;
        panel.scroll_offset = 3;
        panel.ensure_cursor_visible(4);
        // cursor = 6, visible area is 3..6 (indices 3, 4, 5), cursor at edge
        // cursor equals scroll_offset + max_height (3 + 4 = 7)? No 3 + 4 = 7
        // Wait, indices 3, 4, 5, 6 are visible (4 items)
        // Visible is [3,4,5,6], cursor=6 is at last index
        // scroll_offset + max_height = 3 + 4 = 7, so cursor=6 < 7
        // Actually with formula "cursor >= scroll_offset + max_height",
        // 6 >= 3 + 4 = 7 is false, so no scroll
        assert_eq!(panel.scroll_offset, 3);
    }
}
