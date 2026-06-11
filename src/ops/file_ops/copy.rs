use crate::ops::chunk_copy;
use crate::ops::helpers::{cleanup_file, lexical_path_starts_with};

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::common::{
    MAX_RECURSION_DEPTH, canonicalize_existing_path, canonicalize_with_nearest_existing_parent,
    check_canceled, ensure_destination_absent, reject_same_file, validate_copy_targets,
};
#[cfg(test)]
use super::temp::reserve_temp_file_for;
use super::temp::{publish_temp_dir, reserve_temp_dir_for, swap_temp_to_dest};

#[cfg(test)]
fn copy_file_to_temp(src: &Path, dest: &Path) -> io::Result<(std::path::PathBuf, u64)> {
    let temp = reserve_temp_file_for(dest)?;
    let bytes = match fs::copy(src, &temp) {
        Ok(b) => b,
        Err(e) => {
            cleanup_file(&temp);
            return Err(e);
        }
    };
    let src_meta = fs::metadata(src)?;
    if let Err(e) = apply_metadata(&temp, &src_meta) {
        cleanup_file(&temp);
        return Err(e);
    }
    Ok((temp, bytes))
}

#[cfg(test)]
pub fn copy_file(src: &Path, dest: &Path, overwrite: bool) -> io::Result<u64> {
    reject_same_file(src, dest)?;
    if !overwrite {
        ensure_destination_absent(dest)?;
    }
    let (temp, bytes) = copy_file_to_temp(src, dest)?;
    if !overwrite {
        match fs::hard_link(&temp, dest) {
            Ok(()) => {
                cleanup_file(&temp);
                return Ok(bytes);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                cleanup_file(&temp);
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("destination already exists: {}", dest.display()),
                ));
            }
            Err(_) => {}
        }
    }
    if let Err(err) = swap_temp_to_dest(&temp, dest, overwrite) {
        cleanup_file(&temp);
        return Err(err);
    }
    Ok(bytes)
}

pub fn copy_file_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<u64> {
    check_canceled(cancel)?;
    reject_same_file(src, dest)?;
    if !overwrite {
        ensure_destination_absent(dest)?;
    }

    chunk_copy::copy_with_progress(src, dest, progress_tx, cancel, overwrite)
}

#[cfg(test)]
pub fn copy_dir_recursive(src: &Path, dest: &Path, overwrite: bool) -> io::Result<u64> {
    let (tx, _rx) = std::sync::mpsc::channel();
    let cancel = AtomicBool::new(false);
    copy_dir_recursive_with_progress(src, dest, &tx, &cancel, overwrite)
}

pub fn copy_dir_recursive_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<u64> {
    check_canceled(cancel)?;
    validate_copy_targets(src, dest, overwrite)?;
    let ctx = CopyContext {
        progress_tx,
        cancel,
        overwrite,
    };
    let mut guard = reserve_temp_dir_for(dest)?;
    let src_perms = fs::metadata(src)?.permissions();
    let src_root = canonicalize_existing_path(src)?;
    let result = copy_dir_recursive_with_progress_inner(src, guard.path(), &ctx, 0);
    match result {
        Ok(bytes) => {
            check_canceled(cancel)?;
            let revalidated_dest = canonicalize_with_nearest_existing_parent(dest)?;
            if src_root == revalidated_dest
                || lexical_path_starts_with(&src_root, &revalidated_dest)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "destination changed during copy — possible symlink race",
                ));
            }
            publish_temp_dir(guard.path(), dest, overwrite, src_perms)?;
            guard.commit();
            Ok(bytes)
        }
        Err(err) => Err(err),
    }
}

pub struct CopyContext<'a> {
    pub(super) progress_tx: &'a Sender<u64>,
    pub(super) cancel: &'a AtomicBool,
    pub overwrite: bool,
}

fn copy_dir_recursive_with_progress_inner(
    src: &Path,
    dest: &Path,
    ctx: &CopyContext<'_>,
    depth: usize,
) -> io::Result<u64> {
    check_canceled(ctx.cancel)?;
    if depth >= MAX_RECURSION_DEPTH {
        return Err(io::Error::other(format!(
            "directory too deeply nested (>={MAX_RECURSION_DEPTH} levels): {}",
            src.display()
        )));
    }

    if depth > 0 {
        ensure_destination_absent(dest)?;
        fs::create_dir(dest)?;
    }
    let src_meta = fs::metadata(src)?;
    let src_perms = src_meta.permissions();

    let mut total_bytes: u64 = 0;
    for entry in fs::read_dir(src)? {
        check_canceled(ctx.cancel)?;
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let copied =
                copy_dir_recursive_with_progress_inner(&entry_path, &dest_path, ctx, depth + 1)?;
            total_bytes = total_bytes.saturating_add(copied);
        } else if file_type.is_symlink() {
            copy_symlink(&entry_path, &dest_path, ctx.overwrite)?;
            check_canceled(ctx.cancel)?;
        } else {
            total_bytes = total_bytes.saturating_add(copy_file_with_progress(
                &entry_path,
                &dest_path,
                ctx.progress_tx,
                ctx.cancel,
                ctx.overwrite,
            )?);
        }
    }

    check_canceled(ctx.cancel)?;
    fs::set_permissions(dest, src_perms)?;
    preserve_timestamps(dest, &src_meta)?;
    Ok(total_bytes)
}

pub fn copy_symlink(src: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    if !overwrite {
        ensure_destination_absent(dest)?;
    }

    let target = fs::read_link(src)?;
    #[cfg(unix)]
    {
        if overwrite {
            let dest_dir = dest.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "destination has no parent directory",
                )
            })?;
            let name = dest.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename")
            })?;
            let temp = {
                let base = name.to_string_lossy();
                let pid = std::process::id();
                let mut chosen = None;
                for counter in 0u32..1024 {
                    let path = dest_dir.join(format!("{base}.{pid}.{counter}.lc-symlink.tmp"));
                    match std::os::unix::fs::symlink(&target, &path) {
                        Ok(()) => {
                            chosen = Some(path);
                            break;
                        }
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                        Err(e) => return Err(e),
                    }
                }
                chosen.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "could not create temporary symlink (exhausted 1024 attempts)",
                    )
                })?
            };
            if let Err(err) = swap_temp_to_dest(&temp, dest, overwrite) {
                cleanup_file(&temp);
                return Err(err);
            }
        } else {
            std::os::unix::fs::symlink(&target, dest)?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (target, dest, overwrite);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks not supported on this platform",
        ));
    }
    Ok(())
}

pub(crate) fn preserve_timestamps(target: &Path, src_meta: &fs::Metadata) -> io::Result<()> {
    let atime = filetime::FileTime::from_last_access_time(src_meta);
    let mtime = filetime::FileTime::from_last_modification_time(src_meta);
    filetime::set_file_times(target, atime, mtime).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "failed to preserve timestamps for {}: {}",
                target.display(),
                e
            ),
        )
    })
}

#[cfg(test)]
fn apply_metadata(target: &Path, src_meta: &fs::Metadata) -> io::Result<()> {
    fs::set_permissions(target, src_meta.permissions())?;
    preserve_timestamps(target, src_meta)
}
