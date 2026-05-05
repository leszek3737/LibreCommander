use std::collections::{HashMap, HashSet};

use crate::app::types::{CompareMode, FileEntry, PanelState};

#[derive(Clone, Copy, PartialEq)]
struct EntryMeta {
    is_dir: bool,
    size: u64,
    mtime: std::time::SystemTime,
}

fn meta_matches(left: &EntryMeta, right: &EntryMeta, mode: CompareMode) -> bool {
    if left.is_dir != right.is_dir {
        return false;
    }
    if left.is_dir {
        return true;
    }
    match mode {
        CompareMode::Quick => true,
        CompareMode::Size => left.size == right.size,
        CompareMode::Thorough => left.size == right.size && left.mtime == right.mtime,
    }
}

pub struct CompareReport {
    pub left_marks: HashSet<String>,
    pub right_marks: HashSet<String>,
    pub unique_left: usize,
    pub unique_right: usize,
    pub differing: usize,
}

pub fn compare_entries(
    left: &[FileEntry],
    right: &[FileEntry],
    mode: CompareMode,
) -> CompareReport {
    let right_meta: HashMap<&str, EntryMeta> = right
        .iter()
        .filter(|e| e.name != "..")
        .map(|e| {
            (
                e.name.as_str(),
                EntryMeta {
                    is_dir: e.is_dir(),
                    size: e.len(),
                    mtime: e.mtime(),
                },
            )
        })
        .collect();

    let left_meta: HashMap<&str, EntryMeta> = left
        .iter()
        .filter(|e| e.name != "..")
        .map(|e| {
            (
                e.name.as_str(),
                EntryMeta {
                    is_dir: e.is_dir(),
                    size: e.len(),
                    mtime: e.mtime(),
                },
            )
        })
        .collect();

    let mut unique_left: usize = 0;
    let mut unique_right: usize = 0;
    let mut differing: usize = 0;

    for (name, left_m) in &left_meta {
        match right_meta.get(name) {
            None => unique_left += 1,
            Some(right_m) => {
                if !meta_matches(left_m, right_m, mode) {
                    differing += 1;
                }
            }
        }
    }
    for name in right_meta.keys() {
        if !left_meta.contains_key(name) {
            unique_right += 1;
        }
    }

    let mut left_to_mark: HashSet<String> = HashSet::new();
    let mut right_to_mark: HashSet<String> = HashSet::new();

    for (name, left_m) in &left_meta {
        let should_mark = match right_meta.get(name) {
            None => true,
            Some(right_m) => !meta_matches(left_m, right_m, mode),
        };
        if should_mark {
            left_to_mark.insert(name.to_string());
        }
    }

    for (name, right_m) in &right_meta {
        match left_meta.get(name) {
            None => right_to_mark.insert(name.to_string()),
            Some(left_m) => {
                if meta_matches(left_m, right_m, mode) {
                    false
                } else {
                    right_to_mark.insert(name.to_string())
                }
            }
        };
    }

    CompareReport {
        left_marks: left_to_mark,
        right_marks: right_to_mark,
        unique_left,
        unique_right,
        differing,
    }
}

pub fn apply_compare_to_panels(
    left_panel: &mut PanelState,
    right_panel: &mut PanelState,
    report: &CompareReport,
) {
    apply_marks(left_panel, &report.left_marks);
    left_panel.recalculate_selection_stats();

    apply_marks(right_panel, &report.right_marks);
    right_panel.recalculate_selection_stats();
}

fn apply_marks(panel: &mut PanelState, marks: &HashSet<String>) {
    for entry in &mut panel.entries {
        entry.selected = if entry.name != ".." {
            marks.contains(&entry.name)
        } else {
            false
        };
    }
    for entry in &mut panel.unfiltered_entries {
        entry.selected = if entry.name != ".." {
            marks.contains(&entry.name)
        } else {
            false
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::FileEntry;
    use std::path::PathBuf;

    fn entry(name: &str, size: u64) -> FileEntry {
        FileEntry::builder()
            .name(name)
            .path(format!("/tmp/{name}"))
            .size(size)
            .build()
    }

    fn dir_entry(name: &str) -> FileEntry {
        FileEntry::builder()
            .name(name)
            .path(format!("/tmp/{name}"))
            .is_dir(true)
            .permissions(0o755)
            .build()
    }

    #[test]
    fn quick_mode_matches_by_name_only() {
        let left = vec![entry("a.txt", 10), entry("b.txt", 20)];
        let right = vec![entry("a.txt", 99), entry("c.txt", 30)];

        let report = compare_entries(&left, &right, CompareMode::Quick);

        assert_eq!(report.unique_left, 1);
        assert_eq!(report.unique_right, 1);
        assert_eq!(report.differing, 0);
        assert!(report.left_marks.contains("b.txt"));
        assert!(report.right_marks.contains("c.txt"));
        assert!(!report.left_marks.contains("a.txt"));
    }

    #[test]
    fn size_mode_detects_different_sizes() {
        let left = vec![entry("a.txt", 10)];
        let right = vec![entry("a.txt", 20)];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.differing, 1);
        assert!(report.left_marks.contains("a.txt"));
        assert!(report.right_marks.contains("a.txt"));
    }

    #[test]
    fn thorough_mode_matches_on_size_and_mtime() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let left = vec![
            FileEntry::builder()
                .name("a.txt")
                .path("/tmp/a.txt")
                .size(100)
                .modified(t)
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build(),
        ];
        let right = vec![
            FileEntry::builder()
                .name("a.txt")
                .path("/tmp/a.txt")
                .size(100)
                .modified(t + std::time::Duration::from_secs(1))
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 1);
    }

    #[test]
    fn dotdot_entries_are_ignored() {
        let left = vec![
            FileEntry::builder()
                .name("..")
                .path("/tmp/..")
                .is_dir(true)
                .permissions(0o755)
                .build(),
        ];
        let right = vec![];

        let report = compare_entries(&left, &right, CompareMode::Quick);

        assert_eq!(report.unique_left, 0);
        assert!(report.left_marks.is_empty());
    }

    #[test]
    fn dirs_always_match_in_quick_mode() {
        let left = vec![dir_entry("src")];
        let right = vec![dir_entry("src")];

        let report = compare_entries(&left, &right, CompareMode::Quick);

        assert_eq!(report.unique_left, 0);
        assert_eq!(report.unique_right, 0);
        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
    }

    #[test]
    fn dirs_ignore_filesystem_size_in_size_mode() {
        let left = vec![
            FileEntry::builder()
                .name("src")
                .path("/tmp/src")
                .is_dir(true)
                .size(4096)
                .permissions(0o755)
                .build(),
        ];
        let right = vec![
            FileEntry::builder()
                .name("src")
                .path("/tmp/src")
                .is_dir(true)
                .size(8192)
                .permissions(0o755)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn dirs_match_in_size_mode_when_equal() {
        let left = vec![
            FileEntry::builder()
                .name("src")
                .path("/tmp/src")
                .is_dir(true)
                .size(4096)
                .permissions(0o755)
                .build(),
        ];
        let right = vec![
            FileEntry::builder()
                .name("src")
                .path("/tmp/src")
                .is_dir(true)
                .size(4096)
                .permissions(0o755)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn dirs_ignore_filesystem_size_and_mtime_in_thorough_mode() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let left = vec![
            FileEntry::builder()
                .name("lib")
                .path("/tmp/lib")
                .is_dir(true)
                .size(4096)
                .modified(t)
                .permissions(0o755)
                .build(),
        ];
        let right = vec![
            FileEntry::builder()
                .name("lib")
                .path("/tmp/lib")
                .is_dir(true)
                .size(8192)
                .modified(t + std::time::Duration::from_secs(60))
                .permissions(0o755)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn dirs_match_in_thorough_mode_when_identical() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let left = vec![
            FileEntry::builder()
                .name("lib")
                .path("/tmp/lib")
                .is_dir(true)
                .size(4096)
                .modified(t)
                .permissions(0o755)
                .build(),
        ];
        let right = vec![
            FileEntry::builder()
                .name("lib")
                .path("/tmp/lib")
                .is_dir(true)
                .size(4096)
                .modified(t)
                .permissions(0o755)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn apply_marks_selected_flags() {
        let mut left_panel = PanelState {
            path: PathBuf::from("/tmp"),
            entries: vec![entry("a.txt", 10), entry("b.txt", 20)],
            cursor: 0,
            scroll_offset: 0,
            sort_mode: crate::app::types::SortMode::NameAsc,
            listing_mode: crate::app::types::ListingMode::Long,
            show_hidden: false,
            filter: None,
            sort_options: crate::app::types::SortOptions::default(),
            selected_count: 0,
            selected_size: 0,
            total_size: 0,
            last_error: None,
            history: vec![],
            unfiltered_entries: vec![],
            unfiltered_dirty: true,
        };
        let mut right_panel = PanelState {
            path: PathBuf::from("/tmp"),
            entries: vec![entry("a.txt", 10)],
            cursor: 0,
            scroll_offset: 0,
            sort_mode: crate::app::types::SortMode::NameAsc,
            listing_mode: crate::app::types::ListingMode::Long,
            show_hidden: false,
            filter: None,
            sort_options: crate::app::types::SortOptions::default(),
            selected_count: 0,
            selected_size: 0,
            total_size: 0,
            last_error: None,
            history: vec![],
            unfiltered_entries: vec![],
            unfiltered_dirty: true,
        };

        let report = CompareReport {
            left_marks: vec!["b.txt".to_string()].into_iter().collect(),
            right_marks: HashSet::new(),
            unique_left: 1,
            unique_right: 0,
            differing: 0,
        };

        apply_compare_to_panels(&mut left_panel, &mut right_panel, &report);

        assert!(!left_panel.entries[0].selected);
        assert!(left_panel.entries[1].selected);
        assert!(!right_panel.entries[0].selected);
    }
}
