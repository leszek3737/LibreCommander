//! File searching operations for Libre Commander (lc).
//!
//! This module provides quick search (incremental) and full file search functionality.
//! Uses TDD approach with comprehensive tests.
//!
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

// Minimal FileEntry as defined in the prompt requirements
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub size: u64,
}

pub trait Named {
    fn name(&self) -> &str;
}

impl Named for FileEntry {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for crate::app::types::FileEntry {
    fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// QuickSearch: Incremental search for panel (Ctrl+S style)
// ============================================================================

#[derive(Debug, Clone)]
pub struct QuickSearch {
    pub query: String,
    pub is_active: bool,
}

impl QuickSearch {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            is_active: false,
        }
    }

    pub fn activate(&mut self) {
        self.is_active = true;
        self.query.clear();
    }

    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.query.clear();
    }

    pub fn push_char(&mut self, c: char) {
        if self.is_active {
            self.query.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if self.is_active && !self.query.is_empty() {
            self.query.pop();
        }
    }

    pub fn find_match<T: Named>(entries: &[T], query: &str, start_from: usize) -> Option<usize> {
        if query.is_empty() {
            return Some(start_from);
        }
        let query_lower = query.to_lowercase();
        entries
            .iter()
            .enumerate()
            .skip(start_from)
            .find(|(_, entry)| entry.name().to_lowercase().starts_with(&query_lower))
            .map(|(idx, _)| idx)
    }

    pub fn find_next_match<T: Named>(entries: &[T], query: &str, current: usize) -> Option<usize> {
        if query.is_empty() || entries.is_empty() {
            return None;
        }
        let len = entries.len();
        let query_lower = query.to_lowercase();

        let forward = ((current + 1)..len)
            .find(|&i| entries[i].name().to_lowercase().starts_with(&query_lower));
        if forward.is_some() {
            return forward;
        }
        (0..current)
            .find(|&i| entries[i].name().to_lowercase().starts_with(&query_lower))
    }
}

impl Default for QuickSearch {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FileSearch: Full file search (Alt+?)
// ============================================================================

#[derive(Debug, Clone)]
pub struct FileSearch {
    pub query: String,
    pub search_path: PathBuf,
    pub results: Vec<PathBuf>,
    pub is_searching: bool,
    pub search_content: bool, // search in file contents too
    pub case_sensitive: bool,
    pub use_regex: bool,
}

impl FileSearch {
    const MAX_SEARCH_DEPTH: usize = 20;
    const MAX_SEARCH_ITEMS: usize = 10000;

    pub fn new(path: PathBuf) -> Self {
        Self {
            query: String::new(),
            search_path: path,
            results: Vec::new(),
            is_searching: false,
            search_content: false,
            case_sensitive: false,
            use_regex: false,
        }
    }

    /// Search files by filename pattern (glob-like: * and ?)
    pub fn search_files(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> Vec<PathBuf> {
        let mut results = Vec::new();
        Self::search_files_recursive(path, pattern, recursive, case_sensitive, &mut results);
        results
    }

    fn search_files_recursive(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        results: &mut Vec<PathBuf>,
    ) {
        if !path.is_dir() {
            return;
        }

        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries {
            let Ok(entry) = entry else { continue };
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if Self::matches_pattern(
                &entry.file_name().to_string_lossy(),
                pattern,
                case_sensitive,
            ) {
                results.push(entry_path.clone());
            }

            if recursive && file_type.is_dir() && !file_type.is_symlink() {
                Self::search_files_recursive(
                    &entry_path,
                    pattern,
                    recursive,
                    case_sensitive,
                    results,
                );
            }
        }
    }

    pub fn search_content(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> Vec<(PathBuf, usize, String)> {
        let mut results = Vec::new();
        let mut item_count: usize = 0;
        Self::search_content_recursive(path, pattern, recursive, case_sensitive, 0, &mut results, &mut item_count);
        results
    }

    fn search_content_recursive(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        depth: usize,
        results: &mut Vec<(PathBuf, usize, String)>,
        item_count: &mut usize,
    ) {
        if !path.is_dir() || depth >= Self::MAX_SEARCH_DEPTH || *item_count >= Self::MAX_SEARCH_ITEMS {
            return;
        }

        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries {
            if *item_count >= Self::MAX_SEARCH_ITEMS {
                return;
            }
            let Ok(entry) = entry else { continue };
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            *item_count += 1;

            if file_type.is_dir() && !file_type.is_symlink() {
                if recursive {
                    Self::search_content_recursive(
                        &entry_path,
                        pattern,
                        recursive,
                        case_sensitive,
                        depth + 1,
                        results,
                        item_count,
                    );
                }
            } else {
                let target_meta = std::fs::metadata(&entry_path).ok();
                if target_meta.as_ref().is_some_and(|m| m.is_file()) {
                    Self::search_in_file(&entry_path, pattern, case_sensitive, results);
                }
            }
        }
    }

    fn search_in_file(
        path: &Path,
        pattern: &str,
        case_sensitive: bool,
        results: &mut Vec<(PathBuf, usize, String)>,
    ) {
        if pattern.is_empty() {
            return;
        }

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let reader = BufReader::new(file);
        let pattern_chars: Vec<char> = pattern.chars().collect();
        let pattern_lower: Vec<char> = pattern_chars.iter().flat_map(|c| c.to_lowercase()).collect();

        for (line_no, line) in reader.lines().enumerate() {
            let Ok(line_text) = line else { continue };
            let match_found = if case_sensitive {
                line_text.contains(pattern)
            } else {
                Self::contains_case_insensitive(&line_text, &pattern_lower)
            };

            if match_found {
                results.push((path.to_path_buf(), line_no + 1, line_text));
            }
        }
    }

    fn contains_case_insensitive(haystack: &str, needle_lower: &[char]) -> bool {
        if needle_lower.is_empty() {
            return true;
        }
        let haystack_chars: Vec<char> = haystack.chars().collect();
        let haystack_len = haystack_chars.len();
        let needle_len = needle_lower.len();
        if needle_len > haystack_len {
            return false;
        }
        for i in 0..=(haystack_len - needle_len) {
            let mut matched = true;
            for (j, &nc) in needle_lower.iter().enumerate() {
                let hc = haystack_chars[i + j];
                let hc_lower = hc.to_lowercase().next().unwrap_or(hc);
                if hc_lower != nc {
                    matched = false;
                    break;
                }
            }
            if matched {
                return true;
            }
        }
        false
    }

    /// Check if filename matches glob pattern (* matches any, ? matches single char)
    pub fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
        let (name, pattern) = if case_sensitive {
            (name.to_string(), pattern.to_string())
        } else {
            (name.to_lowercase(), pattern.to_lowercase())
        };

        let name: Vec<char> = name.chars().collect();
        let pattern: Vec<char> = pattern.chars().collect();
        let n = name.len();
        let m = pattern.len();

        // dp[i][j] = matches pattern[0..j] with name[0..i]
        let mut dp = vec![vec![false; m + 1]; n + 1];
        dp[0][0] = true;

        // Handle patterns starting with *
        for j in 1..=m {
            if pattern[j - 1] == '*' {
                dp[0][j] = dp[0][j - 1];
            }
        }

        for i in 1..=n {
            for j in 1..=m {
                match pattern[j - 1] {
                    '*' => {
                        // Use wildcard: match any char (dp[i-1][j]) or skip wildcard (dp[i][j-1])
                        dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
                    }
                    '?' => {
                        // Match any single char
                        dp[i][j] = dp[i - 1][j - 1];
                    }
                    c => {
                        // Exact match required
                        if name[i - 1] == c {
                            dp[i][j] = dp[i - 1][j - 1];
                        }
                    }
                }
            }
        }

        dp[n][m]
    }
}

// ============================================================================
// Helper: Render Quick Search Bar
// ============================================================================

pub fn render_quick_search(f: &mut Frame, area: Rect, query: &str) {
    let text_style = Style::default().fg(Color::White);

    let text = if query.is_empty() {
        " Search: ".to_string()
    } else {
        format!(" Search: {query} ")
    };

    let paragraph = Paragraph::new(Span::styled(text, text_style))
        .block(Block::default().borders(Borders::NONE))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

// ============================================================================
// Tests (TDD)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;
    // use tempfile::tempdir; // Removing dependency

    // Helper to create test entries
    fn create_test_entries() -> Vec<FileEntry> {
        vec![
            FileEntry {
                name: "apple.txt".to_string(),
                path: PathBuf::from("apple.txt"),
                is_dir: false,
                is_hidden: false,
                size: 100,
            },
            FileEntry {
                name: "banana.txt".to_string(),
                path: PathBuf::from("banana.txt"),
                is_dir: false,
                is_hidden: false,
                size: 200,
            },
            FileEntry {
                name: "Apricot.txt".to_string(),
                path: PathBuf::from("Apricot.txt"),
                is_dir: false,
                is_hidden: false,
                size: 150,
            },
            FileEntry {
                name: "blueberry".to_string(),
                path: PathBuf::from("blueberry"),
                is_dir: true,
                is_hidden: false,
                size: 0,
            },
        ]
    }

    // QuickSearch Tests
    #[test]
    fn test_quick_search_new() {
        let qs = QuickSearch::new();
        assert_eq!(qs.query, "");
        assert!(!qs.is_active);
    }

    #[test]
    fn test_quick_search_activate_deactivate() {
        let mut qs = QuickSearch::new();
        qs.activate();
        assert!(qs.is_active);
        qs.deactivate();
        assert!(!qs.is_active);
    }

    #[test]
    fn test_quick_search_push_char() {
        let mut qs = QuickSearch::new();
        qs.activate();
        qs.push_char('a');
        qs.push_char('p');
        assert_eq!(qs.query, "ap");
    }

    #[test]
    fn test_quick_search_pop_char() {
        let mut qs = QuickSearch::new();
        qs.activate();
        qs.push_char('a');
        qs.push_char('p');
        qs.pop_char();
        assert_eq!(qs.query, "a");
    }

    #[test]
    fn test_quick_search_pop_char_empty() {
        let mut qs = QuickSearch::new();
        qs.activate();
        qs.pop_char(); // Should not panic
        assert_eq!(qs.query, "");
    }

    #[test]
    fn test_quick_search_find_match_prefix() {
        let entries = create_test_entries();
        let idx = QuickSearch::find_match(&entries, "ap", 0);
        assert_eq!(idx, Some(0)); // "apple.txt"
    }

    #[test]
    fn test_quick_search_find_match_case_insensitive() {
        let entries = create_test_entries();
        // "Ap" matches "Apricot.txt"
        let idx = QuickSearch::find_match(&entries, "ap", 0);
        assert_eq!(idx, Some(0)); // apple.txt starts with "ap" is first
    }

    #[test]
    fn test_quick_search_find_match_start_from() {
        let entries = create_test_entries();
        // Start from index 1, so "apple.txt" is skipped
        let idx = QuickSearch::find_match(&entries, "ap", 1);
        // "Apricot.txt" is index 2
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_quick_search_find_match_no_match() {
        let entries = create_test_entries();
        let idx = QuickSearch::find_match(&entries, "xyz", 0);
        assert!(idx.is_none());
    }

    #[test]
    fn test_quick_search_find_match_empty_query() {
        let entries = create_test_entries();
        let idx = QuickSearch::find_match(&entries, "", 0);
        assert_eq!(idx, Some(0)); // Returns start_from
    }

    #[test]
    fn test_quick_search_find_next_match() {
        let entries = create_test_entries();
        // Current is "apple.txt" (index 0), next "ap" should be "Apricot.txt" (index 2)
        let idx = QuickSearch::find_next_match(&entries, "ap", 0);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_quick_search_find_next_match_wrap() {
        let entries = create_test_entries();
        // Current is "Apricot.txt" (index 2), next "ap" wraps to "apple.txt" (index 0)
        let idx = QuickSearch::find_next_match(&entries, "ap", 2);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn test_file_search_matches_pattern_exact() {
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.txt", false));
    }

    #[test]
    fn test_file_search_matches_pattern_wildcard_star() {
        assert!(FileSearch::matches_pattern("file.txt", "*.txt", true));
        assert!(FileSearch::matches_pattern("file.txt", "file.*", true));
        assert!(FileSearch::matches_pattern("file.txt", "*", true));
        assert!(FileSearch::matches_pattern(
            "long_file_name.txt",
            "*.txt",
            true
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
    fn test_file_search_search_files() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lc_search_files_{}_{}", std::process::id(), id));
        fs::create_dir_all(&dir).unwrap();
        let dir_path = dir.as_path();

        // Create test files
        {
            let mut f1 = File::create(dir_path.join("test1.txt")).unwrap();
            writeln!(f1, "test").unwrap();
            drop(f1); // Ensure file is closed

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

        // Search for *.txt recursively
        let results = FileSearch::search_files(dir_path, "*.txt", true, false);
        eprintln!("Recursive search results: {:?}", results);
        assert_eq!(results.len(), 2, "Expected 2 results, found {:?}", results);
        assert!(results.iter().any(|p| p.ends_with("test1.txt")));
        assert!(results.iter().any(|p| p.ends_with("test3.txt")));

        // Search non-recursive
        let results = FileSearch::search_files(dir_path, "*.txt", false, false);
        eprintln!("Non-recursive search results: {:?}", results);
        assert_eq!(results.len(), 1, "Expected 1 result, found {:?}", results);
        assert!(results.iter().any(|p| p.ends_with("test1.txt")));

        // Cleanup
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
        // Empty query should match nothing in file content search
        let dir = std::env::temp_dir().join(format!("lc_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let results = FileSearch::search_content(&dir, "", true, false);
        assert!(results.is_empty());

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
    fn test_quick_search_find_next_match_no_matches() {
        let entries = create_test_entries();
        let idx = QuickSearch::find_next_match(&entries, "xyz", 0);
        assert!(idx.is_none());
    }
}
