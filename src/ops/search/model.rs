#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    DepthLimit,
    ItemLimit,
    ContentResultLimit,
    FileTooLarge,
    LineTooLong,
    BinaryFile,
}

#[derive(Debug, Clone)]
pub struct SearchOutcome<T> {
    pub matches: Vec<T>,
    pub errors: Vec<String>,
    pub truncated: Option<TruncationReason>,
}

impl<T> Default for SearchOutcome<T> {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            errors: Vec::new(),
            truncated: None,
        }
    }
}

pub const MAX_SEARCH_DEPTH: usize = 20;
pub const MAX_SEARCH_ITEMS: usize = 10000;

pub const MAX_CONTENT_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_CONTENT_LINE_BYTES: usize = 64 * 1024;
pub const MAX_CONTENT_RESULTS: usize = 1000;
