//! File sorting operations for Libre Commander (lc).
//!
//! This module provides comprehensive file sorting functionality with TDD-tested
//! implementations for various sorting modes.
//!
use std::cmp::Ordering;
use std::cmp::Reverse;

pub use crate::app::types::FileEntry;
pub use crate::app::types::SortMode;
pub use crate::app::types::SortOptions;

use crate::ops::natsort;

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

/// Compares two file entries based on the specified sort mode and options.
///
/// This function implements the core comparison logic used by the sort function.
/// It ensures:
/// - ".." is always treated as the top entry
/// - Directories before files when `options.dir_first` is true
/// - Case sensitivity based on `options.sort_sensitive`
pub fn compare_entries(
    a: &FileEntry,
    b: &FileEntry,
    mode: SortMode,
    options: SortOptions,
) -> std::cmp::Ordering {
    let dir_first = options.dir_first;
    let sensitive = options.sort_sensitive;

    if a.name == ".." && b.name == ".." {
        return Ordering::Equal;
    }
    if a.name == ".." {
        return Ordering::Less;
    }
    if b.name == ".." {
        return Ordering::Greater;
    }

    if dir_first && a.is_dir() && !b.is_dir() {
        return Ordering::Less;
    }
    if dir_first && !a.is_dir() && b.is_dir() {
        return Ordering::Greater;
    }

    let name_cmp = |x: &FileEntry, y: &FileEntry| {
        if sensitive {
            x.name.cmp(&y.name)
        } else {
            cmp_ignore_case(&x.name, &y.name)
        }
    };

    match mode {
        SortMode::NameAsc => name_cmp(a, b),
        SortMode::NameDesc => name_cmp(b, a),
        SortMode::ExtensionAsc => {
            let ord = if sensitive {
                get_extension(&a.name).cmp(get_extension(&b.name))
            } else {
                cmp_ignore_case(get_extension(&a.name), get_extension(&b.name))
            };
            ord.then_with(|| name_cmp(a, b))
        }
        SortMode::ExtensionDesc => {
            let ord = if sensitive {
                get_extension(&b.name).cmp(get_extension(&a.name))
            } else {
                cmp_ignore_case(get_extension(&b.name), get_extension(&a.name))
            };
            ord.then_with(|| name_cmp(a, b))
        }
        SortMode::SizeAsc => a.len().cmp(&b.len()).then_with(|| name_cmp(a, b)),
        SortMode::SizeDesc => b.len().cmp(&a.len()).then_with(|| name_cmp(a, b)),
        SortMode::ModTimeAsc => a.mtime().cmp(&b.mtime()).then_with(|| name_cmp(a, b)),
        SortMode::ModTimeDesc => b.mtime().cmp(&a.mtime()).then_with(|| name_cmp(a, b)),
        SortMode::NaturalNameAsc => {
            natsort::natsort(a.name.as_bytes(), b.name.as_bytes(), !sensitive)
                .then_with(|| name_cmp(a, b))
        }
        SortMode::NaturalNameDesc => {
            natsort::natsort(b.name.as_bytes(), a.name.as_bytes(), !sensitive)
                .then_with(|| name_cmp(b, a))
        }
        SortMode::BtimeAsc => {
            let has_a = a.cha.btime.is_some();
            let has_b = b.cha.btime.is_some();
            has_b
                .cmp(&has_a)
                .then_with(|| a.btime().cmp(&b.btime()).then_with(|| name_cmp(a, b)))
        }
        SortMode::BtimeDesc => {
            let has_a = a.cha.btime.is_some();
            let has_b = b.cha.btime.is_some();
            has_b
                .cmp(&has_a)
                .then_with(|| b.btime().cmp(&a.btime()).then_with(|| name_cmp(a, b)))
        }
    }
}

/// Sorts a vector of file entries based on the specified mode.
///
/// This function modifies the entries in-place, ensuring:
/// - ".." is always at the top
/// - Directories are sorted before files
/// - Case-insensitive name sorting
pub fn sort_entries(entries: &mut [FileEntry], mode: SortMode, options: SortOptions) {
    let dir_first = options.dir_first;
    let sensitive = options.sort_sensitive;

    match mode {
        SortMode::NameAsc => entries.sort_by_cached_key(|entry| {
            (entry_group(entry, dir_first), name_key(entry, sensitive))
        }),
        SortMode::NameDesc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                Reverse(name_key(entry, sensitive)),
            )
        }),
        SortMode::ExtensionAsc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                extension_key(entry, sensitive),
                name_key(entry, sensitive),
            )
        }),
        SortMode::ExtensionDesc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                Reverse(extension_key(entry, sensitive)),
                name_key(entry, sensitive),
            )
        }),
        SortMode::SizeAsc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                entry.len(),
                name_key(entry, sensitive),
            )
        }),
        SortMode::SizeDesc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                Reverse(entry.len()),
                name_key(entry, sensitive),
            )
        }),
        SortMode::ModTimeAsc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                entry.mtime(),
                name_key(entry, sensitive),
            )
        }),
        SortMode::ModTimeDesc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                Reverse(entry.mtime()),
                name_key(entry, sensitive),
            )
        }),
        SortMode::BtimeAsc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                std::cmp::Reverse(entry.cha.btime.is_some()),
                entry.btime(),
                name_key(entry, sensitive),
            )
        }),
        SortMode::BtimeDesc => entries.sort_by_cached_key(|entry| {
            (
                entry_group(entry, dir_first),
                std::cmp::Reverse(entry.cha.btime.is_some()),
                Reverse(entry.btime()),
                name_key(entry, sensitive),
            )
        }),
        SortMode::NaturalNameAsc => entries.sort_by(|a, b| {
            entry_group(a, dir_first)
                .cmp(&entry_group(b, dir_first))
                .then_with(|| natsort::natsort(a.name.as_bytes(), b.name.as_bytes(), !sensitive))
                .then_with(|| a.name.cmp(&b.name))
        }),
        SortMode::NaturalNameDesc => entries.sort_by(|a, b| {
            entry_group(a, dir_first)
                .cmp(&entry_group(b, dir_first))
                .then_with(|| natsort::natsort(b.name.as_bytes(), a.name.as_bytes(), !sensitive))
                .then_with(|| b.name.cmp(&a.name))
        }),
    }
}

fn entry_group(entry: &FileEntry, dir_first: bool) -> u8 {
    match (entry.name.as_str(), dir_first, entry.is_dir()) {
        ("..", _, _) => 0,
        (_, true, true) => 1,
        (_, true, false) => 2,
        (_, false, _) => 1,
    }
}

fn name_key(entry: &FileEntry, sensitive: bool) -> String {
    if sensitive {
        entry.name.clone()
    } else {
        entry.name.to_lowercase()
    }
}

fn extension_key(entry: &FileEntry, sensitive: bool) -> String {
    if sensitive {
        get_extension(&entry.name).to_string()
    } else {
        get_extension(&entry.name).to_lowercase()
    }
}

/// Cycles through sort modes in the specified order.
///
/// Order: NameAsc -> NameDesc -> NaturalNameAsc -> NaturalNameDesc -> SizeAsc -> SizeDesc
///        -> ModTimeAsc -> ModTimeDesc -> BtimeAsc -> BtimeDesc
///        -> ExtensionAsc -> ExtensionDesc -> NameAsc
pub fn cycle_sort_mode(current: SortMode) -> SortMode {
    match current {
        SortMode::NameAsc => SortMode::NameDesc,
        SortMode::NameDesc => SortMode::NaturalNameAsc,
        SortMode::NaturalNameAsc => SortMode::NaturalNameDesc,
        SortMode::NaturalNameDesc => SortMode::SizeAsc,
        SortMode::SizeAsc => SortMode::SizeDesc,
        SortMode::SizeDesc => SortMode::ModTimeAsc,
        SortMode::ModTimeAsc => SortMode::ModTimeDesc,
        SortMode::ModTimeDesc => SortMode::BtimeAsc,
        SortMode::BtimeAsc => SortMode::BtimeDesc,
        SortMode::BtimeDesc => SortMode::ExtensionAsc,
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
        SortMode::NaturalNameAsc => "Nat ↑",
        SortMode::NaturalNameDesc => "Nat ↓",
        SortMode::BtimeAsc => "Created ↑",
        SortMode::BtimeDesc => "Created ↓",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn create_test_entry(name: &str, is_dir: bool, size: u64, modified_secs: u64) -> FileEntry {
        let ts = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified_secs);
        FileEntry::builder()
            .name(name)
            .path(name)
            .is_dir(is_dir)
            .size(size)
            .modified(ts)
            .created(ts)
            .owner("testuser")
            .group("testgroup")
            .build()
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
        assert_eq!(sort_mode_label(SortMode::NaturalNameAsc), "Nat ↑");
        assert_eq!(sort_mode_label(SortMode::NaturalNameDesc), "Nat ↓");
    }

    #[test]
    fn test_cycle_sort_mode() {
        assert_eq!(cycle_sort_mode(SortMode::NameAsc), SortMode::NameDesc);
        assert_eq!(
            cycle_sort_mode(SortMode::NameDesc),
            SortMode::NaturalNameAsc
        );
        assert_eq!(
            cycle_sort_mode(SortMode::NaturalNameAsc),
            SortMode::NaturalNameDesc
        );
        assert_eq!(
            cycle_sort_mode(SortMode::NaturalNameDesc),
            SortMode::SizeAsc
        );
        assert_eq!(cycle_sort_mode(SortMode::SizeAsc), SortMode::SizeDesc);
        assert_eq!(cycle_sort_mode(SortMode::SizeDesc), SortMode::ModTimeAsc);
        assert_eq!(cycle_sort_mode(SortMode::ModTimeAsc), SortMode::ModTimeDesc);
        assert_eq!(cycle_sort_mode(SortMode::ModTimeDesc), SortMode::BtimeAsc);
        assert_eq!(cycle_sort_mode(SortMode::BtimeAsc), SortMode::BtimeDesc);
        assert_eq!(cycle_sort_mode(SortMode::BtimeDesc), SortMode::ExtensionAsc);
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

        sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());

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

        sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());

        // Directories should come before files
        assert!(entries[0].is_dir());
        assert!(entries[1].is_dir());
        assert!(!entries[2].is_dir());
        assert!(!entries[3].is_dir());
    }

    #[test]
    fn test_case_insensitive_sorting() {
        let mut entries = vec![
            create_test_entry("zebra", false, 100, 1000),
            create_test_entry("Apple", false, 200, 1500),
            create_test_entry("banana", false, 150, 1200),
            create_test_entry("Cherry", false, 180, 1300),
        ];

        sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());

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

        sort_entries(&mut entries, SortMode::NameDesc, SortOptions::default());

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

        sort_entries(&mut entries, SortMode::SizeAsc, SortOptions::default());

        assert_eq!(entries[0].name, "small.txt");
        assert_eq!(entries[1].name, "medium.txt");
        assert_eq!(entries[2].name, "large.txt");

        sort_entries(&mut entries, SortMode::SizeDesc, SortOptions::default());

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

        sort_entries(&mut entries, SortMode::ModTimeAsc, SortOptions::default());

        assert_eq!(entries[0].name, "old.txt");
        assert_eq!(entries[1].name, "middle.txt");
        assert_eq!(entries[2].name, "new.txt");

        sort_entries(&mut entries, SortMode::ModTimeDesc, SortOptions::default());

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
        sort_entries(&mut entries, SortMode::ExtensionAsc, SortOptions::default());

        assert_eq!(entries[0].name, "image.png");
        assert_eq!(entries[1].name, "script.sh");
        assert_eq!(entries[2].name, "file.txt");
        assert_eq!(entries[3].name, "archive.zip");

        sort_entries(
            &mut entries,
            SortMode::ExtensionDesc,
            SortOptions::default(),
        );

        assert_eq!(entries[0].name, "archive.zip");
        assert_eq!(entries[1].name, "file.txt");
        assert_eq!(entries[2].name, "script.sh");
        assert_eq!(entries[3].name, "image.png");
    }

    #[test]
    fn test_empty_entries_list() {
        let mut entries: Vec<FileEntry> = vec![];

        // Should not panic
        sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_sort_with_same_values() {
        let now = SystemTime::now();
        let mut entries = vec![
            FileEntry::builder()
                .name("a.txt")
                .path("a.txt")
                .size(100)
                .modified(now)
                .created(now)
                .build(),
            FileEntry::builder()
                .name("b.txt")
                .path("b.txt")
                .size(100)
                .modified(now)
                .created(now)
                .build(),
        ];

        sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());
        assert!(matches!(entries[0].name.as_str(), "a.txt" | "b.txt"));
    }

    #[test]
    fn test_compare_entries_directories_first() {
        let dir = create_test_entry("dir", true, 0, 1000);
        let file = create_test_entry("file.txt", false, 100, 1000);

        assert_eq!(
            compare_entries(&dir, &file, SortMode::NameAsc, SortOptions::default()),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(&file, &dir, SortMode::NameAsc, SortOptions::default()),
            Ordering::Greater
        );
    }

    #[test]
    fn test_compare_entries_ellipsis_priority() {
        let ellipsis = create_test_entry("..", true, 0, 0);
        let dir = create_test_entry("dir", true, 0, 1000);
        let file = create_test_entry("file.txt", false, 100, 1000);

        assert_eq!(
            compare_entries(&ellipsis, &dir, SortMode::NameAsc, SortOptions::default()),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(&ellipsis, &file, SortMode::NameAsc, SortOptions::default()),
            Ordering::Less
        );
    }

    #[test]
    fn test_sort_natural_name_asc() {
        let mut entries = vec![
            create_test_entry("a10.txt", false, 100, 100),
            create_test_entry("a2.txt", false, 100, 100),
            create_test_entry("a1.txt", false, 100, 100),
        ];

        sort_entries(
            &mut entries,
            SortMode::NaturalNameAsc,
            SortOptions::default(),
        );

        assert_eq!(entries[0].name, "a1.txt");
        assert_eq!(entries[1].name, "a2.txt");
        assert_eq!(entries[2].name, "a10.txt");
    }

    #[test]
    fn test_sort_natural_name_desc() {
        let mut entries = vec![
            create_test_entry("a10.txt", false, 100, 100),
            create_test_entry("a2.txt", false, 100, 100),
            create_test_entry("a1.txt", false, 100, 100),
        ];

        sort_entries(
            &mut entries,
            SortMode::NaturalNameDesc,
            SortOptions::default(),
        );

        assert_eq!(entries[0].name, "a10.txt");
        assert_eq!(entries[1].name, "a2.txt");
        assert_eq!(entries[2].name, "a1.txt");
    }

    #[test]
    fn test_sort_natural_with_directories_first() {
        let mut entries = vec![
            create_test_entry("file10", false, 100, 100),
            create_test_entry("file2", false, 100, 100),
            create_test_entry("dir10", true, 0, 100),
            create_test_entry("dir2", true, 0, 100),
        ];

        sort_entries(
            &mut entries,
            SortMode::NaturalNameAsc,
            SortOptions::default(),
        );

        assert_eq!(entries[0].name, "dir2");
        assert_eq!(entries[1].name, "dir10");
        assert_eq!(entries[2].name, "file2");
        assert_eq!(entries[3].name, "file10");
    }

    #[test]
    fn test_sort_natural_ellipsis_first() {
        let mut entries = vec![
            create_test_entry("..", true, 0, 0),
            create_test_entry("z10", false, 100, 100),
            create_test_entry("a2", false, 100, 100),
            create_test_entry("a1", false, 100, 100),
        ];

        sort_entries(
            &mut entries,
            SortMode::NaturalNameAsc,
            SortOptions::default(),
        );

        assert_eq!(entries[0].name, "..");
    }

    #[test]
    fn test_compare_entries_natural() {
        let a2 = create_test_entry("a2", false, 100, 100);
        let a10 = create_test_entry("a10", false, 100, 100);

        assert_eq!(
            compare_entries(&a2, &a10, SortMode::NaturalNameAsc, SortOptions::default()),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(&a10, &a2, SortMode::NaturalNameAsc, SortOptions::default()),
            Ordering::Greater
        );
        assert_eq!(
            compare_entries(&a2, &a10, SortMode::NaturalNameDesc, SortOptions::default()),
            Ordering::Greater
        );
        assert_eq!(
            compare_entries(&a10, &a2, SortMode::NaturalNameDesc, SortOptions::default()),
            Ordering::Less
        );
    }

    #[test]
    fn test_natsort_unit() {
        use crate::ops::natsort::natsort;

        assert_eq!(natsort(b"a2", b"a10", true), Ordering::Less);
        assert_eq!(natsort(b"a10", b"a2", true), Ordering::Greater);
        assert_eq!(natsort(b"a1", b"a1", true), Ordering::Equal);
        assert_eq!(natsort(b"b1", b"a10", true), Ordering::Greater);
        assert_eq!(natsort(b"file2.txt", b"file10.txt", true), Ordering::Less);
    }
}
