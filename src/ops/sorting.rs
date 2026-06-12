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

const GROUP_UP: u8 = 0;
const GROUP_DIR: u8 = 1;
const GROUP_FILE: u8 = 2;

/// Pre-computed sort key for name comparisons.
/// Caches case-folded form and uppercase flag to avoid repeated UTF-8 scans.
#[derive(Clone, PartialEq, Eq)]
struct NameSortKey {
    primary: Box<str>,
    has_upper: bool,
    tiebreaker: Box<str>,
}

impl NameSortKey {
    fn new(name: &str, sensitive: bool) -> Self {
        if sensitive {
            NameSortKey {
                primary: name.into(),
                has_upper: false,
                tiebreaker: String::new().into_boxed_str(),
            }
        } else {
            let lower = name.to_lowercase();
            let has_upper = name.chars().any(|c| c.is_uppercase());
            NameSortKey {
                primary: lower.into_boxed_str(),
                has_upper,
                tiebreaker: name.into(),
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
            .then_with(|| self.tiebreaker.as_ref().cmp(other.tiebreaker.as_ref()))
    }
}

impl PartialOrd for NameSortKey {
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

fn sort_by_name(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    } else {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(NameSortKey::new(&e.name, sensitive)),
            )
        })
    }
}

fn sort_by_extension(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                NameSortKey::new(get_extension(&e.name), sensitive),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    } else {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(NameSortKey::new(get_extension(&e.name), sensitive)),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    }
}

fn sort_by_size(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.size(),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    } else {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(e.size()),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    }
}

fn sort_by_mod_time(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.mtime(),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    } else {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(e.mtime()),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    }
}

fn sort_by_btime(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.cha.btime.is_none() as u8,
                e.cha.btime,
                NameSortKey::new(&e.name, sensitive),
            )
        })
    } else {
        entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.cha.btime.is_none() as u8,
                e.cha.btime.map(Reverse),
                NameSortKey::new(&e.name, sensitive),
            )
        })
    }
}

fn sort_by_natural_name(entries: &mut [FileEntry], dir_first: bool, sensitive: bool, asc: bool) {
    if asc {
        entries.sort_by_cached_key(|e| natural_sort_key(e, dir_first, sensitive))
    } else {
        entries.sort_by_cached_key(|e| {
            let key = natural_sort_key(e, dir_first, sensitive);
            (key.group, Reverse((key.segments, key.tiebreaker)))
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
struct NaturalSortKey {
    group: u8,
    segments: Vec<natsort::NatKeySegment>,
    tiebreaker: NameSortKey,
}

impl Ord for NaturalSortKey {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.group
            .cmp(&other.group)
            .then_with(|| self.segments.cmp(&other.segments))
            .then_with(|| self.tiebreaker.cmp(&other.tiebreaker))
    }
}

impl PartialOrd for NaturalSortKey {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn natural_sort_key(entry: &FileEntry, dir_first: bool, sensitive: bool) -> NaturalSortKey {
    NaturalSortKey {
        group: entry_group(entry, dir_first),
        segments: natsort::natsort_key(entry.name.as_bytes(), !sensitive),
        tiebreaker: NameSortKey::new(&entry.name, sensitive),
    }
}

#[inline]
fn entry_group(entry: &FileEntry, dir_first: bool) -> u8 {
    match (entry.name.as_str(), dir_first, entry.is_dir()) {
        ("..", _, _) => GROUP_UP,
        (_, true, true) => GROUP_DIR,
        (_, true, false) => GROUP_FILE,
        (_, false, _) => GROUP_DIR,
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
