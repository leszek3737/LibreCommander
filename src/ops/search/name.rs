use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::app::types::FileEntry;
use crate::fs::reader::{file_info_from_metadata, get_file_info};
use crate::ops::helpers::get_inode_key;
use crate::ops::search::pattern::{CompiledPattern, MatchScratch};
use crate::ops::search::walk::{
    FileSearchContext, SearchContext, item_limit_reached, prepare_dir_scan, seed_visited_dir,
    with_fresh_cancel,
};
use crate::ops::search::{
    MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, SearchError, SearchErrorKind, SearchOutcome,
};

/// Initial capacity for the visited inode set. Most directories contain well under
/// 256 entries; this avoids reallocations for typical workloads while staying small.
const VISITED_INODE_CAP: usize = 256;

pub fn search_files(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
) -> Vec<FileEntry> {
    search_files_with_diagnostics(path, pattern, recursive, case_sensitive).matches
}

pub fn search_files_with_diagnostics(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
) -> SearchOutcome<FileEntry, SearchError> {
    with_fresh_cancel(|cancel| {
        search_files_with_diagnostics_cancellable(path, pattern, recursive, case_sensitive, cancel)
    })
}

pub fn search_files_with_diagnostics_cancellable(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    cancel: &AtomicBool,
) -> SearchOutcome<FileEntry, SearchError> {
    let mut outcome = SearchOutcome::default();
    let compiled_pattern = CompiledPattern::new(pattern, case_sensitive);
    let mut visited = HashSet::with_capacity(VISITED_INODE_CAP);
    seed_visited_dir(path, &mut visited);
    let mut scratch = MatchScratch::default();
    let mut ctx = FileSearchContext {
        outcome: &mut outcome,
        visited: &mut visited,
        cancel: Some(cancel),
    };
    search_files_recursive(
        path,
        &compiled_pattern,
        recursive,
        0,
        &mut ctx,
        &mut scratch,
    );
    outcome
}

/// Decide whether to descend into a directory, given the `lstat` result already
/// fetched for it. A fresh inode → recurse; a previously seen inode → cycle,
/// skip. If the metadata could not be read we still recurse (without cycle
/// detection), matching the historical best-effort behavior.
fn should_recurse(meta: io::Result<Metadata>, visited: &mut HashSet<(u64, u64)>) -> bool {
    match meta {
        Ok(meta) => get_inode_key(&meta).is_none_or(|key| visited.insert(key)),
        Err(_) => true,
    }
}

fn search_files_recursive(
    path: &Path,
    pattern: &CompiledPattern,
    recursive: bool,
    depth: usize,
    ctx: &mut FileSearchContext<'_>,
    scratch: &mut MatchScratch,
) {
    if ctx.is_cancelled() {
        return;
    }
    let Some(entries) =
        prepare_dir_scan(path, depth, MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, ctx.outcome)
    else {
        return;
    };

    for entry in entries {
        if ctx.is_cancelled() {
            return;
        }
        if item_limit_reached(ctx.outcome, MAX_SEARCH_ITEMS) {
            return;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                ctx.outcome.errors.push(SearchError {
                    path: Some(path.to_path_buf()),
                    kind: SearchErrorKind::ReadEntry,
                    message: err.to_string(),
                });
                continue;
            }
        };
        let entry_path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                ctx.outcome.errors.push(SearchError {
                    path: Some(entry_path.clone()),
                    kind: SearchErrorKind::FileType,
                    message: err.to_string(),
                });
                continue;
            }
        };

        ctx.outcome.items_scanned += 1;

        let name = entry.file_name();
        let matched = pattern.matches_os(&name, scratch);

        // `entry.metadata()` is an `lstat`. Fetch it once for a non-symlink
        // directory we may recurse into and reuse it for both the matched
        // FileEntry and cycle detection — a matched directory then stats once,
        // not twice (build + inode).
        let dir_meta: Option<io::Result<Metadata>> =
            (recursive && file_type.is_dir() && !file_type.is_symlink()).then(|| entry.metadata());

        if matched {
            let built = match &dir_meta {
                Some(Ok(meta)) => Ok(file_info_from_metadata(entry_path.clone(), meta)),
                _ => get_file_info(&entry_path),
            };
            match built {
                Ok(file_entry) => ctx.outcome.matches.push(file_entry),
                Err(err) => ctx.outcome.errors.push(SearchError {
                    path: Some(entry_path.clone()),
                    kind: SearchErrorKind::Metadata,
                    message: err.to_string(),
                }),
            }
        }

        // Symlinked files are included in results (pattern matched above).
        // Symlinked directories are skipped for recursion to prevent
        // infinite loops via cyclic symlinks.
        if file_type.is_symlink() {
            continue;
        }

        if let Some(meta) = dir_meta
            && should_recurse(meta, ctx.visited)
        {
            search_files_recursive(&entry_path, pattern, recursive, depth + 1, ctx, scratch);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;

    use super::*;
    use crate::ops::search::TruncationReason;

    #[test]
    fn test_file_search_search_files() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_files_{}_{}", std::process::id(), id));
        fs::create_dir_all(&dir).unwrap();
        let dir_path = dir.as_path();

        {
            let mut f1 = File::create(dir_path.join("test1.txt")).unwrap();
            writeln!(f1, "test").unwrap();
            drop(f1);

            let mut f2 = File::create(dir_path.join("test2.log")).unwrap();
            writeln!(f2, "log").unwrap();
            drop(f2);
        }
        fs::create_dir(dir_path.join("sub")).unwrap();
        {
            let mut f3 = File::create(dir_path.join("sub/test3.txt")).unwrap();
            writeln!(f3, "test").unwrap();
            drop(f3);
        }

        let results = search_files(dir_path, "*.txt", true, false);
        assert_eq!(results.len(), 2, "Expected 2 results, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));
        assert!(results.iter().any(|e| e.name == "test3.txt"));

        let results = search_files(dir_path, "*.txt", false, false);
        assert_eq!(results.len(), 1, "Expected 1 result, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_files_populates_owner_and_group() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_metadata_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("target.txt"), "metadata").unwrap();

        let results = search_files(&dir, "target.txt", false, false);

        assert_eq!(results.len(), 1);
        assert!(!results[0].owner.is_empty());
        assert!(!results[0].group.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_files_reports_missing_directory() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_missing_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);

        let outcome = search_files_with_diagnostics(&dir, "*.txt", true, false);

        assert!(outcome.matches.is_empty());
        assert!(!outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);
    }

    #[test]
    fn test_search_files_truncates_after_item_limit() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_truncated_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        for i in 0..=MAX_SEARCH_ITEMS {
            File::create(dir.join(format!("file_{i}.txt"))).unwrap();
        }

        let outcome = search_files_with_diagnostics(&dir, "*.txt", false, false);

        assert_eq!(outcome.matches.len(), MAX_SEARCH_ITEMS);
        assert_eq!(outcome.truncated, Some(TruncationReason::ItemLimit));

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_files_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_symlink_files_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);

        fs::create_dir_all(dir.join("root")).unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        fs::write(dir.join("outside/target.txt"), "x").unwrap();
        symlink(dir.join("outside"), dir.join("root/linkdir")).unwrap();

        let results = search_files(&dir.join("root"), "target.txt", true, false);
        assert!(results.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn search_files_includes_symlinked_file_in_results() {
        // matches_pattern runs before the is_symlink check; symlink files
        // appear in search results (only symlink directories are skipped
        // for recursion).
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_symlink_file_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("real.txt"), "x").unwrap();
        symlink(dir.join("real.txt"), dir.join("link.txt")).unwrap();

        let results = search_files(&dir, "*.txt", false, false);
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"real.txt"));
        assert!(names.contains(&"link.txt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_files_empty_directory() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_empty_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let outcome = search_files_with_diagnostics(&dir, "*.txt", true, false);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }
}
