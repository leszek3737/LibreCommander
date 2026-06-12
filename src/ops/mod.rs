//! High-level facade for file operations: copy, move, delete, search, sort,
//! compare, and archive.  Re-exports the stable public API of each submodule
//! so callers outside `crate::ops` can `use crate::ops::{…}`.

// archive and search are `pub` so that integration tests and power users
// can reach enum variants / internal submodules directly.  All other
// submodules are `pub(crate)` — no external code needs them.
pub mod archive;
pub(crate) mod batch;
pub(crate) mod chunk_copy;
pub(crate) mod compare;
pub(crate) mod file_ops;
pub(crate) mod helpers;
pub(crate) mod natsort;
pub mod search;
pub(crate) mod sorting;

pub use archive::{ArchiveEntry, ArchiveError, ArchiveFormat};
pub use compare::{CompareReport, apply_compare_to_panels, compare_entries};
pub use file_ops::{chmod, create_directory, rename_entry};
pub use search::{FileSearch, SearchOutcome, TruncationReason};
pub use sorting::{cmp_ignore_case, cycle_sort_mode, sort_entries};
