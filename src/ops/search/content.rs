use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use memchr::{memchr, memmem};

use crate::ops::search::pattern::contains_case_insensitive;
use crate::ops::search::walk::{
    ContentSearchContext, SearchContext, item_limit_reached, prepare_content_dir_scan,
    seed_visited_dir, should_recurse, with_fresh_cancel,
};
use crate::ops::search::{
    MAX_CONTENT_FILE_BYTES, MAX_CONTENT_LINE_BYTES, MAX_CONTENT_RESULTS, MAX_SEARCH_DEPTH,
    MAX_SEARCH_ITEMS, SearchError, SearchErrorKind, SearchOutcome, TruncationReason,
};

/// A content-search hit: the file it was found in, the 1-based line number, and
/// the matched line text. The path is an [`Arc`] so a file with many matches
/// shares one allocation instead of cloning a `PathBuf` per hit.
type ContentMatch = (Arc<Path>, usize, String);

/// Search file contents for `pattern`.
///
/// When `cancel` is `None`, a fresh never-set flag is used (uncancellable run).
pub fn search_content(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    cancel: Option<&AtomicBool>,
) -> SearchOutcome<ContentMatch, SearchError> {
    match cancel {
        Some(c) => search_content_inner(path, pattern, recursive, case_sensitive, c),
        None => {
            with_fresh_cancel(|c| search_content_inner(path, pattern, recursive, case_sensitive, c))
        }
    }
}

fn search_content_inner(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    cancel: &AtomicBool,
) -> SearchOutcome<ContentMatch, SearchError> {
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

fn search_content_recursive(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    depth: usize,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    cancel: Option<&AtomicBool>,
) {
    let pattern_bytes: Vec<u8> = if !case_sensitive {
        pattern.to_lowercase().into_bytes()
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
    let Some(entries) = prepare_content_dir_scan(
        path,
        depth,
        MAX_SEARCH_DEPTH,
        MAX_SEARCH_ITEMS,
        MAX_CONTENT_RESULTS,
        ctx.outcome,
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
    // Content-result cap takes precedence (first reason wins via get_or_insert);
    // the item cap is shared with the name search via item_limit_reached.
    if ctx.outcome.matches.len() >= MAX_CONTENT_RESULTS {
        ctx.outcome
            .truncated
            .get_or_insert(TruncationReason::ContentResultLimit);
        return true;
    }
    if item_limit_reached(ctx.outcome, MAX_SEARCH_ITEMS) {
        return true;
    }
    let entry = match entry {
        Ok(entry) => entry,
        Err(err) => {
            ctx.outcome.errors.push(SearchError {
                path: Some(path.to_path_buf()),
                kind: SearchErrorKind::ReadEntry,
                message: err.to_string(),
            });
            return false;
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
        if ctx.recursive && should_recurse(entry.metadata(), ctx.visited) {
            search_content_recursive_inner(&entry_path, depth + 1, ctx);
        }
    } else {
        // `search_in_file` opens with O_NOFOLLOW and validates the type/size from
        // the opened handle (fstat), so we do not stat the path separately here:
        // a path swapped to a symlink after this dirent read cannot slip a stat
        // past the open and get read.
        search_in_file(
            &entry_path,
            ctx.pattern,
            ctx.case_sensitive,
            ctx.pattern_bytes,
            ctx.outcome,
            ctx.cancel,
        );
    }
    false
}

/// Opens `path` for reading without following a final-component symlink. On Unix
/// this passes `O_NOFOLLOW` (a symlink swapped in makes `open` fail rather than
/// silently redirect) plus `O_NONBLOCK` (so opening a FIFO/device the dirent
/// mislabeled cannot block). Elsewhere it falls back to a plain open; the
/// following fstat type check still rejects anything that is not a regular file.
#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn search_in_file(
    path: &Path,
    pattern: &str,
    case_sensitive: bool,
    pattern_bytes: &[u8],
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    cancel: Option<&AtomicBool>,
) {
    if pattern.is_empty() {
        return;
    }

    let file = match open_no_follow(path) {
        Ok(f) => f,
        Err(err) => {
            outcome.errors.push(SearchError {
                path: Some(path.to_path_buf()),
                kind: SearchErrorKind::OpenFile,
                message: err.to_string(),
            });
            return;
        }
    };

    // Validate the type and size from the SAME handle we will read from (fstat),
    // not a separate path stat, so a symlink swapped in after the dirent read
    // cannot redirect the read outside the search tree.
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(err) => {
            outcome.errors.push(SearchError {
                path: Some(path.to_path_buf()),
                kind: SearchErrorKind::Metadata,
                message: err.to_string(),
            });
            return;
        }
    };
    if !meta.file_type().is_file() {
        return;
    }
    if meta.len() > MAX_CONTENT_FILE_BYTES {
        if outcome.truncated.is_none() {
            outcome.truncated = Some(TruncationReason::FileTooLarge);
        }
        return;
    }

    let mut reader = BufReader::with_capacity(MAX_CONTENT_LINE_BYTES, file);
    let mut ctx = ScanContext {
        path,
        case_sensitive,
        finder: memmem::Finder::new(if case_sensitive {
            pattern.as_bytes()
        } else {
            pattern_bytes
        }),
        bufs: ScanBuffers::new(),
        cancel,
    };
    scan_lines(&mut ctx, &mut reader, outcome);
}

struct ScanContext<'a> {
    path: &'a Path,
    case_sensitive: bool,
    finder: memmem::Finder<'a>,
    bufs: ScanBuffers,
    cancel: Option<&'a AtomicBool>,
}

struct ScanBuffers {
    line_buf: Vec<u8>,
    ci_buf: String,
}

impl ScanBuffers {
    fn new() -> Self {
        Self {
            line_buf: Vec::new(),
            ci_buf: String::with_capacity(1024),
        }
    }
}

/// Does `finder`'s needle occur in this line? Case-sensitive matching searches
/// the raw `line` bytes (`text` is `None`, so non-matching lines skip UTF-8
/// decoding); case-insensitive passes the decoded `text`, folded into `buf`.
fn line_contains_needle(
    finder: &memmem::Finder<'_>,
    line: &[u8],
    text: Option<&str>,
    buf: &mut String,
) -> bool {
    match text {
        None => finder.find(line).is_some(),
        Some(text) => contains_case_insensitive(text, finder, buf),
    }
}

/// Consume the remainder of an over-long line (past `MAX_CONTENT_LINE_BYTES`)
/// up to and including the next newline, leaving `buf` empty for the next line.
fn skip_rest_of_long_line(reader: &mut BufReader<File>, buf: &mut Vec<u8>) -> std::io::Result<()> {
    loop {
        buf.clear();
        let bytes = reader
            .by_ref()
            .take(MAX_CONTENT_LINE_BYTES as u64)
            .read_until(b'\n', buf)?;
        if bytes == 0 || buf.last() == Some(&b'\n') {
            break;
        }
    }
    buf.clear();
    Ok(())
}

/// Record an I/O read failure for `path` on `outcome`.
fn push_read_error(
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    path: &Path,
    err: &std::io::Error,
) {
    outcome.errors.push(SearchError {
        path: Some(path.to_path_buf()),
        kind: SearchErrorKind::ReadFile,
        message: err.to_string(),
    });
}

fn scan_lines(
    ctx: &mut ScanContext<'_>,
    reader: &mut BufReader<File>,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
) {
    // One Arc per file, shared by every match in it (paths are not re-cloned).
    let file_path: Arc<Path> = Arc::from(ctx.path);
    let mut line_no = 0_usize;
    let mut non_utf8_lines = 0usize;
    loop {
        ctx.bufs.line_buf.clear();
        match reader
            .by_ref()
            .take(MAX_CONTENT_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut ctx.bufs.line_buf)
        {
            Ok(0) => break,
            Ok(bytes_read) => {
                if ctx.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return;
                }
                line_no += 1;
                let found_newline = ctx.bufs.line_buf.last() == Some(&b'\n');
                if !found_newline && bytes_read > MAX_CONTENT_LINE_BYTES {
                    if outcome.truncated.is_none() {
                        outcome.truncated = Some(TruncationReason::LineTooLong);
                    }
                    if let Err(err) = skip_rest_of_long_line(reader, &mut ctx.bufs.line_buf) {
                        push_read_error(outcome, ctx.path, &err);
                        return;
                    }
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

                // Case-sensitive prefilter on raw bytes: skip non-matching lines
                // before paying for UTF-8 validation.
                if ctx.case_sensitive
                    && !line_contains_needle(&ctx.finder, line, None, &mut ctx.bufs.ci_buf)
                {
                    continue;
                }

                let line_text = match std::str::from_utf8(line) {
                    Ok(s) => s.strip_suffix('\r').unwrap_or(s),
                    Err(_) => {
                        non_utf8_lines += 1;
                        if non_utf8_lines <= 3 {
                            outcome.errors.push(SearchError {
                                path: Some(ctx.path.to_path_buf()),
                                kind: SearchErrorKind::NonUtf8,
                                message: format!("non-UTF-8 line {line_no}"),
                            });
                        }
                        continue;
                    }
                };

                if !ctx.case_sensitive
                    && !line_contains_needle(
                        &ctx.finder,
                        line,
                        Some(line_text),
                        &mut ctx.bufs.ci_buf,
                    )
                {
                    continue;
                }

                outcome
                    .matches
                    .push((Arc::clone(&file_path), line_no, line_text.to_owned()));
            }
            Err(err) => {
                push_read_error(outcome, ctx.path, &err);
                return;
            }
        }
    }
    if non_utf8_lines > 3 {
        outcome.errors.push(SearchError {
            path: Some(ctx.path.to_path_buf()),
            kind: SearchErrorKind::NonUtf8,
            message: format!(
                "... and {} more non-UTF-8 lines (suppressed)",
                non_utf8_lines - 3
            ),
        });
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

        let results = search_content(&dir, "test", true, false, None).matches;
        assert_eq!(results.len(), 2);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_file_search_empty_query() {
        let dir = std::env::temp_dir().join(format!("lc_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let results = search_content(&dir, "", true, false, None).matches;
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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let outcome = search_content(&dir, "world", false, false, None);

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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let outcome = search_content(&dir, "needle", false, false, None);

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

        let results = search_content(&dir.join("root"), "needle", true, false, None).matches;
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

        fs::write(dir.join("hello.txt"), "Hello World\naŻółć gęślą\n").unwrap();

        let outcome = search_content(&dir, "hello", false, false, None);
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 1);
        assert!(outcome.errors.is_empty());

        let outcome = search_content(&dir, "ŻÓŁĆ", false, true, None);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());

        let outcome = search_content(&dir, "ŻÓŁĆ", false, false, None);
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 2);
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

        let outcome = search_content(&dir, "hello", false, true, None);
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

        let results = search_content(&dir, "needle", false, false, None).matches;
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

        let outcome = search_content(&dir, "needle", true, false, None);
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.truncated, None);

        let _ = fs::remove_dir_all(dir);
    }
}
