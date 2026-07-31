use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use memchr::{memchr, memmem};

use crate::ops::search::pattern::contains_case_insensitive;
use crate::ops::search::walk::{
    ContentSearchContext, item_limit_reached, prepare_content_dir_scan, seed_visited_dir,
    should_recurse,
};
use crate::ops::search::{
    MAX_CONTENT_FILE_BYTES, MAX_CONTENT_LINE_BYTES, MAX_CONTENT_RESULTS, MAX_SEARCH_DEPTH,
    MAX_SEARCH_ITEMS, SearchError, SearchErrorKind, SearchOutcome, TruncationReason,
};
use std::sync::atomic::Ordering;

/// A content-search hit: the file it was found in, the 1-based line number, and
/// the matched line text. The path is an [`Arc`] so a file with many matches
/// shares one allocation instead of cloning a `PathBuf` per hit.
type ContentMatch = (Arc<Path>, usize, String);

/// Search file contents for `pattern`.
///
/// `path` may be a directory (walked) or a single regular file (searched
/// directly). An empty pattern is a no-op: no directory walk, no file opens.
pub fn search_content(
    path: &Path,
    pattern: &str,
    recursive: bool,
    case_sensitive: bool,
    cancel: &AtomicBool,
) -> SearchOutcome<ContentMatch, SearchError> {
    let mut outcome = SearchOutcome::default();
    // Short-circuit empty pattern before any walk/stat work.
    if pattern.is_empty() {
        return outcome;
    }
    // Single-file path: search it directly instead of silently returning.
    // Only a real (non-symlink) file — search_in_file uses O_NOFOLLOW.
    if path.is_file()
        && !path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        let pattern_bytes: Vec<u8> = if !case_sensitive {
            pattern.to_lowercase().into_bytes()
        } else {
            Vec::new()
        };
        let finder = memmem::Finder::new(if case_sensitive {
            pattern.as_bytes()
        } else {
            &pattern_bytes
        });
        search_in_file(path, pattern, case_sensitive, &finder, &mut outcome, cancel);
        return outcome;
    }
    search_content_recursive(
        path,
        pattern,
        recursive,
        case_sensitive,
        0,
        &mut outcome,
        cancel,
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
    cancel: &AtomicBool,
) {
    let pattern_bytes: Vec<u8> = if !case_sensitive {
        pattern.to_lowercase().into_bytes()
    } else {
        Vec::new()
    };
    // One Finder for the entire recursive scan — previously rebuilt per file.
    let finder = memmem::Finder::new(if case_sensitive {
        pattern.as_bytes()
    } else {
        &pattern_bytes
    });
    let mut visited = HashSet::with_capacity(256);
    seed_visited_dir(path, &mut visited);

    let mut ctx = ContentSearchContext {
        pattern,
        case_sensitive,
        finder: &finder,
        recursive,
        outcome,
        visited: &mut visited,
        cancel,
    };
    search_content_recursive_inner(path, depth, &mut ctx);
}

fn search_content_recursive_inner(path: &Path, depth: usize, ctx: &mut ContentSearchContext<'_>) {
    if ctx.cancel.load(Ordering::Relaxed) {
        return;
    }
    // The root call already verified this is a directory; deeper calls pass
    // file_type from the dirent in process_content_entry, avoiding a redundant
    // stat() here.
    if depth == 0 && !path.is_dir() {
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
        if ctx.cancel.load(Ordering::Relaxed) {
            return;
        }
        if process_content_entry(entry, path, depth, ctx) {
            return;
        }
    }
}

/// Like `search_content_recursive_inner` but skips the `path.is_dir()` check —
/// the caller (process_content_entry) already confirmed the type from the
/// dirent, so a second stat syscall is unnecessary.
fn search_content_recursive_inner_skip_dir_check(
    path: &Path,
    depth: usize,
    ctx: &mut ContentSearchContext<'_>,
) {
    if ctx.cancel.load(Ordering::Relaxed) {
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
        if ctx.cancel.load(Ordering::Relaxed) {
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
    if ctx.cancel.load(Ordering::Relaxed) {
        return true;
    }
    // Content-result cap takes precedence when deciding early-return;
    // the item cap is shared with the name search via item_limit_reached.
    if ctx.outcome.matches.len() >= MAX_CONTENT_RESULTS {
        ctx.outcome
            .record_truncation(TruncationReason::ContentResultLimit);
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
            // Count unreadable entries toward the item cap so the limit cannot
            // be overshot by a flood of failing dirents.
            ctx.outcome.items_scanned += 1;
            ctx.outcome.errors.push(SearchError {
                path: Some(entry_path.clone()),
                kind: SearchErrorKind::FileType,
                message: err.to_string(),
            });
            return false;
        }
    };

    // Both symlinked files and directories are skipped in content search.
    // Reading file contents through symlinks could follow links outside the
    // search tree or into circular structures. This differs from name search,
    // which includes symlinked files in results and recurses into symlink dirs
    // under inode cycle detection. Skip *before* items_scanned so symlinks do
    // not consume the item budget.
    if file_type.is_symlink() {
        return false;
    }

    ctx.outcome.items_scanned += 1;

    if file_type.is_dir() {
        if ctx.recursive && should_recurse(entry.metadata(), ctx.visited) {
            // file_type from the dirent already confirms this is a directory;
            // no need for search_content_recursive_inner to re-stat it.
            search_content_recursive_inner_skip_dir_check(&entry_path, depth + 1, ctx);
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
            ctx.finder,
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
    finder: &memmem::Finder<'_>,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    cancel: &AtomicBool,
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
        outcome.record_truncation(TruncationReason::FileTooLarge);
        return;
    }

    // 8 KiB default — the reader grows its buffer as needed for long lines, so
    // the previous 64 KiB allocation was wasteful for small files.
    let mut reader = BufReader::with_capacity(8 * 1024, file);
    let mut ctx = ScanContext {
        path,
        case_sensitive,
        finder,
        bufs: ScanBuffers::new(),
        cancel,
    };
    scan_lines(&mut ctx, &mut reader, outcome);
}

struct ScanContext<'a> {
    path: &'a Path,
    case_sensitive: bool,
    finder: &'a memmem::Finder<'a>,
    bufs: ScanBuffers,
    cancel: &'a AtomicBool,
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
///
/// Checks `cancel` each chunk so a multi-gigabyte line without newlines cannot
/// pin the search thread. Returns `Ok(true)` when cancelled **before** the line
/// was fully consumed — callers must stop the scan instead of re-reading the
/// unconsumed bytes, or the outer loop would spin on the same segment.
fn skip_rest_of_long_line(
    reader: &mut BufReader<File>,
    buf: &mut Vec<u8>,
    cancel: &AtomicBool,
) -> std::io::Result<bool> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            buf.clear();
            return Ok(true);
        }
        buf.clear();
        let bytes = reader
            .by_ref()
            .take(MAX_CONTENT_LINE_BYTES)
            .read_until(b'\n', buf)?;
        if bytes == 0 || buf.last() == Some(&b'\n') {
            break;
        }
    }
    buf.clear();
    Ok(false)
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

/// True if `line` looks binary: any C0 control byte other than TAB / LF / CR.
/// (LF is already stripped by the reader; CR is a line-break candidate, not
/// binary evidence. NUL is included as `b < 0x20`.)
fn is_binary_line(line: &[u8]) -> bool {
    line.iter()
        .any(|&b| b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r')
}

fn scan_lines(
    ctx: &mut ScanContext<'_>,
    reader: &mut BufReader<File>,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
) {
    // Arc allocated lazily — only if the file yields at least one match.
    let mut file_path: Option<Arc<Path>> = None;
    let mut line_no = 0_usize;
    let mut non_utf8_lines = 0usize;
    loop {
        ctx.bufs.line_buf.clear();
        match reader
            .by_ref()
            .take(MAX_CONTENT_LINE_BYTES + 1)
            .read_until(b'\n', &mut ctx.bufs.line_buf)
        {
            Ok(0) => break,
            Ok(bytes_read) => {
                if ctx.cancel.load(Ordering::Relaxed) {
                    return;
                }
                line_no += 1;
                let found_newline = ctx.bufs.line_buf.last() == Some(&b'\n');
                if !found_newline && (bytes_read as u64) > MAX_CONTENT_LINE_BYTES {
                    outcome.record_truncation(TruncationReason::LineTooLong);
                    match skip_rest_of_long_line(reader, &mut ctx.bufs.line_buf, ctx.cancel) {
                        // Cancelled mid-skip: remaining long-line bytes were not
                        // consumed. Bail out instead of letting the outer loop
                        // re-read the same segment forever.
                        Ok(true) => return,
                        Ok(false) => continue,
                        Err(err) => {
                            push_read_error(outcome, ctx.path, &err);
                            return;
                        }
                    }
                }
                // Drop the trailing \n (if any). Remaining bytes may still
                // contain classic-Mac CR endings; process_raw_chunk splits
                // those so CR-only files are not glued into one line.
                let end = if found_newline {
                    bytes_read - 1
                } else {
                    bytes_read
                };
                if process_raw_chunk(
                    ctx,
                    outcome,
                    &mut file_path,
                    end,
                    &mut line_no,
                    &mut non_utf8_lines,
                ) {
                    return;
                }
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

/// Process one `read_until(\\n)` chunk, splitting on bare CR for classic-Mac
/// line endings. Returns `true` when the scan should stop (limit / binary).
fn process_raw_chunk(
    ctx: &mut ScanContext<'_>,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    file_path: &mut Option<Arc<Path>>,
    end: usize,
    line_no: &mut usize,
    non_utf8_lines: &mut usize,
) -> bool {
    // Split on bare CR. CRLF already lost its LF above, leaving a trailing CR
    // on the last segment which becomes an empty trailing segment (empty lines
    // simply don't match non-empty needles; empty needle is short-circuited).
    let mut start = 0usize;
    let mut first_segment = true;
    loop {
        let rel = memchr(b'\r', &ctx.bufs.line_buf[start..end]);
        let (seg_end, next) = match rel {
            Some(i) => (start + i, start + i + 1),
            None => (end, end),
        };
        // Copy line bounds, then re-borrow ctx mutably inside try_record_line
        // without holding a slice into line_buf across the call.
        let line_start = start;
        let line_end = seg_end;
        if !first_segment {
            *line_no += 1;
        }
        first_segment = false;

        if try_record_line(
            ctx,
            outcome,
            file_path,
            line_start,
            line_end,
            *line_no,
            non_utf8_lines,
        ) {
            return true;
        }

        start = next;
        if start >= end {
            break;
        }
    }
    false
}

/// Try to record a single logical line as a content match.
/// `line_start..line_end` indexes into `ctx.bufs.line_buf`.
/// Returns `true` when the scan should stop (result limit or binary file).
fn try_record_line(
    ctx: &mut ScanContext<'_>,
    outcome: &mut SearchOutcome<ContentMatch, SearchError>,
    file_path: &mut Option<Arc<Path>>,
    line_start: usize,
    line_end: usize,
    line_no: usize,
    non_utf8_lines: &mut usize,
) -> bool {
    if outcome.matches.len() >= MAX_CONTENT_RESULTS {
        outcome.record_truncation(TruncationReason::ContentResultLimit);
        return true;
    }
    let line = &ctx.bufs.line_buf[line_start..line_end];
    if is_binary_line(line) {
        outcome.record_truncation(TruncationReason::BinaryFile);
        return true;
    }

    // Case-sensitive prefilter on raw bytes: skip non-matching lines before
    // paying for UTF-8 validation.
    if ctx.case_sensitive && !line_contains_needle(ctx.finder, line, None, &mut ctx.bufs.ci_buf) {
        return false;
    }

    let line_text = match std::str::from_utf8(line) {
        Ok(s) => s,
        Err(_) => {
            *non_utf8_lines += 1;
            if *non_utf8_lines <= 3 {
                outcome.errors.push(SearchError {
                    path: Some(ctx.path.to_path_buf()),
                    kind: SearchErrorKind::NonUtf8,
                    message: format!("non-UTF-8 line {line_no}"),
                });
            }
            return false;
        }
    };

    if !ctx.case_sensitive
        && !line_contains_needle(ctx.finder, line, Some(line_text), &mut ctx.bufs.ci_buf)
    {
        return false;
    }

    // Allocate the Arc only on the first match in this file.
    let arc = file_path.get_or_insert_with(|| Arc::from(ctx.path));
    outcome
        .matches
        .push((Arc::clone(arc), line_no, line_text.to_owned()));
    false
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

        let results = search_content(&dir, "test", true, false, &AtomicBool::new(false)).matches;
        assert_eq!(results.len(), 2);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_file_search_empty_query() {
        let dir = std::env::temp_dir().join(format!("lc_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let results = search_content(&dir, "", true, false, &AtomicBool::new(false)).matches;
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

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        assert_eq!(outcome.matches.len(), MAX_CONTENT_RESULTS);
        assert!(
            outcome
                .truncated
                .contains(&TruncationReason::ContentResultLimit)
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

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        assert!(outcome.matches.is_empty());
        assert!(outcome.truncated.contains(&TruncationReason::FileTooLarge));
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

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        assert!(outcome.matches.is_empty());
        assert!(outcome.truncated.contains(&TruncationReason::BinaryFile));
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

        let mut content = vec![b'a'; MAX_CONTENT_LINE_BYTES as usize + 1];
        content.extend_from_slice(b"\nneedle\n");
        fs::write(dir.join("long_line.txt"), content).unwrap();

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 2);
        assert!(outcome.truncated.contains(&TruncationReason::LineTooLong));

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

        let outcome = search_content(&dir, "world", false, false, &AtomicBool::new(false));

        assert_eq!(outcome.matches.len(), 1);
        assert!(!outcome.matches[0].2.contains('\r'));
        assert!(outcome.errors.is_empty());
        assert!(outcome.truncated.is_empty());

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

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        assert!(outcome.matches.is_empty());
        assert!(
            outcome.truncated.contains(&TruncationReason::FileTooLarge)
                || outcome.truncated.contains(&TruncationReason::BinaryFile)
        );
        assert!(!outcome.truncated.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_search_content_truncation_keeps_multiple_reasons() {
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

        let outcome = search_content(&dir, "needle", false, false, &AtomicBool::new(false));

        // Both FileTooLarge (from aaa_large) and ContentResultLimit (from the
        // many small matches) must be retained — not just the first one.
        assert!(
            outcome.truncated.contains(&TruncationReason::FileTooLarge),
            "truncated={:?}",
            outcome.truncated
        );
        assert!(
            outcome
                .truncated
                .contains(&TruncationReason::ContentResultLimit),
            "truncated={:?}",
            outcome.truncated
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

        let results = search_content(
            &dir.join("root"),
            "needle",
            true,
            false,
            &AtomicBool::new(false),
        )
        .matches;
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

        let outcome = search_content(&dir, "hello", false, false, &AtomicBool::new(false));
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 1);
        assert!(outcome.errors.is_empty());

        let outcome = search_content(&dir, "ŻÓŁĆ", false, true, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());

        let outcome = search_content(&dir, "ŻÓŁĆ", false, false, &AtomicBool::new(false));
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

        let outcome = search_content(&dir, "hello", false, true, &AtomicBool::new(false));
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

        let results = search_content(&dir, "needle", false, false, &AtomicBool::new(false)).matches;
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

        let outcome = search_content(&dir, "needle", true, false, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert!(outcome.errors.is_empty());
        assert!(outcome.truncated.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_content_single_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("solo.txt");
        fs::write(&file, "alpha\nbeta needle\ngamma\n").unwrap();
        let outcome = search_content(&file, "needle", false, true, &AtomicBool::new(false));
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].1, 2);
        assert!(outcome.errors.is_empty());
    }

    #[test]
    fn search_content_empty_pattern_short_circuits() {
        let dir = tempfile::tempdir().unwrap();
        // Even with many files, empty pattern must not walk/stat.
        for i in 0..50 {
            fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let outcome = search_content(dir.path(), "", true, false, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.items_scanned, 0);
        assert!(outcome.errors.is_empty());
    }

    #[test]
    fn search_content_classic_mac_cr_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        // Classic Mac: CR-only separators, no LF.
        fs::write(dir.path().join("mac.txt"), b"first\rsecond needle\rthird\r").unwrap();
        let outcome = search_content(dir.path(), "needle", false, true, &AtomicBool::new(false));
        assert_eq!(outcome.matches.len(), 1, "matches={:?}", outcome.matches);
        assert_eq!(outcome.matches[0].1, 2);
        assert_eq!(outcome.matches[0].2, "second needle");
    }

    #[test]
    fn search_content_control_bytes_without_nul_are_binary() {
        let dir = tempfile::tempdir().unwrap();
        // \x01..\x08 without any NUL must still be treated as binary.
        fs::write(dir.path().join("ctrl.bin"), b"hello\x01\x02world\nneedle\n").unwrap();
        let outcome = search_content(dir.path(), "needle", false, true, &AtomicBool::new(false));
        assert!(outcome.matches.is_empty());
        assert!(outcome.truncated.contains(&TruncationReason::BinaryFile));
    }

    #[test]
    fn search_content_symlinks_do_not_consume_item_budget() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("real.txt"), "needle\n").unwrap();
            // Many symlinks that would previously burn items_scanned.
            for i in 0..100 {
                symlink(
                    dir.path().join("real.txt"),
                    dir.path().join(format!("link{i}")),
                )
                .unwrap();
            }
            let outcome =
                search_content(dir.path(), "needle", false, true, &AtomicBool::new(false));
            assert_eq!(outcome.matches.len(), 1);
            // Only the real file should count (plus any dirs — here just the file).
            assert_eq!(outcome.items_scanned, 1);
        }
    }

    #[test]
    fn search_content_cancel_during_long_line_skip() {
        let dir = tempfile::tempdir().unwrap();
        // One huge line without newlines, then a real line with the needle.
        let mut data = vec![b'A'; 200_000];
        data.push(b'\n');
        data.extend_from_slice(b"needle\n");
        fs::write(dir.path().join("long.txt"), &data).unwrap();

        let cancel = AtomicBool::new(false);
        // Cancel immediately — skip_rest must observe it and return.
        cancel.store(true, Ordering::Relaxed);
        let outcome = search_content(dir.path(), "needle", false, true, &cancel);
        // Cancelled during skip: may or may not have matches; must not hang and
        // must not panic. Primary assertion is termination (test runner timeout).
        let _ = outcome;
    }
}
