use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    DepthLimit,
    ItemLimit,
    ContentResultLimit,
    FileTooLarge,
    LineTooLong,
    BinaryFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchErrorKind {
    ReadDir,
    ReadEntry,
    FileType,
    Metadata,
    OpenFile,
    ReadFile,
    NonUtf8,
}

#[derive(Debug, Clone)]
pub struct SearchError {
    pub path: Option<PathBuf>,
    pub kind: SearchErrorKind,
    pub message: String,
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(p) => write!(f, "{}: {}", p.display(), self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for SearchError {}

// NOTE: Deriving Clone forces T: Clone + E: Clone bounds on SearchOutcome<T, E>.
// NOTE: Default cannot be derived because it requires T: Default;
// FileEntry does not implement Default, so we provide a manual impl.
#[derive(Debug, Clone)]
pub struct SearchOutcome<T, E = String> {
    pub matches: Vec<T>,
    pub errors: Vec<E>,
    /// All truncation reasons observed during the scan (unique, insertion order).
    /// Multiple limits can fire in one run (e.g. DepthLimit + ItemLimit); keep
    /// every distinct reason so callers are not limited to the first one.
    pub truncated: Vec<TruncationReason>,
    pub items_scanned: usize,
}

impl<T, E> Default for SearchOutcome<T, E> {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            errors: Vec::new(),
            truncated: Vec::new(),
            items_scanned: 0,
        }
    }
}

impl<T, E> SearchOutcome<T, E> {
    /// Record a truncation reason if it is not already present.
    #[inline]
    pub fn record_truncation(&mut self, reason: TruncationReason) {
        if !self.truncated.contains(&reason) {
            self.truncated.push(reason);
        }
    }

    /// Whether any truncation reason was recorded.
    #[inline]
    pub fn is_truncated(&self) -> bool {
        !self.truncated.is_empty()
    }
}

/// Maximum directory recursion depth during search.
pub const MAX_SEARCH_DEPTH: usize = 20;
/// Maximum number of items (files + dirs) to scan per search.
pub const MAX_SEARCH_ITEMS: usize = 10000;

/// Maximum file size (10 MiB) to read for content search.
///
/// Typed as `u64` to match [`std::fs::Metadata::len`].
pub const MAX_CONTENT_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// Maximum line length (64 KiB) read per line during content search.
///
/// Typed as `u64` to stay consistent with [`MAX_CONTENT_FILE_BYTES`]; cast to
/// `usize` at buffer-capacity call sites.
pub const MAX_CONTENT_LINE_BYTES: u64 = 64 * 1024;
/// Maximum content search matches to collect before truncating.
pub const MAX_CONTENT_RESULTS: usize = 1000;
