//! File searching operations for Libre Commander (lc).
//!
//! Full file search by name pattern or content.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::FileEntry;
use crate::fs::reader::get_file_info;
use crate::ops::helpers::get_inode_key;
use memchr::{memchr, memmem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    DepthLimit,
    ItemLimit,
    ContentResultLimit,
    FileTooLarge,
    LineTooLong,
    BinaryFile,
}

#[derive(Debug, Clone)]
pub struct SearchOutcome<T> {
    pub matches: Vec<T>,
    pub errors: Vec<String>,
    pub truncated: Option<TruncationReason>,
}

impl<T> Default for SearchOutcome<T> {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            errors: Vec::new(),
            truncated: None,
        }
    }
}

pub const MAX_SEARCH_DEPTH: usize = 20;
pub const MAX_SEARCH_ITEMS: usize = 10000;

pub const MAX_CONTENT_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_CONTENT_LINE_BYTES: usize = 64 * 1024;
pub const MAX_CONTENT_RESULTS: usize = 1000;

/// Inline char buffer that lives on the stack for sizes <= N,
/// falling back to heap allocation for larger sizes.
struct SmallCharBuf<const N: usize> {
    inline: [char; N],
    heap: Option<Vec<char>>,
}

impl<const N: usize> SmallCharBuf<N> {
    fn new(len: usize) -> Self {
        let inline = ['\0'; N];
        let heap = if len > N { Some(vec!['\0'; len]) } else { None };
        Self { inline, heap }
    }
}

impl<const N: usize> std::ops::Index<usize> for SmallCharBuf<N> {
    type Output = char;
    fn index(&self, index: usize) -> &char {
        match &self.heap {
            Some(v) => &v[index],
            None => &self.inline[index],
        }
    }
}

impl<const N: usize> std::ops::IndexMut<usize> for SmallCharBuf<N> {
    fn index_mut(&mut self, index: usize) -> &mut char {
        match &mut self.heap {
            Some(v) => &mut v[index],
            None => &mut self.inline[index],
        }
    }
}

pub enum CompiledPattern {
    Plain {
        needle: Vec<char>,
        needle_str: String,
        needle_ascii: Option<String>,
        insensitive: bool,
    },
    WildcardSimple {
        prefix: Option<Vec<char>>,
        suffix: Option<Vec<char>>,
        contains: bool,
        insensitive: bool,
    },
    WildcardDp {
        chars: Vec<char>,
        insensitive: bool,
    },
}

impl CompiledPattern {
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        let insensitive = !case_sensitive;

        if !pattern.contains(['*', '?']) {
            let needle: Vec<char> = if insensitive {
                pattern.chars().flat_map(|c| c.to_lowercase()).collect()
            } else {
                pattern.chars().collect()
            };
            let needle_ascii = if insensitive && pattern.is_ascii() {
                Some(pattern.to_ascii_lowercase())
            } else {
                None
            };
            let needle_str: String = needle.iter().collect();
            return Self::Plain {
                needle,
                needle_str,
                needle_ascii,
                insensitive,
            };
        }

        if let Some(simple) = Self::try_simple_wildcard(pattern, insensitive) {
            return simple;
        }

        let chars = if insensitive {
            pattern.chars().flat_map(|c| c.to_lowercase()).collect()
        } else {
            pattern.chars().collect()
        };
        Self::WildcardDp { chars, insensitive }
    }

    fn try_simple_wildcard(pattern: &str, insensitive: bool) -> Option<Self> {
        if pattern.contains('?') {
            return None;
        }
        let star_count = pattern.chars().filter(|&c| c == '*').count();
        if star_count == 1 {
            let pos = pattern.find('*')?;
            let prefix_str = &pattern[..pos];
            let suffix_str = &pattern[pos + 1..];
            let prefix = Self::maybe_lower(prefix_str, insensitive);
            let suffix = Self::maybe_lower(suffix_str, insensitive);
            return Some(Self::WildcardSimple {
                prefix,
                suffix,
                contains: false,
                insensitive,
            });
        }
        if star_count == 2 {
            let f = pattern.find('*')?;
            let l = pattern.rfind('*')?;
            if l <= f {
                return None;
            }
            let prefix_str = &pattern[..f];
            let inner_str = &pattern[f + 1..l];
            let suffix_str = &pattern[l + 1..];
            if inner_str.is_empty() {
                return None;
            }
            let prefix_empty = prefix_str.is_empty();
            let suffix_empty = suffix_str.is_empty();
            if prefix_empty && suffix_empty {
                let inner = Self::maybe_lower(inner_str, insensitive)?;
                return Some(Self::WildcardSimple {
                    prefix: None,
                    suffix: Some(inner),
                    contains: true,
                    insensitive,
                });
            }
        }
        None
    }

    fn maybe_lower(s: &str, insensitive: bool) -> Option<Vec<char>> {
        if s.is_empty() {
            return None;
        }
        Some(if insensitive {
            s.chars().flat_map(|c| c.to_lowercase()).collect()
        } else {
            s.chars().collect()
        })
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Plain {
                needle,
                needle_str: _,
                needle_ascii,
                insensitive: true,
            } => {
                if needle.is_empty() {
                    return true;
                }
                if let Some(ascii_needle) = needle_ascii
                    && name.is_ascii()
                {
                    return name
                        .as_bytes()
                        .windows(ascii_needle.len())
                        .any(|w| w.eq_ignore_ascii_case(ascii_needle.as_bytes()));
                }
                Self::contains_case_insensitive_compiled(name, needle)
            }
            Self::Plain {
                needle: _,
                needle_str,
                needle_ascii: _,
                insensitive: false,
            } => {
                if needle_str.is_empty() {
                    return true;
                }
                name.contains(needle_str.as_str())
            }
            Self::WildcardSimple {
                prefix,
                suffix,
                contains,
                insensitive,
            } => {
                let name_chars: Vec<char> = if *insensitive {
                    name.chars().flat_map(|c| c.to_lowercase()).collect()
                } else {
                    name.chars().collect()
                };
                if *contains {
                    return suffix.as_deref().is_some_and(|suffix_chars| {
                        name_chars
                            .windows(suffix_chars.len())
                            .any(|window| window == suffix_chars)
                    });
                }
                let prefix_len = prefix.as_ref().map_or(0, |p: &Vec<char>| p.len());
                let suffix_len = suffix.as_ref().map_or(0, |s: &Vec<char>| s.len());
                if name_chars.len() < prefix_len + suffix_len {
                    return false;
                }
                if let Some(prefix_chars) = prefix {
                    if name_chars.len() < prefix_chars.len() {
                        return false;
                    }
                    if name_chars[..prefix_chars.len()] != prefix_chars[..] {
                        return false;
                    }
                }
                if let Some(suffix_chars) = suffix {
                    if name_chars.len() < suffix_chars.len() {
                        return false;
                    }
                    let start = name_chars.len() - suffix_chars.len();
                    if name_chars[start..] != suffix_chars[..] {
                        return false;
                    }
                }
                true
            }
            // NOTE: to_lowercase() can expand one char to multiple (e.g. İ → i + \u{307}).
            // The matcher treats each folded char independently, so `?` may partially
            // match a multi-char lowercase expansion. Known limitation for Turkish İ,
            // German ß, and similar. Full fix requires index-map from original positions
            // to folded ranges.
            Self::WildcardDp { chars, insensitive } => {
                let name_chars: Vec<char> = if *insensitive {
                    name.chars().flat_map(|c| c.to_lowercase()).collect()
                } else {
                    name.chars().collect()
                };
                Self::greedy_wildcard_match(&name_chars, chars)
            }
        }
    }

    fn contains_case_insensitive_compiled(haystack: &str, needle_lower: &[char]) -> bool {
        if needle_lower.is_empty() {
            return true;
        }
        let needle_len = needle_lower.len();
        let mut buf = SmallCharBuf::<64>::new(needle_len);
        let mut filled = 0usize;
        let mut head = 0usize;

        for c in haystack.chars().flat_map(|c| c.to_lowercase()) {
            buf[head] = c;
            head = (head + 1) % needle_len;
            if filled < needle_len {
                filled += 1;
            }
            if filled == needle_len {
                let mut all_match = true;
                for (i, &nc) in needle_lower.iter().enumerate() {
                    let idx = (head + i) % needle_len;
                    if buf[idx] != nc {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    return true;
                }
            }
        }
        false
    }

    fn greedy_wildcard_match(name: &[char], pattern: &[char]) -> bool {
        let mut ni = 0;
        let mut pi = 0;
        let mut star_pi: Option<usize> = None;
        let mut star_ni = 0;
        while ni < name.len() {
            match pattern.get(pi) {
                Some('*') => {
                    star_pi = Some(pi);
                    star_ni = ni;
                    pi += 1;
                }
                Some('?') => {
                    ni += 1;
                    pi += 1;
                }
                Some(c) if name[ni] == *c => {
                    ni += 1;
                    pi += 1;
                }
                _ => match star_pi {
                    Some(sp) => {
                        star_ni += 1;
                        ni = star_ni;
                        pi = sp + 1;
                    }
                    None => return false,
                },
            }
        }
        while pi < pattern.len() && pattern[pi] == '*' {
            pi += 1;
        }
        pi == pattern.len()
    }
}

pub struct FileSearch;

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
        let mut outcome = SearchOutcome::default();
        let mut item_count: usize = 0;
        let compiled_pattern = CompiledPattern::new(pattern, case_sensitive);
        let mut visited = HashSet::new();
        Self::seed_visited_dir(path, &mut visited);
        Self::search_files_recursive(
            path,
            &compiled_pattern,
            recursive,
            &mut outcome,
            0,
            &mut item_count,
            &mut visited,
        );
        outcome
    }

    pub fn search_files_with_diagnostics_cancellable(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        cancel: &AtomicBool,
    ) -> SearchOutcome<FileEntry> {
        let mut outcome = SearchOutcome::default();
        let mut item_count: usize = 0;
        let compiled_pattern = CompiledPattern::new(pattern, case_sensitive);
        let mut visited = HashSet::new();
        Self::seed_visited_dir(path, &mut visited);
        Self::search_files_recursive_cancellable(
            path,
            &compiled_pattern,
            recursive,
            &mut outcome,
            0,
            &mut item_count,
            &mut visited,
            cancel,
        );
        outcome
    }

    fn seed_visited_dir(path: &Path, visited: &mut HashSet<(u64, u64)>) {
        if let Ok(meta) = std::fs::metadata(path)
            && meta.is_dir()
            && let Some(key) = get_inode_key(&meta)
        {
            visited.insert(key);
        }
    }

    fn prepare_dir_scan<T>(
        path: &Path,
        depth: usize,
        max_depth: usize,
        max_items: usize,
        item_count: &mut usize,
        outcome: &mut SearchOutcome<T>,
        extra_guard: impl Fn(&SearchOutcome<T>) -> bool,
    ) -> Option<std::fs::ReadDir> {
        if !extra_guard(outcome) {
            outcome
                .truncated
                .get_or_insert(TruncationReason::ContentResultLimit);
            return None;
        }
        if depth >= max_depth {
            outcome
                .truncated
                .get_or_insert(TruncationReason::DepthLimit);
            return None;
        }
        if *item_count >= max_items {
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

    fn search_files_recursive(
        path: &Path,
        pattern: &CompiledPattern,
        recursive: bool,
        outcome: &mut SearchOutcome<FileEntry>,
        depth: usize,
        item_count: &mut usize,
        visited: &mut HashSet<(u64, u64)>,
    ) {
        if !path.is_dir() {
            outcome
                .errors
                .push(format!("Not a directory: {}", path.display()));
            return;
        }
        let Some(entries) = Self::prepare_dir_scan(
            path,
            depth,
            MAX_SEARCH_DEPTH,
            MAX_SEARCH_ITEMS,
            item_count,
            outcome,
            |_| true,
        ) else {
            return;
        };

        for entry in entries {
            if *item_count >= MAX_SEARCH_ITEMS {
                outcome.truncated.get_or_insert(TruncationReason::ItemLimit);
                return;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    outcome
                        .errors
                        .push(format!("Failed to read entry in {}: {err}", path.display()));
                    continue;
                }
            };
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    outcome.errors.push(format!(
                        "Failed to read type for {}: {err}",
                        entry_path.display()
                    ));
                    continue;
                }
            };

            *item_count += 1;

            let name = entry.file_name();
            let name_lossy = name.to_string_lossy();
            if pattern.matches(&name_lossy) {
                match get_file_info(&entry_path) {
                    Ok(file_entry) => outcome.matches.push(file_entry),
                    Err(err) => outcome.errors.push(format!(
                        "Failed to read metadata for {}: {err}",
                        entry_path.display()
                    )),
                }
            }

            if file_type.is_symlink() {
                continue;
            }

            if recursive && file_type.is_dir() {
                if let Ok(meta) = entry.metadata()
                    && let Some(key) = get_inode_key(&meta)
                    && !visited.insert(key)
                {
                    continue;
                }
                Self::search_files_recursive(
                    &entry_path,
                    pattern,
                    recursive,
                    outcome,
                    depth + 1,
                    item_count,
                    visited,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn search_files_recursive_cancellable(
        path: &Path,
        pattern: &CompiledPattern,
        recursive: bool,
        outcome: &mut SearchOutcome<FileEntry>,
        depth: usize,
        item_count: &mut usize,
        visited: &mut HashSet<(u64, u64)>,
        cancel: &AtomicBool,
    ) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if !path.is_dir() {
            outcome
                .errors
                .push(format!("Not a directory: {}", path.display()));
            return;
        }
        let Some(entries) = Self::prepare_dir_scan(
            path,
            depth,
            MAX_SEARCH_DEPTH,
            MAX_SEARCH_ITEMS,
            item_count,
            outcome,
            |_| true,
        ) else {
            return;
        };

        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if *item_count >= MAX_SEARCH_ITEMS {
                outcome.truncated.get_or_insert(TruncationReason::ItemLimit);
                return;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    outcome
                        .errors
                        .push(format!("Failed to read entry in {}: {err}", path.display()));
                    continue;
                }
            };
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    outcome.errors.push(format!(
                        "Failed to read type for {}: {err}",
                        entry_path.display()
                    ));
                    continue;
                }
            };

            *item_count += 1;

            let name = entry.file_name();
            let name_lossy = name.to_string_lossy();
            if pattern.matches(&name_lossy) {
                match get_file_info(&entry_path) {
                    Ok(file_entry) => outcome.matches.push(file_entry),
                    Err(err) => outcome.errors.push(format!(
                        "Failed to read metadata for {}: {err}",
                        entry_path.display()
                    )),
                }
            }

            if file_type.is_symlink() {
                continue;
            }

            if recursive && file_type.is_dir() {
                if let Ok(meta) = entry.metadata()
                    && let Some(key) = get_inode_key(&meta)
                    && !visited.insert(key)
                {
                    continue;
                }
                Self::search_files_recursive_cancellable(
                    &entry_path,
                    pattern,
                    recursive,
                    outcome,
                    depth + 1,
                    item_count,
                    visited,
                    cancel,
                );
            }
        }
    }

    #[allow(dead_code)]
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
        let mut outcome = SearchOutcome::default();
        let mut item_count: usize = 0;
        Self::search_content_recursive(
            path,
            pattern,
            recursive,
            case_sensitive,
            0,
            &mut outcome,
            &mut item_count,
        );
        outcome
    }

    pub fn search_content_recursive(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        depth: usize,
        outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
        item_count: &mut usize,
    ) {
        let pattern_lower: Vec<char> = if !case_sensitive {
            pattern.chars().flat_map(|c| c.to_lowercase()).collect()
        } else {
            Vec::new()
        };
        let mut visited = HashSet::new();
        Self::seed_visited_dir(path, &mut visited);

        Self::search_content_recursive_inner(
            path,
            pattern,
            case_sensitive,
            &pattern_lower,
            recursive,
            depth,
            outcome,
            item_count,
            &mut visited,
        );
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    fn search_content_recursive_inner(
        path: &Path,
        pattern: &str,
        case_sensitive: bool,
        pattern_lower: &[char],
        recursive: bool,
        depth: usize,
        outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
        item_count: &mut usize,
        visited: &mut HashSet<(u64, u64)>,
    ) {
        if !path.is_dir() {
            return;
        }
        let Some(entries) = Self::prepare_dir_scan(
            path,
            depth,
            MAX_SEARCH_DEPTH,
            MAX_SEARCH_ITEMS,
            item_count,
            outcome,
            |o| o.matches.len() < MAX_CONTENT_RESULTS,
        ) else {
            return;
        };

        for entry in entries {
            if *item_count >= MAX_SEARCH_ITEMS || outcome.matches.len() >= MAX_CONTENT_RESULTS {
                outcome
                    .truncated
                    .get_or_insert(if outcome.matches.len() >= MAX_CONTENT_RESULTS {
                        TruncationReason::ContentResultLimit
                    } else {
                        TruncationReason::ItemLimit
                    });
                return;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    outcome
                        .errors
                        .push(format!("Failed to read entry in {}: {err}", path.display()));
                    continue;
                }
            };
            let entry_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    outcome.errors.push(format!(
                        "Failed to read type for {}: {err}",
                        entry_path.display()
                    ));
                    continue;
                }
            };

            *item_count += 1;

            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                if recursive {
                    if let Ok(meta) = entry.metadata()
                        && let Some(key) = get_inode_key(&meta)
                        && !visited.insert(key)
                    {
                        continue;
                    }
                    Self::search_content_recursive_inner(
                        &entry_path,
                        pattern,
                        case_sensitive,
                        pattern_lower,
                        recursive,
                        depth + 1,
                        outcome,
                        item_count,
                        visited,
                    );
                }
            } else {
                let target_meta = match std::fs::metadata(&entry_path) {
                    Ok(meta) => meta,
                    Err(err) => {
                        outcome.errors.push(format!(
                            "Failed to read metadata for {}: {err}",
                            entry_path.display()
                        ));
                        continue;
                    }
                };
                if target_meta.is_file() {
                    Self::search_in_file(
                        &entry_path,
                        pattern,
                        case_sensitive,
                        pattern_lower,
                        target_meta.len(),
                        outcome,
                    );
                }
            }
        }
    }

    fn search_in_file(
        path: &Path,
        pattern: &str,
        case_sensitive: bool,
        pattern_lower: &[char],
        file_len: u64,
        outcome: &mut SearchOutcome<(PathBuf, usize, String)>,
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
        let mut line_buf = Vec::new();
        let mut line_no = 0_usize;
        let mut non_utf8_lines = 0usize;
        loop {
            line_buf.clear();
            match reader.read_until(b'\n', &mut line_buf) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    line_no += 1;
                    let line = if line_buf.last() == Some(&b'\n') {
                        &line_buf[..bytes_read - 1]
                    } else {
                        &line_buf[..bytes_read]
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
                    if line.len() > MAX_CONTENT_LINE_BYTES {
                        if outcome.truncated.is_none() {
                            outcome.truncated = Some(TruncationReason::LineTooLong);
                        }
                        continue;
                    }
                    if case_sensitive && memmem::find(line, pattern.as_bytes()).is_none() {
                        continue;
                    }

                    let line_text = match std::str::from_utf8(line) {
                        Ok(s) => s.strip_suffix('\r').unwrap_or(s).to_owned(),
                        Err(_) => {
                            non_utf8_lines += 1;
                            if non_utf8_lines <= 3 {
                                outcome.errors.push(format!(
                                    "non-UTF-8 line {} in {}",
                                    line_no,
                                    path.display()
                                ));
                            }
                            continue;
                        }
                    };
                    let match_found = if case_sensitive {
                        line_text.contains(pattern)
                    } else {
                        Self::contains_case_insensitive(&line_text, pattern_lower)
                    };

                    if match_found {
                        outcome
                            .matches
                            .push((path.to_path_buf(), line_no, line_text));
                    }
                }
                Err(err) => {
                    outcome
                        .errors
                        .push(format!("Failed to read {}: {err}", path.display()));
                    return;
                }
            }
        }
        if non_utf8_lines > 3 {
            outcome.errors.push(format!(
                "... and {} more non-UTF-8 lines in {} (suppressed)",
                non_utf8_lines - 3,
                path.display()
            ));
        }
    }

    /// Case-insensitive substring search over Unicode lowercase chars.
    /// The circular buffer stays on the stack for needles up to 64 chars.
    fn contains_case_insensitive(haystack: &str, needle_lower: &[char]) -> bool {
        if needle_lower.is_empty() {
            return true;
        }
        let needle_len = needle_lower.len();
        let mut buf = SmallCharBuf::<64>::new(needle_len);
        let mut filled = 0usize;
        let mut head = 0usize;

        for c in haystack.chars().flat_map(|c| c.to_lowercase()) {
            buf[head] = c;
            head = (head + 1) % needle_len;
            if filled < needle_len {
                filled += 1;
            }
            if filled == needle_len {
                let mut all_match = true;
                for (i, &nc) in needle_lower.iter().enumerate() {
                    let idx = (head + i) % needle_len;
                    if buf[idx] != nc {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    return true;
                }
            }
        }
        false
    }

    pub fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
        let compiled = CompiledPattern::new(pattern, case_sensitive);
        compiled.matches(name)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_file_search_matches_pattern_exact() {
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", false));
    }

    #[test]
    fn test_file_search_matches_pattern_plain_contains() {
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "file",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            false
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_star() {
        assert!(FileSearch::matches_pattern("file.txt", "*.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.*", true));
        assert!(FileSearch::matches_pattern("file.txt", "*", true));
        assert!(FileSearch::matches_pattern(
            "prefix-foo-suffix",
            "*foo*",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "long_file_name.txt",
            "*.txt",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order() {
        assert!(FileSearch::matches_pattern(
            "pre-mid-tail",
            "pre*mid*",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "head-mid-suf",
            "*mid*suf",
            true
        ));
        assert!(FileSearch::matches_pattern("abXYcdZZ", "ab*cd*", true));
        assert!(FileSearch::matches_pattern("ZZabXYcd", "*ab*cd", true));
        assert!(FileSearch::matches_pattern(
            "preXmidYsuf",
            "pre*mid*suf",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_order_false() {
        assert!(!FileSearch::matches_pattern(
            "mid-tail-pre",
            "pre*mid*",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "head-suf-mid",
            "*mid*suf",
            true
        ));
        assert!(!FileSearch::matches_pattern("cdXYabZZ", "ab*cd*", true));
        assert!(!FileSearch::matches_pattern("ZZcdXYab", "*ab*cd", true));
        assert!(!FileSearch::matches_pattern(
            "preXsufYmid",
            "pre*mid*suf",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_multi_star_case_insensitive() {
        assert!(FileSearch::matches_pattern(
            "PRE-MID-tail",
            "pre*mid*",
            false
        ));
        assert!(FileSearch::matches_pattern(
            "head-MID-SUF",
            "*mid*suf",
            false
        ));
        assert!(FileSearch::matches_pattern("ABxyCDzz", "ab*cd*", false));
        assert!(FileSearch::matches_pattern("zzABxyCD", "*ab*cd", false));
        assert!(FileSearch::matches_pattern(
            "PREfooMIDbarSUF",
            "pre*mid*suf",
            false
        ));
        assert!(FileSearch::matches_pattern(
            "prefix-FOO-suffix",
            "*foo*",
            false
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_question() {
        assert!(!FileSearch::matches_pattern("file.txt", "file.?", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.???", true));
        assert!(!FileSearch::matches_pattern("file.txt", "file.??", true));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive() {
        assert!(FileSearch::matches_pattern("FILE.TXT", "*.txt", false));
        assert!(FileSearch::matches_pattern("file.txt", "*.TXT", false));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_ascii_substring() {
        assert!(FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            false
        ));
        assert!(!FileSearch::matches_pattern(
            "archive-file.txt",
            "FILE",
            true
        ));
    }

    #[test]
    fn test_file_search_matches_pattern_case_insensitive_unicode_substring() {
        assert!(FileSearch::matches_pattern(
            "istanbul-İSTANBUL.txt",
            "i\u{307}stanbul",
            false
        ));
    }

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

    // ── SmallCharBuf ──────────────────────────────────────────────

    #[test]
    fn small_char_buf_small_inline() {
        let mut buf = SmallCharBuf::<4>::new(3);
        buf[0] = 'a';
        buf[1] = 'b';
        buf[2] = 'c';
        assert_eq!(buf[0], 'a');
        assert_eq!(buf[1], 'b');
        assert_eq!(buf[2], 'c');
    }

    #[test]
    fn small_char_buf_large_heap() {
        let mut buf = SmallCharBuf::<4>::new(10);
        buf[0] = 'x';
        buf[5] = 'y';
        buf[9] = 'z';
        assert_eq!(buf[0], 'x');
        assert_eq!(buf[5], 'y');
        assert_eq!(buf[9], 'z');
    }

    #[test]
    fn small_char_buf_exactly_n_uses_inline() {
        let mut buf = SmallCharBuf::<4>::new(4);
        buf[0] = 'p';
        buf[3] = 'q';
        assert_eq!(buf[0], 'p');
        assert_eq!(buf[3], 'q');
    }

    // ── wildcard edge cases ───────────────────────────────────────

    #[test]
    fn wildcard_star_crosses_slash_in_dp() {
        // The DP treats / as a regular char; filenames never contain /,
        // so this is academic but documents the matching behaviour.
        assert!(FileSearch::matches_pattern("a/b", "*/b", true));
        assert!(!FileSearch::matches_pattern("a/b/c", "*.txt", true));
    }

    #[test]
    fn wildcard_question_matches_exactly_one_char() {
        assert!(FileSearch::matches_pattern("ab", "a?", true));
        assert!(!FileSearch::matches_pattern("abc", "a?", true));
        assert!(!FileSearch::matches_pattern("a", "a?", true));
        assert!(FileSearch::matches_pattern("a", "?", true));
        assert!(!FileSearch::matches_pattern("", "?", true));
        assert!(FileSearch::matches_pattern("abc", "???", true));
    }

    #[test]
    fn wildcard_mixed_star_and_question() {
        assert!(FileSearch::matches_pattern(
            "file001.txt",
            "file???.txt",
            true
        ));
        assert!(!FileSearch::matches_pattern(
            "file1.txt",
            "file???.txt",
            true
        ));
        assert!(FileSearch::matches_pattern(
            "file001.txt",
            "file*.txt",
            true
        ));
    }

    // ── case-insensitive content ──────────────────────────────────

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

    // ── case-sensitive content ────────────────────────────────────

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

    // ── depth limit ───────────────────────────────────────────────

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
        for i in 0..MAX_SEARCH_DEPTH + 2 {
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
        for i in 0..MAX_SEARCH_DEPTH + 2 {
            deep = deep.join(format!("d{i}"));
            fs::create_dir_all(&deep).unwrap();
        }
        fs::write(deep.join("deep.txt"), "needle\n").unwrap();

        let outcome = FileSearch::search_content_with_diagnostics(&dir, "needle", true, false);
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.truncated, Some(TruncationReason::DepthLimit));

        let _ = fs::remove_dir_all(dir);
    }

    // ── symlink file ──────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn search_files_includes_symlinked_file_in_results() {
        // matches_pattern runs before the is_symlink check; symlink files
        // appear in search results (only symlink *directories* are skipped
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

        FileSearch::seed_visited_dir(&dir, &mut visited);
        assert!(visited.contains(&key));

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

    // ── empty directory ───────────────────────────────────────────

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
