use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::dir_tree::TreeEntry;
use super::user_menu::MenuEntry;
use crate::fs::cha::{Cha, ChaKind, ChaMode};

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
    Font,
    Executable,
    Symlink,
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
    pub cha: Cha,
    pub owner: String,
    pub group: String,
    pub selected: bool,
    pub mime_type: Option<String>,
}

// ============================================================================
// 2a. SortOptions struct definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SortOptions {
    #[serde(default = "default_true")]
    pub dir_first: bool,
    #[serde(default)]
    pub sort_sensitive: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        Self {
            dir_first: true,
            sort_sensitive: false,
        }
    }
}

fn default_true() -> bool {
    true
}

// ============================================================================
// 2b. SortMode enum definition
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
    NaturalNameAsc,
    NaturalNameDesc,
    BtimeAsc,
    BtimeDesc,
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
    pub sort_options: SortOptions,
    pub listing_mode: ListingMode,
    pub show_hidden: bool,
    pub show_permissions: bool,
    pub filter: Option<String>,
    pub selected_count: usize,
    pub selected_size: u64,
    pub total_size: u64,
    pub last_error: Option<String>,
    pub history: Vec<PathBuf>,
    pub unfiltered_entries: Vec<FileEntry>,
    pub unfiltered_dirty: bool,
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
// 4b. ConfirmDetails struct
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDetails {
    pub title: String,
    pub message: String,
    pub files: Option<Vec<PathBuf>>,
}

impl ConfirmDetails {
    pub fn simple(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: None,
        }
    }

    pub fn with_files(title: &str, message: &str, files: Vec<PathBuf>) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: if files.is_empty() { None } else { Some(files) },
        }
    }
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
    Confirm(ConfirmDetails),
    Input {
        prompt: String,
        default_text: String,
        action: InputAction,
    },
    Error(String),
    Help {
        message: String,
        scroll_offset: usize,
    },
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
    OverwriteConfirm {
        conflicting: Vec<String>,
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
    /// Match files by name + size; directories match by name only.
    Size,
    /// Match files by name + size + mtime; directories match by name only.
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
// 7. AppState struct definition
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,
    pub mode: AppMode,
    pub command_line: String,
    pub command_cursor: usize,
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
    Text,
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
        overwrite: bool,
    },
    Move {
        sources: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
        overwrite: bool,
    },
    Delete {
        paths: Vec<std::path::PathBuf>,
    },
}

// ============================================================================
// FileEntryBuilder
// ============================================================================

#[derive(Debug)]
pub struct FileEntryBuilder {
    name: String,
    path: PathBuf,
    cha: Cha,
    owner: String,
    group: String,
    selected: bool,
    mime_type: Option<String>,
}

impl FileEntryBuilder {
    pub fn name(mut self, v: impl Into<String>) -> Self {
        self.name = v.into();
        self
    }
    pub fn path(mut self, v: impl Into<PathBuf>) -> Self {
        self.path = v.into();
        self
    }
    pub fn cha(mut self, v: Cha) -> Self {
        self.cha = v;
        self
    }
    pub fn is_dir(mut self, v: bool) -> Self {
        let perms = self.cha.mode.permissions() as u32;
        if v {
            self.cha.mode = ChaMode::new(0o040000 | perms);
        } else if self.cha.is_dir() {
            self.cha.mode = ChaMode::new(0o100000 | perms);
        }
        self
    }
    pub fn is_symlink(mut self, v: bool) -> Self {
        let perms = self.cha.mode.permissions() as u32;
        if v {
            self.cha.mode = ChaMode::new(0o120000 | perms);
        } else if self.cha.is_link() {
            self.cha.mode = ChaMode::new(0o100000 | perms);
        }
        self
    }
    pub fn is_executable(mut self, v: bool) -> Self {
        self.cha = self.cha.with_executable(v);
        self
    }
    pub fn size(mut self, v: u64) -> Self {
        self.cha.len = v;
        self
    }
    pub fn modified(mut self, v: SystemTime) -> Self {
        self.cha.mtime = Some(v);
        self
    }
    pub fn created(mut self, v: SystemTime) -> Self {
        self.cha.btime = Some(v);
        self
    }
    pub fn permissions(mut self, v: u32) -> Self {
        let file_type = self.cha.mode.mode_u32() & 0o170000;
        self.cha.mode = ChaMode::new(file_type | (v & 0o7777));
        self
    }
    pub fn owner(mut self, v: impl Into<String>) -> Self {
        self.owner = v.into();
        self
    }
    pub fn group(mut self, v: impl Into<String>) -> Self {
        self.group = v.into();
        self
    }
    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }
    pub fn is_hidden(mut self, v: bool) -> Self {
        self.cha = self.cha.with_hidden(v);
        self
    }
    pub fn mime_type(mut self, v: Option<String>) -> Self {
        self.mime_type = v;
        self
    }
    pub fn build(self) -> FileEntry {
        FileEntry {
            name: self.name,
            path: self.path,
            cha: self.cha,
            owner: self.owner,
            group: self.group,
            selected: self.selected,
            mime_type: self.mime_type,
        }
    }
}

// ============================================================================
// FileEntry implementation
// ============================================================================

impl FileEntry {
    pub fn builder() -> FileEntryBuilder {
        FileEntryBuilder {
            name: String::new(),
            path: PathBuf::new(),
            cha: Cha {
                kind: ChaKind::empty(),
                mode: ChaMode::new(0o100644),
                len: 0,
                mtime: None,
                btime: None,
                ctime: None,
                atime: None,
                uid: 0,
                gid: 0,
                dev: 0,
                nlink: 0,
            },
            owner: String::new(),
            group: String::new(),
            selected: false,
            mime_type: None,
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.cha.len()
    }

    pub fn mtime(&self) -> SystemTime {
        self.cha.mtime().unwrap_or(std::time::UNIX_EPOCH)
    }

    pub fn btime(&self) -> SystemTime {
        self.cha.btime().unwrap_or(std::time::UNIX_EPOCH)
    }

    pub fn mode_bits(&self) -> u32 {
        self.cha.mode.mode_u32()
    }

    pub fn uid(&self) -> u32 {
        self.cha.uid
    }

    pub fn gid(&self) -> u32 {
        self.cha.gid
    }

    pub fn is_dir(&self) -> bool {
        self.cha.is_dir()
    }

    pub fn is_symlink(&self) -> bool {
        self.cha.is_link()
    }

    pub fn is_executable(&self) -> bool {
        self.cha.is_executable()
    }

    pub fn is_hidden(&self) -> bool {
        self.cha.is_hidden()
    }

    /// Returns the primary `FileCategory` based on a priority hierarchy.
    ///
    /// Priority (highest to lowest):
    /// 1. `Symlink` — always a symlink (regardless of target type)
    /// 2. `Dir` — always a directory
    /// 3. `Code` → `Config` → `Archive` → `Image` → `Video` → `Audio` → `Document` → `Font`
    /// 4. `Executable` — files with execute permission (fallback for extensionless binaries)
    /// 5. `Other` — fallback
    ///
    /// Hidden files get their real type (e.g. `.bashrc` → Config, `.backup.zip` → Archive).
    /// A symlink to a directory is `Symlink`, not `Dir`.
    pub fn category(&self) -> FileCategory {
        use crate::app::file_type as ft;
        if self.is_symlink() {
            return FileCategory::Symlink;
        }
        if self.is_dir() {
            return FileCategory::Dir;
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
        if ft::is_font(&self.name) {
            return FileCategory::Font;
        }
        if self.is_executable() {
            return FileCategory::Executable;
        }
        FileCategory::Other
    }

    pub fn display_size(&self) -> String {
        Self::format_size(self.len())
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
        Self::display_permissions_raw(self.mode_bits())
    }

    pub fn display_permissions_raw(mode: u32) -> String {
        use crate::fs::cha::ChaMode;
        ChaMode::new(mode).to_string()
    }

    pub fn display_modified(&self) -> String {
        use std::time::UNIX_EPOCH;

        if let Ok(duration) = self.mtime().duration_since(UNIX_EPOCH) {
            i64::try_from(duration.as_secs())
                .ok()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%d-%m-%y %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "Unknown".to_string())
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
            sort_options: SortOptions::default(),
            listing_mode: ListingMode::default(),
            show_hidden: true,
            show_permissions: false,
            filter: None,
            selected_count: 0,
            selected_size: 0,
            total_size: 0,
            last_error: None,
            history: Vec::new(),
            unfiltered_entries: Vec::new(),
            unfiltered_dirty: true,
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
            let size = entry.len();
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
            let size = entry.len();
            let path = entry.path.clone();
            self.update_selection_stats(size, selected);
            self.set_unfiltered_selection(&path, selected);
        }
    }

    pub fn toggle_selection_at(&mut self, index: usize) {
        let selected = self.entries.get(index).is_some_and(|e| !e.selected);
        self.set_selection_at(index, selected);
    }

    fn set_unfiltered_selection(&mut self, path: &Path, selected: bool) {
        if let Some(ue) = self.unfiltered_entries.iter_mut().find(|e| e.path == *path) {
            ue.selected = selected;
        }
    }

    pub fn sync_unfiltered_selection(&mut self) {
        if self.unfiltered_entries.is_empty() {
            return;
        }

        let selection: HashMap<_, _> = self
            .entries
            .iter()
            .map(|entry| (entry.path.as_path(), entry.selected))
            .collect();

        for entry in &mut self.unfiltered_entries {
            if let Some(selected) = selection.get(entry.path.as_path()) {
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
    }

    pub fn recalculate_selection_stats(&mut self) {
        self.selected_count = 0;
        self.selected_size = 0;
        self.total_size = 0;
        let source = if self.unfiltered_entries.is_empty() {
            &self.entries
        } else {
            &self.unfiltered_entries
        };
        for entry in source {
            self.total_size += entry.len();
            if entry.selected {
                self.selected_count += 1;
                self.selected_size += entry.len();
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

        Self {
            left_panel: PanelState::new(current_dir.clone()),
            right_panel: PanelState::new(current_dir.clone()),
            active_panel: ActivePanel::Left,
            mode: AppMode::Normal,
            command_line: String::new(),
            command_cursor: 0,
            search_query: String::new(),
            should_quit: false,
            status_message: None,
            dialog_input: String::new(),
            dialog_cursor_pos: 0,
            command_history: VecDeque::new(),
            history_index: None,
            command_draft: String::new(),
            directory_hotlist: vec![current_dir],
            menu_selected: 0,
            menu_item_selected: 0,
            picker_selected: 0,
            user_menu_entries: Vec::new(),
            tree_root: PathBuf::new(),
            tree_entries: Vec::new(),
            tree_selected: 0,
            tree_scroll: 0,
            prev_mode: None,
            menu_restore_panel: None,
            dialog_selection: 0,
            last_click_time: None,
            last_click_position: None,
            pending_action: None,
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
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[derive(Debug, Clone, PartialEq)]
    struct PanelsState {
        left_panel: PanelState,
        right_panel: PanelState,
        active_panel: ActivePanel,
    }

    impl PanelsState {
        fn new(current_dir: PathBuf) -> Self {
            Self {
                left_panel: PanelState::new(current_dir.clone()),
                right_panel: PanelState::new(current_dir),
                active_panel: ActivePanel::Left,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    struct DialogState {
        dialog_input: String,
        dialog_cursor_pos: usize,
        dialog_selection: usize,
        pending_action: Option<PendingAction>,
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    struct MenuState {
        directory_hotlist: Vec<PathBuf>,
        menu_selected: usize,
        menu_item_selected: usize,
        user_menu_entries: Vec<MenuEntry>,
        menu_restore_panel: Option<ActivePanel>,
    }

    impl MenuState {
        fn new(initial_hotlist_path: PathBuf) -> Self {
            Self {
                directory_hotlist: vec![initial_hotlist_path],
                ..Self::default()
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    struct PickerState {
        picker_selected: usize,
    }

    #[derive(Debug, Clone, PartialEq, Default)]
    struct DirectoryTreeState {
        tree_root: PathBuf,
        tree_entries: Vec<TreeEntry>,
        tree_selected: usize,
        tree_scroll: usize,
    }

    // Helper to create a test FileEntry
    fn create_test_entry(
        name: &str,
        is_dir: bool,
        size: u64,
        permissions: u32,
        is_selected: bool,
    ) -> FileEntry {
        FileEntry::builder()
            .name(name)
            .path(PathBuf::from(name))
            .is_dir(is_dir)
            .size(size)
            .permissions(permissions)
            .selected(is_selected)
            .is_hidden(name.starts_with('.'))
            .modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .created(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .owner("testuser")
            .group("testgroup")
            .build()
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
            .format("%d-%m-%y %H:%M")
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

        assert_eq!(panels.left_panel.path, current_dir);
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
        let details = ConfirmDetails::simple("Confirm", "Are you sure?");
        let dialog = DialogKind::Confirm(details);
        if let DialogKind::Confirm(cd) = dialog {
            assert_eq!(cd.title, "Confirm");
            assert_eq!(cd.message, "Are you sure?");
            assert!(cd.files.is_none());
        } else {
            panic!("Expected Confirm variant");
        }
    }

    #[test]
    fn test_confirm_details_simple() {
        let cd = ConfirmDetails::simple("Delete", "Delete 'foo.txt'?");
        assert_eq!(cd.title, "Delete");
        assert_eq!(cd.message, "Delete 'foo.txt'?");
        assert!(cd.files.is_none());
    }

    #[test]
    fn test_confirm_details_with_files() {
        let files = vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")];
        let cd = ConfirmDetails::with_files("Delete", "Delete 2 entries?", files.clone());
        assert_eq!(cd.files.as_ref().unwrap(), &files);
    }

    #[test]
    fn test_confirm_details_with_empty_files() {
        let cd = ConfirmDetails::with_files("Delete", "Nothing?", vec![]);
        assert!(cd.files.is_none());
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

        let dialog = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple("Test", "test")));
        if let AppMode::Dialog(DialogKind::Confirm(cd)) = &dialog {
            assert_eq!(cd.message, "test");
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

    #[test]
    fn test_total_size_computed_by_recalculate() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("a.txt", false, 100, 0o644, false),
            create_test_entry("b.txt", false, 200, 0o644, false),
            create_test_entry("c.txt", false, 300, 0o644, true),
        ];
        panel.recalculate_selection_stats();
        assert_eq!(panel.total_size, 600);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 300);
    }

    fn cha_entry(name: &str, mode: u32, size: u64, hidden: bool) -> FileEntry {
        let is_link = (mode & 0o170000) == 0o120000;
        let is_directory = (mode & 0o170000) == 0o040000;
        FileEntry::builder()
            .name(name)
            .path(PathBuf::from(name))
            .is_dir(is_directory)
            .is_symlink(is_link)
            .size(size)
            .permissions(mode & 0o7777)
            .is_hidden(hidden)
            .modified(UNIX_EPOCH)
            .created(UNIX_EPOCH)
            .owner("testuser")
            .group("testgroup")
            .build()
    }

    #[test]
    fn test_hidden_script_is_code() {
        let entry = cha_entry(".script.sh", 0o100755, 100, true);
        assert_eq!(entry.category(), FileCategory::Code);
    }

    #[test]
    fn test_hidden_archive_is_archive() {
        let entry = cha_entry(".backup.zip", 0o100644, 100, true);
        assert_eq!(entry.category(), FileCategory::Archive);
    }

    #[test]
    fn test_symlink_overrides_dir() {
        let entry = cha_entry("link_to_dir", 0o120777, 0, false);
        assert_eq!(entry.category(), FileCategory::Symlink);
    }

    #[test]
    fn test_symlink_overrides_hidden() {
        let entry = cha_entry(".hidden_link", 0o120777, 0, true);
        assert_eq!(entry.category(), FileCategory::Symlink);
    }

    #[test]
    fn test_executable_without_extension_is_executable() {
        let entry = cha_entry("mybinary", 0o100755, 100, false);
        assert_eq!(entry.category(), FileCategory::Executable);
    }

    #[test]
    fn test_hidden_apk_is_archive() {
        let entry = cha_entry(".app.apk", 0o100644, 100, true);
        assert_eq!(entry.category(), FileCategory::Archive);
    }

    #[test]
    fn test_total_size_includes_all_entries() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("small.txt", false, 50, 0o644, false),
            create_test_entry("big.txt", false, 5000, 0o644, true),
        ];
        panel.recalculate_selection_stats();
        assert_eq!(panel.total_size, 5050);
        assert_eq!(panel.selected_size, 5000);
    }
}
