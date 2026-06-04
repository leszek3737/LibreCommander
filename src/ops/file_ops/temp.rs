use crate::debug_log;
use crate::ops::helpers::{cleanup_dir, cleanup_dir_all};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::common::remove_any;

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

fn suffixed_path(dest: &Path, tag: &str, seq: u64) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "copy".into());
    name.push(format!(".lc-dir-{tag}-{}-{}.tmp", std::process::id(), seq));
    dest.with_file_name(name)
}

pub(super) fn temp_dir_path_for(dest: &Path, seq: u64) -> PathBuf {
    suffixed_path(dest, "copy", seq)
}

pub(super) fn reserve_temp_dir_for(dest: &Path) -> io::Result<TempDirGuard> {
    // 128 attempts balances collision avoidance against starvation;
    // the counter (TEMP_DIR_COUNTER) spaces names linearly so even with
    // high contention the odds of exceeding 128 unique conflicts are
    // negligibly small.
    for _ in 0..128 {
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = temp_dir_path_for(dest, seq);
        match fs::create_dir(&temp) {
            Ok(()) => return Ok(TempDirGuard::new(temp)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve temporary copy directory",
    ))
}

pub(super) struct DestBackup {
    pub(super) container: PathBuf,
    pub(super) entry: PathBuf,
}

pub(super) fn publish_temp_dir(
    temp_dest: &Path,
    dest: &Path,
    overwrite: bool,
    src_perms: fs::Permissions,
) -> io::Result<()> {
    fs::set_permissions(temp_dest, src_perms)?;
    if !overwrite {
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
                fs::rename(&backup.entry, dest).map_err(|restore_err| {
                    io::Error::new(
                        restore_err.kind(),
                        format!(
                            "failed to restore destination from backup {}: {restore_err}",
                            backup.entry.display()
                        ),
                    )
                })?;
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
    match fs::symlink_metadata(dest) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    }

    for _ in 0..128 {
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let container = backup_path_for(dest, seq);
        match fs::create_dir(&container) {
            Ok(()) => {
                let entry = container.join("dest");
                return match fs::rename(dest, &entry) {
                    Ok(()) => Ok(Some(DestBackup { container, entry })),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        cleanup_dir(&container);
                        Ok(None)
                    }
                    Err(err) => {
                        cleanup_dir(&container);
                        Err(err)
                    }
                };
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve backup path for destination",
    ))
}

fn backup_path_for(dest: &Path, seq: u64) -> PathBuf {
    suffixed_path(dest, "backup", seq)
}

pub(super) fn swap_temp_to_dest(temp: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    if overwrite {
        replace_file_inner(temp, dest, "cannot replace a directory with a file")?;
    } else {
        std::fs::rename(temp, dest)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn reserve_temp_file_for(dest: &Path) -> io::Result<PathBuf> {
    let dir = dest.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent directory",
        )
    })?;
    let name = dest.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename")
    })?;
    let pid = std::process::id();
    for counter in 0..1024 {
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
        "could not reserve temporary file (exhausted 1024 attempts)",
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
    }
    let _ = need_remove;
    fs::rename(temp, dest)
}
