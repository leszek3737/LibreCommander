use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::app::types::FileEntry;
use crate::fs::reader::get_file_info;
use crate::ops::helpers::get_inode_key;
use crate::ops::search::pattern::CompiledPattern;
use crate::ops::search::walk::{
    FileSearchContext, SearchContext, prepare_dir_scan, seed_visited_dir,
};
use crate::ops::search::{
    FileSearch, MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, SearchOutcome, TruncationReason,
};

impl FileSearch {
    pub fn search_files(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> Vec<FileEntry> {
        Self::search_files_with_diagnostics(path, pattern, recursive, case_sensitive).matches
    }

    pub fn search_files_with_diagnostics(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> SearchOutcome<FileEntry> {
        let cancel = AtomicBool::new(false);
        Self::search_files_with_diagnostics_cancellable(
            path,
            pattern,
            recursive,
            case_sensitive,
            &cancel,
        )
    }

    pub fn search_files_with_diagnostics_cancellable(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        cancel: &AtomicBool,
    ) -> SearchOutcome<FileEntry> {
        let mut outcome = SearchOutcome::default();
        let compiled_pattern = CompiledPattern::new(pattern, case_sensitive);
        let mut visited = HashSet::with_capacity(256);
        seed_visited_dir(path, &mut visited);
        let mut ctx = FileSearchContext {
            outcome: &mut outcome,
            visited: &mut visited,
            cancel: Some(cancel),
        };
        search_files_recursive(path, &compiled_pattern, recursive, 0, &mut ctx);
        outcome
    }
}

fn search_files_recursive(
    path: &Path,
    pattern: &CompiledPattern,
    recursive: bool,
    depth: usize,
    ctx: &mut FileSearchContext<'_>,
) {
    if ctx.is_cancelled() {
        return;
    }
    let Some(entries) = prepare_dir_scan(
        path,
        depth,
        MAX_SEARCH_DEPTH,
        MAX_SEARCH_ITEMS,
        ctx.outcome,
        |_| true,
        TruncationReason::ItemLimit,
    ) else {
        return;
    };

    for entry in entries {
        if ctx.is_cancelled() {
            return;
        }
        if ctx.outcome.items_scanned >= MAX_SEARCH_ITEMS {
            ctx.outcome
                .truncated
                .get_or_insert(TruncationReason::ItemLimit);
            return;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                ctx.outcome
                    .errors
                    .push(format!("Failed to read entry in {}: {err}", path.display()));
                continue;
            }
        };
        let entry_path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                ctx.outcome.errors.push(format!(
                    "Failed to read type for {}: {err}",
                    entry_path.display()
                ));
                continue;
            }
        };

        ctx.outcome.items_scanned += 1;

        let name = entry.file_name();
        let name_lossy = name.to_string_lossy();
        if pattern.matches(&name_lossy) {
            match get_file_info(&entry_path) {
                Ok(file_entry) => ctx.outcome.matches.push(file_entry),
                Err(err) => ctx.outcome.errors.push(format!(
                    "Failed to read metadata for {}: {err}",
                    entry_path.display()
                )),
            }
        }

        // Symlinked files are included in results (pattern matched above).
        // Symlinked directories are skipped for recursion to prevent
        // infinite loops via cyclic symlinks.
        if file_type.is_symlink() {
            continue;
        }

        if recursive && file_type.is_dir() {
            if let Ok(meta) = entry.metadata()
                && let Some(key) = get_inode_key(&meta)
                && !ctx.visited.insert(key)
            {
                continue;
            }
            search_files_recursive(&entry_path, pattern, recursive, depth + 1, ctx);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;

    use super::*;

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

        let results = FileSearch::search_files(dir_path, "*.txt", true, false);
        assert_eq!(results.len(), 2, "Expected 2 results, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));
        assert!(results.iter().any(|e| e.name == "test3.txt"));

        let results = FileSearch::search_files(dir_path, "*.txt", false, false);
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

        let results = FileSearch::search_files(&dir, "target.txt", false, false);

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

        let outcome = FileSearch::search_files_with_diagnostics(&dir, "*.txt", true, false);

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

        let outcome = FileSearch::search_files_with_diagnostics(&dir, "*.txt", false, false);

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

        let results = FileSearch::search_files(&dir.join("root"), "target.txt", true, false);
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

        let results = FileSearch::search_files(&dir, "*.txt", false, false);
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

        let outcome = FileSearch::search_files_with_diagnostics(&dir, "*.txt", true, false);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }
}
