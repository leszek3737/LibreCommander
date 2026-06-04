use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::file_entry::FileEntry;
use super::sorting::{ListingMode, SortMode, SortOptions};

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

#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    pub(crate) path: PathBuf,
    pub(crate) canonical_path: Option<PathBuf>,
    pub listing: PanelListing,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub(crate) history: Vec<PathBuf>,
    pub(crate) sort_mode: SortMode,
    pub(crate) sort_options: SortOptions,
    pub(crate) listing_mode: ListingMode,
    pub(crate) show_hidden: bool,
    pub(crate) show_permissions: bool,
    pub(crate) filter: Option<String>,
    pub(crate) selected_count: usize,
    pub(crate) selected_size: u64,
    pub(crate) total_size: u64,
    pub(crate) last_error: Option<String>,
}

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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn canonical_path(&self) -> Option<&Path> {
        self.canonical_path.as_deref()
    }

    pub fn set_canonical_path(&mut self, canonical: Option<PathBuf>) {
        self.canonical_path = canonical;
    }

    pub fn history(&self) -> &[PathBuf] {
        &self.history
    }

    const MAX_HISTORY: usize = 256;

    pub fn push_history(&mut self, path: PathBuf) {
        if self.history.len() >= Self::MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(path);
    }

    pub fn pop_history(&mut self) -> Option<PathBuf> {
        self.history.pop()
    }

    pub fn sort_mode(&self) -> SortMode {
        self.sort_mode
    }

    pub fn set_sort_mode(&mut self, mode: SortMode) {
        self.sort_mode = mode;
    }

    pub fn sort_options(&self) -> &SortOptions {
        &self.sort_options
    }

    pub fn set_sort_options(&mut self, opts: SortOptions) {
        self.sort_options = opts;
    }

    pub fn listing_mode(&self) -> ListingMode {
        self.listing_mode
    }

    pub fn set_listing_mode(&mut self, mode: ListingMode) {
        self.listing_mode = mode;
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
    }

    pub fn show_permissions(&self) -> bool {
        self.show_permissions
    }

    pub fn set_show_permissions(&mut self, show: bool) {
        self.show_permissions = show;
    }

    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    pub fn set_filter(&mut self, f: Option<String>) {
        self.filter = f;
    }

    pub fn selected_count(&self) -> usize {
        self.selected_count
    }

    pub fn set_selected_count(&mut self, count: usize) {
        self.selected_count = count;
    }

    pub fn selected_size(&self) -> u64 {
        self.selected_size
    }

    pub fn set_selected_size(&mut self, size: u64) {
        self.selected_size = size;
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn set_last_error(&mut self, err: Option<String>) {
        self.last_error = err;
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
        self.listing.entries.get(self.cursor)
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

        let index: HashMap<PathBuf, usize> = self
            .listing
            .unfiltered_entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();

        for entry in &self.listing.entries {
            if let Some(&idx) = index.get(&entry.path)
                && let Some(ue) = self.listing.unfiltered_entries.get_mut(idx)
            {
                ue.selected = entry.selected;
            }
        }
    }

    fn source_entries(&self) -> &[FileEntry] {
        if self.listing.unfiltered_entries.is_empty() {
            &self.listing.entries
        } else {
            &self.listing.unfiltered_entries
        }
    }

    pub fn selected_entries(&self) -> Vec<&FileEntry> {
        self.source_entries()
            .iter()
            .filter(|e| e.selected)
            .collect()
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
        let mut selected_count: usize = 0;
        let mut selected_size: u64 = 0;
        let mut total_size: u64 = 0;
        for entry in self.source_entries() {
            total_size = total_size.saturating_add(entry.size());
            if entry.selected {
                selected_count = selected_count.saturating_add(1);
                selected_size = selected_size.saturating_add(entry.size());
            }
        }
        self.selected_count = selected_count;
        self.selected_size = selected_size;
        self.total_size = total_size;
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

        let max_index = self.listing.entries.len() - 1;

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

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.listing.set_unfiltered(entries.clone());
        self.listing.set_entries(entries);
        self.cursor = 0;
        self.scroll_offset = 0;
        self.recalculate_selection_stats();
    }

    pub fn mark_dirty(&mut self) {
        self.listing.mark_dirty();
    }

    pub fn mark_unfiltered_dirty(&mut self) {
        self.listing.mark_unfiltered_dirty();
    }
}

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
