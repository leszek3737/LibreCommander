//! File searching operations for Libre Commander (lc).
//!
//! Full file search by name pattern or content.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::FileEntry;

#[derive(Debug, Clone)]
pub struct SearchOutcome<T> {
    pub matches: Vec<T>,
    pub errors: Vec<String>,
    pub truncated: bool,
}

impl<T> Default for SearchOutcome<T> {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            errors: Vec::new(),
            truncated: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileSearch {
    pub query: String,
    pub search_path: PathBuf,
    pub results: Vec<PathBuf>,
    pub case_sensitive: bool,
}

pub const MAX_SEARCH_DEPTH: usize = 20;
pub const MAX_SEARCH_ITEMS: usize = 10000;

impl FileSearch {
    pub fn new(path: PathBuf) -> Self {
        Self {
            query: String::new(),
            search_path: path,
            results: Vec::new(),
            case_sensitive: false,
        }
    }

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
        Self::search_files_recursive(
            path,
            pattern,
            recursive,
            case_sensitive,
            &mut outcome,
            0,
            &mut item_count,
        );
        outcome
    }

    fn search_files_recursive(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        outcome: &mut SearchOutcome<FileEntry>,
        depth: usize,
        item_count: &mut usize,
    ) {
        if depth >= MAX_SEARCH_DEPTH {
            outcome.truncated = true;
            return;
        }
        if !path.is_dir() {
            outcome
                .errors
                .push(format!("Not a directory: {}", path.display()));
            return;
        }

        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(err) => {
                outcome
                    .errors
                    .push(format!("Failed to read {}: {err}", path.display()));
                return;
            }
        };

        for entry in entries {
            if *item_count >= MAX_SEARCH_ITEMS {
                outcome.truncated = true;
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
            if Self::matches_pattern(&name_lossy, pattern, case_sensitive) {
                let metadata = entry.metadata().ok();
                let is_hidden = name_lossy.starts_with('.');
                outcome.matches.push(FileEntry {
                    name: name_lossy.into_owned(),
                    path: entry_path.clone(),
                    is_dir: file_type.is_dir(),
                    is_hidden,
                    size: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
                    permissions: metadata
                        .as_ref()
                        .map(|m| {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                m.permissions().mode()
                            }
                            #[cfg(not(unix))]
                            {
                                0u32
                            }
                        })
                        .unwrap_or(0),
                    modified: metadata
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                    is_symlink: file_type.is_symlink(),
                    is_executable: false,
                    owner: String::new(),
                    group: String::new(),
                    selected: false,
                });
            }

            if recursive && file_type.is_dir() && !file_type.is_symlink() {
                Self::search_files_recursive(
                    &entry_path,
                    pattern,
                    recursive,
                    case_sensitive,
                    outcome,
                    depth + 1,
                    item_count,
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
        Self::search_content_recursive(
            path,
            pattern,
            recursive,
            case_sensitive,
            0,
            &mut results,
            &mut item_count,
        );
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
        if !path.is_dir() || depth >= MAX_SEARCH_DEPTH || *item_count >= MAX_SEARCH_ITEMS {
            return;
        }

        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries {
            if *item_count >= MAX_SEARCH_ITEMS {
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
        let pattern_lower: Vec<char> = pattern.chars().flat_map(|c| c.to_lowercase()).collect();

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
        let haystack_lower: Vec<char> = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
        if haystack_lower.len() < needle_lower.len() {
            return false;
        }
        haystack_lower
            .windows(needle_lower.len())
            .any(|w| w == needle_lower)
    }

    pub fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
        if !pattern.contains(['*', '?']) {
            return if case_sensitive {
                name.contains(pattern)
            } else {
                let pattern_lower: Vec<char> =
                    pattern.chars().flat_map(|c| c.to_lowercase()).collect();
                Self::contains_case_insensitive(name, &pattern_lower)
            };
        }

        let (name_chars, pattern_chars): (Vec<char>, Vec<char>) = if case_sensitive {
            (name.chars().collect(), pattern.chars().collect())
        } else {
            (
                name.chars().flat_map(|c| c.to_lowercase()).collect(),
                pattern.chars().flat_map(|c| c.to_lowercase()).collect(),
            )
        };
        let n = name_chars.len();
        let m = pattern_chars.len();

        let mut dp_prev = vec![false; m + 1];
        let mut dp_curr = vec![false; m + 1];
        dp_prev[0] = true;

        for j in 1..=m {
            if pattern_chars[j - 1] == '*' {
                dp_prev[j] = dp_prev[j - 1];
            }
        }

        for i in 1..=n {
            dp_curr.fill(false);
            for j in 1..=m {
                match pattern_chars[j - 1] {
                    '*' => {
                        dp_curr[j] = dp_prev[j] || dp_curr[j - 1];
                    }
                    '?' => {
                        dp_curr[j] = dp_prev[j - 1];
                    }
                    c => {
                        dp_curr[j] = if name_chars[i - 1] == c {
                            dp_prev[j - 1]
                        } else {
                            false
                        };
                    }
                }
            }
            std::mem::swap(&mut dp_prev, &mut dp_curr);
        }

        dp_prev[m]
    }
}

#[cfg(test)]
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
        eprintln!("Recursive search results: {:?}", results);
        assert_eq!(results.len(), 2, "Expected 2 results, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));
        assert!(results.iter().any(|e| e.name == "test3.txt"));

        let results = FileSearch::search_files(dir_path, "*.txt", false, false);
        eprintln!("Non-recursive search results: {:?}", results);
        assert_eq!(results.len(), 1, "Expected 1 result, found {:?}", results);
        assert!(results.iter().any(|e| e.name == "test1.txt"));

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
        assert!(!outcome.truncated);
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
        assert!(outcome.truncated);

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
}
