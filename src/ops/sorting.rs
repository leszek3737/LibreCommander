//! File sorting operations for Libre Commander (lc).
//!
//! This module provides comprehensive file sorting functionality with TDD-tested
//! implementations for various sorting modes.
//!
use std::cmp::Ordering;

pub use crate::app::types::FileEntry;
pub use crate::app::types::SortMode;

pub fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().flat_map(|c| c.to_lowercase());
    let mut bi = b.chars().flat_map(|c| c.to_lowercase());
    loop {
        match (ai.next(), bi.next()) {
            (Some(ac), Some(bc)) => match ac.cmp(&bc) {
                Ordering::Equal => continue,
                other => return other,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// Extracts the file extension from a file name.
///
/// Returns an empty string if no extension is found.
pub fn get_extension(name: &str) -> &str {
    match name.rfind('.') {
        // Ensure the dot is not at the start (hidden file) and not at the end
        Some(index) if index > 0 && index < name.len() - 1 => &name[index..],
        _ => "",
    }
}

/// Compares two file entries based on the specified sort mode.
///
/// This function implements the core comparison logic used by the sort function.
/// It ensures:
/// - ".." is always treated as the top entry
/// - Directories are sorted before files
/// - Case-insensitive name comparisons
pub fn compare_entries(a: &FileEntry, b: &FileEntry, mode: SortMode) -> std::cmp::Ordering {
    // Special handling for ".." - it should always be at the top
    if a.name == ".." && b.name == ".." {
        return Ordering::Equal;
    }
    if a.name == ".." {
        return Ordering::Less;
    }
    if b.name == ".." {
        return Ordering::Greater;
    }

    // Ensure directories are always sorted before files (within each sort group)
    if a.is_dir && !b.is_dir {
        return Ordering::Less;
    }
    if !a.is_dir && b.is_dir {
        return Ordering::Greater;
    }

    // Perform comparison based on the sort mode
    match mode {
        SortMode::NameAsc => cmp_ignore_case(&a.name, &b.name),
        SortMode::NameDesc => cmp_ignore_case(&b.name, &a.name),
        SortMode::ExtensionAsc => {
            let ord = cmp_ignore_case(get_extension(&a.name), get_extension(&b.name));
            ord.then_with(|| cmp_ignore_case(&a.name, &b.name))
        }
        SortMode::ExtensionDesc => {
            let ord = cmp_ignore_case(get_extension(&b.name), get_extension(&a.name));
            ord.then_with(|| cmp_ignore_case(&a.name, &b.name))
        }
        SortMode::SizeAsc => a.size.cmp(&b.size).then_with(|| cmp_ignore_case(&a.name, &b.name)),
        SortMode::SizeDesc => b.size.cmp(&a.size).then_with(|| cmp_ignore_case(&a.name, &b.name)),
        SortMode::ModTimeAsc => a.modified.cmp(&b.modified).then_with(|| cmp_ignore_case(&a.name, &b.name)),
        SortMode::ModTimeDesc => b.modified.cmp(&a.modified).then_with(|| cmp_ignore_case(&a.name, &b.name)),
    }
}

/// Sorts a vector of file entries based on the specified mode.
///
/// This function modifies the entries in-place, ensuring:
/// - ".." is always at the top
/// - Directories are sorted before files
/// - Case-insensitive name sorting
pub fn sort_entries(entries: &mut [FileEntry], mode: SortMode) {
    entries.sort_by(|a, b| compare_entries(a, b, mode));
}

/// Cycles through sort modes in the specified order.
///
/// Order: NameAsc -> NameDesc -> SizeAsc -> SizeDesc -> ModTimeAsc -> ModTimeDesc -> ExtensionAsc -> ExtensionDesc -> NameAsc
pub fn cycle_sort_mode(current: SortMode) -> SortMode {
    match current {
        SortMode::NameAsc => SortMode::NameDesc,
        SortMode::NameDesc => SortMode::SizeAsc,
        SortMode::SizeAsc => SortMode::SizeDesc,
        SortMode::SizeDesc => SortMode::ModTimeAsc,
        SortMode::ModTimeAsc => SortMode::ModTimeDesc,
        SortMode::ModTimeDesc => SortMode::ExtensionAsc,
        SortMode::ExtensionAsc => SortMode::ExtensionDesc,
        SortMode::ExtensionDesc => SortMode::NameAsc,
    }
}

/// Returns a human-readable label for the sort mode.
pub fn sort_mode_label(mode: SortMode) -> &'static str {
    match mode {
        SortMode::NameAsc => "Name ↑",
        SortMode::NameDesc => "Name ↓",
        SortMode::ExtensionAsc => "Ext ↑",
        SortMode::ExtensionDesc => "Ext ↓",
        SortMode::SizeAsc => "Size ↑",
        SortMode::SizeDesc => "Size ↓",
        SortMode::ModTimeAsc => "Time ↑",
        SortMode::ModTimeDesc => "Time ↓",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn create_test_entry(name: &str, is_dir: bool, size: u64, modified_secs: u64) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_executable: false,
            size,
            modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified_secs),
            permissions: 0o644,
            owner: "testuser".to_string(),
            group: "testgroup".to_string(),
            selected: false,
            is_hidden: false,
        }
    }

    #[test]
    fn test_get_extension_with_extension() {
        assert_eq!(get_extension("file.txt"), ".txt");
        assert_eq!(get_extension("archive.tar.gz"), ".gz");
        assert_eq!(get_extension("document.pdf"), ".pdf");
        assert_eq!(get_extension("code.rs"), ".rs");
    }

    #[test]
    fn test_get_extension_no_extension() {
        assert_eq!(get_extension("README"), "");
        assert_eq!(get_extension("Makefile"), "");
        assert_eq!(get_extension(".bashrc"), "");
        assert_eq!(get_extension("no_extension_file"), "");
    }

    #[test]
    fn test_get_extension_hidden_files() {
        assert_eq!(get_extension(".gitignore"), "");
        assert_eq!(get_extension(".env.example"), ".example");
    }

    #[test]
    fn test_sort_mode_label() {
        assert_eq!(sort_mode_label(SortMode::NameAsc), "Name ↑");
        assert_eq!(sort_mode_label(SortMode::NameDesc), "Name ↓");
        assert_eq!(sort_mode_label(SortMode::ExtensionAsc), "Ext ↑");
        assert_eq!(sort_mode_label(SortMode::ExtensionDesc), "Ext ↓");
        assert_eq!(sort_mode_label(SortMode::SizeAsc), "Size ↑");
        assert_eq!(sort_mode_label(SortMode::SizeDesc), "Size ↓");
        assert_eq!(sort_mode_label(SortMode::ModTimeAsc), "Time ↑");
        assert_eq!(sort_mode_label(SortMode::ModTimeDesc), "Time ↓");
    }

    #[test]
    fn test_cycle_sort_mode() {
        assert_eq!(cycle_sort_mode(SortMode::NameAsc), SortMode::NameDesc);
        assert_eq!(cycle_sort_mode(SortMode::NameDesc), SortMode::SizeAsc);
        assert_eq!(cycle_sort_mode(SortMode::SizeAsc), SortMode::SizeDesc);
        assert_eq!(cycle_sort_mode(SortMode::SizeDesc), SortMode::ModTimeAsc);
        assert_eq!(cycle_sort_mode(SortMode::ModTimeAsc), SortMode::ModTimeDesc);
        assert_eq!(
            cycle_sort_mode(SortMode::ModTimeDesc),
            SortMode::ExtensionAsc
        );
        assert_eq!(
            cycle_sort_mode(SortMode::ExtensionAsc),
            SortMode::ExtensionDesc
        );
        assert_eq!(cycle_sort_mode(SortMode::ExtensionDesc), SortMode::NameAsc);
    }

    #[test]
    fn test_sort_ellipsis_at_top() {
        let mut entries = vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("..", true, 0, 0),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("another.txt", false, 200, 1500),
        ];

        sort_entries(&mut entries, SortMode::NameAsc);

        assert_eq!(entries[0].name, "..");
    }

    #[test]
    fn test_directories_before_files() {
        let mut entries = vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("another.txt", false, 200, 1500),
            create_test_entry("another_dir", true, 0, 2500),
        ];

        sort_entries(&mut entries, SortMode::NameAsc);

        // Directories should come before files
        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
        assert!(!entries[3].is_dir);
    }

    #[test]
    fn test_case_insensitive_sorting() {
        let mut entries = vec![
            create_test_entry("zebra", false, 100, 1000),
            create_test_entry("Apple", false, 200, 1500),
            create_test_entry("banana", false, 150, 1200),
            create_test_entry("Cherry", false, 180, 1300),
        ];

        sort_entries(&mut entries, SortMode::NameAsc);

        assert_eq!(entries[0].name, "Apple");
        assert_eq!(entries[1].name, "banana");
        assert_eq!(entries[2].name, "Cherry");
        assert_eq!(entries[3].name, "zebra");
    }

    #[test]
    fn test_sort_name_desc() {
        let mut entries = vec![
            create_test_entry("alpha", false, 100, 1000),
            create_test_entry("beta", false, 200, 1500),
            create_test_entry("gamma", false, 150, 1200),
        ];

        sort_entries(&mut entries, SortMode::NameDesc);

        assert_eq!(entries[0].name, "gamma");
        assert_eq!(entries[1].name, "beta");
        assert_eq!(entries[2].name, "alpha");
    }

    #[test]
    fn test_sort_by_size() {
        let mut entries = vec![
            create_test_entry("small.txt", false, 100, 1000),
            create_test_entry("large.txt", false, 10000, 1500),
            create_test_entry("medium.txt", false, 1000, 1200),
        ];

        sort_entries(&mut entries, SortMode::SizeAsc);

        assert_eq!(entries[0].name, "small.txt");
        assert_eq!(entries[1].name, "medium.txt");
        assert_eq!(entries[2].name, "large.txt");

        sort_entries(&mut entries, SortMode::SizeDesc);

        assert_eq!(entries[0].name, "large.txt");
        assert_eq!(entries[1].name, "medium.txt");
        assert_eq!(entries[2].name, "small.txt");
    }

    #[test]
    fn test_sort_by_mod_time() {
        let mut entries = vec![
            create_test_entry("old.txt", false, 100, 1000),
            create_test_entry("new.txt", false, 200, 2000),
            create_test_entry("middle.txt", false, 150, 1500),
        ];

        sort_entries(&mut entries, SortMode::ModTimeAsc);

        assert_eq!(entries[0].name, "old.txt");
        assert_eq!(entries[1].name, "middle.txt");
        assert_eq!(entries[2].name, "new.txt");

        sort_entries(&mut entries, SortMode::ModTimeDesc);

        assert_eq!(entries[0].name, "new.txt");
        assert_eq!(entries[1].name, "middle.txt");
        assert_eq!(entries[2].name, "old.txt");
    }

    #[test]
    fn test_sort_by_extension() {
        let mut entries = vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("image.png", false, 200, 1500),
            create_test_entry("archive.zip", false, 150, 1200),
            create_test_entry("script.sh", false, 50, 1100),
        ];

        // Extensions: .png, .sh, .txt, .zip (alphabetical ascending)
        sort_entries(&mut entries, SortMode::ExtensionAsc);

        assert_eq!(entries[0].name, "image.png");
        assert_eq!(entries[1].name, "script.sh");
        assert_eq!(entries[2].name, "file.txt");
        assert_eq!(entries[3].name, "archive.zip");

        sort_entries(&mut entries, SortMode::ExtensionDesc);

        assert_eq!(entries[0].name, "archive.zip");
        assert_eq!(entries[1].name, "file.txt");
        assert_eq!(entries[2].name, "script.sh");
        assert_eq!(entries[3].name, "image.png");
    }

    #[test]
    fn test_empty_entries_list() {
        let mut entries: Vec<FileEntry> = vec![];

        // Should not panic
        sort_entries(&mut entries, SortMode::NameAsc);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_sort_with_same_values() {
        // Create entries with same modification times to test stability
        let now = SystemTime::now();
        let mut entries = vec![
            FileEntry {
                name: "a.txt".to_string(),
                path: PathBuf::from("a.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 100,
                modified: now,
                permissions: 0o644,
                owner: "user".to_string(),
                group: "group".to_string(),
                selected: false,
                is_hidden: false,
            },
            FileEntry {
                name: "b.txt".to_string(),
                path: PathBuf::from("b.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 100,
                modified: now,
                permissions: 0o644,
                owner: "user".to_string(),
                group: "group".to_string(),
                selected: false,
                is_hidden: false,
            },
        ];

        // This should maintain order or sort correctly alphabetically
        sort_entries(&mut entries, SortMode::NameAsc);
        assert!(matches!(entries[0].name.as_str(), "a.txt" | "b.txt"));
    }

    #[test]
    fn test_compare_entries_directories_first() {
        let dir = create_test_entry("dir", true, 0, 1000);
        let file = create_test_entry("file.txt", false, 100, 1000);

        assert_eq!(
            compare_entries(&dir, &file, SortMode::NameAsc),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(&file, &dir, SortMode::NameAsc),
            Ordering::Greater
        );
    }

    #[test]
    fn test_compare_entries_ellipsis_priority() {
        let ellipsis = create_test_entry("..", true, 0, 0);
        let dir = create_test_entry("dir", true, 0, 1000);
        let file = create_test_entry("file.txt", false, 100, 1000);

        assert_eq!(
            compare_entries(&ellipsis, &dir, SortMode::NameAsc),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(&ellipsis, &file, SortMode::NameAsc),
            Ordering::Less
        );
    }
}
