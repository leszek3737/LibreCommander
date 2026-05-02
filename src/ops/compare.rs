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
                    is_dir: e.is_dir,
                    size: e.size,
                    mtime: e.modified,
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
                    is_dir: e.is_dir,
                    size: e.size,
                    mtime: e.modified,
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
    for entry in &mut left_panel.entries {
        if entry.name != ".." {
            entry.selected = report.left_marks.contains(&entry.name);
        } else {
            entry.selected = false;
        }
    }
    left_panel.recalculate_selection_stats();

    for entry in &mut right_panel.entries {
        if entry.name != ".." {
            entry.selected = report.right_marks.contains(&entry.name);
        } else {
            entry.selected = false;
        }
    }
    right_panel.recalculate_selection_stats();
}
