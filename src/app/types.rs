use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::dir_tree::TreeEntry;
use super::user_menu::{MenuEntry, MenuSource};
use crate::fs::cha::{Cha, ChaKind, ChaMode};
use crate::ui::theme::ColorPalette;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextInput {
    pub text: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.grapheme_count());
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn byte_pos(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    pub fn insert_char(&mut self, c: char) -> bool {
        self.clamp_cursor();
        let pos = self.byte_pos();
        self.text.insert(pos, c);
        self.cursor += 1;
        true
    }

    pub fn backspace(&mut self) -> bool {
        self.clamp_cursor();
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        let pos = self.byte_pos();
        let end = self.text[pos..]
            .graphemes(true)
            .next()
            .map(|g| pos + g.len())
            .unwrap_or_else(|| self.text.len());
        self.text.drain(pos..end);
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        self.clamp_cursor();
        let pos = self.byte_pos();
        if pos >= self.text.len() {
            return false;
        }
        let end = self.text[pos..]
            .graphemes(true)
            .next()
            .map(|g| pos + g.len())
            .unwrap_or_else(|| self.text.len());
        self.text.drain(pos..end);
        true
    }

    pub fn cursor_left(&mut self) {
        self.clamp_cursor();
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn cursor_right(&mut self) {
        self.clamp_cursor();
        if self.cursor < self.grapheme_count() {
            self.cursor += 1;
        }
    }

    pub fn cursor_start(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.grapheme_count();
    }

    pub fn delete_word_backward(&mut self) -> bool {
        self.clamp_cursor();
        let pos = self.byte_pos();
        if pos == 0 {
            return false;
        }
        let text = &self.text[..pos];
        let word_start = text
            .grapheme_indices(true)
            .rev()
            .skip_while(|&(_, g)| g.chars().all(|c| c.is_whitespace()))
            .find(|&(_, g)| g.chars().all(|c| c.is_whitespace()))
            .map(|(i, g)| i + g.len())
            .unwrap_or(0);
        let removed_graphemes = text[word_start..].graphemes(true).count();
        self.text.drain(word_start..pos);
        self.cursor = self.cursor.saturating_sub(removed_graphemes);
        removed_graphemes > 0
    }

    pub fn drain_to_start(&mut self) {
        self.clamp_cursor();
        let pos = self.byte_pos();
        self.text.drain(..pos);
        self.cursor = 0;
    }
}

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

fn format_system_time(modified: SystemTime) -> Option<String> {
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    let ts = i64::try_from(duration.as_secs()).ok()?;
    let dt = DateTime::from_timestamp(ts, 0)?;
    Some(
        dt.with_timezone(&chrono::Local)
            .format("%d-%m-%y %H:%M")
            .to_string(),
    )
}

pub fn format_time(modified: SystemTime) -> String {
    format_system_time(modified).unwrap_or_else(|| "??-??-?? ??:??".to_string())
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
    pub time_str: String,
    pub size_str: String,
    pub name_width: usize,
    pub size_width: usize,
    pub time_width: usize,
    pub category: FileCategory,
}

pub(crate) fn compute_category(cha: &Cha, name: &str) -> FileCategory {
    use crate::app::file_type as ft;
    if cha.is_link() {
        return FileCategory::Symlink;
    }
    if cha.is_dir() {
        return FileCategory::Dir;
    }
    if ft::is_source_code(name) {
        return FileCategory::Code;
    }
    if ft::is_config(name) {
        return FileCategory::Config;
    }
    if ft::is_archive(name) {
        return FileCategory::Archive;
    }
    if ft::is_image(name) {
        return FileCategory::Image;
    }
    if ft::is_video(name) {
        return FileCategory::Video;
    }
    if ft::is_audio(name) {
        return FileCategory::Audio;
    }
    if ft::is_document(name) {
        return FileCategory::Document;
    }
    if ft::is_font(name) {
        return FileCategory::Font;
    }
    if cha.is_executable() {
        return FileCategory::Executable;
    }
    FileCategory::Other
}

// ============================================================================
// 2a. SortOptions struct definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SortOptions {
    #[serde(default = "default_true")]
    pub dir_first: bool,
    #[serde(default, alias = "sort_sensitive")]
    pub sensitive: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        Self {
            dir_first: true,
            sensitive: false,
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
// 3. PanelListing struct definition
// ============================================================================

/// Listing state for one file panel — entries, unfiltered set, and cache.
///
/// Tracks both the filtered (visible) entries and the full unfiltered set,
/// along with a path→index lookup and dirty flags for lazy rebuild.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelListing {
    pub entries: Vec<FileEntry>,
    pub unfiltered_entries: Vec<FileEntry>,
    pub path_index: HashMap<PathBuf, usize>,
    pub needs_rebuild: bool,
    pub unfiltered_dirty: bool,
}

impl PanelListing {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            unfiltered_entries: Vec::new(),
            path_index: HashMap::new(),
            needs_rebuild: false,
            unfiltered_dirty: true,
        }
    }

    pub fn set_unfiltered(&mut self, entries: Vec<FileEntry>) {
        self.path_index.clear();
        for (i, entry) in entries.iter().enumerate() {
            self.path_index.insert(entry.path.clone(), i);
        }
        self.unfiltered_entries = entries;
        self.unfiltered_dirty = false;
        self.needs_rebuild = false;
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.entries = entries;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.unfiltered_entries.clear();
        self.path_index.clear();
        self.needs_rebuild = false;
        self.unfiltered_dirty = true;
    }

    pub fn mark_dirty(&mut self) {
        self.needs_rebuild = true;
    }

    pub fn mark_unfiltered_dirty(&mut self) {
        self.unfiltered_dirty = true;
    }
}

impl Default for PanelListing {
    fn default() -> Self {
        Self::new()
    }
}

/// State for one file panel (left or right).
///
/// Combines listing state (entries, path), navigation (cursor, scroll),
/// display options (sort, hidden, permissions), filter, and selection.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    // --- Listing state ---
    pub path: PathBuf,
    pub canonical_path: Option<PathBuf>,
    pub listing: PanelListing,

    // --- Navigation state ---
    pub cursor: usize,
    pub scroll_offset: usize,
    pub history: Vec<PathBuf>,

    // --- Display options ---
    pub sort_mode: SortMode,
    pub sort_options: SortOptions,
    pub listing_mode: ListingMode,
    pub show_hidden: bool,
    pub show_permissions: bool,

    // --- Filter ---
    pub filter: Option<String>,

    // --- Selection ---
    pub selected_count: usize,
    pub selected_size: u64,
    pub total_size: u64,

    // --- Error ---
    pub last_error: Option<String>,
}

// ============================================================================
// 4. ActivePanel enum definition
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Left,
    Right,
}

impl ActivePanel {
    pub fn toggle(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
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
        action: InputAction,
    },
    Error(String),
    Help {
        message: String,
        scroll_offset: usize,
    },
    Progress {
        message: String,
        progress_fraction: f32,
        cancellable: bool,
    },
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

/// Top-level application state.
///
/// Fields are grouped into domain sections. Each section represents a
/// cohesive concern; future refactoring may extract some sections into
/// dedicated types.
///
/// # Sections
///
/// | Section              | Purpose                                          |
/// |----------------------|--------------------------------------------------|
/// | Panels               | Left/right panels, active selection              |
/// | Mode                 | Current mode, previous mode, quit flag           |
/// | Dialog               | Input, selection, pending action                 |
/// | Command line         | Input, history, draft                            |
/// | Search               | Query string, cursor                             |
/// | Status               | Status bar message                               |
/// | Menu                 | User menu, hotlist picker, cached strings        |
/// | Directory tree       | Tree root, entries, selection, scroll            |
/// | Mouse / drag         | Click detection, drag state, scroll acceleration |
/// | Theme                | Color palette, viewer animation                  |
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    // --- Panels ---
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub active_panel: ActivePanel,

    // --- Mode ---
    pub mode: AppMode,
    pub prev_mode: Option<AppMode>,
    pub should_quit: bool,

    // --- Dialog ---
    pub dialog_input: TextInput,
    pub dialog_selection: usize,
    pub pending_action: Option<PendingAction>,

    // --- Command line ---
    pub command_line: TextInput,
    pub command_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub command_draft: String,

    // --- Search ---
    pub search_query: String,
    pub search_cursor: usize,

    // --- Status ---
    pub status_message: Option<String>,

    // --- Menu ---
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

    // --- Directory tree ---
    pub tree_root: PathBuf,
    pub tree_entries: Vec<TreeEntry>,
    pub tree_selected: usize,
    pub tree_scroll: usize,

    // --- Mouse / drag ---
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_position: Option<(u16, u16)>,
    pub drag_active: bool,
    pub drag_source_pane: ActivePanel,
    pub drag_source_path: PathBuf,
    pub drag_source_name: String,
    pub drag_current_row: u16,
    pub drag_current_col: u16,
    pub scroll_accel: u8,
    pub last_scroll_time: Option<std::time::Instant>,
    pub drag_anchor_index: Option<usize>,

    // --- Theme ---
    pub theme_colors: ColorPalette,
    pub viewer_spinner_frame: Option<std::time::Instant>,
}

// ============================================================================
// ViewMode enum
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Image,
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

impl PendingAction {
    pub fn set_overwrite(&mut self) {
        match self {
            Self::Copy { overwrite, .. } | Self::Move { overwrite, .. } => {
                *overwrite = true;
            }
            Self::Delete { .. } => {}
        }
    }
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
        let perms = self.cha.mode.permissions();
        if v {
            self.cha.mode = ChaMode::new(0o040000 | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        } else if self.cha.is_dir() {
            self.cha.mode = ChaMode::new(0o100000 | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        }
        self
    }
    pub fn is_symlink(mut self, v: bool) -> Self {
        let perms = self.cha.mode.permissions();
        if v {
            self.cha.mode = ChaMode::new(0o120000 | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        } else if self.cha.is_link() {
            self.cha.mode = ChaMode::new(0o100000 | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
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
        let (time_str, size_str, name_width, size_width, time_width) =
            FileEntry::cached_fields(&self.cha, &self.name);
        let category = compute_category(&self.cha, &self.name);
        FileEntry {
            name: self.name,
            path: self.path,
            cha: self.cha,
            owner: self.owner,
            group: self.group,
            selected: self.selected,
            mime_type: self.mime_type,
            time_str,
            size_str,
            name_width,
            size_width,
            time_width,
            category,
        }
    }
}

// ============================================================================
// FileEntry implementation
// ============================================================================

impl FileEntry {
    pub fn cached_fields(cha: &Cha, name: &str) -> (String, String, usize, usize, usize) {
        let time_str = format_time(cha.mtime().unwrap_or(std::time::UNIX_EPOCH));
        let size_str = if cha.is_dir() {
            "     <DIR>".to_string()
        } else {
            format!("{:>10}", format_size(cha.len()))
        };
        let name_width = UnicodeWidthStr::width(name);
        let size_width = UnicodeWidthStr::width(size_str.as_str());
        let time_width = UnicodeWidthStr::width(time_str.as_str());
        (time_str, size_str, name_width, size_width, time_width)
    }

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

    /// Returns the file size in bytes.
    pub fn size(&self) -> u64 {
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
        self.category
    }

    pub fn display_size(&self) -> String {
        Self::format_size(self.size())
    }

    pub fn format_size(size: u64) -> String {
        format!("{:>6}", crate::app::types::format_size(size))
    }

    pub fn display_permissions(&self) -> String {
        Self::display_permissions_raw(self.mode_bits())
    }

    pub fn display_permissions_raw(mode: u32) -> String {
        use crate::fs::cha::ChaMode;
        ChaMode::new(mode).to_string()
    }

    pub fn display_modified(&self) -> String {
        let Some(mtime) = self.cha.mtime else {
            return "Unknown".to_string();
        };
        format_system_time(mtime).unwrap_or_else(|| "Unknown".to_string())
    }
}

// ============================================================================
// PanelState implementation
// ============================================================================

impl PanelState {
    pub fn new(path: PathBuf) -> Self {
        let canonical_path = path.canonicalize().ok();
        Self {
            path,
            canonical_path,
            listing: PanelListing::new(),
            cursor: 0,
            scroll_offset: 0,
            history: Vec::new(),
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
        }
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.canonical_path = path.canonicalize().ok();
        self.path = path;
    }

    fn update_selection_stats(&mut self, size: u64, selected: bool) {
        if selected {
            self.selected_count = self.selected_count.saturating_add(1);
            self.selected_size = self.selected_size.saturating_add(size);
        } else {
            self.selected_count = self.selected_count.saturating_sub(1);
            self.selected_size = self.selected_size.saturating_sub(size);
        }
    }

    pub fn current_entry(&self) -> Option<&FileEntry> {
        if self.cursor < self.listing.entries.len() {
            Some(&self.listing.entries[self.cursor])
        } else {
            None
        }
    }

    pub fn toggle_selection(&mut self) {
        if let Some(entry) = self.listing.entries.get_mut(self.cursor) {
            if entry.name == ".." {
                return;
            }
            entry.selected = !entry.selected;
            let size = entry.size();
            let selected = entry.selected;
            let path = entry.path.clone();
            self.update_selection_stats(size, selected);
            self.set_unfiltered_selection(&path, selected);
        }
    }

    pub fn set_selection_at(&mut self, index: usize, selected: bool) {
        if let Some(entry) = self.listing.entries.get_mut(index) {
            if entry.name == ".." || entry.selected == selected {
                return;
            }
            entry.selected = selected;
            let size = entry.size();
            let path = entry.path.clone();
            self.update_selection_stats(size, selected);
            self.set_unfiltered_selection(&path, selected);
        }
    }

    pub fn toggle_selection_at(&mut self, index: usize) {
        let selected = self.listing.entries.get(index).is_some_and(|e| !e.selected);
        self.set_selection_at(index, selected);
    }

    fn set_unfiltered_selection(&mut self, path: &Path, selected: bool) {
        if let Some(&idx) = self.listing.path_index.get(path) {
            if let Some(ue) = self.listing.unfiltered_entries.get_mut(idx) {
                ue.selected = selected;
            }
        } else if let Some(ue) = self
            .listing
            .unfiltered_entries
            .iter_mut()
            .find(|e| e.path == *path)
        {
            ue.selected = selected;
        }
    }

    pub fn sync_unfiltered_selection(&mut self) {
        if self.listing.unfiltered_entries.is_empty() {
            return;
        }

        let selection: HashMap<_, _> = self
            .listing
            .entries
            .iter()
            .map(|entry| (entry.path.as_path(), entry.selected))
            .collect();

        for entry in &mut self.listing.unfiltered_entries {
            if let Some(selected) = selection.get(entry.path.as_path()) {
                entry.selected = *selected;
            }
        }
    }

    pub fn selected_entries(&self) -> Vec<&FileEntry> {
        let source = if self.listing.unfiltered_entries.is_empty() {
            &self.listing.entries
        } else {
            &self.listing.unfiltered_entries
        };
        source.iter().filter(|e| e.selected).collect()
    }

    pub fn clear_selection(&mut self) {
        for entry in &mut self.listing.entries {
            entry.selected = false;
        }
        for entry in &mut self.listing.unfiltered_entries {
            entry.selected = false;
        }
        self.selected_count = 0;
        self.selected_size = 0;
    }

    pub fn recalculate_selection_stats(&mut self) {
        self.selected_count = 0;
        self.selected_size = 0;
        self.total_size = 0;
        let source = if self.listing.unfiltered_entries.is_empty() {
            &self.listing.entries
        } else {
            &self.listing.unfiltered_entries
        };
        for entry in source {
            self.total_size = self.total_size.saturating_add(entry.size());
            if entry.selected {
                self.selected_count = self.selected_count.saturating_add(1);
                self.selected_size = self.selected_size.saturating_add(entry.size());
            }
        }
    }

    pub fn move_cursor_up(&mut self, max_height: usize) {
        if self.listing.entries.is_empty() {
            return;
        }

        if self.cursor == 0 {
            self.cursor = self.listing.entries.len().saturating_sub(1);
            if max_height > 0 {
                self.scroll_offset = self.listing.entries.len().saturating_sub(max_height);
            }
        } else {
            self.cursor = self.cursor.saturating_sub(1);
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            }
        }
    }

    pub fn move_cursor_down(&mut self, max_height: usize) {
        if self.listing.entries.is_empty() {
            return;
        }

        let max_index = self.listing.entries.len().saturating_sub(1);

        if self.cursor >= max_index {
            self.cursor = 0;
            self.scroll_offset = 0;
        } else {
            self.cursor += 1;
            if max_height > 0 && self.cursor >= self.scroll_offset + max_height {
                self.scroll_offset = self.cursor.saturating_sub(max_height) + 1;
            }
        }
    }

    pub fn ensure_cursor_visible(&mut self, visible_height: usize) {
        let max_scroll = self.listing.entries.len().saturating_sub(1);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
        if self.scroll_offset > self.cursor {
            self.scroll_offset = self.cursor;
        }
        if visible_height > 0 && self.cursor >= self.scroll_offset.saturating_add(visible_height) {
            self.scroll_offset = self.cursor.saturating_sub(visible_height).saturating_add(1);
        }
    }

    pub fn set_cursor(&mut self, idx: usize) {
        if self.listing.entries.is_empty() {
            self.cursor = 0;
            self.scroll_offset = 0;
        } else {
            self.cursor = idx.min(self.listing.entries.len() - 1);
        }
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.listing.set_unfiltered(entries.clone());
        self.listing.set_entries(entries);
        self.cursor = 0;
        self.scroll_offset = 0;
        self.recalculate_selection_stats();
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
        self.scroll_offset = 0;
    }

    pub fn mark_unfiltered_dirty(&mut self) {
        self.listing.mark_unfiltered_dirty();
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn is_empty(&self) -> bool {
        self.listing.entries.is_empty()
    }
}

// ================================================================================
// AppState implementation
// ================================================================================

impl AppState {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

        Self {
            // Panels
            left_panel: PanelState::new(current_dir.clone()),
            right_panel: PanelState::new(current_dir.clone()),
            active_panel: ActivePanel::Left,
            // Mode
            mode: AppMode::Normal,
            prev_mode: None,
            should_quit: false,
            // Dialog
            dialog_input: TextInput::default(),
            dialog_selection: 0,
            pending_action: None,
            // Command line
            command_line: TextInput::default(),
            command_history: VecDeque::new(),
            history_index: None,
            command_draft: String::new(),
            // Search
            search_query: String::new(),
            search_cursor: 0,
            // Status
            status_message: None,
            // Menu
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
            // Directory tree
            tree_root: PathBuf::new(),
            tree_entries: Vec::new(),
            tree_selected: 0,
            tree_scroll: 0,
            // Mouse / drag
            last_click_time: None,
            last_click_position: None,
            drag_active: false,
            drag_source_pane: ActivePanel::Left,
            drag_source_path: PathBuf::new(),
            drag_source_name: String::new(),
            drag_current_row: 0,
            drag_current_col: 0,
            scroll_accel: 0,
            last_scroll_time: None,
            drag_anchor_index: None,
            // Theme
            theme_colors: crate::ui::theme::DEFAULT_COLORS,
            viewer_spinner_frame: None,
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

    pub fn rebuild_hotlist_cache(&mut self) {
        self.cached_hotlist_strings = self
            .directory_hotlist
            .iter()
            .map(|p| p.display().to_string())
            .collect();
    }

    pub fn rebuild_user_menu_cache(&mut self) {
        self.cached_user_menu_strings = self
            .user_menu_entries
            .iter()
            .map(|e| format!("{}  {}", e.hotkey, e.title))
            .collect();
    }

    pub fn hotlist_push(&mut self, path: PathBuf) {
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

    pub fn hotlist_set(&mut self, hotlist: Vec<PathBuf>) {
        self.directory_hotlist = hotlist;
        self.rebuild_hotlist_cache();
    }

    pub fn user_menu_set(&mut self, entries: Vec<MenuEntry>) {
        self.user_menu_entries = entries;
        self.rebuild_user_menu_cache();
    }

    pub fn enter_command_line_mode(&mut self) {
        self.command_line.clear();
        self.history_index = None;
        self.prev_mode = None;
        self.mode = AppMode::CommandLine;
    }

    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        match self.active_panel {
            ActivePanel::Left => &mut self.right_panel,
            ActivePanel::Right => &mut self.left_panel,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn reset_drag_state(&mut self) {
        self.drag_active = false;
        self.drag_source_path.clear();
        self.drag_source_name.clear();
        self.drag_current_row = 0;
        self.drag_current_col = 0;
        self.drag_anchor_index = None;
    }

    pub fn panel_mut(&mut self, panel: ActivePanel) -> &mut PanelState {
        match panel {
            ActivePanel::Left => &mut self.left_panel,
            ActivePanel::Right => &mut self.right_panel,
        }
    }

    pub fn panel(&self, panel: ActivePanel) -> &PanelState {
        match panel {
            ActivePanel::Left => &self.left_panel,
            ActivePanel::Right => &self.right_panel,
        }
    }
}

// Default implementation for AppState
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn restore_prev_mode(state: &mut AppState) {
    state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
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
        assert_eq!(entry.display_size(), " 500 B");
    }

    #[test]
    fn test_file_entry_display_size_kilobytes() {
        let entry = create_test_entry("test.txt", false, 1500, 0o644, false);
        assert_eq!(entry.display_size(), "1.5 KB");
    }

    #[test]
    fn test_file_entry_display_size_megabytes() {
        let entry = create_test_entry("test.txt", false, 1_500_000, 0o644, false);
        assert_eq!(entry.display_size(), "1.4 MB");
    }

    #[test]
    fn test_file_entry_display_size_gigabytes() {
        let entry = create_test_entry("test.txt", false, 1_500_000_000, 0o644, false);
        assert_eq!(entry.display_size(), "1.4 GB");
    }

    #[test]
    fn test_file_entry_display_size_zero() {
        let entry = create_test_entry("test.txt", false, 0, 0o644, false);
        assert_eq!(entry.display_size(), "   0 B");
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
        assert_eq!(panel.listing.entries.len(), 0);
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
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        assert_eq!(panel.current_entry().unwrap().name, "file1.txt");
    }

    #[test]
    fn test_panel_state_current_entry_out_of_bounds() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 5;
        assert!(panel.current_entry().is_none());
    }

    #[test]
    fn test_panel_state_toggle_selection_toggle_on() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.toggle_selection();
        assert!(panel.listing.entries[0].selected);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 100);
    }

    #[test]
    fn test_panel_state_toggle_selection_toggle_off() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));
        panel.cursor = 0;
        assert!(panel.listing.entries[0].selected);
        panel.toggle_selection();
        assert!(!panel.listing.entries[0].selected);
        assert_eq!(panel.selected_count, 0);
        assert_eq!(panel.selected_size, 0);
    }

    #[test]
    fn test_panel_state_set_selection_at_on() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));

        panel.set_selection_at(0, true);

        assert!(panel.listing.entries[0].selected);
        assert_eq!(panel.selected_count, 1);
        assert_eq!(panel.selected_size, 100);
    }

    #[test]
    fn test_panel_state_set_selection_at_off() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));

        panel.set_selection_at(0, false);

        assert!(!panel.listing.entries[0].selected);
        assert_eq!(panel.selected_count, 0);
        assert_eq!(panel.selected_size, 0);
    }

    #[test]
    fn test_panel_state_sync_unfiltered_selection() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = vec![
            create_test_entry("file1.txt", false, 100, 0o644, true),
            create_test_entry("file2.txt", false, 200, 0o644, false),
        ];
        panel.listing.unfiltered_entries = vec![
            create_test_entry("file1.txt", false, 100, 0o644, false),
            create_test_entry("file2.txt", false, 200, 0o644, true),
            create_test_entry("file3.txt", false, 300, 0o644, true),
        ];

        panel.sync_unfiltered_selection();

        assert!(panel.listing.unfiltered_entries[0].selected);
        assert!(!panel.listing.unfiltered_entries[1].selected);
        assert!(panel.listing.unfiltered_entries[2].selected);
    }

    #[test]
    fn test_panel_state_selected_entries() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, true));
        panel
            .listing
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel
            .listing
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
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .listing
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel.cursor = 1;
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_up_at_top() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_down() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .listing
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
            .listing
            .entries
            .push(create_test_entry("file1.txt", false, 100, 0o644, false));
        panel
            .listing
            .entries
            .push(create_test_entry("file2.txt", false, 200, 0o644, false));
        panel.cursor = 1;
        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_down_scroll() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.listing.entries.push(create_test_entry(
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
        assert_eq!(TextInput::default().cursor, 0);
        assert_eq!(PickerState::default().picker_selected, 0);
        assert!(DirectoryTreeState::default().tree_entries.is_empty());
    }

    #[test]
    fn test_text_input_mutations_clamp_cursor() {
        let mut input = TextInput {
            text: "ąb".to_string(),
            cursor: 99,
        };

        assert!(input.backspace());
        assert_eq!(input.text, "ą");
        assert_eq!(input.cursor, 1);

        assert!(input.insert_char('x'));
        assert_eq!(input.text, "ąx");
        assert_eq!(input.cursor, 2);

        input.cursor = 99;
        assert!(input.delete_word_backward());
        assert_eq!(input.text, "");
        assert_eq!(input.cursor, 0);
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
        panel.set_path(PathBuf::from("/modified"));
        assert_eq!(state.left_panel.path, PathBuf::from("/modified"));
    }

    #[test]
    fn test_app_state_active_panel_mut_right() {
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Right;
        let panel = state.active_panel_mut();
        panel.set_path(PathBuf::from("/modified"));
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
            action: InputAction::Rename,
        };
        if let DialogKind::Input { prompt, action } = dialog {
            assert_eq!(prompt, "Enter name:");
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
        let dialog = DialogKind::Progress {
            message: "Copying...".to_string(),
            progress_fraction: 0.5,
            cancellable: true,
        };
        if let DialogKind::Progress {
            message,
            progress_fraction,
            cancellable,
        } = dialog
        {
            assert_eq!(message, "Copying...");
            assert_eq!(progress_fraction, 0.5);
            assert!(cancellable);
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
            panel.listing.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 3;
        panel.scroll_offset = 5;
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 2);
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_panel_state_move_cursor_up_no_scroll_when_visible() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{}.txt", i),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 5;
        panel.scroll_offset = 3;
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 4);
        assert_eq!(panel.scroll_offset, 3);
    }

    #[test]
    fn test_panel_state_move_cursor_down_scroll_new_formula() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..10 {
            panel.listing.entries.push(create_test_entry(
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
            panel.listing.entries.push(create_test_entry(
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
            panel.listing.entries.push(create_test_entry(
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
            panel.listing.entries.push(create_test_entry(
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
            panel.listing.entries.push(create_test_entry(
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
            panel.listing.entries.push(create_test_entry(
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
        panel.listing.entries = vec![
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
        panel.listing.entries = vec![
            create_test_entry("small.txt", false, 50, 0o644, false),
            create_test_entry("big.txt", false, 5000, 0o644, true),
        ];
        panel.recalculate_selection_stats();
        assert_eq!(panel.total_size, 5050);
        assert_eq!(panel.selected_size, 5000);
    }

    #[test]
    fn test_panel_state_empty_entries_cursor_scroll_zero() {
        let panel = PanelState::new(PathBuf::from("/test"));
        assert_eq!(panel.listing.entries.len(), 0);
        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_panel_state_single_item_cursor() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = vec![create_test_entry("only.txt", false, 10, 0o644, false)];

        assert_eq!(panel.cursor, 0);
        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 0);
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_cursor_stays_at_last_after_entry_removal() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = vec![
            create_test_entry("a.txt", false, 10, 0o644, false),
            create_test_entry("b.txt", false, 10, 0o644, false),
            create_test_entry("c.txt", false, 10, 0o644, false),
        ];
        panel.cursor = 2;

        panel.listing.entries.truncate(1);

        let max_index = panel.listing.entries.len().saturating_sub(1);
        panel.cursor = panel.cursor.min(max_index);

        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_move_cursor_down_clamped_at_last() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = vec![
            create_test_entry("a.txt", false, 10, 0o644, false),
            create_test_entry("b.txt", false, 10, 0o644, false),
        ];
        panel.cursor = 1;

        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 0);

        panel.move_cursor_down(10);
        assert_eq!(panel.cursor, 1);
    }

    #[test]
    fn test_panel_state_move_cursor_up_clamped_at_zero() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = vec![create_test_entry("a.txt", false, 10, 0o644, false)];
        panel.cursor = 0;

        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 0);
        panel.move_cursor_up(10);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_state_current_entry_empty_returns_none() {
        let panel = PanelState::new(PathBuf::from("/test"));
        assert!(panel.current_entry().is_none());
    }

    #[test]
    fn test_panel_state_scroll_offset_with_many_entries() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.listing.entries = (0..100)
            .map(|i| create_test_entry(&format!("file{i:03}.txt"), false, 10, 0o644, false))
            .collect();
        panel.cursor = 99;

        let visible_height = 20;
        panel.move_cursor_down(visible_height);

        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.scroll_offset, 0);
        assert!(
            panel.scroll_offset + visible_height > panel.cursor,
            "cursor must be visible within scroll window"
        );
    }

    #[test]
    fn file_entry_builder_clears_dir_target_follow_when_type_changes() {
        let dir_entry = FileEntry::builder()
            .name("d")
            .path(PathBuf::from("d"))
            .is_dir(true)
            .build();
        let mut cha = dir_entry.cha;
        cha.kind.insert(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        assert!(cha.kind.contains(ChaKind::DIR_TARGET));
        assert!(cha.kind.contains(ChaKind::FOLLOW));

        let cleared = FileEntry::builder()
            .name("d")
            .path(PathBuf::from("d"))
            .cha(cha)
            .is_dir(false)
            .build();
        assert!(!cleared.cha.kind.contains(ChaKind::DIR_TARGET));
        assert!(!cleared.cha.kind.contains(ChaKind::FOLLOW));

        let link_entry = FileEntry::builder()
            .name("l")
            .path(PathBuf::from("l"))
            .is_symlink(true)
            .build();
        let mut cha = link_entry.cha;
        cha.kind.insert(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        assert!(cha.kind.contains(ChaKind::DIR_TARGET));
        assert!(cha.kind.contains(ChaKind::FOLLOW));

        let cleared = FileEntry::builder()
            .name("l")
            .path(PathBuf::from("l"))
            .cha(cha)
            .is_symlink(false)
            .build();
        assert!(!cleared.cha.kind.contains(ChaKind::DIR_TARGET));
        assert!(!cleared.cha.kind.contains(ChaKind::FOLLOW));
    }

    #[test]
    fn mtime_none_displays_unknown_and_sorts_after_known() {
        let no_mtime = FileEntry::builder()
            .name("unknown.txt")
            .path(PathBuf::from("unknown.txt"))
            .build();
        assert_eq!(no_mtime.display_modified(), "Unknown");

        let with_mtime = FileEntry::builder()
            .name("known.txt")
            .path(PathBuf::from("known.txt"))
            .modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .build();

        let mut entries = vec![no_mtime, with_mtime];
        crate::ops::sorting::sort_entries(
            &mut entries,
            SortMode::ModTimeDesc,
            SortOptions::default(),
        );
        assert_eq!(entries[0].name, "known.txt");
        assert_eq!(entries[1].name, "unknown.txt");
    }

    #[test]
    fn test_move_cursor_up_wraps_to_last_entry() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..5 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{i}.txt"),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 0;
        panel.move_cursor_up(3);
        assert_eq!(panel.cursor, 4);
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_move_cursor_up_wraps_with_single_entry() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.move_cursor_up(3);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_move_cursor_down_wraps_to_first_entry() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..5 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{i}.txt"),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 4;
        panel.move_cursor_down(3);
        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_move_cursor_down_wraps_with_single_entry() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel
            .listing
            .entries
            .push(create_test_entry("file.txt", false, 100, 0o644, false));
        panel.cursor = 0;
        panel.move_cursor_down(3);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_move_cursor_up_wrap_then_down_wrap() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..5 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{i}.txt"),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 0;
        panel.move_cursor_up(5);
        assert_eq!(panel.cursor, 4);
        panel.move_cursor_down(5);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_move_cursor_down_wrap_with_many_entries_scroll_check() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..20 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{i}.txt"),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 19;
        panel.move_cursor_down(5);
        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_move_cursor_up_wrap_with_many_entries_scroll_check() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        for i in 0..20 {
            panel.listing.entries.push(create_test_entry(
                &format!("file{i}.txt"),
                false,
                100,
                0o644,
                false,
            ));
        }
        panel.cursor = 0;
        panel.move_cursor_up(5);
        assert_eq!(panel.cursor, 19);
        assert_eq!(panel.scroll_offset, 15);
    }
}
