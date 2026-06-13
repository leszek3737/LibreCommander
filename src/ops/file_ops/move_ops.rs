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

#[derive(Clone, Copy)]
enum MoveKind {
    Symlink,
    Directory,
    File,
}

impl MoveKind {
    fn from_file_type(ft: &std::fs::FileType) -> Self {
        if ft.is_symlink() {
            MoveKind::Symlink
        } else if ft.is_dir() {
            MoveKind::Directory
        } else {
            MoveKind::File
        }
    }

    fn label(self) -> &'static str {
        match self {
            MoveKind::Symlink => "symlink",
            MoveKind::Directory => "directory",
            MoveKind::File => "file",
        }
    }

    fn copy(
        self,
        src: &Path,
        dest: &Path,
        progress_tx: &Sender<u64>,
        cancel: &AtomicBool,
        overwrite: bool,
    ) -> io::Result<()> {
        match self {
            MoveKind::Symlink => {
                check_canceled(cancel)?;
                copy_symlink(src, dest, overwrite)
            }
            MoveKind::Directory => {
                // Byte count discarded — move operations track progress via
                // the `progress_tx` channel only, not via the return value.
                copy_dir_recursive_with_progress(src, dest, progress_tx, cancel, overwrite)
                    .map(|_bytes| ())
            }
            MoveKind::File => {
                copy_file_with_progress(src, dest, progress_tx, cancel, overwrite).map(|_bytes| ())
            }
        }
    }

    fn remove_src(self, src: &Path, cancel: &AtomicBool) -> io::Result<()> {
        match self {
            MoveKind::Symlink => remove_any(src),
            MoveKind::File => fs::remove_file(src),
            MoveKind::Directory => delete_dir_recursive_cancelable(src, cancel),
        }
    }
}

/// Move or rename a filesystem entry.
///
/// The caller is responsible for obtaining user confirmation before
/// invoking this function when overwrite or path-conflict resolution is
/// required. This function performs the operation; it does not prompt.
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
                "source and destination are the same symlink",
            ));
        }
        return fs::rename(src, dest);
    }
    if src_meta.file_type().is_dir() && path_contains(src, dest)? {
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
            let kind = MoveKind::from_file_type(&src_meta.file_type());
            copy_then_remove_src(
                src,
                dest,
                cancel,
                || kind.copy(src, dest, progress_tx, cancel, overwrite),
                || kind.remove_src(src, cancel),
                || remove_any(dest),
                kind.label(),
            )
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
    if let Err(err) = check_canceled(cancel) {
        if let Err(rollback_err) = rollback_fn() {
            return Err(io::Error::other(format!(
                "cross-device move canceled after copy, rollback also failed: \
                 dest '{}' left on disk ({}): {}, rollback: {}",
                dest.display(),
                entry_kind,
                err,
                rollback_err,
            )));
        }
        return Err(err);
    }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering;

    #[test]
    fn copy_then_remove_src_cancel_after_copy_rolls_back_dest_and_keeps_source() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src.txt");
        let dest = temp.path().join("dest.txt");
        fs::write(&src, b"source").unwrap();

        let cancel = AtomicBool::new(false);
        let remove_called = AtomicBool::new(false);

        let err = copy_then_remove_src(
            &src,
            &dest,
            &cancel,
            || {
                fs::copy(&src, &dest)?;
                cancel.store(true, Ordering::Relaxed);
                Ok(())
            },
            || {
                remove_called.store(true, Ordering::Relaxed);
                fs::remove_file(&src)
            },
            || fs::remove_file(&dest),
            "file",
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert!(src.exists());
        assert_eq!(fs::read(&src).unwrap(), b"source");
        assert!(!remove_called.load(Ordering::Relaxed));
    }

    #[test]
    fn copy_then_remove_src_success_skips_rollback() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src.txt");
        let dest = temp.path().join("dest.txt");
        fs::write(&src, b"source").unwrap();

        let cancel = AtomicBool::new(false);
        let rollback_called = AtomicBool::new(false);

        copy_then_remove_src(
            &src,
            &dest,
            &cancel,
            || fs::copy(&src, &dest).map(|_| ()),
            || fs::remove_file(&src),
            || {
                rollback_called.store(true, Ordering::Relaxed);
                fs::remove_file(&dest)
            },
            "file",
        )
        .unwrap();

        assert!(!src.exists());
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"source");
        assert!(!rollback_called.load(Ordering::Relaxed));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_move_copy_checks_cancel_before_copy() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let src = temp.path().join("link.txt");
        let dest = temp.path().join("dest-link.txt");
        fs::write(&target, b"target").unwrap();
        symlink(&target, &src).unwrap();

        let cancel = AtomicBool::new(true);
        let (progress_tx, _progress_rx) = std::sync::mpsc::channel();

        let err = MoveKind::Symlink
            .copy(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert!(src.symlink_metadata().unwrap().file_type().is_symlink());
    }
}
