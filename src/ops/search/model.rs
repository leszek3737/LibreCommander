#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    DepthLimit,
    ItemLimit,
    ContentResultLimit,
    FileTooLarge,
    LineTooLong,
    BinaryFile,
}

// NOTE: Deriving Clone forces T: Clone bound on SearchOutcome<T>.
// NOTE: Default cannot be derived because it requires T: Default;
// FileEntry does not implement Default, so we provide a manual impl.
#[derive(Debug, Clone)]
pub struct SearchOutcome<T> {
    pub matches: Vec<T>,
    // TODO: Parameterize error type (currently String) to allow richer error reporting.
    pub errors: Vec<String>,
    pub truncated: Option<TruncationReason>,
    pub items_scanned: usize,
}

impl<T> Default for SearchOutcome<T> {
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
