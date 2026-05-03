use std::path::{Path, PathBuf};

use crate::app::types::PendingAction;
use crate::ops::file_ops;

pub fn action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Copy { .. } => "Copy",
        PendingAction::Move { .. } => "Move",
        PendingAction::Delete { .. } => "Delete",
    }
}

pub(crate) fn path_contains_canonical(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}

pub(crate) fn path_size(path: &Path) -> u64 {
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => 0,
        Ok(meta) if meta.is_dir() => file_ops::calculate_dir_size(path).unwrap_or(0),
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
