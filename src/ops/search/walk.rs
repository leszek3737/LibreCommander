use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::FileEntry;
use crate::ops::helpers::get_inode_key;
use crate::ops::search::{SearchOutcome, TruncationReason};

pub(super) struct FileSearchContext<'a> {
    pub(super) outcome: &'a mut SearchOutcome<FileEntry>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: Option<&'a AtomicBool>,
}

impl FileSearchContext<'_> {
    pub(super) fn is_cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

pub(super) struct ContentSearchContext<'a> {
    pub(super) pattern: &'a str,
    pub(super) case_sensitive: bool,
    pub(super) pattern_lower: &'a [char],
    pub(super) recursive: bool,
    pub(super) outcome: &'a mut SearchOutcome<(std::path::PathBuf, usize, String)>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: Option<&'a AtomicBool>,
}

impl ContentSearchContext<'_> {
    pub(super) fn is_cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

pub(super) fn seed_visited_dir(path: &Path, visited: &mut HashSet<(u64, u64)>) {
    if let Ok(meta) = std::fs::metadata(path)
        && meta.is_dir()
        && let Some(key) = get_inode_key(&meta)
    {
        visited.insert(key);
    }
}

pub(super) fn prepare_dir_scan<T>(
    path: &Path,
    depth: usize,
    max_depth: usize,
    max_items: usize,
    outcome: &mut SearchOutcome<T>,
    extra_guard: impl Fn(&SearchOutcome<T>) -> bool,
    guard_reason: TruncationReason,
) -> Option<std::fs::ReadDir> {
    if !extra_guard(outcome) {
        outcome.truncated.get_or_insert(guard_reason);
        return None;
    }
    if depth >= max_depth {
        outcome
            .truncated
            .get_or_insert(TruncationReason::DepthLimit);
        return None;
    }
    if outcome.items_scanned >= max_items {
        outcome.truncated.get_or_insert(TruncationReason::ItemLimit);
        return None;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => Some(entries),
        Err(err) => {
            outcome
                .errors
                .push(format!("Failed to read {}: {err}", path.display()));
            None
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;

    use crate::ops::helpers::get_inode_key;
    use crate::ops::search::FileSearch;

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

        let outcome = FileSearch::search_files_with_diagnostics(&dir, "*.txt", true, false);
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

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", true, false);
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.truncated, Some(TruncationReason::DepthLimit));

        let _ = fs::remove_dir_all(dir);
    }
}
