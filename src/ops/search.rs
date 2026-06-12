//! File searching operations for Libre Commander (lc).
//!
//! Full file search by name pattern or content.

#[path = "search/content.rs"]
mod content;
#[path = "search/model.rs"]
mod model;
#[path = "search/name.rs"]
mod name;
#[path = "search/pattern.rs"]
mod pattern;
#[path = "search/walk.rs"]
mod walk;

pub use model::{
    MAX_CONTENT_FILE_BYTES, MAX_CONTENT_LINE_BYTES, MAX_CONTENT_RESULTS, MAX_SEARCH_DEPTH,
    MAX_SEARCH_ITEMS, SearchError, SearchErrorKind, SearchOutcome, TruncationReason,
};
pub use pattern::CompiledPattern;

pub struct FileSearch;
