use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::FileEntry;
use crate::ops::helpers::get_inode_key;
use crate::ops::search::{SearchError, SearchErrorKind, SearchOutcome, TruncationReason};

pub(super) trait SearchContext {
    fn is_cancelled(&self) -> bool {
        self.cancel().is_some_and(|c| c.load(Ordering::Relaxed))
    }

    fn cancel(&self) -> Option<&AtomicBool>;
}

pub(super) struct FileSearchContext<'a> {
    pub(super) outcome: &'a mut SearchOutcome<FileEntry, SearchError>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: Option<&'a AtomicBool>,
}

impl SearchContext for FileSearchContext<'_> {
    fn cancel(&self) -> Option<&AtomicBool> {
        self.cancel
    }
}

pub(super) struct ContentSearchContext<'a> {
    pub(super) pattern: &'a str,
    pub(super) case_sensitive: bool,
    pub(super) pattern_bytes: &'a [u8],
    pub(super) recursive: bool,
    pub(super) outcome: &'a mut SearchOutcome<(Arc<Path>, usize, String), SearchError>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: Option<&'a AtomicBool>,
}

impl SearchContext for ContentSearchContext<'_> {
    fn cancel(&self) -> Option<&AtomicBool> {
        self.cancel
    }
}

/// Run `f` with a fresh, never-cancelled flag.
///
/// Collapses the boilerplate in the two non-cancellable entry points
/// (`search_files_with_diagnostics` / `search_content_with_diagnostics`), which
/// each forward to their `*_cancellable` counterpart with a throwaway flag.
pub(super) fn with_fresh_cancel<T>(f: impl FnOnce(&AtomicBool) -> T) -> T {
    let cancel = AtomicBool::new(false);
    f(&cancel)
}

/// Decide whether to descend into a directory given its already-fetched `lstat`
/// result. A fresh inode → recurse; a previously seen inode → cycle, skip. If
/// the metadata could not be read we still recurse (without cycle detection),
/// matching the historical best-effort behavior. Shared by the name and content
/// searches.
pub(super) fn should_recurse(
    meta: std::io::Result<std::fs::Metadata>,
    visited: &mut HashSet<(u64, u64)>,
) -> bool {
    match meta {
        Ok(meta) => get_inode_key(&meta).is_none_or(|key| visited.insert(key)),
        Err(_) => true,
    }
}

pub(super) fn seed_visited_dir(path: &Path, visited: &mut HashSet<(u64, u64)>) {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.is_dir()
        && let Some(key) = get_inode_key(&meta)
    {
        visited.insert(key);
    }
}

/// Single source of truth for the per-scan item cap. Records the `ItemLimit`
/// truncation (first reason wins) and returns whether the cap is reached. Shared
/// by `prepare_dir_scan` and the per-entry loops in `name.rs` / `content.rs`.
pub(super) fn item_limit_reached<T>(
    outcome: &mut SearchOutcome<T, SearchError>,
    max_items: usize,
) -> bool {
    if outcome.items_scanned >= max_items {
        outcome.truncated.get_or_insert(TruncationReason::ItemLimit);
        true
    } else {
        false
    }
}

pub(super) fn prepare_dir_scan<T>(
    path: &Path,
    depth: usize,
    max_depth: usize,
    max_items: usize,
    outcome: &mut SearchOutcome<T, SearchError>,
) -> Option<std::fs::ReadDir> {
    if depth >= max_depth {
        outcome
            .truncated
            .get_or_insert(TruncationReason::DepthLimit);
        return None;
    }
    if item_limit_reached(outcome, max_items) {
        return None;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => Some(entries),
        Err(err) => {
            outcome.errors.push(SearchError {
                path: Some(path.to_path_buf()),
                kind: SearchErrorKind::ReadDir,
                message: err.to_string(),
            });
            None
        }
    }
}

/// Content-search variant of [`prepare_dir_scan`]: additionally stops once the
/// content-result cap is reached. The name search has no such guard, so the
/// base `prepare_dir_scan` takes no guard parameter.
pub(super) fn prepare_content_dir_scan<T>(
    path: &Path,
    depth: usize,
    max_depth: usize,
    max_items: usize,
    max_results: usize,
    outcome: &mut SearchOutcome<T, SearchError>,
) -> Option<std::fs::ReadDir> {
    if outcome.matches.len() >= max_results {
        outcome
            .truncated
            .get_or_insert(TruncationReason::ContentResultLimit);
        return None;
    }
    prepare_dir_scan(path, depth, max_depth, max_items, outcome)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    // Used only by the unix-gated inode-seeding test below.
    #[cfg(unix)]
    use std::collections::HashSet;
    use std::fs;

    #[cfg(unix)]
    use crate::ops::helpers::get_inode_key;
    use crate::ops::search::{search_content_with_diagnostics, search_files_with_diagnostics};

    #[cfg(unix)]
    #[test]
    fn search_files_seeds_root_inode() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_root_seed_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let meta = fs::metadata(&dir).unwrap();
        let key = get_inode_key(&meta).unwrap();
        let mut visited = HashSet::new();

        seed_visited_dir(&dir, &mut visited);
        assert!(visited.contains(&key));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_files_respects_depth_limit() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_deep_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut deep = dir.clone();
        for i in 0..crate::ops::search::MAX_SEARCH_DEPTH + 2 {
            deep = deep.join(format!("d{i}"));
            fs::create_dir_all(&deep).unwrap();
        }
        fs::write(deep.join("deep.txt"), "found").unwrap();

        let outcome = search_files_with_diagnostics(&dir, "*.txt", true, false);
        assert!(!outcome.matches.iter().any(|e| e.name == "deep.txt"));
        assert_eq!(outcome.truncated, Some(TruncationReason::DepthLimit));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_content_respects_depth_limit() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_depth_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut deep = dir.clone();
        for i in 0..crate::ops::search::MAX_SEARCH_DEPTH + 2 {
            deep = deep.join(format!("d{i}"));
            fs::create_dir_all(&deep).unwrap();
        }
        fs::write(deep.join("deep.txt"), "needle\n").unwrap();

        let outcome = search_content_with_diagnostics(&dir, "needle", true, false);
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.truncated, Some(TruncationReason::DepthLimit));

        let _ = fs::remove_dir_all(dir);
    }
}
