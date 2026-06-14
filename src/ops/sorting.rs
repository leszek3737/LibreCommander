//! File sorting operations for Libre Commander (lc).
//!
//! This module provides comprehensive file sorting functionality with TDD-tested
//! implementations for various sorting modes.
//!
//! All comparators use `sort_by_cached_key` to pre-compute keys once per entry,
//! eliminating repeated UTF-8 scans and `rfind` calls in O(n log n) comparisons.
use std::cmp::Ordering;
use std::cmp::Reverse;

pub use crate::app::types::Direction;
pub use crate::app::types::FileEntry;
pub use crate::app::types::SortField;
pub use crate::app::types::SortMode;
pub use crate::app::types::SortOptions;

use crate::ops::natsort;

// Listing groups, ordered by the `u8` value (ascending) so they always sort in
// this order regardless of the chosen direction:
/// The `..` parent entry — always sorts first.
const GROUP_UP: u8 = 0;
/// Directories when `dir_first` is set; also every non-parent entry when it is
/// not (in which case dirs and files share this group and interleave by key).
const GROUP_DIR: u8 = 1;
/// Files when `dir_first` is set (never used otherwise).
const GROUP_FILE: u8 = 2;

/// Pre-computed sort key for name comparisons.
/// Caches case-folded form and uppercase flag to avoid repeated UTF-8 scans.
#[derive(Clone, PartialEq, Eq)]
struct NameSortKey {
    primary: Box<str>,
    has_upper: bool,
    /// Original-case name, used only to order case-insensitive ties (e.g.
    /// `apple` vs `Apple`). `None` in case-sensitive mode, where `primary`
    /// already carries the original name and no tiebreak is needed.
    tiebreaker: Option<Box<str>>,
}

impl NameSortKey {
    fn new(name: &str, sensitive: bool) -> Self {
        if sensitive {
            NameSortKey {
                primary: name.into(),
                has_upper: false,
                tiebreaker: None,
            }
        } else {
            let lower = name.to_lowercase();
            let has_upper = name.chars().any(|c| c.is_uppercase());
            NameSortKey {
                primary: lower.into_boxed_str(),
                has_upper,
                tiebreaker: Some(name.into()),
            }
        }
    }
}

impl Ord for NameSortKey {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.primary
            .as_ref()
            .cmp(other.primary.as_ref())
            .then_with(|| self.has_upper.cmp(&other.has_upper))
            .then_with(|| self.tiebreaker.cmp(&other.tiebreaker))
    }
}

impl PartialOrd for NameSortKey {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A sort key whose ordering flips with the chosen direction.
///
/// Wrapping only the direction-sensitive component (e.g. size, mtime, the name
/// itself) lets the surrounding key parts — the listing group and the name
/// tiebreaker — stay ascending in both directions. This replaces the per-field
/// `if asc { … } else { … Reverse … }` duplication.
#[derive(Clone, PartialEq, Eq)]
struct Directional<T> {
    value: T,
    ascending: bool,
}

#[inline]
fn directional<T>(value: T, ascending: bool) -> Directional<T> {
    Directional { value, ascending }
}

impl<T: Ord> Ord for Directional<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        let ord = self.value.cmp(&other.value);
        if self.ascending { ord } else { ord.reverse() }
    }
}

impl<T: Ord> PartialOrd for Directional<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

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
        Some(index) if index > 0 && index < name.len() - 1 => &name[index..],
        _ => "",
    }
}

/// Sort directory entries by the given mode.
///
/// All modes use `sort_by_cached_key` to pre-compute comparison keys once per
/// entry, eliminating repeated UTF-8 scans and `rfind` calls.
///
/// Natural sort (`NatAsc`/`NatDesc`) uses ASCII-only case folding
/// (`make_ascii_lowercase`). Name and Extension sorts use full Unicode
/// `str::to_lowercase()`. Results may disagree on non-ASCII filenames.
/// Name sort keys serve as deterministic tiebreakers for natural sort.
#[inline]
pub fn sort_entries(entries: &mut [FileEntry], mode: SortMode, options: SortOptions) {
    let dir_first = options.dir_first;
    let sensitive = options.sensitive;
    let asc = mode.direction.is_ascending();

    match mode.field {
        SortField::Name => sort_by_name(entries, dir_first, sensitive, asc),
        SortField::Extension => sort_by_extension(entries, dir_first, sensitive, asc),
        SortField::Size => sort_by_size(entries, dir_first, sensitive, asc),
        SortField::ModTime => sort_by_mod_time(entries, dir_first, sensitive, asc),
        SortField::Btime => sort_by_btime(entries, dir_first, sensitive, asc),
        SortField::NaturalName => sort_by_natural_name(entries, dir_first, sensitive, asc),
    }
}

/// Sort `entries` by a cached key built from `key_fn`, which receives `asc` so
/// it can wrap the direction-sensitive part in [`directional`]. Centralizes the
/// `sort_by_cached_key` call shared by every field.
#[inline]
fn sort_with_direction<K, F>(entries: &mut [FileEntry], asc: bool, key_fn: F)
where
    K: Ord,
    F: Fn(&FileEntry, bool) -> K,
{
    entries.sort_by_cached_key(|e| key_fn(e, asc));
}

fn sort_by_name(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            directional(NameSortKey::new(&e.name, sensitive), asc),
        )
    });
}

fn sort_by_extension(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            directional(NameSortKey::new(get_extension(&e.name), sensitive), asc),
            NameSortKey::new(&e.name, sensitive),
        )
    });
}

fn sort_by_size(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            directional(e.size(), asc),
            NameSortKey::new(&e.name, sensitive),
        )
    });
}

fn sort_by_mod_time(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            directional(e.mtime(), asc),
            NameSortKey::new(&e.name, sensitive),
        )
    });
}

fn sort_by_btime(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            // Present btimes first, missing ones last — in both directions.
            // `Reverse(is_some())` makes `Some` (true) rank ahead of `None`.
            Reverse(e.cha.btime.is_some()),
            directional(e.cha.btime, asc),
            NameSortKey::new(&e.name, sensitive),
        )
    });
}

fn sort_by_natural_name(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    sort_with_direction(entries, asc, |e, asc| {
        (
            entry_group(e, dir_first),
            directional(
                (
                    natsort::natsort_key(e.name.as_bytes(), !sensitive),
                    NameSortKey::new(&e.name, sensitive),
                ),
                asc,
            ),
        )
    });
}

#[inline]
fn entry_group(entry: &FileEntry, dir_first: bool) -> u8 {
    if entry.name == ".." {
        GROUP_UP
    } else if dir_first && !entry.is_dir() {
        GROUP_FILE
    } else {
        // `dir_first` directories, and *every* non-parent entry when
        // `!dir_first` (dirs and files are not separated then).
        GROUP_DIR
    }
}

const FIELD_CYCLE_ORDER: [SortField; 6] = [
    SortField::Name,
    SortField::NaturalName,
    SortField::Size,
    SortField::ModTime,
    SortField::Btime,
    SortField::Extension,
];

fn next_field_in_cycle(current: SortField) -> SortField {
    let idx = FIELD_CYCLE_ORDER
        .iter()
        .position(|&f| f == current)
        .unwrap_or(0);
    FIELD_CYCLE_ORDER[(idx + 1) % FIELD_CYCLE_ORDER.len()]
}

pub fn cycle_sort_mode(current: SortMode) -> SortMode {
    match current.direction {
        Direction::Asc => SortMode {
            field: current.field,
            direction: Direction::Desc,
        },
        Direction::Desc => SortMode {
            field: next_field_in_cycle(current.field),
            direction: Direction::Asc,
        },
    }
}

#[cfg(test)]
mod tests;
