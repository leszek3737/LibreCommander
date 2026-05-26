use crate::ops::chunk_copy;
use crate::ops::helpers::{cleanup_dir_all, cleanup_file, lexical_path_starts_with};

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
pub fn copy_file(src: &Path, dest: &Path, overwrite: bool) -> io::Result<u64> {
    reject_same_file(src, dest)?;
    if !overwrite {
        ensure_destination_absent(dest)?;
    }
    let src_meta = fs::metadata(src)?;
    if overwrite {
        let temp = reserve_temp_file_for(dest)?;
        let bytes = match fs::copy(src, &temp) {
            Ok(b) => b,
            Err(e) => {
                cleanup_file(&temp);
                return Err(e);
            }
        };
        if let Err(e) = apply_metadata(&temp, &src_meta) {
            cleanup_file(&temp);
            return Err(e);
        }
        if let Err(err) = swap_temp_to_dest(&temp, dest, overwrite) {
            cleanup_file(&temp);
            return Err(err);
        }
        Ok(bytes)
    } else {
        let temp = reserve_temp_file_for(dest)?;
        let bytes = match fs::copy(src, &temp) {
            Ok(b) => b,
            Err(e) => {
                cleanup_file(&temp);
                return Err(e);
            }
        };
        if let Err(e) = apply_metadata(&temp, &src_meta) {
            cleanup_file(&temp);
            return Err(e);
        }
        if dest.exists() {
            cleanup_file(&temp);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("destination already exists: {}", dest.display()),
            ));
        }
        if let Err(e) = swap_temp_to_dest(&temp, dest, overwrite) {
            cleanup_file(&temp);
            return Err(e);
        }
        Ok(bytes)
    }
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
    let src_root = canonicalize_existing_path(src)?;
    let dest_root = canonicalize_with_nearest_existing_parent(dest)?;
    if src_root == dest_root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into itself",
        ));
    }
    if lexical_path_starts_with(&src_root, &dest_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into its descendant",
        ));
    }
    if !overwrite {
        ensure_destination_absent(dest)?;
    }
    let temp_dest = reserve_temp_dir_for(dest)?;
    let src_perms = fs::metadata(src)?.permissions();
    let result = copy_dir_recursive_inner(src, &temp_dest, &src_root, &dest_root, overwrite, 0);
    match result {
        Ok(bytes) => {
            let revalidated_dest = match canonicalize_with_nearest_existing_parent(dest) {
                Ok(p) => p,
                Err(e) => {
                    cleanup_dir_all(&temp_dest);
                    return Err(e);
                }
            };
            if src_root == revalidated_dest
                || lexical_path_starts_with(&src_root, &revalidated_dest)
            {
                cleanup_dir_all(&temp_dest);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "destination changed during copy — possible symlink race",
                ));
            }
            if let Err(err) = publish_temp_dir(&temp_dest, dest, overwrite, &src_perms) {
                cleanup_dir_all(&temp_dest);
                return Err(err);
            }
            Ok(bytes)
        }
        Err(err) => {
            cleanup_dir_all(&temp_dest);
            Err(err)
        }
    }
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
    let temp_dest = reserve_temp_dir_for(dest)?;
    let src_perms = fs::metadata(src)?.permissions();
    let src_root = canonicalize_existing_path(src)?;
    let result = copy_dir_recursive_with_progress_inner(src, &temp_dest, &ctx, 0);
    match result {
        Ok(bytes) => {
            if let Err(err) = check_canceled(cancel) {
                cleanup_dir_all(&temp_dest);
                return Err(err);
            }
            let revalidated_dest = match canonicalize_with_nearest_existing_parent(dest) {
                Ok(p) => p,
                Err(e) => {
                    cleanup_dir_all(&temp_dest);
                    return Err(e);
                }
            };
            if src_root == revalidated_dest
                || lexical_path_starts_with(&src_root, &revalidated_dest)
            {
                cleanup_dir_all(&temp_dest);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "destination changed during copy — possible symlink race",
                ));
            }
            if let Err(err) = publish_temp_dir(&temp_dest, dest, overwrite, &src_perms) {
                cleanup_dir_all(&temp_dest);
                return Err(err);
            }
            Ok(bytes)
        }
        Err(err) => {
            cleanup_dir_all(&temp_dest);
            Err(err)
        }
    }
}

#[cfg(test)]
fn copy_dir_recursive_inner(
    src: &Path,
    dest: &Path,
    src_root: &Path,
    dest_root: &Path,
    overwrite: bool,
    depth: usize,
) -> io::Result<u64> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(io::Error::other(format!(
            "directory too deeply nested (>{MAX_RECURSION_DEPTH} levels): {}",
            src.display()
        )));
    }

    if depth == 0 && src_root == dest_root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into itself",
        ));
    }
    if depth == 0 && lexical_path_starts_with(src_root, dest_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into its descendant",
        ));
    }
    if depth > 0 {
        ensure_destination_absent(dest)?;
        fs::create_dir(dest)?;
    }
    let src_perms = fs::metadata(src)?.permissions();

    let mut total_bytes: u64 = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let copied = copy_dir_recursive_inner(
                &entry_path,
                &dest_path,
                src_root,
                dest_root,
                overwrite,
                depth + 1,
            )?;
            total_bytes = total_bytes.saturating_add(copied);
        } else if file_type.is_symlink() {
            copy_symlink(&entry_path, &dest_path, overwrite)?;
        } else {
            total_bytes =
                total_bytes.saturating_add(copy_file(&entry_path, &dest_path, overwrite)?);
        }
    }

    fs::set_permissions(dest, src_perms)?;
    Ok(total_bytes)
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
    if depth > MAX_RECURSION_DEPTH {
        return Err(io::Error::other(format!(
            "directory too deeply nested (>{MAX_RECURSION_DEPTH} levels): {}",
            src.display()
        )));
    }

    if depth > 0 {
        ensure_destination_absent(dest)?;
        fs::create_dir(dest)?;
    }
    let src_perms = fs::metadata(src)?.permissions();

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

#[cfg(test)]
fn apply_metadata(target: &Path, src_meta: &fs::Metadata) -> io::Result<()> {
    let mode = src_meta.permissions();
    fs::set_permissions(target, mode)?;
    let atime = filetime::FileTime::from_last_access_time(src_meta);
    let mtime = filetime::FileTime::from_last_modification_time(src_meta);
    if let Err(e) = filetime::set_file_times(target, atime, mtime) {
        crate::debug_log!("set_file_times failed for {}: {e}", target.display());
    }
    Ok(())
}
