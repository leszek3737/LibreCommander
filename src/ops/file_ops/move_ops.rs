use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::common::{check_canceled, ensure_destination_absent, path_contains};
use super::copy::{copy_dir_recursive_with_progress, copy_file_with_progress, copy_symlink};
use super::delete::delete_dir_recursive_cancelable;

/// Move or rename a filesystem entry.
///
/// Case-only rename semantics:
/// - On case-insensitive filesystems, `canonicalize()` resolves both `src` and
///   `dest` to the same inode, so the `same_file` branch fires and the rename
///   is performed via `fs::rename`, which handles the case change atomically.
/// - On case-sensitive filesystems, `dest.canonicalize()` fails (target does
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
    if src == dest {
        return Ok(());
    }
    let same_file = match (src.canonicalize().ok(), dest.canonicalize().ok()) {
        (Some(s), Some(d)) => s == d,
        _ => false,
    };
    if same_file {
        return {
            let src_is_link = src
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink());
            let dest_is_link = dest
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink());
            if src_is_link && !dest_is_link {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot move symlink onto its own target",
                ));
            }
            fs::rename(src, dest)
        };
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
            let meta = src.symlink_metadata()?;
            if meta.file_type().is_symlink() {
                copy_symlink(src, dest, overwrite)?;
                check_canceled(cancel)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "cross-device move: copied '{}' to '{}' but failed to remove source: {}",
                        src.display(),
                        dest.display(),
                        del_err
                    )));
                }
            } else if meta.is_dir() {
                copy_dir_recursive_with_progress(src, dest, progress_tx, cancel, overwrite)?;
                check_canceled(cancel)?;
                if !path_contains(src, dest)?
                    && let Err(del_err) = delete_dir_recursive_cancelable(src, cancel)
                {
                    return Err(io::Error::other(format!(
                        "cross-device move: copied '{}' to '{}' but failed to remove source directory: {}",
                        src.display(),
                        dest.display(),
                        del_err
                    )));
                }
            } else {
                copy_file_with_progress(src, dest, progress_tx, cancel, overwrite)?;
                check_canceled(cancel)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "cross-device move: copied '{}' to '{}' but failed to remove source: {}",
                        src.display(),
                        dest.display(),
                        del_err
                    )));
                }
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}
