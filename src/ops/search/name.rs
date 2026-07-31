use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::app::types::FileEntry;
use crate::fs::reader::{file_info_from_metadata, get_file_info};
use crate::ops::search::pattern::{CompiledPattern, MatchScratch};
use crate::ops::search::walk::{
    FileSearchContext, item_limit_reached, prepare_dir_scan, seed_visited_dir, should_recurse,
};
use crate::ops::search::{
    MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, SearchError, SearchErrorKind, SearchOutcome,
};
use std::sync::atomic::Ordering;

/// Initial capacity for the visited inode set. Most directories contain well under
/// 256 entries; this avoids reallocations for typical workloads while staying small.
const VISITED_INODE_CAP: usize = 256;

/// Search for files by name pattern.
pub fn search_files(
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
        cancel,
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

fn search_files_recursive(
    path: &Path,
    pattern: &CompiledPattern,
    recursive: bool,
    depth: usize,
    ctx: &mut FileSearchContext<'_>,
    scratch: &mut MatchScratch,
) {
    if ctx.cancel.load(Ordering::Relaxed) {
        return;
    }
    let Some(entries) =
        prepare_dir_scan(path, depth, MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, ctx.outcome)
    else {
        return;
    };

    for entry in entries {
        if ctx.cancel.load(Ordering::Relaxed) {
            return;
        }
        if item_limit_reached(ctx.outcome, MAX_SEARCH_ITEMS) {
            return;
        }

        // Count every dirent toward the item cap, including entries whose
        // file_type() later fails — otherwise MAX_SEARCH_ITEMS can be
        // overshot by N unreadable entries.
        ctx.outcome.items_scanned += 1;

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
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                ctx.outcome.errors.push(SearchError {
                    path: Some(entry.path()),
                    kind: SearchErrorKind::FileType,
                    message: err.to_string(),
                });
                continue;
            }
        };

        let name = entry.file_name();
        let matched = pattern.matches_os(&name, scratch);

        // Symlinked files are included in results (pattern matched above).
        // Symlinked directories ARE recursed into: inode-based cycle detection
        // (`should_recurse` + `seed_visited_dir`) already protects against
        // loops, so skipping them would silently drop whole subtrees.
        //
        // For a plain directory `entry.metadata()` (lstat) is enough and is
        // reused for FileEntry + cycle detection. For a symlink we follow
        // once via `fs::metadata` and only recurse when the target is a dir.
        let plain_dir = recursive && file_type.is_dir() && !file_type.is_symlink();
        let dir_meta: Option<io::Result<Metadata>> = plain_dir.then(|| entry.metadata());

        // Whether this entry needs its path allocated (for a match result or
        // for recursion). Non-matching, non-recursive entries skip the PathBuf.
        let needs_path = matched || dir_meta.is_some() || (recursive && file_type.is_symlink());

        let entry_path = needs_path.then(|| entry.path());

        if let Some(entry_path) = &entry_path
            && matched
        {
            let built = match &dir_meta {
                Some(Ok(meta)) => Ok(file_info_from_metadata(entry_path.clone(), meta)),
                // dir_meta Some(Err): the stat already failed; don't retry via
                // get_file_info (which would repeat the failed lstat).
                Some(Err(e)) => Err(std::io::Error::new(e.kind(), e.to_string())),
                None => get_file_info(entry_path),
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

        if let Some(meta) = dir_meta {
            if should_recurse(meta, ctx.visited)
                && let Some(ref entry_path) = entry_path
            {
                search_files_recursive(entry_path, pattern, recursive, depth + 1, ctx, scratch);
            }
        } else if recursive && file_type.is_symlink() {
            // Follow the symlink once; recurse only when the target is a dir
            // and its inode is new.
            if let Some(ref entry_path) = entry_path
                && let Ok(meta) = std::fs::metadata(entry_path)
                && meta.is_dir()
                && should_recurse(Ok(meta), ctx.visited)
            {
                search_files_recursive(entry_path, pattern, recursive, depth + 1, ctx, scratch);
            }
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

        let results = search_files(dir_path, "*.txt", true, false, &AtomicBool::new(false)).matches;
        assert_eq!(results.len(), 2, "Expected 2 results, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));
        assert!(results.iter().any(|e| e.name == "test3.txt"));

        let results =
            search_files(dir_path, "*.txt", false, false, &AtomicBool::new(false)).matches;
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

        let results =
            search_files(&dir, "target.txt", false, false, &AtomicBool::new(false)).matches;

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

        let outcome = search_files(&dir, "*.txt", true, false, &AtomicBool::new(false));

        assert!(outcome.matches.is_empty());
        assert!(!outcome.errors.is_empty());
        assert!(outcome.truncated.is_empty());
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

        let outcome = search_files(&dir, "*.txt", false, false, &AtomicBool::new(false));

        assert_eq!(outcome.matches.len(), MAX_SEARCH_ITEMS);
        assert!(outcome.truncated.contains(&TruncationReason::ItemLimit));

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_files_follows_symlinked_directories() {
        // Symlink-to-dir is recursed into; inode cycle detection prevents loops.
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("root")).unwrap();
        fs::create_dir_all(dir.path().join("outside")).unwrap();
        fs::write(dir.path().join("outside/target.txt"), "x").unwrap();
        symlink(dir.path().join("outside"), dir.path().join("root/linkdir")).unwrap();

        let results = search_files(
            &dir.path().join("root"),
            "target.txt",
            true,
            false,
            &AtomicBool::new(false),
        )
        .matches;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "target.txt");
    }

    #[cfg(unix)]
    #[test]
    fn test_search_files_symlink_cycle_does_not_loop() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("hit.txt"), "x").unwrap();
        // a/link -> b, b/link -> a  (cycle)
        symlink(&b, a.join("link")).unwrap();
        symlink(&a, b.join("link")).unwrap();

        let outcome = search_files(dir.path(), "hit.txt", true, false, &AtomicBool::new(false));
        // Should find hit.txt exactly once and terminate (no hang / no blowup).
        assert_eq!(outcome.matches.len(), 1);
        assert!(outcome.items_scanned < 100);
    }

    #[cfg(unix)]
    #[test]
    fn search_files_includes_symlinked_file_in_results() {
        // Symlink files appear in search results; symlink directories are
        // also recursed into (with cycle detection).
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "x").unwrap();
        symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();

        let results =
            search_files(dir.path(), "*.txt", false, false, &AtomicBool::new(false)).matches;
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"real.txt"));
        assert!(names.contains(&"link.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn search_files_broken_symlink_does_not_abort() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ok.txt"), "x").unwrap();
        symlink(dir.path().join("missing"), dir.path().join("broken")).unwrap();

        let outcome = search_files(dir.path(), "*.txt", true, false, &AtomicBool::new(false));
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].name, "ok.txt");
    }

    #[test]
    fn search_files_unicode_filename() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("żółć.txt"), "x").unwrap();
        fs::write(dir.path().join("ascii.txt"), "x").unwrap();

        let results =
            search_files(dir.path(), "żółć*", true, false, &AtomicBool::new(false)).matches;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "żółć.txt");
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

        let outcome = search_files(&dir, "*.txt", true, false, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert!(outcome.truncated.is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}
