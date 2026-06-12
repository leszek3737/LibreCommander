use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::app::types::PendingAction;
use crate::debug_log;

/// Return a human-readable label for the pending action type.
#[inline]
pub(crate) fn action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Copy(_) => "Copy",
        PendingAction::Move(_) => "Move",
        PendingAction::Delete { .. } => "Delete",
        PendingAction::ExtractArchive { .. } => "Extract",
        PendingAction::CreateArchive { .. } => "Archive",
    }
}

/// Returns `true` if `child` is a proper descendant of `parent`.
///
/// Note: an empty `parent` matches any non-equal child because
/// every path starts with the empty component sequence.
pub(crate) fn lexical_path_starts_with(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}

pub(crate) const MAX_RECURSION_DEPTH: usize = 256;

#[cfg(unix)]
/// Return a stable filesystem identity for cycle detection.
#[inline]
pub(crate) fn get_inode_key(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
/// Return a stable filesystem identity for cycle detection.
#[inline]
pub(crate) fn get_inode_key(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::windows::fs::MetadataExt;
    let vol = metadata.volume_serial_number()? as u64;
    let idx = metadata.file_index()?;
    Some((vol, idx))
}

#[cfg(not(any(unix, windows)))]
/// Platforms without inode-like identifiers.
#[inline]
pub(crate) fn get_inode_key(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

fn seed_visited_dir(path: &Path, visited: &mut HashSet<(u64, u64)>) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            debug_log!(
                "seed_visited_dir: symlink_metadata failed for {}: {e}",
                path.display()
            );
            return;
        }
    };
    if meta.is_dir()
        && let Some(key) = get_inode_key(&meta)
    {
        visited.insert(key);
    }
}

fn dir_size_rec(path: &Path, depth: usize, visited: &mut HashSet<(u64, u64)>) -> io::Result<u64> {
    if depth >= MAX_RECURSION_DEPTH {
        debug_log!(
            "dir_size: depth limit ({MAX_RECURSION_DEPTH}) reached at {}",
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
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                debug_log!("dir_size: entry read failed in {}: {e}", path.display());
                continue;
            }
        };
        let entry_path = entry.path();
        let meta = match entry_path.symlink_metadata() {
            Ok(m) => m,
            Err(e) => {
                debug_log!(
                    "dir_size: symlink_metadata failed for {}: {e}",
                    entry_path.display()
                );
                continue;
            }
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            // insert() returns false when the key was already present,
            // meaning we've seen this inode before → cycle detected.
            if let Some(key) = get_inode_key(&meta)
                && !visited.insert(key)
            {
                debug_log!(
                    "dir_size: cycle detected, skipping {}",
                    entry_path.display()
                );
                continue;
            }
            let child = dir_size_rec(&entry_path, depth + 1, visited).unwrap_or_else(|e| {
                debug_log!("dir_size: subdir failed {}: {e}", entry_path.display());
                0
            });
            total = total.saturating_add(child);
        } else {
            total = total.saturating_add(meta.len());
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
pub(crate) fn dir_size(path: &Path) -> io::Result<u64> {
    let mut visited = HashSet::new();
    seed_visited_dir(path, &mut visited);
    dir_size_rec(path, 0, &mut visited)
}

/// Compute the size of a single path (file or directory).
///
/// Symlinks and empty files both report size 0 and are indistinguishable
/// from the return value alone.
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
#[cfg(feature = "parallel")]
pub(crate) fn path_sizes(paths: &[PathBuf]) -> Vec<u64> {
    use rayon::prelude::*;

    paths
        .par_iter()
        .map(|p| {
            path_size(p).unwrap_or_else(|e| {
                debug_log!("path_sizes: using 0 for {}: {e}", p.display());
                0
            })
        })
        .collect()
}

/// Best-effort size computation for multiple paths.
///
/// Individual failures are logged and reported as 0 so that batch progress
/// can still proceed.
#[cfg(not(feature = "parallel"))]
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
    sizes.iter().copied().fold(0, u64::saturating_add)
}

#[derive(Clone, Copy)]
enum CleanupOp {
    File,
    Dir,
    DirAll,
}

fn cleanup_path(path: &Path, op: CleanupOp) {
    let result = match op {
        CleanupOp::File => fs::remove_file(path),
        CleanupOp::Dir => fs::remove_dir(path),
        CleanupOp::DirAll => fs::remove_dir_all(path),
    };
    if let Err(e) = result {
        let label = match op {
            CleanupOp::File => "file",
            CleanupOp::Dir => "directory",
            CleanupOp::DirAll => "directory tree",
        };
        debug_log!("failed to clean up {label} {}: {e}", path.display());
    }
}

pub(crate) fn cleanup_file(path: &Path) {
    cleanup_path(path, CleanupOp::File);
}

pub(crate) fn cleanup_dir(path: &Path) {
    cleanup_path(path, CleanupOp::Dir);
}

pub(crate) fn cleanup_dir_all(path: &Path) {
    cleanup_path(path, CleanupOp::DirAll);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_dir_size() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("size_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("small.txt"), b"abc").unwrap();
        fs::write(dir.join("medium.txt"), b"abcdefghij").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("nested.txt"), b"12345").unwrap();

        let size = dir_size(&dir).unwrap();
        assert_eq!(size, 18);
    }

    #[cfg(unix)]
    #[test]
    fn test_dir_size_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("size_dir");
        let linked = tmp.path().join("linked_dir");
        fs::create_dir(&dir).unwrap();
        fs::create_dir(&linked).unwrap();
        fs::write(dir.join("local.txt"), b"abc").unwrap();
        fs::write(linked.join("outside.txt"), b"outside").unwrap();
        symlink(&linked, dir.join("symlink_dir")).unwrap();

        let size = dir_size(&dir).unwrap();
        assert_eq!(size, 3);
    }

    #[cfg(unix)]
    #[test]
    fn test_dir_size_seeds_root_inode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("size_dir");
        fs::create_dir(&dir).unwrap();

        let meta = fs::metadata(&dir).unwrap();
        let key = get_inode_key(&meta).unwrap();
        let mut visited = HashSet::with_capacity(256);

        seed_visited_dir(&dir, &mut visited);
        assert!(visited.contains(&key));
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let result = dir_size(Path::new("/tmp/lc_nonexistent_dir_xyz_12345"));
        assert!(result.is_err());
    }

    #[test]
    fn test_lexical_path_starts_with() {
        let parent = Path::new("/foo/bar");
        let child = Path::new("/foo/bar/baz");
        assert!(lexical_path_starts_with(parent, child));
        assert!(!lexical_path_starts_with(parent, parent));
        assert!(lexical_path_starts_with(Path::new(""), child));
    }

    #[test]
    fn test_action_label() {
        let copy = PendingAction::Copy(crate::app::types::TransferAction {
            sources: vec![],
            dest: PathBuf::new(),
            overwrite: false,
        });
        assert_eq!(action_label(&copy), "Copy");

        let mv = PendingAction::Move(crate::app::types::TransferAction {
            sources: vec![],
            dest: PathBuf::new(),
            overwrite: false,
        });
        assert_eq!(action_label(&mv), "Move");

        let del = PendingAction::Delete { paths: vec![] };
        assert_eq!(action_label(&del), "Delete");

        let extract = PendingAction::ExtractArchive {
            source: PathBuf::new(),
            dest: PathBuf::new(),
            overwrite: false,
        };
        assert_eq!(action_label(&extract), "Extract");

        let create = PendingAction::CreateArchive {
            sources: vec![],
            dest: PathBuf::new(),
            format: crate::ops::archive::ArchiveFormat::Zip,
            overwrite: false,
        };
        assert_eq!(action_label(&create), "Archive");
    }

    #[test]
    fn test_path_size_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("data.txt");
        fs::write(&file, b"hello").unwrap();
        assert_eq!(path_size(&file).unwrap(), 5);
    }

    #[test]
    fn test_path_size_dir_calls_dir_size() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sizedir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("f.txt"), b"xyz").unwrap();
        assert_eq!(path_size(&dir).unwrap(), 3);
    }

    #[test]
    fn test_sum_sizes() {
        assert_eq!(sum_sizes(&[]), 0);
        assert_eq!(sum_sizes(&[1, 2, 3]), 6);
        assert_eq!(sum_sizes(&[u64::MAX, 1]), u64::MAX);
    }
}
