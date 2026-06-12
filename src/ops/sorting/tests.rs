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
    let ts = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified_secs);
    let mut builder = FileEntry::builder()
        .name(name)
        .path(name)
        .is_dir(is_dir)
        .size(size)
        .modified(ts)
        .owner("testuser")
        .group("testgroup");
    if let Some(btime) = btime_secs {
        builder = builder.created(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(btime));
    }
    builder.build()
}

fn create_test_entry(name: &str, is_dir: bool, size: u64, modified_secs: u64) -> FileEntry {
    make_entry(name, is_dir, size, modified_secs, None)
}

macro_rules! sort_options_test {
    ($fn_name:ident, $mode:expr, $dir_first:literal, $sensitive:literal,
     $entries:expr, $expected_names:expr) => {
        #[test]
        fn $fn_name() {
            let mut entries = $entries;
            sort_entries(
                &mut entries,
                $mode,
                SortOptions {
                    dir_first: $dir_first,
                    sensitive: $sensitive,
                },
            );
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, $expected_names);
        }
    };
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
fn test_sort_ellipsis_at_top() {
    let mut entries = vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("..", true, 0, 0),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("another.txt", false, 200, 1500),
    ];

    sort_entries(&mut entries, sort_mode!(Name, Asc), SortOptions::default());

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
fn test_sort_by_size() {
    let mut entries = vec![
        create_test_entry("small.txt", false, 100, 1000),
        create_test_entry("large.txt", false, 10000, 1500),
        create_test_entry("medium.txt", false, 1000, 1200),
    ];

    sort_entries(&mut entries, sort_mode!(Size, Asc), SortOptions::default());

    assert_eq!(entries[0].name, "small.txt");
    assert_eq!(entries[1].name, "medium.txt");
    assert_eq!(entries[2].name, "large.txt");

    sort_entries(&mut entries, sort_mode!(Size, Desc), SortOptions::default());

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

    sort_entries(
        &mut entries,
        sort_mode!(ModTime, Asc),
        SortOptions::default(),
    );

    assert_eq!(entries[0].name, "old.txt");
    assert_eq!(entries[1].name, "middle.txt");
    assert_eq!(entries[2].name, "new.txt");

    sort_entries(
        &mut entries,
        sort_mode!(ModTime, Desc),
        SortOptions::default(),
    );

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

    sort_entries(
        &mut entries,
        sort_mode!(Extension, Asc),
        SortOptions::default(),
    );

    assert_eq!(entries[0].name, "image.png");
    assert_eq!(entries[1].name, "script.sh");
    assert_eq!(entries[2].name, "file.txt");
    assert_eq!(entries[3].name, "archive.zip");

    sort_entries(
        &mut entries,
        sort_mode!(Extension, Desc),
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
fn test_sort_natural_ellipsis_first() {
    let mut entries = vec![
        create_test_entry("..", true, 0, 0),
        create_test_entry("z10", false, 100, 100),
        create_test_entry("a2", false, 100, 100),
        create_test_entry("a1", false, 100, 100),
    ];

    sort_entries(
        &mut entries,
        sort_mode!(NaturalName, Asc),
        SortOptions::default(),
    );

    assert_eq!(entries[0].name, "..");
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

// ── SortOptions parameterized tests ──

sort_options_test!(
    test_sort_dir_first_false,
    sort_mode!(Name, Asc),
    false,
    false,
    vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
    ],
    ["file.txt", "subdir"]
);

sort_options_test!(
    test_sort_sensitive_true,
    sort_mode!(Name, Asc),
    true,
    true,
    vec![
        create_test_entry("banana", false, 100, 1000),
        create_test_entry("Apple", false, 200, 1000),
        create_test_entry("cherry", false, 150, 1000),
    ],
    ["Apple", "banana", "cherry"]
);

sort_options_test!(
    test_sort_extension_asc_sensitive_true,
    sort_mode!(Extension, Asc),
    true,
    true,
    vec![
        create_test_entry("b.TXT", false, 100, 1000),
        create_test_entry("a.txt", false, 200, 1500),
        create_test_entry("c.txt", false, 150, 1200),
    ],
    ["b.TXT", "a.txt", "c.txt"]
);

sort_options_test!(
    test_sort_size_asc_dir_first_false,
    sort_mode!(Size, Asc),
    false,
    false,
    vec![
        create_test_entry("medium.txt", false, 500, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("large.bin", false, 1000, 1500),
        create_test_entry("tiny", false, 10, 1200),
    ],
    ["subdir", "tiny", "medium.txt", "large.bin"]
);

sort_options_test!(
    test_sort_natural_name_desc_dir_first_false,
    sort_mode!(NaturalName, Desc),
    false,
    false,
    vec![
        create_test_entry("a10.txt", false, 100, 100),
        create_test_entry("a2.txt", false, 100, 100),
        create_test_entry("a1.txt", false, 100, 100),
    ],
    ["a10.txt", "a2.txt", "a1.txt"]
);

sort_options_test!(
    test_sort_ellipsis_first_even_with_dir_first_false,
    sort_mode!(Name, Asc),
    false,
    false,
    vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("..", true, 0, 0),
    ],
    ["..", "file.txt", "subdir"]
);

// ── Previously missing combinations ──

sort_options_test!(
    test_sort_dir_first_false_sensitive_true,
    sort_mode!(Name, Asc),
    false,
    true,
    vec![
        create_test_entry("zebra.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("Apple.txt", false, 200, 1500),
        create_test_entry("banana.txt", false, 150, 1200),
    ],
    ["Apple.txt", "banana.txt", "subdir", "zebra.txt"]
);

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
    let no_mtime = FileEntry::builder()
        .name("unknown.txt")
        .path("unknown.txt")
        .build();
    let with_mtime = FileEntry::builder()
        .name("known.txt")
        .path("known.txt")
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
