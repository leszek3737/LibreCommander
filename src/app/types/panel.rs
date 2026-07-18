use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use super::file_entry::FileEntry;
use super::sorting::{ListingMode, SortMode, SortOptions};

use crate::ops::CompiledPattern;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListingState {
    #[default]
    Clean,
    NeedsRebuild,
    NeedsFullRead,
}

/// Outcome of a single-entry selection toggle.
///
/// The cursor toggle used to be a silent no-op for the `..` parent link and for
/// an empty listing, leaving callers unable to distinguish "nothing happened"
/// from "selection flipped off". Encoding the result as a type makes those
/// states explicit (type-driven: invalid/ignored cases are not silently
/// swallowed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleResult {
    /// Selection flipped; payload is the entry's new `selected` state.
    Toggled(bool),
    /// The cursor entry is the `..` parent link; selection is not allowed.
    SkippedParent,
    /// No entry exists under the cursor (empty or out-of-range view).
    NoEntry,
}

/// A directory listing backed by a single owning store.
///
/// `unfiltered_entries` is the sole owner of every [`FileEntry`] (and the sole
/// home of per-entry selection state). `entries` is the *filtered view*: a list
/// of indices into `unfiltered_entries`, in display order. Storing the view as
/// indices (rather than a second `Vec<FileEntry>`) removes the dual-store
/// duplication whose selection had to be hand-synced on every toggle/rebuild —
/// the historic source of selection-desync bugs.
///
/// `path_index` maps each entry path to its slot in `unfiltered_entries` and is
/// kept consistent by every mutator below; it is the canonical path lookup used
/// by the watcher upsert/remove fast paths.
///
/// Invariant: indices in `entries` either point at a live slot in
/// `unfiltered_entries` or are transiently stale after an in-place
/// `upsert`/`remove` — in which case the owning panel is marked dirty and the
/// view is rebuilt before it is read again. Reads defensively skip dead indices,
/// so a stale index can never panic.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelListing {
    unfiltered_entries: Vec<FileEntry>,
    entries: Vec<usize>,
    path_index: HashMap<PathBuf, usize>,
    state: ListingState,
}

impl PanelListing {
    pub fn new() -> Self {
        Self {
            unfiltered_entries: Vec::new(),
            entries: Vec::new(),
            path_index: HashMap::new(),
            state: ListingState::NeedsFullRead,
        }
    }

    /// Replace the backing store. Rebuilds `path_index` and invalidates the
    /// filtered view (rebuild it afterwards via
    /// [`set_filtered`](Self::set_filtered) or
    /// [`set_filtered_all`](Self::set_filtered_all)).
    pub fn set_unfiltered(&mut self, entries: Vec<FileEntry>) {
        self.path_index.clear();
        self.path_index.reserve(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            // Owned-key clone is required: the HashMap key must outlive `entries`,
            // which is moved into the backing store on the next line. This is the
            // index's own cost; the *filtered view* no longer pays a clone since it
            // stores indices, not entry copies.
            self.path_index.insert(entry.path.clone(), i);
        }
        self.unfiltered_entries = entries;
        self.entries.clear();
        self.state = ListingState::Clean;
    }

    /// Rebuild the filtered view from an ordered slice of entries, mapping each
    /// back to its slot in the backing store by path. Entries whose path is not
    /// in the store are skipped.
    ///
    /// Selection is intentionally NOT copied from `ordered` (which may be a stale
    /// clone): it lives solely in `unfiltered_entries`, so the filtered view can
    /// never carry a divergent selection.
    pub fn set_filtered(&mut self, ordered: &[FileEntry]) {
        self.ensure_index();
        self.entries.clear();
        self.entries.reserve(ordered.len());
        for e in ordered {
            if let Some(&idx) = self.path_index.get(&e.path) {
                self.entries.push(idx);
            }
        }
    }

    /// Set the filtered view to the full backing store, in storage order
    /// (the no-filter case).
    pub fn set_filtered_all(&mut self) {
        self.entries.clear();
        self.entries.extend(0..self.unfiltered_entries.len());
    }

    /// Number of entries in the filtered (visible) view.
    pub fn filtered_len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the filtered (visible) view is empty.
    pub fn filtered_is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate the filtered (visible) entries in display order. Dead indices
    /// (transiently stale after an in-place mutation) are skipped.
    pub fn filtered(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries
            .iter()
            .filter_map(|&i| self.unfiltered_entries.get(i))
    }

    /// Entry at filtered position `i`, if any.
    pub fn filtered_get(&self, i: usize) -> Option<&FileEntry> {
        self.entries
            .get(i)
            .and_then(|&idx| self.unfiltered_entries.get(idx))
    }

    /// The backing store (every entry, regardless of filter), as a slice.
    pub fn unfiltered(&self) -> &[FileEntry] {
        &self.unfiltered_entries
    }

    /// Mutable access to the backing store for in-place edits (e.g. restoring
    /// selection by path). Cannot resize — use [`upsert`](Self::upsert) or
    /// [`remove`](Self::remove) for that.
    pub fn unfiltered_mut(&mut self) -> &mut [FileEntry] {
        &mut self.unfiltered_entries
    }

    /// Look up a backing entry by path via `path_index`.
    pub fn entry_by_path(&self, path: &Path) -> Option<&FileEntry> {
        self.path_index
            .get(path)
            .and_then(|&i| self.unfiltered_entries.get(i))
    }

    /// Backing-store index for `path`, if present.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.path_index.get(path).copied()
    }

    /// Whether `path` is present in the backing store.
    pub fn contains_path(&self, path: &Path) -> bool {
        self.path_index.contains_key(path)
    }

    /// Rebuild `path_index` only if it is currently empty (lazy refresh used by
    /// the watcher fast paths before a lookup).
    pub fn ensure_index(&mut self) {
        if self.path_index.is_empty() {
            self.rebuild_index();
        }
    }

    /// Unconditionally rebuild `path_index` from the backing store.
    pub fn rebuild_index(&mut self) {
        self.path_index.clear();
        self.path_index.reserve(self.unfiltered_entries.len());
        for (i, entry) in self.unfiltered_entries.iter().enumerate() {
            self.path_index.insert(entry.path.clone(), i);
        }
    }

    /// Insert `entry`, or replace the existing entry with the same path
    /// (preserving its selection). Keeps `path_index` consistent.
    ///
    /// Note: a replace edits in place (filtered view unaffected), but the caller
    /// should mark the panel dirty so the view is rebuilt — a newly inserted
    /// entry is not yet part of the filtered view.
    pub fn upsert(&mut self, mut entry: FileEntry) {
        self.ensure_index();
        if let Some(&idx) = self.path_index.get(&entry.path) {
            if let Some(existing) = self.unfiltered_entries.get_mut(idx) {
                entry.selected = existing.selected;
                *existing = entry;
            }
        } else {
            let new_idx = self.unfiltered_entries.len();
            self.path_index.insert(entry.path.clone(), new_idx);
            self.unfiltered_entries.push(entry);
        }
    }

    /// Remove the entry at `path`, returning whether it existed. Uses
    /// `swap_remove` and repairs `path_index` for the moved tail entry.
    ///
    /// `swap_remove` can leave indices in the filtered view stale, so on a real
    /// removal this self-marks the panel dirty (via [`mark_dirty`]) to enforce
    /// the rebuild-before-next-read invariant instead of trusting every caller to
    /// remember it.
    pub fn remove(&mut self, path: &Path) -> bool {
        if self.unfiltered_entries.is_empty() {
            return false;
        }
        self.ensure_index();
        let Some(idx) = self.path_index.remove(path) else {
            return false;
        };
        let last = self.unfiltered_entries.len() - 1;
        if idx < last {
            let last_path = self.unfiltered_entries[last].path.clone();
            self.unfiltered_entries.swap_remove(idx);
            self.path_index.insert(last_path, idx);
        } else {
            self.unfiltered_entries.pop();
        }
        self.mark_dirty();
        true
    }

    pub fn state(&self) -> ListingState {
        self.state
    }

    pub fn is_clean(&self) -> bool {
        self.state == ListingState::Clean
    }

    pub fn is_dirty(&self) -> bool {
        self.state == ListingState::NeedsRebuild
    }

    pub fn needs_full_read(&self) -> bool {
        self.state == ListingState::NeedsFullRead
    }

    #[cfg(test)]
    pub(crate) fn force_state(&mut self, state: ListingState) {
        self.state = state;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.unfiltered_entries.clear();
        self.path_index.clear();
        self.state = ListingState::NeedsFullRead;
    }

    pub fn mark_dirty(&mut self) {
        if self.state == ListingState::Clean {
            self.state = ListingState::NeedsRebuild;
        }
    }

    pub fn mark_unfiltered_dirty(&mut self) {
        self.state = ListingState::NeedsFullRead;
    }

    pub fn mark_rebuilt(&mut self) {
        if self.state == ListingState::NeedsRebuild {
            self.state = ListingState::Clean;
        }
    }
}

impl Default for PanelListing {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: ~30 pub getters/setters below. By design this struct exposes individual
// field access instead of a single `set_fields()` mega-method so input handlers
// can update only what changed without re-allocating the rest. Invariant-bearing
// fields are NOT plain beans: their setters enforce the invariant (`set_path`
// re-canonicalizes, `push_history` caps at `MAX_HISTORY`) and selection is
// single-sourced in `listing` (see `PanelListing`). `cursor`/`scroll_offset`
// stay public because their only invariant — staying within the view and
// on-screen — depends on the viewport height, which a field setter does not
// have; callers enforce it via `ensure_cursor_visible(height)`. If you add a
// field, add its getter+setter pair (and any invariant it carries).
#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    pub(crate) path: PathBuf,
    pub(crate) canonical_path: Option<PathBuf>,
    pub listing: PanelListing,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub(crate) history: VecDeque<PathBuf>,
    pub(crate) sort_mode: SortMode,
    pub(crate) sort_options: SortOptions,
    pub(crate) listing_mode: ListingMode,
    pub(crate) show_hidden: bool,
    pub(crate) show_permissions: bool,
    pub(crate) filter: Option<String>,
    /// Cached compiled form of `filter`. Recompiled lazily by
    /// [`compiled_filter_cached`](Self::compiled_filter_cached) whenever
    /// `filter_text` drifts from `filter`, so repeated calls with an
    /// unchanged filter reuse the precomputed pattern (avoids recompiling
    /// the search table on every refresh/rebuild).
    pub(crate) compiled_filter: Option<CompiledPattern>,
    /// Snapshot of `filter` at the time `compiled_filter` was last built.
    /// Drives the invalidation check in `compiled_filter_cached`.
    pub(crate) filter_text: Option<String>,
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
            history: VecDeque::new(),
            sort_mode: SortMode::default(),
            sort_options: SortOptions::default(),
            listing_mode: ListingMode::default(),
            show_hidden: true,
            show_permissions: false,
            filter: None,
            compiled_filter: None,
            filter_text: None,
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

    pub fn history(&self) -> &VecDeque<PathBuf> {
        &self.history
    }

    const MAX_HISTORY: usize = 256;

    pub fn push_history(&mut self, path: PathBuf) {
        // `VecDeque` so the capacity cap evicts the oldest entry in O(1)
        // (`pop_front`) instead of `Vec::remove(0)`'s O(n) element shift.
        if self.history.len() >= Self::MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(path);
    }

    pub fn pop_history(&mut self) -> Option<PathBuf> {
        self.history.pop_back()
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

    /// Returns the compiled form of `self.filter`, rebuilding it only when
    /// `filter` has changed since the last call. The result is a cheap
    /// `Clone` of the cached pattern (the search-table allocation happens at
    /// most once per filter change, not once per refresh).
    pub fn compiled_filter_cached(&mut self) -> Option<CompiledPattern> {
        if self.filter_text != self.filter {
            self.compiled_filter = self.filter.as_ref().map(|f| CompiledPattern::new(f, false));
            self.filter_text = self.filter.clone();
        }
        self.compiled_filter.clone()
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
        self.listing.filtered_get(self.cursor)
    }

    /// Toggle selection of the entry under the cursor.
    ///
    /// Selection is mutated directly on the single backing store via the
    /// filtered index, so there is no path lookup and no second store to sync.
    pub fn toggle_selection(&mut self) -> ToggleResult {
        let Some(&idx) = self.listing.entries.get(self.cursor) else {
            return ToggleResult::NoEntry;
        };
        // Defensive `get_mut` (rather than indexing): the filtered view may be
        // transiently stale after an in-place mutation, matching the rest of the
        // read API which skips dead indices instead of panicking.
        let Some(entry) = self.listing.unfiltered_entries.get_mut(idx) else {
            return ToggleResult::NoEntry;
        };
        if entry.name == ".." {
            return ToggleResult::SkippedParent;
        }
        entry.selected = !entry.selected;
        let size = entry.size();
        let selected = entry.selected;
        self.update_selection_stats(size, selected);
        ToggleResult::Toggled(selected)
    }

    pub fn set_selection_at(&mut self, index: usize, selected: bool) {
        let Some(&idx) = self.listing.entries.get(index) else {
            return;
        };
        let Some(entry) = self.listing.unfiltered_entries.get_mut(idx) else {
            return;
        };
        if entry.name == ".." || entry.selected == selected {
            return;
        }
        entry.selected = selected;
        let size = entry.size();
        self.update_selection_stats(size, selected);
    }

    pub fn toggle_selection_at(&mut self, index: usize) {
        let selected = self
            .listing
            .filtered_get(index)
            .is_some_and(|e| !e.selected);
        self.set_selection_at(index, selected);
    }

    /// Iterate the currently selected entries. Selection lives only in the
    /// backing store, so this returns a borrowing iterator (no allocation).
    pub fn selected_entries(&self) -> impl Iterator<Item = &FileEntry> {
        self.listing.unfiltered().iter().filter(|e| e.selected)
    }

    pub fn clear_selection(&mut self) {
        // Single store: clearing the backing entries clears the filtered view too.
        for entry in self.listing.unfiltered_mut() {
            entry.selected = false;
        }
        self.selected_count = 0;
        self.selected_size = 0;
    }

    pub fn recalculate_selection_stats(&mut self) {
        let mut selected_count: usize = 0;
        let mut selected_size: u64 = 0;
        let mut total_size: u64 = 0;
        for entry in self.listing.unfiltered() {
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
        let len = self.listing.filtered_len();
        if len == 0 {
            return;
        }

        if self.cursor == 0 {
            self.cursor = len.saturating_sub(1);
            if max_height > 0 {
                self.scroll_offset = len.saturating_sub(max_height);
            }
        } else {
            self.cursor = self.cursor.saturating_sub(1);
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            }
        }
    }

    pub fn move_cursor_down(&mut self, max_height: usize) {
        let len = self.listing.filtered_len();
        if len == 0 {
            return;
        }

        let max_index = len - 1;

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
        let max_scroll = self.listing.filtered_len().saturating_sub(1);
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

    /// Replace the listing with `entries`, no filter applied (the filtered view
    /// becomes the full set). No clone: the dual-store duplication that once
    /// required one is gone.
    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.listing.set_unfiltered(entries);
        self.listing.set_filtered_all();
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
