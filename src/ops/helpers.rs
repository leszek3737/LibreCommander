use std::path::{Path, PathBuf};

use crate::app::types::PendingAction;
pub(crate) fn action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Copy { .. } => "Copy",
        PendingAction::Move { .. } => "Move",
        PendingAction::Delete { .. } => "Delete",
    }
}

pub(crate) fn path_contains_canonical(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}

fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let meta = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path()));
        } else {
            total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
    total
}

pub(crate) fn path_size(path: &Path) -> u64 {
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => 0,
        Ok(meta) if meta.is_dir() => dir_size(path),
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

pub(crate) fn path_sizes(paths: &[PathBuf]) -> Vec<u64> {
    paths.iter().map(|path| path_size(path)).collect()
}

pub(crate) fn sum_sizes(sizes: &[u64]) -> u64 {
    sizes
        .iter()
        .fold(0, |total, size| total.saturating_add(*size))
}

pub(crate) fn next_path(paths: &[PathBuf], idx: usize) -> Option<&Path> {
    paths.get(idx).map(PathBuf::as_path)
}
