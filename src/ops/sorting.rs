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

type NaturalSortKey = (u8, Vec<natsort::NatKeySegment>, Vec<u8>);
type ReverseNaturalSortKey = (u8, Reverse<Vec<natsort::NatKeySegment>>, Reverse<Vec<u8>>);

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

const GROUP_UP: u8 = 0;
const GROUP_DIR: u8 = 1;
const GROUP_FILE: u8 = 2;

/// Sort directory entries by the given mode.
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
        SortMode::NameAsc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first).then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::NameDesc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first).then_with(|| cmp_name(&a.name, &b.name, sensitive).reverse())
        }),
        SortMode::ExtensionAsc => entries
            .sort_by(|a, b| cmp_group(a, b, dir_first).then_with(|| cmp_ext(a, b, sensitive))),
        SortMode::ExtensionDesc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first).then_with(|| cmp_ext(a, b, sensitive).reverse())
        }),
        SortMode::SizeAsc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.size().cmp(&b.size()))
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::SizeDesc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.size().cmp(&b.size()).reverse())
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::ModTimeAsc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.mtime().cmp(&b.mtime()))
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::ModTimeDesc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.mtime().cmp(&b.mtime()).reverse())
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::BtimeAsc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.cha.btime.is_some().cmp(&b.cha.btime.is_some()).reverse())
                .then_with(|| a.btime().cmp(&b.btime()))
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::BtimeDesc => entries.sort_by(|a, b| {
            cmp_group(a, b, dir_first)
                .then_with(|| a.cha.btime.is_some().cmp(&b.cha.btime.is_some()).reverse())
                .then_with(|| a.btime().cmp(&b.btime()).reverse())
                .then_with(|| cmp_name(&a.name, &b.name, sensitive))
        }),
        SortMode::NaturalNameAsc => {
            entries.sort_by_cached_key(|entry| natural_sort_key(entry, dir_first, sensitive))
        }
        SortMode::NaturalNameDesc => entries
            .sort_by_cached_key(|entry| reverse_natural_sort_key(entry, dir_first, sensitive)),
    }
}

fn natural_sort_key(entry: &FileEntry, dir_first: bool, sensitive: bool) -> NaturalSortKey {
    (
        entry_group(entry, dir_first),
        natsort::natsort_key(entry.name.as_bytes(), !sensitive),
        entry.name.as_bytes().to_vec(),
    )
}

fn reverse_natural_sort_key(
    entry: &FileEntry,
    dir_first: bool,
    sensitive: bool,
) -> ReverseNaturalSortKey {
    let (_, key, bytes) = natural_sort_key(entry, dir_first, sensitive);
    (entry_group(entry, dir_first), Reverse(key), Reverse(bytes))
}

fn entry_group(entry: &FileEntry, dir_first: bool) -> u8 {
    match (entry.name.as_str(), dir_first, entry.is_dir()) {
        ("..", _, _) => GROUP_UP,
        (_, true, true) => GROUP_DIR,
        (_, true, false) => GROUP_FILE,
        (_, false, _) => GROUP_DIR,
    }
}

#[inline]
fn cmp_group(a: &FileEntry, b: &FileEntry, dir_first: bool) -> Ordering {
    entry_group(a, dir_first).cmp(&entry_group(b, dir_first))
}

fn cmp_name(a: &str, b: &str, sensitive: bool) -> Ordering {
    if sensitive {
        return a.cmp(b);
    }
    cmp_ignore_case(a, b).then_with(|| {
        let a_lower = a.chars().all(|c| !c.is_uppercase());
        let b_lower = b.chars().all(|c| !c.is_uppercase());
        match (a_lower, b_lower) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.cmp(b),
        }
    })
}

fn cmp_ext(a: &FileEntry, b: &FileEntry, sensitive: bool) -> Ordering {
    let ext_a = get_extension(&a.name);
    let ext_b = get_extension(&b.name);
    cmp_name(ext_a, ext_b, sensitive).then_with(|| cmp_name(&a.name, &b.name, sensitive))
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
