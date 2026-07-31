use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use crate::app::types::{CompareMode, FileEntry, PanelState};

/// Cross-filesystem mtime resolution tolerance (e.g. FAT32 has 2s granularity,
/// network filesystems may lose sub-second precision during sync). Set slightly
/// above FAT32's 2s granularity: when two filesystems round a timestamp in
/// opposite directions the recorded values can differ by *just over* 2s for what
/// is really the same modification, so an exact 2s bound would report a spurious
/// difference. The extra 500ms absorbs that boundary rounding.
// ponytail: FAT32-coupled constant is fine — the 2.5s ceiling covers all
// common filesystem precisions without per-FS tuning.
const MTIME_TOLERANCE: Duration = Duration::from_millis(2500);

/// Pseudo-entries — ignored during comparison.
const PARENT_DIR: &str = "..";
const CURRENT_DIR: &str = ".";

#[derive(Clone, Copy)]
struct EntryMeta {
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    mtime: Option<SystemTime>,
}

fn meta_matches(left: EntryMeta, right: EntryMeta, mode: CompareMode) -> bool {
    if left.is_dir != right.is_dir {
        return false;
    }
    // Symlink vs non-symlink is a type mismatch even when both resolve to files.
    if left.is_symlink != right.is_symlink {
        return false;
    }
    // Symlinks match by name only. `Cha` built via `from_link_metadata` with a
    // resolved target stores the *target's* size/mtime in `len`/`mtime`, not
    // the link's own lstat data — and the link's defining datum (its target
    // path string) is not kept in `Cha` at all. Comparing target-derived
    // size/mtime would compare *targets*, not *links* (AGENTS.md: symlinks are
    // data, so a symlink is compared by existence/name, not its target's stats).
    if left.is_symlink {
        return true;
    }
    if left.is_dir {
        return true;
    }
    match mode {
        CompareMode::Quick => true,
        CompareMode::Size => left.size == right.size,
        CompareMode::Thorough => {
            left.size == right.size
                && match (left.mtime, right.mtime) {
                    (Some(l), Some(r)) => mtime_matches(l, r),
                    (None, None) => true,
                    _ => false,
                }
        }
    }
}

fn entry_to_meta(entry: &FileEntry) -> EntryMeta {
    // Symlinks are data (AGENTS.md): a symlink pointing at a directory must
    // not be treated as one. `is_symlink` is carried separately so
    // `meta_matches` can match links by name only — their `cha.len`/`cha.mtime`
    // are target-derived when the link was followed, not link-lstat data.
    EntryMeta {
        is_dir: !entry.is_symlink() && entry.is_dir(),
        is_symlink: entry.is_symlink(),
        size: entry.size(),
        mtime: entry.cha.mtime,
    }
}

fn mtime_matches(left: SystemTime, right: SystemTime) -> bool {
    // `duration_since` fails when the argument is in the future relative
    // to `self`.  The `left > right` / `right > left` guards above make
    // this logically impossible, but clock adjustments or filesystem
    // inconsistencies can produce timestamps that appear future-dated.
    // Fallback: return a value strictly larger than tolerance so the
    // comparison always reports a mismatch (conservative; no false match).
    let diff = if left > right {
        left.duration_since(right)
            .unwrap_or(MTIME_TOLERANCE + Duration::from_secs(1))
    } else {
        right
            .duration_since(left)
            .unwrap_or(MTIME_TOLERANCE + Duration::from_secs(1))
    };
    diff <= MTIME_TOLERANCE
}

#[derive(Debug)]
pub struct CompareReport {
    pub left_marks: HashSet<String>,
    pub right_marks: HashSet<String>,
    pub unique_left: usize,
    pub unique_right: usize,
    pub differing: usize,
}

/// Compare two directory listings by file name and report the differences.
///
/// Entries are matched by name (the `.` and `..` pseudo-entries are ignored).
/// For names present on both sides, [`meta_matches`] decides whether they
/// differ under the given [`CompareMode`]. The returned [`CompareReport`] carries
/// the names to mark on each side plus the unique/differing counts.
///
/// Duplicate names on the right side are resolved first-wins (the first entry
/// seen for a name is kept); the left side is not deduplicated, so duplicate
/// left names are each matched independently against the right-side map.
pub fn compare_entries(
    left: &[FileEntry],
    right: &[FileEntry],
    mode: CompareMode,
) -> CompareReport {
    // First-wins: `or_insert` keeps the first entry seen for a duplicate name,
    // mirroring the left side's left-to-right iteration order.
    let mut right_meta: HashMap<&str, EntryMeta> = HashMap::with_capacity(right.len());
    for entry in right
        .iter()
        .filter(|e| e.name != PARENT_DIR && e.name != CURRENT_DIR)
    {
        right_meta
            .entry(entry.name.as_str())
            .or_insert(entry_to_meta(entry));
    }

    let mut unique_left: usize = 0;
    let mut unique_right: usize = 0;
    let mut differing: usize = 0;
    let mut left_to_mark: HashSet<String> = HashSet::with_capacity(left.len());
    let mut right_to_mark: HashSet<String> = HashSet::with_capacity(right.len());
    let mut seen_right: HashSet<&str> = HashSet::with_capacity(right_meta.len());

    for entry in left
        .iter()
        .filter(|e| e.name != PARENT_DIR && e.name != CURRENT_DIR)
    {
        let name = entry.name.as_str();
        match right_meta.get(name) {
            None => {
                unique_left += 1;
                left_to_mark.insert(name.to_string());
            }
            Some(right_m) => {
                let left_m = entry_to_meta(entry);
                seen_right.insert(name);
                if !meta_matches(left_m, *right_m, mode) {
                    differing += 1;
                    left_to_mark.insert(name.to_string());
                    right_to_mark.insert(name.to_string());
                }
            }
        }
    }

    for name in right_meta.keys() {
        if !seen_right.contains(name) {
            unique_right += 1;
            right_to_mark.insert(name.to_string());
        }
    }

    CompareReport {
        left_marks: left_to_mark,
        right_marks: right_to_mark,
        unique_left,
        unique_right,
        differing,
    }
}

/// Apply a [`CompareReport`] to both panels, selecting the marked entries.
///
/// Marks are applied to each panel's single entry store so the selection
/// survives a filter toggle, then each panel's selection stats are recomputed.
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

/// Set the selection on every entry to match `marks`.
///
/// This performs a **full selection reset**: every entry's `selected` flag is
/// unconditionally overwritten — entries not in `marks` are deselected,
/// including any prior manual selection or marks from a previous compare with
/// a different scope. Compare results fully replace the current selection.
fn apply_marks(panel: &mut PanelState, marks: &HashSet<String>) {
    for entry in panel.listing.unfiltered_mut() {
        entry.selected = entry.name != PARENT_DIR && marks.contains(&entry.name);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::app::types::FileEntry;
    use crate::app::types::test_helpers::TestEntry;
    use std::path::PathBuf;

    fn entry(name: &str, size: u64) -> FileEntry {
        TestEntry::new(name)
            .path(format!("/tmp/{name}"))
            .file(size)
            .build()
    }

    fn dir_entry(name: &str) -> FileEntry {
        TestEntry::new(name)
            .path(format!("/tmp/{name}"))
            .permissions(0o755)
            .build()
    }

    fn panel_with_entries(entries: Vec<FileEntry>) -> PanelState {
        let mut panel = PanelState::new(PathBuf::from("/tmp"));
        panel.set_entries(entries);
        panel
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
            TestEntry::new("a.txt")
                .path("/tmp/a.txt")
                .file(100)
                .modified(t)
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build(),
        ];
        let right = vec![
            TestEntry::new("a.txt")
                .path("/tmp/a.txt")
                .file(100)
                .modified(t + std::time::Duration::from_secs(3))
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 1);
    }

    #[test]
    fn dotdot_entries_are_ignored() {
        let left = vec![
            TestEntry::new(PARENT_DIR)
                .path("/tmp/..")
                .permissions(0o755)
                .build(),
        ];
        let right = vec![];

        let report = compare_entries(&left, &right, CompareMode::Quick);

        assert_eq!(report.unique_left, 0);
        assert!(report.left_marks.is_empty());
    }

    #[test]
    fn dot_entries_are_ignored() {
        let left = vec![
            TestEntry::new(CURRENT_DIR)
                .path("/tmp/.")
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
            TestEntry::new("src")
                .path("/tmp/src")
                .permissions(0o755)
                .len(4096)
                .build(),
        ];
        let right = vec![
            TestEntry::new("src")
                .path("/tmp/src")
                .permissions(0o755)
                .len(8192)
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
            TestEntry::new("src")
                .path("/tmp/src")
                .permissions(0o755)
                .len(4096)
                .build(),
        ];
        let right = vec![
            TestEntry::new("src")
                .path("/tmp/src")
                .permissions(0o755)
                .len(4096)
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
            TestEntry::new("lib")
                .path("/tmp/lib")
                .permissions(0o755)
                .len(4096)
                .modified(t)
                .build(),
        ];
        let right = vec![
            TestEntry::new("lib")
                .path("/tmp/lib")
                .permissions(0o755)
                .len(8192)
                .modified(t + std::time::Duration::from_secs(60))
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
            TestEntry::new("lib")
                .path("/tmp/lib")
                .permissions(0o755)
                .len(4096)
                .modified(t)
                .build(),
        ];
        let right = vec![
            TestEntry::new("lib")
                .path("/tmp/lib")
                .permissions(0o755)
                .len(4096)
                .modified(t)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn thorough_both_mtime_none_same_size_marks_differing() {
        let left = vec![entry("a.txt", 100)];
        let right = vec![entry("a.txt", 100)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn thorough_one_mtime_none_other_present_differ() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let left = vec![entry("a.txt", 100)];
        let right = vec![
            TestEntry::new("a.txt")
                .path("/tmp/a.txt")
                .file(100)
                .modified(t)
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build(),
        ];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 1);
        assert!(report.left_marks.contains("a.txt"));
        assert!(report.right_marks.contains("a.txt"));
    }

    #[test]
    fn thorough_mtime_within_tolerance_matches() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let make = |delta: u64| {
            TestEntry::new("a.txt")
                .path("/tmp/a.txt")
                .file(100)
                .modified(t + std::time::Duration::from_secs(delta))
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build()
        };
        let left = vec![make(0)];
        let right = vec![make(2)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);
        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn thorough_mtime_outside_tolerance_differs() {
        let t = std::time::SystemTime::UNIX_EPOCH;
        let make = |delta: u64| {
            TestEntry::new("a.txt")
                .path("/tmp/a.txt")
                .file(100)
                .modified(t + std::time::Duration::from_secs(delta))
                .created(std::time::SystemTime::UNIX_EPOCH)
                .build()
        };
        let left = vec![make(0)];
        let right = vec![make(3)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);
        assert_eq!(report.differing, 1);
        assert!(report.left_marks.contains("a.txt"));
        assert!(report.right_marks.contains("a.txt"));
    }

    #[test]
    fn type_mismatch_dir_vs_file_differs_in_thorough() {
        let left = vec![dir_entry("src")];
        let right = vec![entry("src", 0)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 1);
        assert!(report.left_marks.contains("src"));
        assert!(report.right_marks.contains("src"));
    }

    #[test]
    fn empty_panels_zero_diffs() {
        let empty: Vec<FileEntry> = vec![];

        let report = compare_entries(&empty, &empty, CompareMode::Thorough);

        assert_eq!(report.unique_left, 0);
        assert_eq!(report.unique_right, 0);
        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn size_mode_equal_size_files_match() {
        let left = vec![entry("data.bin", 1024)];
        let right = vec![entry("data.bin", 1024)];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.differing, 0);
        assert_eq!(report.unique_left, 0);
        assert_eq!(report.unique_right, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn duplicate_names_use_first_wins() {
        // Duplicate names on the right: first occurrence wins (size 100, not 200).
        let left = vec![entry("dup.txt", 100)];
        let right = vec![entry("dup.txt", 100), entry("dup.txt", 200)];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.unique_left, 0);
        assert_eq!(report.unique_right, 0);
        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn mixed_same_name_different_size_counts_differing() {
        let left = vec![entry("data.bin", 512)];
        let right = vec![entry("data.bin", 1024)];

        let report = compare_entries(&left, &right, CompareMode::Size);

        assert_eq!(report.differing, 1);
        assert_eq!(report.unique_left, 0);
        assert_eq!(report.unique_right, 0);
        assert!(report.left_marks.contains("data.bin"));
        assert!(report.right_marks.contains("data.bin"));
    }

    #[test]
    fn apply_marks_selected_flags() {
        let mut left_panel = panel_with_entries(vec![entry("a.txt", 10), entry("b.txt", 20)]);
        let mut right_panel = panel_with_entries(vec![entry("a.txt", 10)]);

        let report = CompareReport {
            left_marks: vec!["b.txt".to_string()].into_iter().collect(),
            right_marks: HashSet::new(),
            unique_left: 1,
            unique_right: 0,
            differing: 0,
        };

        apply_compare_to_panels(&mut left_panel, &mut right_panel, &report);

        assert!(
            !left_panel
                .listing
                .filtered_get(0)
                .expect("entry 0")
                .selected
        );
        assert!(
            left_panel
                .listing
                .filtered_get(1)
                .expect("entry 1")
                .selected
        );
        assert!(
            !right_panel
                .listing
                .filtered_get(0)
                .expect("entry 0")
                .selected
        );
    }

    fn symlink_entry(name: &str, size: u64) -> FileEntry {
        // `cha.len` simulates target-derived data (what production produces
        // via `from_link_metadata` when the target is resolved). `follow`
        // stays false in tests, but the point is the same: size/mtime are not
        // link-lstat data and must not drive the match.
        TestEntry::new(name)
            .path(format!("/tmp/{name}"))
            .file(size)
            .symlink()
            .build()
    }

    #[test]
    fn symlinks_match_by_name_regardless_of_target_size() {
        // Two symlinks with the same name but different target sizes must not
        // be flagged as differing: a symlink is data about the link, and `cha`
        // holds target-derived stats, not link-lstat data.
        let left = vec![symlink_entry("link.txt", 10)];
        let right = vec![symlink_entry("link.txt", 9999)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 0);
        assert!(report.left_marks.is_empty());
        assert!(report.right_marks.is_empty());
    }

    #[test]
    fn symlink_vs_regular_file_type_mismatch() {
        // A symlink and a regular file of the same name differ even if their
        // sizes happen to match: symlinks are data, the type is the difference.
        let left = vec![symlink_entry("item", 100)];
        let right = vec![entry("item", 100)];

        let report = compare_entries(&left, &right, CompareMode::Thorough);

        assert_eq!(report.differing, 1);
        assert!(report.left_marks.contains("item"));
        assert!(report.right_marks.contains("item"));
    }
}
