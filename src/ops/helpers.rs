use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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
/// An empty `parent` matches every path via `starts_with`, which is never a
/// meaningful descendant check — return `false` to avoid false positives.
pub(crate) fn lexical_path_starts_with(parent: &Path, child: &Path) -> bool {
    !parent.as_os_str().is_empty() && child != parent && child.starts_with(parent)
}

pub(crate) const MAX_RECURSION_DEPTH: usize = 256;

#[cfg(unix)]
/// Return a stable filesystem identity for cycle detection.
#[inline]
pub(crate) fn get_inode_key(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
/// Platforms without stable inode-like identifiers (Windows'
/// `file_index()`/`volume_serial_number()` need the unstable `windows_by_handle`
/// feature, rust-lang/rust#63010): cycle detection is skipped entirely.
/// [`MAX_RECURSION_DEPTH`] is the sole backstop against runaway recursion via
/// junction points or deep symlink chains on these platforms.
#[inline]
pub(crate) fn get_inode_key(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

/// Seed the cycle-detection set with the scan root's inode before recursing.
///
/// `dir_size_rec` / search walkers only record inodes of subdirectories they
/// descend into, so a symlink inside the tree pointing back at the root would
/// otherwise go undetected. Inserting the root key up front closes that
/// one-level gap.
///
/// When `path` itself is a symlink, we seed the *target*'s inode (via
/// `metadata`, which follows the final component) so a later re-entry to the
/// real directory is recognised as a cycle. Falls back to `symlink_metadata`
/// if the target is unreachable (broken link).
pub(crate) fn seed_visited_dir(path: &Path, visited: &mut HashSet<(u64, u64)>) {
    // Prefer the resolved target so a root-symlink seeds the real directory.
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            // Broken symlink / unreadable target: fall back to the link itself
            // so we still seed *something* and keep the rest of the walk going.
            match std::fs::symlink_metadata(path) {
                Ok(m) => m,
                Err(e2) => {
                    debug_log!(
                        "seed_visited_dir: metadata failed for {} ({e}); symlink_metadata also failed: {e2}",
                        path.display()
                    );
                    return;
                }
            }
        }
    };
    if meta.is_dir()
        && let Some(key) = get_inode_key(&meta)
    {
        visited.insert(key);
    }
}

fn dir_size_rec(
    path: &Path,
    depth: usize,
    visited: &mut HashSet<(u64, u64)>,
    cancel: Option<&AtomicBool>,
) -> io::Result<u64> {
    if depth >= MAX_RECURSION_DEPTH {
        debug_log!(
            "dir_size: depth limit ({MAX_RECURSION_DEPTH}) reached at {} — \
             size is incomplete for this subtree",
            path.display()
        );
        return Ok(0);
    }
    // Check cancellation before the potentially expensive `read_dir` so a
    // cancelled scan doesn't do more I/O before noticing (AGENTS.md: ops must
    // be cancellable).
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        debug_log!("dir_size: cancelled at entry of {}", path.display());
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
        // Check cancellation before each entry — the scan may traverse huge
        // trees and MUST be abortable (AGENTS.md: ops must be cancellable).
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            debug_log!("dir_size: cancelled at {}", path.display());
            return Ok(total);
        }
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
            let child = dir_size_rec(&entry_path, depth + 1, visited, cancel).unwrap_or_else(|e| {
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
/// Symlinks are intentionally skipped to avoid cycles. The scan checks the
/// `cancel` flag between entries and returns the partial total accumulated so
/// far when cancelled.
///
/// **Blocking:** walks the directory tree synchronously on the caller's thread.
/// Must be invoked from `job_runner`, not the event loop.
pub(crate) fn dir_size(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<u64> {
    let mut visited = HashSet::new();
    seed_visited_dir(path, &mut visited);
    dir_size_rec(path, 0, &mut visited, cancel)
}

/// Compute the size of a single path (file or directory).
///
/// Symlinks and empty files both report size 0 and are indistinguishable
/// from the return value alone.
pub(crate) fn path_size(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<u64> {
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => Ok(0),
        Ok(meta) if meta.is_dir() => dir_size(path, cancel),
        Ok(meta) => Ok(meta.len()),
        Err(e) => {
            debug_log!("path_size: metadata failed for {}: {e}", path.display());
            Err(e)
        }
    }
}

/// Size of a single path, logging and returning 0 on failure so a batch can
/// keep making progress past one unreadable entry.
#[inline]
fn path_size_or_zero(path: &Path, cancel: Option<&AtomicBool>) -> u64 {
    path_size(path, cancel).unwrap_or_else(|e| {
        debug_log!("path_sizes: using 0 for {}: {e}", path.display());
        0
    })
}

/// Best-effort size computation for multiple paths.
///
/// Individual failures are logged and reported as 0 so that batch progress
/// can still proceed.
///
/// **Blocking pre-scan:** walks every path's subtree synchronously on the
/// caller's thread before returning. Used up-front by batch operations to
/// size the total byte budget for progress reporting; large trees stall the
/// caller until the walk completes. The `cancel` flag is checked between
/// top-level paths and within each subtree walk so the pre-scan can be
/// aborted mid-scan (AGENTS.md: ops must be cancellable).
pub(crate) fn path_sizes(paths: &[PathBuf], cancel: Option<&AtomicBool>) -> Vec<u64> {
    paths
        .iter()
        .map(|p| {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                debug_log!("path_sizes: cancelled before {}", p.display());
                return 0;
            }
            path_size_or_zero(p, cancel)
        })
        .collect()
}

pub(crate) fn cleanup_file(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        debug_log!("failed to clean up file {}: {e}", path.display());
    }
}

pub(crate) fn cleanup_dir(path: &Path) {
    if let Err(e) = fs::remove_dir(path) {
        debug_log!("failed to clean up directory {}: {e}", path.display());
    }
}

pub(crate) fn cleanup_dir_all(path: &Path) {
    if let Err(e) = fs::remove_dir_all(path) {
        debug_log!("failed to clean up directory tree {}: {e}", path.display());
    }
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

        let size = dir_size(&dir, None).unwrap();
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

        let size = dir_size(&dir, None).unwrap();
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
        let result = dir_size(Path::new("/tmp/lc_nonexistent_dir_xyz_12345"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_lexical_path_starts_with() {
        let parent = Path::new("/foo/bar");
        let child = Path::new("/foo/bar/baz");
        assert!(lexical_path_starts_with(parent, child));
        assert!(!lexical_path_starts_with(parent, parent));
        assert!(!lexical_path_starts_with(Path::new(""), child));
    }

    #[test]
    fn test_dir_size_cancellable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("cancel_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"aaa").unwrap();
        fs::write(dir.join("b.txt"), b"bbb").unwrap();

        let cancel = AtomicBool::new(true);
        let size = dir_size(&dir, Some(&cancel)).unwrap();
        // Pre-set cancel flag → returns 0 immediately after first check.
        assert_eq!(size, 0);

        let cancel = AtomicBool::new(false);
        let size = dir_size(&dir, Some(&cancel)).unwrap();
        assert_eq!(size, 6);
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
            entry_tops: vec![],
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
        assert_eq!(path_size(&file, None).unwrap(), 5);
    }

    #[test]
    fn test_path_size_dir_calls_dir_size() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sizedir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("f.txt"), b"xyz").unwrap();
        assert_eq!(path_size(&dir, None).unwrap(), 3);
    }

    #[test]
    fn test_path_sizes_cancels_between_top_level_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dir1 = tmp.path().join("d1");
        let dir2 = tmp.path().join("d2");
        fs::create_dir(&dir1).unwrap();
        fs::write(dir1.join("a.txt"), b"aaaa").unwrap();
        fs::create_dir(&dir2).unwrap();
        fs::write(dir2.join("b.txt"), b"bb").unwrap();

        // Pre-set cancel: the first path returns 0 via the entry check in
        // `dir_size_rec`, and the second is short-circuited at the top-level
        // guard in `path_sizes` before any I/O.
        let cancel = AtomicBool::new(true);
        let sizes = path_sizes(&[dir1, dir2], Some(&cancel));
        assert_eq!(sizes, vec![0, 0]);
    }
}
