use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use memchr::{memchr, memmem};

use crate::ops::helpers::get_inode_key;
use crate::ops::search::pattern::contains_case_insensitive;
use crate::ops::search::walk::{
    ContentSearchContext, SearchContext, prepare_dir_scan, seed_visited_dir,
};
use crate::ops::search::{
    FileSearch, MAX_CONTENT_FILE_BYTES, MAX_CONTENT_LINE_BYTES, MAX_CONTENT_RESULTS,
    MAX_SEARCH_DEPTH, MAX_SEARCH_ITEMS, SearchOutcome, TruncationReason,
};

impl FileSearch {
    #[cfg(test)]
    fn search_content(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> Vec<(PathBuf, usize, String)> {
        Self::search_content_with_diagnostics(path, pattern, recursive, case_sensitive).matches
    }

    pub fn search_content_with_diagnostics(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> SearchOutcome<(PathBuf, usize, String)> {
        let cancel = AtomicBool::new(false);
        Self::search_content_with_diagnostics_cancellable(
            path,
            pattern,
            recursive,
            case_sensitive,
            &cancel,
        )
    }

    pub fn search_content_with_diagnostics_cancellable(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        cancel: &AtomicBool,
    ) -> SearchOutcome<(PathBuf, usize, String)> {
        let mut outcome = SearchOutcome::default();
        search_content_recursive(
            path,
            pattern,
            recursive,
            case_sensitive,
            0,
            &mut outcome,
            Some(cancel),
        );
        outcome
    }
}

fn search_content_recursive(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    depth: usize,
    outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
    cancel: Option<&AtomicBool>,
) {
    let pattern_bytes: Vec<u8> = if !case_sensitive {
        pattern
            .chars()
            .flat_map(|c| c.to_lowercase())
            .flat_map(|c| (c as u32).to_ne_bytes())
            .collect()
    } else {
        Vec::new()
    };
    let mut visited = HashSet::with_capacity(256);
    seed_visited_dir(path, &mut visited);

    let mut ctx = ContentSearchContext {
        pattern,
        case_sensitive,
        pattern_bytes: &pattern_bytes,
        recursive,
        outcome,
        visited: &mut visited,
        cancel,
    };
    search_content_recursive_inner(path, depth, &mut ctx);
}

fn search_content_recursive_inner(path: &Path, depth: usize, ctx: &mut ContentSearchContext<'_>) {
    if ctx.is_cancelled() {
        return;
    }
    if !path.is_dir() {
        return;
    }
    let Some(entries) = prepare_dir_scan(
        path,
        depth,
        MAX_SEARCH_DEPTH,
        MAX_SEARCH_ITEMS,
        ctx.outcome,
        |o| o.matches.len() < MAX_CONTENT_RESULTS,
        TruncationReason::ContentResultLimit,
    ) else {
        return;
    };

    for entry in entries {
        if ctx.is_cancelled() {
            return;
        }
        if process_content_entry(entry, path, depth, ctx) {
            return;
        }
    }
}

fn process_content_entry(
    entry: std::io::Result<std::fs::DirEntry>,
    path: &Path,
    depth: usize,
    ctx: &mut ContentSearchContext<'_>,
) -> bool {
    if ctx.is_cancelled() {
        return true;
    }
    if ctx.outcome.items_scanned >= MAX_SEARCH_ITEMS
        || ctx.outcome.matches.len() >= MAX_CONTENT_RESULTS
    {
        ctx.outcome
            .truncated
            .get_or_insert(if ctx.outcome.matches.len() >= MAX_CONTENT_RESULTS {
                TruncationReason::ContentResultLimit
            } else {
                TruncationReason::ItemLimit
            });
        return true;
    }
    let entry = match entry {
        Ok(entry) => entry,
        Err(err) => {
            ctx.outcome
                .errors
                .push(format!("Failed to read entry in {}: {err}", path.display()));
            return false;
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
            return false;
        }
    };

    ctx.outcome.items_scanned += 1;

    // Both symlinked files and directories are skipped in content search.
    // Reading file contents through symlinks could follow links outside the
    // search tree or into circular structures. This differs from name search,
    // which includes symlinked files in results (only recursing into symlinked
    // directories is prevented there).
    if file_type.is_symlink() {
        return false;
    }

    if file_type.is_dir() {
        if ctx.recursive {
            if let Ok(meta) = entry.metadata()
                && let Some(key) = get_inode_key(&meta)
                && !ctx.visited.insert(key)
            {
                return false;
            }
            search_content_recursive_inner(&entry_path, depth + 1, ctx);
        }
    } else {
        let target_meta = match std::fs::metadata(&entry_path) {
            Ok(meta) => meta,
            Err(err) => {
                ctx.outcome.errors.push(format!(
                    "Failed to read metadata for {}: {err}",
                    entry_path.display()
                ));
                return false;
            }
        };
        if target_meta.is_file() {
            search_in_file(
                &entry_path,
                ctx.pattern,
                ctx.case_sensitive,
                ctx.pattern_bytes,
                target_meta.len(),
                ctx.outcome,
                ctx.cancel,
            );
        }
    }
    false
}

fn search_in_file(
    path: &Path,
    pattern: &str,
    case_sensitive: bool,
    pattern_bytes: &[u8],
    file_len: u64,
    outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
    cancel: Option<&AtomicBool>,
) {
    if pattern.is_empty() {
        return;
    }
    if file_len > MAX_CONTENT_FILE_BYTES {
        if outcome.truncated.is_none() {
            outcome.truncated = Some(TruncationReason::FileTooLarge);
        }
        return;
    }

    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            outcome
                .errors
                .push(format!("Failed to open {}: {err}", path.display()));
            return;
        }
    };

    let mut reader = BufReader::with_capacity(MAX_CONTENT_LINE_BYTES, file);
    let mut ctx = ScanContext {
        path,
        pattern,
        case_sensitive,
        finder: memmem::Finder::new(pattern_bytes),
        bufs: ScanBuffers::new(),
        cancel,
    };
    scan_lines(&mut ctx, &mut reader, outcome);
}

struct ScanContext<'a> {
    path: &'a Path,
    pattern: &'a str,
    case_sensitive: bool,
    finder: memmem::Finder<'a>,
    bufs: ScanBuffers,
    cancel: Option<&'a AtomicBool>,
}

struct ScanBuffers {
    line_buf: Vec<u8>,
    ci_buf: Vec<u8>,
}

impl ScanBuffers {
    fn new() -> Self {
        Self {
            line_buf: Vec::new(),
            ci_buf: Vec::new(),
        }
    }
}

fn scan_lines(
    ctx: &mut ScanContext<'_>,
    reader: &mut BufReader<File>,
    outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
) {
    let mut line_no = 0_usize;
    let mut non_utf8_lines = 0usize;
    loop {
        ctx.bufs.line_buf.clear();
        match reader
            .by_ref()
            .take(MAX_CONTENT_LINE_BYTES as u64)
            .read_until(b'\n', &mut ctx.bufs.line_buf)
        {
            Ok(0) => break,
            Ok(bytes_read) => {
                if ctx.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return;
                }
                line_no += 1;
                let found_newline = ctx.bufs.line_buf.last() == Some(&b'\n');
                if !found_newline && bytes_read == MAX_CONTENT_LINE_BYTES {
                    if outcome.truncated.is_none() {
                        outcome.truncated = Some(TruncationReason::LineTooLong);
                    }
                    ctx.bufs.line_buf.clear();
                    let _ = reader.read_until(b'\n', &mut ctx.bufs.line_buf);
                    continue;
                }
                let line = if found_newline {
                    &ctx.bufs.line_buf[..bytes_read - 1]
                } else {
                    &ctx.bufs.line_buf[..bytes_read]
                };
                if outcome.matches.len() >= MAX_CONTENT_RESULTS {
                    if outcome.truncated.is_none() {
                        outcome.truncated = Some(TruncationReason::ContentResultLimit);
                    }
                    return;
                }
                if memchr(0, line).is_some() {
                    if outcome.truncated.is_none() {
                        outcome.truncated = Some(TruncationReason::BinaryFile);
                    }
                    return;
                }
                if ctx.case_sensitive && memmem::find(line, ctx.pattern.as_bytes()).is_none() {
                    continue;
                }

                let line_text = match std::str::from_utf8(line) {
                    Ok(s) => s.strip_suffix('\r').unwrap_or(s),
                    Err(_) => {
                        non_utf8_lines += 1;
                        if non_utf8_lines <= 3 {
                            outcome.errors.push(format!(
                                "non-UTF-8 line {} in {}",
                                line_no,
                                ctx.path.display()
                            ));
                        }
                        continue;
                    }
                };

                if !ctx.case_sensitive
                    && !contains_case_insensitive(line_text, &ctx.finder, &mut ctx.bufs.ci_buf)
                {
                    continue;
                }

                outcome
                    .matches
                    .push((ctx.path.to_path_buf(), line_no, line_text.to_owned()));
            }
            Err(err) => {
                outcome
                    .errors
                    .push(format!("Failed to read {}: {err}", ctx.path.display()));
                return;
            }
        }
    }
    if non_utf8_lines > 3 {
        outcome.errors.push(format!(
            "... and {} more non-UTF-8 lines in {} (suppressed)",
            non_utf8_lines - 3,
            ctx.path.display()
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;

    use super::*;

    #[test]
    fn test_file_search_search_content() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_content_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut file1 = File::create(dir.join("file1.txt")).unwrap();
        writeln!(file1, "Hello World").unwrap();
        writeln!(file1, "This is a test").unwrap();
        drop(file1);

        let mut file2 = File::create(dir.join("file2.log")).unwrap();
        writeln!(file2, "Goodbye World").unwrap();
        writeln!(file2, "This is a test too").unwrap();
        drop(file2);

        let results = FileSearch::search_content(&dir, "test", true, false);
        assert_eq!(results.len(), 2);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_file_search_empty_query() {
        let dir = std::env::temp_dir().join(format!("lc_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let results = FileSearch::search_content(&dir, "", true, false);
        assert!(results.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_reports_result_limit_truncation() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_limit_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let content = std::iter::repeat_n("needle\n", MAX_CONTENT_RESULTS + 1).collect::<String>();
        fs::write(dir.join("many.txt"), content).unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert_eq!(outcome.matches.len(), MAX_CONTENT_RESULTS);
        assert_eq!(
            outcome.truncated,
            Some(TruncationReason::ContentResultLimit)
        );
        assert!(outcome.errors.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_skips_large_files() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_large_file_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file = File::create(dir.join("large.txt")).unwrap();
        file.set_len(MAX_CONTENT_FILE_BYTES + 1).unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.truncated, Some(TruncationReason::FileTooLarge));
        assert!(outcome.errors.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_skips_binary_files() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_binary_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("binary.bin"), b"needle\0needle\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.truncated, Some(TruncationReason::BinaryFile));
        assert!(outcome.errors.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_counts_skipped_long_lines() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_long_line_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut content = vec![b'a'; MAX_CONTENT_LINE_BYTES + 1];
        content.extend_from_slice(b"\nneedle\n");
        fs::write(dir.join("long_line.txt"), content).unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 2);
        assert_eq!(outcome.truncated, Some(TruncationReason::LineTooLong));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_strips_crlf() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_crlf_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("crlf.txt"), b"hello world\r\nfoo bar\r\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "world", false, false);

        assert_eq!(outcome.matches.len(), 1);
        assert!(!outcome.matches[0].2.contains('\r'));
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_preserves_first_truncation_reason() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_truncation_guard_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let large = File::create(dir.join("aaa_large.txt")).unwrap();
        large.set_len(MAX_CONTENT_FILE_BYTES + 1).unwrap();

        fs::write(dir.join("bbb_binary.bin"), b"needle\0needle\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert!(outcome.matches.is_empty());
        let reason = outcome.truncated.unwrap();
        assert!(reason == TruncationReason::FileTooLarge || reason == TruncationReason::BinaryFile,);
        assert_ne!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_truncation_not_overwritten_by_later_trigger() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_truncation_guard2_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let large = File::create(dir.join("aaa_large.txt")).unwrap();
        large.set_len(MAX_CONTENT_FILE_BYTES + 1).unwrap();

        for i in 0..MAX_CONTENT_RESULTS + 1 {
            fs::write(dir.join(format!("bbb_match_{i}.txt")), "needle\n").unwrap();
        }

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", false, false);

        assert!(outcome.truncated.is_some());
        assert_ne!(
            outcome.truncated,
            Some(TruncationReason::ContentResultLimit)
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_content_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_symlink_content_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);

        fs::create_dir_all(dir.join("root")).unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        fs::write(dir.join("outside/target.txt"), "needle").unwrap();
        symlink(dir.join("outside"), dir.join("root/linkdir")).unwrap();

        let results = FileSearch::search_content(&dir.join("root"), "needle", true, false);
        assert!(results.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_content_case_insensitive_match() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_case_insensitive_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("hello.txt"), "Hello World\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "hello", false, false);
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 1);
        assert!(outcome.errors.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_content_case_sensitive_no_match() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_case_sensitive_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("hello.txt"), "Hello World\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "hello", false, true);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn search_content_skips_symlinked_file() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_symlink_file_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("real.txt"), "needle\n").unwrap();
        symlink(dir.join("real.txt"), dir.join("link.txt")).unwrap();

        let results = FileSearch::search_content(&dir, "needle", false, false);
        assert_eq!(results.len(), 1);
        assert!(results[0].0.ends_with("real.txt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_content_empty_directory() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "lc_search_content_empty_{}_{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", true, false);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }
}
