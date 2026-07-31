use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app::types::FileEntry;
use crate::ops::helpers::get_inode_key;
use crate::ops::search::{SearchError, SearchErrorKind, SearchOutcome, TruncationReason};

pub(super) use crate::ops::helpers::seed_visited_dir;

pub(super) struct FileSearchContext<'a> {
    pub(super) outcome: &'a mut SearchOutcome<FileEntry, SearchError>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: &'a AtomicBool,
}

pub(super) struct ContentSearchContext<'a> {
    pub(super) pattern: &'a str,
    pub(super) case_sensitive: bool,
    pub(super) pattern_bytes: &'a [u8],
    pub(super) recursive: bool,
    pub(super) outcome: &'a mut SearchOutcome<(Arc<Path>, usize, String), SearchError>,
    pub(super) visited: &'a mut HashSet<(u64, u64)>,
    pub(super) cancel: &'a AtomicBool,
}

/// Decide whether to descend into a directory given its already-fetched `lstat`
/// result.
///
/// - Fresh inode → recurse (insert into `visited`).
/// - Previously seen inode → cycle, skip.
/// - Platforms without stable inode keys (non-unix): still recurse, but cycle
///   detection is unavailable — callers must rely on `max_depth`.
/// - Metadata failure → do **not** recurse. Recursing without an identity key
///   re-opens the door to slow loops on broken NFS mounts / dangling symlink
///   trees (depth 20 is not free).
pub(super) fn should_recurse(
    meta: std::io::Result<std::fs::Metadata>,
    visited: &mut HashSet<(u64, u64)>,
) -> bool {
    match meta {
        Ok(meta) => match get_inode_key(&meta) {
            Some(key) => visited.insert(key),
            // Non-unix / no stable key: allow recursion; depth limit is the
            // only backstop against symlink cycles on those platforms.
            None => true,
        },
        // Metadata failed — refuse to recurse without cycle detection.
        Err(_) => false,
    }
}

/// Single source of truth for the per-scan item cap. Records the `ItemLimit`
/// truncation and returns whether the cap is reached. Shared by
/// `prepare_dir_scan` and the per-entry loops in `name.rs` / `content.rs`.
pub(super) fn item_limit_reached<T>(
    outcome: &mut SearchOutcome<T, SearchError>,
    max_items: usize,
) -> bool {
    if outcome.items_scanned >= max_items {
        outcome.record_truncation(TruncationReason::ItemLimit);
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
        outcome.record_truncation(TruncationReason::DepthLimit);
        return None;
    }
    if item_limit_reached(outcome, max_items) {
        return None;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => Some(entries),
        Err(err) => {
            // Prefer the static Display of the ErrorKind when it is specific
            // enough — that avoids allocating a fresh String for every
            // permission-denied directory on a large tree. Fall back to
            // `err.to_string()` only for kinds whose Display is unhelpful
            // (Other / Uncategorized) so the OS message is preserved.
            let message = match err.kind() {
                std::io::ErrorKind::PermissionDenied
                | std::io::ErrorKind::NotFound
                | std::io::ErrorKind::NotADirectory
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::AlreadyExists
                | std::io::ErrorKind::InvalidInput => err.kind().to_string(),
                _ => err.to_string(),
            };
            outcome.errors.push(SearchError {
                path: Some(path.to_path_buf()),
                kind: SearchErrorKind::ReadDir,
                message,
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
        outcome.record_truncation(TruncationReason::ContentResultLimit);
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
    use crate::ops::helpers::get_inode_key;
    use crate::ops::search::{search_content, search_files};
    #[cfg(unix)]
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::AtomicBool;

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

    #[cfg(unix)]
    #[test]
    fn seed_visited_dir_follows_root_symlink_to_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let target_key = get_inode_key(&fs::metadata(&real).unwrap()).unwrap();
        let link_key = get_inode_key(&fs::symlink_metadata(&link).unwrap()).unwrap();
        assert_ne!(target_key, link_key);

        let mut visited = HashSet::new();
        seed_visited_dir(&link, &mut visited);
        assert!(
            visited.contains(&target_key),
            "root symlink should seed the target inode"
        );
        assert!(
            !visited.contains(&link_key),
            "root symlink should not seed the link's own inode"
        );
    }

    #[test]
    fn should_recurse_false_on_metadata_error() {
        let mut visited = HashSet::new();
        let err = Err(std::io::Error::other("boom"));
        assert!(!should_recurse(err, &mut visited));
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
        let outcome = search_files(&dir, "*.txt", true, false, &AtomicBool::new(false));
        assert!(!outcome.matches.iter().any(|e| e.name == "deep.txt"));
        assert!(outcome.truncated.contains(&TruncationReason::DepthLimit));
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
        let outcome = search_content(&dir, "needle", true, false, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert!(outcome.truncated.contains(&TruncationReason::DepthLimit));
        let _ = fs::remove_dir_all(dir);
    }
}
