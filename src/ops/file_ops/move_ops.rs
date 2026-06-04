use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::common::{
    check_canceled, ensure_destination_absent, path_contains, remove_any, same_inode,
};
use super::copy::{copy_dir_recursive_with_progress, copy_file_with_progress, copy_symlink};
use super::delete::delete_dir_recursive_cancelable;

/// Move or rename a filesystem entry.
///
/// Case-only rename semantics:
/// - On case-insensitive filesystems, device+inode comparison detects when
///   `src` and `dest` refer to the same physical entry (including hard links),
///   and the rename is performed via `fs::rename`, which handles the case
///   change atomically.
/// - On case-sensitive filesystems, `dest` metadata lookup fails (target does
///   not exist), so the function proceeds as a normal move.
#[cfg(test)]
pub fn move_entry(src: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    let cancel = AtomicBool::new(false);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<u64>();
    drop(progress_rx);
    move_entry_impl(src, dest, &progress_tx, &cancel, overwrite)
}

pub fn move_entry_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<()> {
    move_entry_impl(src, dest, progress_tx, cancel, overwrite)
}

fn move_entry_impl(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<()> {
    check_canceled(cancel)?;
    let src_meta = match fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("cannot move: source does not exist: {}", src.display()),
            ));
        }
    };
    let same_file = match fs::symlink_metadata(dest) {
        Ok(dest_meta) => same_inode(&src_meta, &dest_meta),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if same_file {
        if src_meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot move symlink onto its own target",
            ));
        }
        return fs::rename(src, dest);
    }
    if src.is_dir() && path_contains(src, dest)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot move directory into its descendant",
        ));
    }
    if !overwrite {
        ensure_destination_absent(dest)?;
    }

    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            check_canceled(cancel)?;
            let file_type = src_meta.file_type();
            if file_type.is_symlink() {
                copy_then_remove_src(
                    src,
                    dest,
                    cancel,
                    || copy_symlink(src, dest, overwrite),
                    || fs::remove_file(src),
                    || remove_any(dest),
                    "symlink",
                )
            } else if file_type.is_dir() {
                copy_then_remove_src(
                    src,
                    dest,
                    cancel,
                    || {
                        copy_dir_recursive_with_progress(src, dest, progress_tx, cancel, overwrite)
                            .map(|_| ())
                    },
                    || delete_dir_recursive_cancelable(src, cancel),
                    || remove_any(dest),
                    "directory",
                )
            } else {
                copy_then_remove_src(
                    src,
                    dest,
                    cancel,
                    || {
                        copy_file_with_progress(src, dest, progress_tx, cancel, overwrite)
                            .map(|_| ())
                    },
                    || fs::remove_file(src),
                    || remove_any(dest),
                    "file",
                )
            }
        }
        Err(e) => Err(e),
    }
}

fn copy_then_remove_src(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    copy_fn: impl FnOnce() -> io::Result<()>,
    remove_src_fn: impl FnOnce() -> io::Result<()>,
    rollback_fn: impl FnOnce() -> io::Result<()>,
    entry_kind: &str,
) -> io::Result<()> {
    copy_fn()?;
    check_canceled(cancel)?;
    if let Err(del_err) = remove_src_fn() {
        if let Err(rollback_err) = rollback_fn() {
            return Err(io::Error::other(format!(
                "cross-device move: copied '{}' to '{}' but failed to remove source {}: {}. rollback also failed: {}",
                src.display(),
                dest.display(),
                entry_kind,
                del_err,
                rollback_err
            )));
        }
        return Err(io::Error::other(format!(
            "cross-device move: copied '{}' to '{}' but failed to remove source {}: {}",
            src.display(),
            dest.display(),
            entry_kind,
            del_err
        )));
    }
    Ok(())
}
