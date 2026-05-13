use std::io;
use std::path::{Path, PathBuf};

use crate::app::types::PendingAction;
use crate::debug_log;

pub(crate) fn action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Copy { .. } => "Copy",
        PendingAction::Move { .. } => "Move",
        PendingAction::Delete { .. } => "Delete",
    }
}

pub(crate) fn path_starts_with(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}

const MAX_DIR_DEPTH: u32 = 256;

fn dir_size_rec(path: &Path, depth: u32) -> io::Result<u64> {
    if depth >= MAX_DIR_DEPTH {
        debug_log!(
            "dir_size: depth limit ({MAX_DIR_DEPTH}) reached at {}",
            path.display()
        );
        return Ok(0);
    }
    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            debug_log!("dir_size: read_dir failed for {}: {e}", path.display());
            return Err(e);
        }
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
            let child = dir_size_rec(&entry.path(), depth + 1).unwrap_or_else(|e| {
                debug_log!("dir_size: subdir failed {}: {e}", entry.path().display());
                0
            });
            total = total.saturating_add(child);
        } else {
            total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
    Ok(total)
}

/// Recursively compute the total size of a directory tree.
///
/// Returns `Err` if the top-level `read_dir` fails. Subdirectory failures are
/// logged and treated as size 0 so that a single unreadable child does not
/// abort the entire scan.
///
/// Symlinks are intentionally skipped to avoid cycles.
fn dir_size(path: &Path) -> io::Result<u64> {
    dir_size_rec(path, 0)
}

/// Compute the size of a single path (file or directory).
///
/// Symlinks report size 0 to avoid cycles.
pub(crate) fn path_size(path: &Path) -> io::Result<u64> {
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => Ok(0),
        Ok(meta) if meta.is_dir() => dir_size(path),
        Ok(meta) => Ok(meta.len()),
        Err(e) => {
            debug_log!("path_size: metadata failed for {}: {e}", path.display());
            Err(e)
        }
    }
}

/// Best-effort size computation for multiple paths.
///
/// Individual failures are logged and reported as 0 so that batch progress
/// can still proceed.
pub(crate) fn path_sizes(paths: &[PathBuf]) -> Vec<u64> {
    paths
        .iter()
        .map(|p| {
            path_size(p).unwrap_or_else(|e| {
                debug_log!("path_sizes: using 0 for {}: {e}", p.display());
                0
            })
        })
        .collect()
}

pub(crate) fn sum_sizes(sizes: &[u64]) -> u64 {
    sizes
        .iter()
        .fold(0, |total, size| total.saturating_add(*size))
}

pub(crate) fn next_path(paths: &[PathBuf], idx: usize) -> Option<&Path> {
    paths.get(idx).map(PathBuf::as_path)
}
