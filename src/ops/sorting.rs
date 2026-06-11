//! File sorting operations for Libre Commander (lc).
//!
//! This module provides comprehensive file sorting functionality with TDD-tested
//! implementations for various sorting modes.
//!
//! All comparators use `sort_by_cached_key` to pre-compute keys once per entry,
//! eliminating repeated UTF-8 scans and `rfind` calls in O(n log n) comparisons.
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::time::SystemTime;

pub use crate::app::types::FileEntry;
pub use crate::app::types::SortMode;
pub use crate::app::types::SortOptions;

use crate::ops::natsort;

const GROUP_UP: u8 = 0;
const GROUP_DIR: u8 = 1;
const GROUP_FILE: u8 = 2;

const TIEBREAKER_INLINE: usize = 64;

#[derive(Clone, PartialEq, Eq)]
enum Tiebreaker {
    Inline([u8; TIEBREAKER_INLINE], u8),
    Heap(Box<[u8]>),
}

impl Tiebreaker {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Tiebreaker::Inline(buf, len) => &buf[..*len as usize],
            Tiebreaker::Heap(bx) => bx,
        }
    }
}

impl Ord for Tiebreaker {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl PartialOrd for Tiebreaker {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn make_tiebreaker(bytes: &[u8]) -> Tiebreaker {
    if bytes.len() <= TIEBREAKER_INLINE {
        let mut buf = [0u8; TIEBREAKER_INLINE];
        buf[..bytes.len()].copy_from_slice(bytes);
        Tiebreaker::Inline(buf, bytes.len() as u8)
    } else {
        Tiebreaker::Heap(bytes.to_vec().into_boxed_slice())
    }
}

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

/// Wrapper for btime that sorts `Some` before `None` (ascending within `Some`).
#[derive(Clone, Copy, PartialEq, Eq)]
struct BtimeAsc(Option<SystemTime>);

impl Ord for BtimeAsc {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0, other.0) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for BtimeAsc {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Wrapper for btime that sorts `Some` before `None` (descending within `Some`).
#[derive(Clone, Copy, PartialEq, Eq)]
struct BtimeDesc(Option<SystemTime>);

impl Ord for BtimeDesc {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0, other.0) {
            (Some(a), Some(b)) => b.cmp(&a),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for BtimeDesc {
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
/// Raw byte values serve as deterministic tiebreaker for natural sort.
#[inline]
pub fn sort_entries(entries: &mut [FileEntry], mode: SortMode, options: SortOptions) {
    let dir_first = options.dir_first;
    let sensitive = options.sensitive;

    match mode {
        SortMode::NameAsc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::NameDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(NameSortKey::new(&e.name, sensitive)),
            )
        }),
        SortMode::ExtensionAsc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                NameSortKey::new(get_extension(&e.name), sensitive),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::ExtensionDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(NameSortKey::new(get_extension(&e.name), sensitive)),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::SizeAsc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.size(),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::SizeDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(e.size()),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::ModTimeAsc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                e.mtime(),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::ModTimeDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse(e.mtime()),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::BtimeAsc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                BtimeAsc(e.cha.btime),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::BtimeDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                BtimeDesc(e.cha.btime),
                NameSortKey::new(&e.name, sensitive),
            )
        }),
        SortMode::NaturalNameAsc => {
            entries.sort_by_cached_key(|e| natural_sort_key(e, dir_first, sensitive))
        }
        SortMode::NaturalNameDesc => entries.sort_by_cached_key(|e| {
            (
                entry_group(e, dir_first),
                Reverse((
                    natsort::natsort_key(e.name.as_bytes(), !sensitive),
                    make_tiebreaker(e.name.as_bytes()),
                )),
            )
        }),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct NaturalSortKey {
    group: u8,
    segments: Vec<natsort::NatKeySegment>,
    tiebreaker: Tiebreaker,
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
        tiebreaker: make_tiebreaker(entry.name.as_bytes()),
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

#[cfg(test)]
mod tests;
