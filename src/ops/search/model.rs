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
    Other,
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

// NOTE: Deriving Clone forces T: Clone + E: Clone bounds on SearchOutcome<T, E>.
// NOTE: Default cannot be derived because it requires T: Default;
// FileEntry does not implement Default, so we provide a manual impl.
#[derive(Debug, Clone)]
pub struct SearchOutcome<T, E = String> {
    pub matches: Vec<T>,
    pub errors: Vec<E>,
    pub truncated: Option<TruncationReason>,
    pub items_scanned: usize,
}

impl<T, E> Default for SearchOutcome<T, E> {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            errors: Vec::new(),
            truncated: None,
            items_scanned: 0,
        }
    }
}

/// Maximum directory recursion depth during search.
pub const MAX_SEARCH_DEPTH: usize = 20;
/// Maximum number of items (files + dirs) to scan per search.
pub const MAX_SEARCH_ITEMS: usize = 10000;

/// Maximum file size (10 MiB) to read for content search.
pub const MAX_CONTENT_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// Maximum line length (64 KiB) read per line during content search.
pub const MAX_CONTENT_LINE_BYTES: usize = 64 * 1024;
/// Maximum content search matches to collect before truncating.
pub const MAX_CONTENT_RESULTS: usize = 1000;
