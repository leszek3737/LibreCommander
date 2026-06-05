use super::*;
use std::time::SystemTime;

fn create_test_entry(name: &str, is_dir: bool, size: u64, modified_secs: u64) -> FileEntry {
    create_test_entry_with_btime(name, is_dir, size, modified_secs, Some(modified_secs))
}

fn create_test_entry_with_btime(
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

    sort_entries(&mut entries, SortMode::NameAsc, SortOptions::default());

    assert_eq!(entries[0].name, "apple");
    assert_eq!(entries[1].name, "Apple");
    assert_eq!(entries[2].name, "banana");
    assert_eq!(entries[3].name, "Cherry");
    assert_eq!(entries[4].name, "zebra");
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
fn test_sort_btime_asc_none_after_some() {
    let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
    let t2 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(200);
    let mut entries = vec![
        FileEntry::builder()
            .name("no_btime.txt")
            .path("no_btime.txt")
            .size(10)
            .modified(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("old.txt")
            .path("old.txt")
            .size(10)
            .modified(t1)
            .created(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("new.txt")
            .path("new.txt")
            .size(10)
            .modified(t2)
            .created(t2)
            .owner("u")
            .group("g")
            .build(),
    ];

    sort_entries(&mut entries, SortMode::BtimeAsc, SortOptions::default());

    assert_eq!(entries[0].name, "old.txt");
    assert_eq!(entries[1].name, "new.txt");
    assert_eq!(entries[2].name, "no_btime.txt");
}

#[test]
fn test_sort_btime_desc_none_after_some() {
    let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
    let t2 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(200);
    let mut entries = vec![
        FileEntry::builder()
            .name("no_btime.txt")
            .path("no_btime.txt")
            .size(10)
            .modified(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("old.txt")
            .path("old.txt")
            .size(10)
            .modified(t1)
            .created(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("new.txt")
            .path("new.txt")
            .size(10)
            .modified(t2)
            .created(t2)
            .owner("u")
            .group("g")
            .build(),
    ];

    sort_entries(&mut entries, SortMode::BtimeDesc, SortOptions::default());

    assert_eq!(entries[0].name, "new.txt");
    assert_eq!(entries[1].name, "old.txt");
    assert_eq!(entries[2].name, "no_btime.txt");
}

#[test]
fn test_sort_btime_same_btime_stable() {
    let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(300);
    let mut entries = vec![
        FileEntry::builder()
            .name("beta.txt")
            .path("beta.txt")
            .size(10)
            .modified(t)
            .created(t)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("alpha.txt")
            .path("alpha.txt")
            .size(10)
            .modified(t)
            .created(t)
            .owner("u")
            .group("g")
            .build(),
    ];

    sort_entries(&mut entries, SortMode::BtimeAsc, SortOptions::default());

    assert_eq!(entries[0].name, "alpha.txt");
    assert_eq!(entries[1].name, "beta.txt");
}

#[test]
fn test_sort_btime_all_none() {
    let mut entries = vec![
        create_test_entry_with_btime("c.txt", false, 10, 100, None),
        create_test_entry_with_btime("a.txt", false, 10, 100, None),
        create_test_entry_with_btime("b.txt", false, 10, 100, None),
    ];

    sort_entries(&mut entries, SortMode::BtimeAsc, SortOptions::default());

    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[1].name, "b.txt");
    assert_eq!(entries[2].name, "c.txt");
}

#[test]
fn test_sort_btime_mixed_with_dirs() {
    let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
    let t2 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(200);
    let mut entries = vec![
        FileEntry::builder()
            .name("file_no_btime")
            .path("file_no_btime")
            .size(10)
            .modified(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("dir_old")
            .path("dir_old")
            .is_dir(true)
            .size(0)
            .modified(t1)
            .created(t1)
            .owner("u")
            .group("g")
            .build(),
        FileEntry::builder()
            .name("dir_new")
            .path("dir_new")
            .is_dir(true)
            .size(0)
            .modified(t2)
            .created(t2)
            .owner("u")
            .group("g")
            .build(),
    ];

    sort_entries(&mut entries, SortMode::BtimeAsc, SortOptions::default());

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

#[test]
fn test_sort_dir_first_false() {
    let mut entries = vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
    ];
    sort_entries(
        &mut entries,
        SortMode::NameAsc,
        SortOptions {
            dir_first: false,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "file.txt");
    assert_eq!(entries[1].name, "subdir");
}

#[test]
fn test_sort_sensitive_true() {
    let mut entries = vec![
        create_test_entry("banana", false, 100, 1000),
        create_test_entry("Apple", false, 200, 1000),
        create_test_entry("cherry", false, 150, 1000),
    ];
    sort_entries(
        &mut entries,
        SortMode::NameAsc,
        SortOptions {
            sensitive: true,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "Apple");
    assert_eq!(entries[1].name, "banana");
    assert_eq!(entries[2].name, "cherry");
}

#[test]
fn test_get_extension_edge_cases() {
    assert_eq!(get_extension("file."), "");
    assert_eq!(get_extension("a.b.c.d"), ".d");
    assert_eq!(get_extension(""), "");
}

#[test]
fn test_cmp_ignore_case_turkish_dotted_i() {
    let left = cmp_ignore_case("B\u{0130}L", "BIL");
    let right = cmp_ignore_case("BIL", "B\u{0130}L");
    assert_eq!(left, Ordering::Greater);
    assert_eq!(right, Ordering::Less);
}

#[test]
fn test_sort_extension_asc_sensitive_true() {
    let mut entries = vec![
        create_test_entry("b.TXT", false, 100, 1000),
        create_test_entry("a.txt", false, 200, 1500),
        create_test_entry("c.txt", false, 150, 1200),
    ];
    sort_entries(
        &mut entries,
        SortMode::ExtensionAsc,
        SortOptions {
            sensitive: true,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "b.TXT");
    assert_eq!(entries[1].name, "a.txt");
    assert_eq!(entries[2].name, "c.txt");
}

#[test]
fn test_sort_size_asc_dir_first_false() {
    let mut entries = vec![
        create_test_entry("medium.txt", false, 500, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("large.bin", false, 1000, 1500),
        create_test_entry("tiny", false, 10, 1200),
    ];
    sort_entries(
        &mut entries,
        SortMode::SizeAsc,
        SortOptions {
            dir_first: false,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "subdir");
    assert_eq!(entries[1].name, "tiny");
    assert_eq!(entries[2].name, "medium.txt");
    assert_eq!(entries[3].name, "large.bin");
}

#[test]
fn test_sort_mod_time_asc_same_mtime_stable() {
    let mut entries = vec![
        create_test_entry("c.txt", false, 100, 1000),
        create_test_entry("a.txt", false, 200, 1000),
        create_test_entry("b.txt", false, 150, 1000),
    ];
    sort_entries(&mut entries, SortMode::ModTimeAsc, SortOptions::default());
    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[1].name, "b.txt");
    assert_eq!(entries[2].name, "c.txt");
}

#[test]
fn test_sort_natural_name_desc_dir_first_false() {
    let mut entries = vec![
        create_test_entry("a10.txt", false, 100, 100),
        create_test_entry("a2.txt", false, 100, 100),
        create_test_entry("a1.txt", false, 100, 100),
    ];
    sort_entries(
        &mut entries,
        SortMode::NaturalNameDesc,
        SortOptions {
            dir_first: false,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "a10.txt");
    assert_eq!(entries[1].name, "a2.txt");
    assert_eq!(entries[2].name, "a1.txt");
}

#[test]
fn test_sort_ellipsis_first_even_with_dir_first_false() {
    let mut entries = vec![
        create_test_entry("file.txt", false, 100, 1000),
        create_test_entry("subdir", true, 0, 2000),
        create_test_entry("..", true, 0, 0),
    ];
    sort_entries(
        &mut entries,
        SortMode::NameAsc,
        SortOptions {
            dir_first: false,
            ..SortOptions::default()
        },
    );
    assert_eq!(entries[0].name, "..");
}
