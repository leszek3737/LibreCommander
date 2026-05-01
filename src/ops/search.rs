//! File searching operations for Libre Commander (lc).
//!
//! Full file search by name pattern or content.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::FileEntry;

#[derive(Debug, Clone)]
pub struct FileSearch {
    pub query: String,
    pub search_path: PathBuf,
    pub results: Vec<PathBuf>,
    pub is_searching: bool,
    pub search_content: bool,
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

    pub fn search_files(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
    ) -> Vec<FileEntry> {
        let mut results = Vec::new();
        Self::search_files_recursive(path, pattern, recursive, case_sensitive, &mut results, 0);
        results
    }

    fn search_files_recursive(
        path: &Path,
        pattern: &str,
        recursive: bool,
        case_sensitive: bool,
        results: &mut Vec<FileEntry>,
        depth: usize,
    ) {
        const MAX_DEPTH: usize = 20;
        if depth > MAX_DEPTH || !path.is_dir() {
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
                let metadata = entry.metadata().ok();
                results.push(FileEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: entry_path.clone(),
                    is_dir: file_type.is_dir(),
                    is_hidden: entry.file_name().to_string_lossy().starts_with('.'),
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
                    results,
                    depth + 1,
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
        if !path.is_dir() || depth >= Self::MAX_SEARCH_DEPTH || *item_count >= Self::MAX_SEARCH_ITEMS
        {
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
        let pattern_lower: Vec<char> = pattern
            .chars()
            .flat_map(|c| c.to_lowercase())
            .collect();

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
        let haystack_lower: String = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
        let needle_str: String = needle_lower.iter().collect();
        haystack_lower.contains(&needle_str)
    }

    pub fn matches_pattern(name: &str, pattern: &str, case_sensitive: bool) -> bool {
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

        let mut dp = vec![vec![false; m + 1]; n + 1];
        dp[0][0] = true;

        for j in 1..=m {
            if pattern_chars[j - 1] == '*' {
                dp[0][j] = dp[0][j - 1];
            }
        }

        for i in 1..=n {
            for j in 1..=m {
                match pattern_chars[j - 1] {
                    '*' => {
                        dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
                    }
                    '?' => {
                        dp[i][j] = dp[i - 1][j - 1];
                    }
                    c => {
                        if name_chars[i - 1] == c {
                            dp[i][j] = dp[i - 1][j - 1];
                        }
                    }
                }
            }
        }

        dp[n][m]
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
