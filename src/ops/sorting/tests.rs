#![allow(clippy::expect_used)]

use super::*;
use std::time::SystemTime;

macro_rules! sort_mode {
    ($field:ident, $dir:ident) => {
        SortMode {
            field: SortField::$field,
            direction: Direction::$dir,
        }
    };
}

fn make_entry(
    name: &str,
    is_dir: bool,
    size: u64,
    modified_secs: u64,
    btime_secs: Option<u64>,
) -> FileEntry {
    use crate::app::types::test_helpers::TestEntry;
    let ts = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified_secs);
    let mut e = TestEntry::new(name)
        .path(name)
        .modified(ts)
        .owner("testuser")
        .group("testgroup");
    if is_dir {
        e = e.len(size);
    } else {
        e = e.file(size);
    }
    if let Some(btime) = btime_secs {
        e = e.created(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(btime));
    }
    e.build()
}

fn create_test_entry(name: &str, is_dir: bool, size: u64, modified_secs: u64) -> FileEntry {
    make_entry(name, is_dir, size, modified_secs, None)
}

/// Sort `entries` and assert the resulting name order.
///
/// A plain function (not a macro) so a failed assertion points at the calling
/// test's line rather than a macro expansion span.
fn assert_sort_order(
    mut entries: Vec<FileEntry>,
    mode: SortMode,
    dir_first: bool,
    sensitive: bool,
    expected: &[&str],
) {
    sort_entries(
        &mut entries,
        mode,
        SortOptions {
            dir_first,
            sensitive,
        },
    );
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, expected);
}

/// Assert `..` sorts first under `mode` regardless of direction or field.
fn assert_ellipsis_first(mode: SortMode) {
    let mut entries = vec![
        create_test_entry("z10", false, 100, 100),
        create_test_entry("..", true, 0, 0),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("a2", false, 100, 100),
        create_test_entry("a1", false, 100, 100),
    ];
    sort_entries(&mut entries, mode, SortOptions::default());
    assert_eq!(entries[0].name, "..", "{mode:?} must keep '..' first");
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
fn test_get_extension_edge_cases() {
    assert_eq!(get_extension("file."), "");
    assert_eq!(get_extension("a.b.c.d"), ".d");
    assert_eq!(get_extension(""), "");
}

#[test]
fn test_cycle_sort_mode() {
    assert_eq!(
        cycle_sort_mode(sort_mode!(Name, Asc)),
        sort_mode!(Name, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Name, Desc)),
        sort_mode!(NaturalName, Asc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(NaturalName, Asc)),
        sort_mode!(NaturalName, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(NaturalName, Desc)),
        sort_mode!(Size, Asc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Size, Asc)),
        sort_mode!(Size, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Size, Desc)),
        sort_mode!(ModTime, Asc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(ModTime, Asc)),
        sort_mode!(ModTime, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(ModTime, Desc)),
        sort_mode!(Btime, Asc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Btime, Asc)),
        sort_mode!(Btime, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Btime, Desc)),
        sort_mode!(Extension, Asc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Extension, Asc)),
        sort_mode!(Extension, Desc)
    );
    assert_eq!(
        cycle_sort_mode(sort_mode!(Extension, Desc)),
        sort_mode!(Name, Asc)
    );
}

#[test]
fn test_directories_before_files() {
    let mut entries = vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("another.txt", false, 200, 1500),
        create_test_entry("another_dir", true, 0, 2500),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());

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
        create_test_entry("apple", false, 220, 1600),
        create_test_entry("banana", false, 150, 1200),
        create_test_entry("Cherry", false, 180, 1300),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "apple");
    assert_eq!(entries[1].name, "Apple");
    assert_eq!(entries[2].name, "banana");
    assert_eq!(entries[3].name, "Cherry");
    assert_eq!(entries[4].name, "zebra");
}

#[test]
fn test_sort_natural_name_case_tiebreak_matches_name_sort() {
    let source = vec![
        create_test_entry("alpha2", false, 100, 100),
        create_test_entry("Alpha2", false, 100, 100),
        create_test_entry("ALPHA2", false, 100, 100),
        create_test_entry("alpha10", false, 100, 100),
    ];

    let mut name_entries = source.clone();
    sort_entries(
        &mut name_entries,
        sort_mode!(Name, Asc),
        SortOptions::default(),
    );

    let mut natural_entries = source;
    sort_entries(
        &mut natural_entries,
        sort_mode!(NaturalName, Asc),
        SortOptions::default(),
    );

    let name_case_order: Vec<&str> = name_entries
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case("alpha2"))
        .map(|entry| entry.name.as_str())
        .collect();
    let natural_case_order: Vec<&str> = natural_entries
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case("alpha2"))
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(name_case_order, ["alpha2", "ALPHA2", "Alpha2"]);
    assert_eq!(natural_case_order, name_case_order);
}

#[test]
fn test_sort_natural_name_case_tiebreak_desc_reverses_asc() {
    let source = vec![
        create_test_entry("alpha2", false, 100, 100),
        create_test_entry("Alpha2", false, 100, 100),
        create_test_entry("ALPHA2", false, 100, 100),
    ];

    let mut ascending = source.clone();
    sort_entries(
        &mut ascending,
        sort_mode!(NaturalName, Asc),
        SortOptions::default(),
    );

    let mut descending = source;
    sort_entries(
        &mut descending,
        sort_mode!(NaturalName, Desc),
        SortOptions::default(),
    );

    let ascending_names: Vec<&str> = ascending.iter().map(|entry| entry.name.as_str()).collect();
    let descending_names: Vec<&str> = descending.iter().map(|entry| entry.name.as_str()).collect();

    assert_eq!(ascending_names, ["alpha2", "ALPHA2", "Alpha2"]);
    assert_eq!(descending_names, ["Alpha2", "ALPHA2", "alpha2"]);
}

#[test]
fn test_sort_name_desc() {
    let mut entries = vec![
        create_test_entry("alpha", false, 100, 1000),
        create_test_entry("beta", false, 200, 1500),
        create_test_entry("gamma", false, 150, 1200),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Desc), SortOptions::default());

    assert_eq!(entries[0].name, "gamma");
    assert_eq!(entries[1].name, "beta");
    assert_eq!(entries[2].name, "alpha");
}

#[test]
fn test_sort_by_size_asc() {
    assert_sort_order(
        vec![
            create_test_entry("small.txt", false, 100, 1000),
            create_test_entry("large.txt", false, 10000, 1500),
            create_test_entry("medium.txt", false, 1000, 1200),
        ],
        sort_mode!(Size, Asc),
        true,
        false,
        &["small.txt", "medium.txt", "large.txt"],
    );
}

#[test]
fn test_sort_by_size_desc() {
    assert_sort_order(
        vec![
            create_test_entry("small.txt", false, 100, 1000),
            create_test_entry("large.txt", false, 10000, 1500),
            create_test_entry("medium.txt", false, 1000, 1200),
        ],
        sort_mode!(Size, Desc),
        true,
        false,
        &["large.txt", "medium.txt", "small.txt"],
    );
}

#[test]
fn test_sort_by_mod_time_asc() {
    assert_sort_order(
        vec![
            create_test_entry("old.txt", false, 100, 1000),
            create_test_entry("new.txt", false, 200, 2000),
            create_test_entry("middle.txt", false, 150, 1500),
        ],
        sort_mode!(ModTime, Asc),
        true,
        false,
        &["old.txt", "middle.txt", "new.txt"],
    );
}

#[test]
fn test_sort_by_mod_time_desc() {
    assert_sort_order(
        vec![
            create_test_entry("old.txt", false, 100, 1000),
            create_test_entry("new.txt", false, 200, 2000),
            create_test_entry("middle.txt", false, 150, 1500),
        ],
        sort_mode!(ModTime, Desc),
        true,
        false,
        &["new.txt", "middle.txt", "old.txt"],
    );
}

#[test]
fn test_sort_by_extension_asc() {
    assert_sort_order(
        vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("image.png", false, 200, 1500),
            create_test_entry("archive.zip", false, 150, 1200),
            create_test_entry("script.sh", false, 50, 1100),
        ],
        sort_mode!(Extension, Asc),
        true,
        false,
        &["image.png", "script.sh", "file.txt", "archive.zip"],
    );
}

#[test]
fn test_sort_by_extension_desc() {
    assert_sort_order(
        vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("image.png", false, 200, 1500),
            create_test_entry("archive.zip", false, 150, 1200),
            create_test_entry("script.sh", false, 50, 1100),
        ],
        sort_mode!(Extension, Desc),
        true,
        false,
        &["archive.zip", "file.txt", "script.sh", "image.png"],
    );
}

#[test]
fn test_empty_entries_list() {
    let mut entries: Vec<FileEntry> = vec![];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_sort_with_same_values() {
    let mut entries = vec![
        make_entry("a.txt", false, 100, 1000, Some(1000)),
        make_entry("b.txt", false, 100, 1000, Some(1000)),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());
    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[1].name, "b.txt");
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
        sort_mode!(NaturalName, Asc),
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
        sort_mode!(NaturalName, Desc),
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
        sort_mode!(NaturalName, Asc),
        SortOptions::default(),
    );

    assert_eq!(entries[0].name, "dir2");
    assert_eq!(entries[1].name, "dir10");
    assert_eq!(entries[2].name, "file2");
    assert_eq!(entries[3].name, "file10");
}

#[test]
fn test_sort_btime_asc_none_after_some() {
    let mut entries = vec![
        make_entry("no_btime.txt", false, 10, 100, None),
        make_entry("old.txt", false, 10, 100, Some(100)),
        make_entry("new.txt", false, 10, 200, Some(200)),
    ];

    sort_entries(&mut entries, sort_mode!(Btime, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "old.txt");
    assert_eq!(entries[1].name, "new.txt");
    assert_eq!(entries[2].name, "no_btime.txt");
}

#[test]
fn test_sort_btime_desc_none_after_some() {
    let mut entries = vec![
        make_entry("no_btime.txt", false, 10, 100, None),
        make_entry("old.txt", false, 10, 100, Some(100)),
        make_entry("new.txt", false, 10, 200, Some(200)),
    ];

    sort_entries(
        &mut entries,
        sort_mode!(Btime, Desc),
        SortOptions::default(),
    );

    assert_eq!(entries[0].name, "new.txt");
    assert_eq!(entries[1].name, "old.txt");
    assert_eq!(entries[2].name, "no_btime.txt");
}

#[test]
fn test_sort_btime_same_btime_stable() {
    let mut entries = vec![
        make_entry("beta.txt", false, 10, 300, Some(300)),
        make_entry("alpha.txt", false, 10, 300, Some(300)),
    ];

    sort_entries(&mut entries, sort_mode!(Btime, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "alpha.txt");
    assert_eq!(entries[1].name, "beta.txt");
}

#[test]
fn test_sort_btime_all_none() {
    let mut entries = vec![
        make_entry("c.txt", false, 10, 100, None),
        make_entry("a.txt", false, 10, 100, None),
        make_entry("b.txt", false, 10, 100, None),
    ];

    sort_entries(&mut entries, sort_mode!(Btime, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[1].name, "b.txt");
    assert_eq!(entries[2].name, "c.txt");
}

#[test]
fn test_sort_btime_mixed_with_dirs() {
    let mut entries = vec![
        make_entry("file_no_btime", false, 10, 100, None),
        make_entry("dir_old", true, 0, 100, Some(100)),
        make_entry("dir_new", true, 0, 200, Some(200)),
    ];

    sort_entries(&mut entries, sort_mode!(Btime, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "dir_old");
    assert_eq!(entries[1].name, "dir_new");
    assert_eq!(entries[2].name, "file_no_btime");
}

#[test]
fn test_cmp_ignore_case_equal() {
    assert_eq!(cmp_ignore_case("hello", "hello"), Ordering::Equal);
}

#[test]
fn test_cmp_ignore_case_different_case() {
    assert_eq!(cmp_ignore_case("Hello", "hello"), Ordering::Equal);
    assert_eq!(cmp_ignore_case("HELLO", "hello"), Ordering::Equal);
}

#[test]
fn test_cmp_ignore_case_different_words() {
    assert_eq!(cmp_ignore_case("apple", "banana"), Ordering::Less);
    assert_eq!(cmp_ignore_case("banana", "apple"), Ordering::Greater);
}

#[test]
fn test_cmp_ignore_case_empty() {
    assert_eq!(cmp_ignore_case("", ""), Ordering::Equal);
    assert_eq!(cmp_ignore_case("", "a"), Ordering::Less);
    assert_eq!(cmp_ignore_case("a", ""), Ordering::Greater);
}

#[test]
fn test_cmp_ignore_case_prefix() {
    assert_eq!(cmp_ignore_case("abc", "abcd"), Ordering::Less);
    assert_eq!(cmp_ignore_case("abcd", "abc"), Ordering::Greater);
}

/// Turkish dotted I (İ, U+0130) is a known limitation of `str::to_lowercase()`.
/// The Rust stdlib lowercases 'İ' to 'i\u{307}' (two chars), not 'i', so
/// case-insensitive comparison produces `Greater` where 'I' vs 'İ' would give
/// `Equal` under Turkish locale rules. This is acceptable — we don't depend on
/// system locale for the TUI.
#[test]
fn test_cmp_ignore_case_turkish_dotted_i() {
    let left = cmp_ignore_case("B\u{0130}L", "BIL");
    let right = cmp_ignore_case("BIL", "B\u{0130}L");
    assert_eq!(left, Ordering::Greater);
    assert_eq!(right, Ordering::Less);
}

// ── SortOptions combinations (via assert_sort_order) ──

#[test]
fn test_sort_dir_first_false() {
    assert_sort_order(
        vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("subdir", true, 0, 2000),
        ],
        sort_mode!(Name, Asc),
        false,
        false,
        &["file.txt", "subdir"],
    );
}

#[test]
fn test_sort_sensitive_true() {
    assert_sort_order(
        vec![
            create_test_entry("banana", false, 100, 1000),
            create_test_entry("Apple", false, 200, 1000),
            create_test_entry("cherry", false, 150, 1000),
        ],
        sort_mode!(Name, Asc),
        true,
        true,
        &["Apple", "banana", "cherry"],
    );
}

#[test]
fn test_sort_extension_asc_sensitive_true() {
    assert_sort_order(
        vec![
            create_test_entry("b.TXT", false, 100, 1000),
            create_test_entry("a.txt", false, 200, 1500),
            create_test_entry("c.txt", false, 150, 1200),
        ],
        sort_mode!(Extension, Asc),
        true,
        true,
        &["b.TXT", "a.txt", "c.txt"],
    );
}

#[test]
fn test_sort_size_asc_dir_first_false() {
    assert_sort_order(
        vec![
            create_test_entry("medium.txt", false, 500, 1000),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("large.bin", false, 1000, 1500),
            create_test_entry("tiny", false, 10, 1200),
        ],
        sort_mode!(Size, Asc),
        false,
        false,
        &["subdir", "tiny", "medium.txt", "large.bin"],
    );
}

#[test]
fn test_sort_natural_name_desc_dir_first_false() {
    assert_sort_order(
        vec![
            create_test_entry("a10.txt", false, 100, 100),
            create_test_entry("a2.txt", false, 100, 100),
            create_test_entry("a1.txt", false, 100, 100),
        ],
        sort_mode!(NaturalName, Desc),
        false,
        false,
        &["a10.txt", "a2.txt", "a1.txt"],
    );
}

#[test]
fn test_sort_ellipsis_first_even_with_dir_first_false() {
    assert_sort_order(
        vec![
            create_test_entry("file.txt", false, 100, 1000),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("..", true, 0, 0),
        ],
        sort_mode!(Name, Asc),
        false,
        false,
        &["..", "file.txt", "subdir"],
    );
}

// ── '..' stays first across every field and direction ──

#[test]
fn test_ellipsis_first_all_modes() {
    for mode in [
        sort_mode!(Name, Asc),
        sort_mode!(Name, Desc),
        sort_mode!(NaturalName, Asc),
        sort_mode!(NaturalName, Desc),
        sort_mode!(Size, Asc),
        sort_mode!(Size, Desc),
        sort_mode!(ModTime, Desc),
        sort_mode!(Btime, Desc),
        sort_mode!(Extension, Desc),
    ] {
        assert_ellipsis_first(mode);
    }
}

// ── Previously missing combinations ──

#[test]
fn test_sort_dir_first_false_sensitive_true() {
    assert_sort_order(
        vec![
            create_test_entry("zebra.txt", false, 100, 1000),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("Apple.txt", false, 200, 1500),
            create_test_entry("banana.txt", false, 150, 1200),
        ],
        sort_mode!(Name, Asc),
        false,
        true,
        &["Apple.txt", "banana.txt", "subdir", "zebra.txt"],
    );
}

// ── Sensitive=true tiebreak coverage (Size / ModTime / Btime / NaturalName) ──

#[test]
fn test_sort_size_sensitive_tiebreak() {
    // Equal size → case-sensitive name tiebreak: uppercase before lowercase.
    assert_sort_order(
        vec![
            make_entry("banana", false, 100, 0, None),
            make_entry("Apple", false, 100, 0, None),
            make_entry("apple", false, 100, 0, None),
        ],
        sort_mode!(Size, Asc),
        true,
        true,
        &["Apple", "apple", "banana"],
    );
}

#[test]
fn test_sort_mod_time_sensitive_tiebreak() {
    assert_sort_order(
        vec![
            make_entry("banana", false, 10, 1000, None),
            make_entry("Apple", false, 20, 1000, None),
            make_entry("apple", false, 30, 1000, None),
        ],
        sort_mode!(ModTime, Asc),
        true,
        true,
        &["Apple", "apple", "banana"],
    );
}

#[test]
fn test_sort_btime_sensitive_tiebreak() {
    assert_sort_order(
        vec![
            make_entry("banana", false, 10, 0, Some(1000)),
            make_entry("Apple", false, 20, 0, Some(1000)),
            make_entry("apple", false, 30, 0, Some(1000)),
        ],
        sort_mode!(Btime, Asc),
        true,
        true,
        &["Apple", "apple", "banana"],
    );
}

#[test]
fn test_sort_natural_name_sensitive_tiebreak() {
    // Same natural key "alpha2" → case-sensitive tiebreak orders by raw bytes.
    assert_sort_order(
        vec![
            create_test_entry("alpha2", false, 100, 100),
            create_test_entry("Alpha2", false, 100, 100),
            create_test_entry("ALPHA2", false, 100, 100),
        ],
        sort_mode!(NaturalName, Asc),
        true,
        true,
        &["ALPHA2", "Alpha2", "alpha2"],
    );
}

// ── dir_first=false coverage (ModTime / Btime / Extension) ──

#[test]
fn test_sort_mod_time_dir_first_false() {
    // Without dir_first, the directory interleaves with files by mtime.
    assert_sort_order(
        vec![
            create_test_entry("old.txt", false, 10, 1000),
            create_test_entry("mid_dir", true, 0, 1500),
            create_test_entry("new.txt", false, 10, 2000),
        ],
        sort_mode!(ModTime, Asc),
        false,
        false,
        &["old.txt", "mid_dir", "new.txt"],
    );
}

#[test]
fn test_sort_btime_dir_first_false() {
    assert_sort_order(
        vec![
            make_entry("old.txt", false, 10, 0, Some(1000)),
            make_entry("mid_dir", true, 0, 0, Some(1500)),
            make_entry("new.txt", false, 10, 0, Some(2000)),
        ],
        sort_mode!(Btime, Asc),
        false,
        false,
        &["old.txt", "mid_dir", "new.txt"],
    );
}

#[test]
fn test_sort_extension_dir_first_false() {
    // Directories have no extension, so they sort with the empty-extension group.
    assert_sort_order(
        vec![
            create_test_entry("image.png", false, 100, 1000),
            create_test_entry("subdir", true, 0, 2000),
            create_test_entry("archive.zip", false, 150, 1200),
        ],
        sort_mode!(Extension, Asc),
        false,
        false,
        &["subdir", "image.png", "archive.zip"],
    );
}

// ── Stability (tiebreaker) tests for untested sort modes ──

#[test]
fn test_sort_mod_time_asc_same_mtime_stable() {
    let mut entries = vec![
        create_test_entry("c.txt", false, 100, 1000),
        create_test_entry("a.txt", false, 200, 1000),
        create_test_entry("b.txt", false, 150, 1000),
    ];
    sort_entries(
        &mut entries,
        sort_mode!(ModTime, Asc),
        SortOptions::default(),
    );
    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[1].name, "b.txt");
    assert_eq!(entries[2].name, "c.txt");
}

#[test]
fn test_sort_size_stable() {
    let mut entries = vec![
        create_test_entry("b_medium.txt", false, 200, 1000),
        create_test_entry("a_medium.txt", false, 200, 1000),
        create_test_entry("c_small.txt", false, 100, 1000),
    ];
    sort_entries(&mut entries, sort_mode!(Size, Asc), SortOptions::default());
    assert_eq!(entries[0].name, "c_small.txt");
    assert_eq!(entries[1].name, "a_medium.txt");
    assert_eq!(entries[2].name, "b_medium.txt");
}

#[test]
fn test_sort_extension_stable() {
    let mut entries = vec![
        create_test_entry("b_data.txt", false, 100, 1000),
        create_test_entry("a_config.txt", false, 100, 1000),
        create_test_entry("c_image.png", false, 100, 1000),
    ];
    sort_entries(
        &mut entries,
        sort_mode!(Extension, Asc),
        SortOptions::default(),
    );
    assert_eq!(entries[0].name, "c_image.png");
    assert_eq!(entries[1].name, "a_config.txt");
    assert_eq!(entries[2].name, "b_data.txt");
}

#[test]
fn test_sort_natural_name_stable() {
    let mut entries = vec![
        create_test_entry("b_file1.txt", false, 100, 1000),
        create_test_entry("a_file1.txt", false, 100, 1000),
        create_test_entry("z_file2.txt", false, 100, 1000),
    ];
    sort_entries(
        &mut entries,
        sort_mode!(NaturalName, Asc),
        SortOptions::default(),
    );
    assert_eq!(entries[0].name, "a_file1.txt");
    assert_eq!(entries[1].name, "b_file1.txt");
    assert_eq!(entries[2].name, "z_file2.txt");
}

// ── Unicode filename sort ──

#[test]
fn test_sort_entries_unicode() {
    let mut entries = vec![
        create_test_entry("\u{6587}\u{4ef6}\u{540d}.txt", false, 100, 1000),
        create_test_entry("\u{6d4b}\u{8bd5}", false, 100, 1000),
        create_test_entry("\u{1f680}.rs", false, 100, 1000),
        create_test_entry("caf\u{e9}.txt", false, 100, 1000),
        create_test_entry("\u{f1}o\u{f1}o", false, 100, 1000),
        create_test_entry("alpha.txt", false, 100, 1000),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "alpha.txt");
    assert_eq!(entries[1].name, "caf\u{e9}.txt");
    assert_eq!(entries[2].name, "\u{f1}o\u{f1}o");
    assert_eq!(entries[3].name, "\u{6587}\u{4ef6}\u{540d}.txt");
    assert_eq!(entries[4].name, "\u{6d4b}\u{8bd5}");
    assert_eq!(entries[5].name, "\u{1f680}.rs");
}

#[test]
fn test_sort_mtime_none_after_known() {
    use crate::app::types::test_helpers::TestEntry;
    let mut no_mtime = TestEntry::new("unknown.txt")
        .path("unknown.txt")
        .file(0)
        .build();
    no_mtime.cha.mtime = None;
    let (time_str, _, _, _, _) = FileEntry::cached_fields(&no_mtime.cha, &no_mtime.name);
    no_mtime.time_str = time_str;
    let with_mtime = TestEntry::new("known.txt")
        .path("known.txt")
        .file(0)
        .modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
        .build();

    let mut entries = vec![no_mtime, with_mtime];
    sort_entries(
        &mut entries,
        sort_mode!(ModTime, Desc),
        SortOptions::default(),
    );
    assert_eq!(entries[0].name, "known.txt");
    assert_eq!(entries[1].name, "unknown.txt");
}
