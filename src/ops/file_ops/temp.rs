use crate::debug_log;
use crate::ops::helpers::{cleanup_dir, cleanup_dir_all};

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::common::{MSG_DEST_EXISTS, remove_any};

const DEFAULT_NAME: &str = "copy";
const TEMP_NAME_MAX_ATTEMPTS: u32 = 128;

pub(crate) struct TempDirGuard {
    path: PathBuf,
    committed: bool,
}

impl TempDirGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if !self.committed {
            cleanup_dir_all(&self.path);
        }
    }
}

pub(super) static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn parent_and_filename(path: &Path) -> io::Result<(&Path, &OsStr)> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let filename = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename")
    })?;
    Ok((parent, filename))
}

fn suffixed_path(dest: &Path, tag: &str, seq: u64) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| DEFAULT_NAME.into());
    name.push(format!(".lc-dir-{tag}-{}-{}.tmp", std::process::id(), seq));
    dest.with_file_name(name)
}

#[cfg(test)]
pub(super) fn temp_dir_path_for(dest: &Path, seq: u64) -> PathBuf {
    suffixed_path(dest, "copy", seq)
}

fn reserve_unique_name(dest: &Path, prefix: &str) -> io::Result<PathBuf> {
    let mut last_err: Option<io::Error> = None;
    for _ in 0..TEMP_NAME_MAX_ATTEMPTS {
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = suffixed_path(dest, prefix, seq);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                last_err = Some(err);
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    let msg = if let Some(err) = last_err {
        format!("could not reserve unique name for {prefix}: last error: {err}")
    } else {
        format!("could not reserve unique name for {prefix}")
    };
    Err(io::Error::new(io::ErrorKind::AlreadyExists, msg))
}

pub(super) fn reserve_temp_dir_for(dest: &Path) -> io::Result<TempDirGuard> {
    reserve_unique_name(dest, "copy").map(TempDirGuard::new)
}

pub(super) struct DestBackup {
    pub(super) container: PathBuf,
    pub(super) entry: PathBuf,
}

/// Is `fname` a backup container this module created for `dest_name`?
/// Containers are named `<dest_name>.lc-dir-backup-<pid>-<seq>.tmp`. Compared on
/// raw bytes so a non-UTF-8 destination name is still matched.
fn is_backup_container_for(fname: &OsStr, dest_name: &OsStr) -> bool {
    let Some(rest) = fname
        .as_encoded_bytes()
        .strip_prefix(dest_name.as_encoded_bytes())
    else {
        return false;
    };
    rest.starts_with(b".lc-dir-backup-") && rest.ends_with(b".tmp")
}

/// Best-effort recovery of an orphaned backup left when a crash interrupts
/// [`publish_temp_dir`] between moving `dest` to a backup and renaming the temp
/// into place — the exact state in which `dest` is missing and the original is
/// stranded in a `.lc-dir-backup-*.tmp` dir. The stranded original is renamed
/// back to `dest` and the empty container removed.
///
/// Only runs when `dest` is missing: if `dest` is present nothing is stranded,
/// and touching backups could race a concurrent publish, so it does nothing.
fn recover_orphaned_backups(dest: &Path) {
    if dest.try_exists().unwrap_or(true) {
        return;
    }
    let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return;
    };
    let Some(dest_name) = dest.file_name() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if !is_backup_container_for(&entry.file_name(), dest_name) {
            continue;
        }
        let container = entry.path();
        match fs::rename(container.join("dest"), dest) {
            // Original restored — done; only one backup can hold it.
            Ok(()) => {
                cleanup_dir(&container);
                return;
            }
            // Empty container (crash before the original was moved in) or a
            // transient failure: drop the empty shell, keep scanning.
            Err(_) => cleanup_dir(&container),
        }
    }
}

/// Two-phase atomic directory replace.
///
/// Phase 1: move existing dest → backup (if `overwrite` && dest exists).
/// Phase 2: rename temp → dest.
/// On phase 2 failure: restore backup → dest.
///
/// # Crash safety
/// A crash between phase 1 and phase 2 strands the original data inside an
/// orphaned `.lc-dir-backup-*.tmp` dir with `dest` missing. That is recovered by
/// [`recover_orphaned_backups`], called at the top of this function: the next
/// publish targeting the same `dest` restores the stranded original before
/// proceeding.
///
/// # TOCTOU
/// When `overwrite=false`, dest existence is re-checked right before rename
/// to narrow the race window. Full atomicity would require `RENAME_NOREPLACE`
/// or `renamex_np`, which are out of stdlib.
pub(super) fn publish_temp_dir(
    temp_dest: &Path,
    dest: &Path,
    overwrite: bool,
    src_perms: fs::Permissions,
) -> io::Result<()> {
    recover_orphaned_backups(dest);
    if let Err(e) = fs::set_permissions(temp_dest, src_perms) {
        cleanup_dir_all(temp_dest);
        return Err(e);
    }
    if !overwrite {
        match fs::symlink_metadata(dest) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{MSG_DEST_EXISTS}: {}", dest.display()),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        return fs::rename(temp_dest, dest);
    }

    let backup = move_existing_dest_to_backup(dest)?;
    match fs::rename(temp_dest, dest) {
        Ok(()) => {
            if let Some(backup) = backup {
                if let Err(e) = remove_any(&backup.entry) {
                    debug_log!(
                        "warning: failed to cleanup backup entry {}: {e}",
                        backup.entry.display()
                    );
                }
                cleanup_dir(&backup.container);
            }
            Ok(())
        }
        Err(err) => {
            if let Some(backup) = backup {
                if let Err(restore_err) = fs::rename(&backup.entry, dest) {
                    debug_log!(
                        "failed to restore backup {} to {}: {restore_err}",
                        backup.entry.display(),
                        dest.display()
                    );
                }
                if let Err(e) = fs::remove_dir(&backup.container) {
                    debug_log!(
                        "warning: failed to cleanup backup container {}: {e}",
                        backup.container.display()
                    );
                }
            }
            Err(err)
        }
    }
}

pub(super) fn move_existing_dest_to_backup(dest: &Path) -> io::Result<Option<DestBackup>> {
    // There is a TOCTOU race between `symlink_metadata` and `rename`:
    // the destination can be removed or replaced after the metadata check.
    // This is benign — the `NotFound` fallback on `rename` handles the
    // case where the destination disappeared, and a concurrent replace
    // is indistinguishable from a legitimate overwrite (the user asked
    // to replace the destination).
    //
    // On Windows, `rename` may fail with a sharing violation if another
    // process has the file open; the `.lc_bak` rename trick used in
    // `replace_file_inner` is not applicable here because this code
    // deals with directories.
    match fs::symlink_metadata(dest) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    }

    let container = reserve_unique_name(dest, "backup")?;
    let entry = container.join("dest");
    match fs::rename(dest, &entry) {
        Ok(()) => Ok(Some(DestBackup { container, entry })),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            cleanup_dir(&container);
            Ok(None)
        }
        Err(err) => {
            cleanup_dir(&container);
            Err(err)
        }
    }
}

pub(super) fn swap_temp_to_dest(temp: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    if overwrite {
        replace_file_inner(temp, dest, "cannot replace a directory with a file")?;
    } else {
        fs::rename(temp, dest)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn reserve_temp_file_for(dest: &Path) -> io::Result<PathBuf> {
    let (dir, name) = parent_and_filename(dest)?;
    let pid = std::process::id();
    for counter in 0..TEMP_NAME_MAX_ATTEMPTS {
        let temp = dir.join(format!(
            "{}.{}.{}.tmp",
            name.to_string_lossy(),
            pid,
            counter
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(_) => return Ok(temp),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve temporary file (exhausted attempts)",
    ))
}

pub fn replace_file_with_temp(temp: &Path, dest: &Path) -> io::Result<()> {
    let msg = format!("cannot overwrite directory with file: {}", dest.display());
    replace_file_inner(temp, dest, &msg)
}

fn replace_file_inner(temp: &Path, dest: &Path, dir_err_msg: &str) -> io::Result<()> {
    let need_remove = match fs::symlink_metadata(dest) {
        Ok(meta) if meta.is_dir() => {
            return Err(io::Error::new(io::ErrorKind::IsADirectory, dir_err_msg));
        }
        Ok(_) => true,
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => return Err(err),
    };
    // On Windows, overwriting a file that another process has open can
    // fail with a sharing violation.  The rename-to-.lc_bak trick works
    // around this: the running file handle stays valid because the inode
    // (NTFS File Record Segment) is preserved across the rename.
    //
    // If the process crashes between rename(dest → .lc_bak) and
    // rename(temp → dest), a stale .lc_bak is left behind.  The next
    // replace attempt will notice the existing .lc_bak and remove it
    // before proceeding, so the orphan is self-healing.
    #[cfg(windows)]
    {
        if need_remove {
            let mut os = dest.as_os_str().to_os_string();
            os.push(".lc_bak");
            let backup = PathBuf::from(os);
            if fs::symlink_metadata(&backup).is_ok() {
                fs::remove_file(&backup)?;
            }
            fs::rename(dest, &backup)?;
            match fs::rename(temp, dest) {
                Ok(()) => {
                    let _ = fs::remove_file(&backup);
                }
                Err(err) => {
                    if let Err(restore_err) = fs::rename(&backup, dest) {
                        debug_log!(
                            "failed to restore backup {} to {}: {restore_err}",
                            backup.display(),
                            dest.display()
                        );
                    }
                    return Err(err);
                }
            }
            return Ok(());
        }
        fs::rename(temp, dest)
    }
    #[cfg(not(windows))]
    {
        let _ = need_remove;
        fs::rename(temp, dest)
    }
}
