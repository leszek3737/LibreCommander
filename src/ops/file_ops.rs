use crate::debug_log;
use crate::ops::chunk_copy;
use crate::ops::helpers::{cleanup_dir, cleanup_dir_all, cleanup_file, lexical_path_starts_with};

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;

const MAX_RECURSION_DEPTH: usize = 256;

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
const CRITICAL_DIRS: &[&str] = &[
    "/",
    "/Applications",
    "/System",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib64",
    "/nix",
    "/private",
    "/private/etc",
    "/private/tmp",
    "/private/var",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
];

#[cfg(not(target_os = "macos"))]
const CRITICAL_DIRS: &[&str] = &[
    "/", "/System", "/bin", "/boot", "/dev", "/etc", "/flatpak", "/gnu", "/lib", "/lib64", "/nix",
    "/proc", "/sbin", "/snap", "/sys", "/usr", "/var",
];

#[cfg(target_os = "macos")]
const CRITICAL_DIR_PREFIXES: &[&str] = &[
    "/Applications",
    "/System",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib64",
    "/nix",
    "/private/etc",
    "/private/tmp",
    "/private/var",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
];

#[cfg(not(target_os = "macos"))]
const CRITICAL_DIR_PREFIXES: &[&str] = &[
    "/System", "/bin", "/boot", "/dev", "/etc", "/flatpak", "/gnu", "/lib", "/lib64", "/nix",
    "/proc", "/sbin", "/snap", "/sys", "/usr", "/var",
];

#[allow(dead_code)]
pub fn copy_file(src: &Path, dest: &Path, overwrite: bool) -> io::Result<u64> {
    reject_same_file(src, dest)?;
    if !overwrite {
        ensure_destination_absent(dest)?;
    }
    let src_meta = fs::metadata(src)?;
    if overwrite {
        let temp = reserve_temp_file_for(dest)?;
        let bytes = fs::copy(src, &temp)?;
        apply_metadata(&temp, &src_meta)?;
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
        apply_metadata(&temp, &src_meta)?;
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

#[allow(dead_code)]
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
            let revalidated_dest = canonicalize_with_nearest_existing_parent(dest)?;
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
            let revalidated_dest = canonicalize_with_nearest_existing_parent(dest)?;
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

#[allow(dead_code)]
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

struct CopyContext<'a> {
    progress_tx: &'a Sender<u64>,
    cancel: &'a AtomicBool,
    overwrite: bool,
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
            let temp = reserve_temp_file_for(dest)?;
            fs::remove_file(&temp)?;
            std::os::unix::fs::symlink(&target, &temp)?;
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

/// Move or rename a filesystem entry.
///
/// Case-only rename semantics:
/// - On case-insensitive filesystems, `canonicalize()` resolves both `src` and
///   `dest` to the same inode, so the `same_file` branch fires and the rename
///   is performed via `fs::rename`, which handles the case change atomically.
/// - On case-sensitive filesystems, `dest.canonicalize()` fails (target does
///   not exist), so the function proceeds as a normal move.
#[allow(dead_code)]
pub fn move_entry(src: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    let same_file = match (src.canonicalize().ok(), dest.canonicalize().ok()) {
        (Some(s), Some(d)) => s == d,
        _ => src == dest,
    };
    if same_file {
        return if src == dest {
            Ok(())
        } else {
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
            let meta = src.symlink_metadata()?;
            if meta.file_type().is_symlink() {
                copy_symlink(src, dest, overwrite)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "cross-device move: copied '{}' to '{}' but failed to remove source: {}",
                        src.display(),
                        dest.display(),
                        del_err
                    )));
                }
            } else if meta.is_dir() {
                copy_dir_recursive(src, dest, overwrite)?;
                if !path_contains(src, dest)?
                    && let Err(del_err) = delete_dir_recursive(src)
                {
                    return Err(io::Error::other(format!(
                        "cross-device move: copied '{}' to '{}' but failed to remove source directory: {}",
                        src.display(),
                        dest.display(),
                        del_err
                    )));
                }
            } else {
                copy_file(src, dest, overwrite)?;
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

pub fn move_entry_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<()> {
    check_canceled(cancel)?;
    let same_file = match (src.canonicalize().ok(), dest.canonicalize().ok()) {
        (Some(s), Some(d)) => s == d,
        _ => src == dest,
    };
    if same_file {
        return if src == dest {
            Ok(())
        } else {
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
                if let Err(del_err) = delete_dir_recursive_cancelable(src, cancel) {
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

pub fn delete_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

pub fn delete_dir_recursive(path: &Path) -> io::Result<()> {
    delete_dir_recursive_with_cancel(path, None)
}

pub fn delete_dir_recursive_cancelable(path: &Path, cancel: &AtomicBool) -> io::Result<()> {
    delete_dir_recursive_with_cancel(path, Some(cancel))
}

/// Recursive delete operates under a non-adversarial filesystem guarantee.
/// It assumes no concurrent process is actively replacing directories with
/// symlinks during the deletion. The critical-directory blocklist provides
/// defense-in-depth against accidental deletion of system directories.
fn delete_dir_recursive_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    check_optional_canceled(cancel)?;
    let root_metadata = fs::symlink_metadata(path)?;
    if root_metadata.file_type().is_symlink() {
        return fs::remove_file(path);
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| io::Error::new(e.kind(), format!("Cannot verify path safety: {e}")))?;
    if canonical.parent().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete root directory",
        ));
    }
    for critical in CRITICAL_DIRS {
        if canonical == Path::new(*critical) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to delete critical system directory: {critical}"),
            ));
        }
    }
    let is_under_temp = {
        let canonical_temp = std::env::temp_dir()
            .canonicalize()
            .unwrap_or_else(|_| std::env::temp_dir());
        canonical.starts_with(&canonical_temp)
    };
    for critical in CRITICAL_DIR_PREFIXES {
        if !is_under_temp && canonical.starts_with(Path::new(*critical)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to delete critical system directory: {critical}"),
            ));
        }
        if !is_under_temp && path.starts_with(Path::new(*critical)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to delete critical system directory: {critical}"),
            ));
        }
    }
    delete_dir_contents(&canonical, path, cancel)?;
    check_optional_canceled(cancel)?;
    fs::remove_dir(path)
}

const DELETE_MAX_DEPTH: usize = 256;

fn delete_dir_contents(root: &Path, path: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    delete_dir_contents_impl(root, path, cancel, 0)
}

fn delete_dir_contents_impl(
    root: &Path,
    path: &Path,
    cancel: Option<&AtomicBool>,
    depth: usize,
) -> io::Result<()> {
    if depth > DELETE_MAX_DEPTH {
        return Err(io::Error::other(format!(
            "directory nesting depth {depth} exceeds maximum allowed {DELETE_MAX_DEPTH}",
        )));
    }

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to recursively delete symlinked directory",
        ));
    }
    let canonical = path.canonicalize().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot canonicalize {}: {e}", path.display()),
        )
    })?;
    if canonical != root && !lexical_path_starts_with(root, &canonical) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete path outside requested directory",
        ));
    }

    for entry in fs::read_dir(path)? {
        check_optional_canceled(cancel)?;
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            fs::remove_file(&entry_path)?;
        } else if file_type.is_dir() {
            delete_dir_contents_impl(root, &entry_path, cancel, depth + 1)?;
            check_optional_canceled(cancel)?;
            fs::remove_dir(&entry_path)?;
        } else {
            fs::remove_file(&entry_path)?;
        }
    }
    Ok(())
}

fn check_optional_canceled(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation canceled",
        ));
    }
    Ok(())
}

pub fn create_directory(path: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path must not contain parent components",
        ));
    }
    fs::create_dir_all(path)
}

pub fn rename_entry(old: &Path, new_name: &str) -> io::Result<()> {
    let mut normal_count = 0;
    for component in Path::new(new_name).components() {
        match component {
            Component::Normal(_) => normal_count += 1,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "new name must not contain path separators or parent components",
                ));
            }
        }
    }
    if normal_count != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "new name must be a single filename component",
        ));
    }
    let parent = old.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cannot determine parent directory",
        )
    })?;
    let new_path = parent.join(new_name);
    fs::rename(old, new_path)
}

pub fn chmod(path: &Path, mode: u32) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chmod refuses to follow symlinks",
        ));
    }

    let permissions = fs::Permissions::from_mode(mode & 0o7777);
    fs::set_permissions(path, permissions)
}

fn validate_copy_targets(src: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
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
    Ok(())
}

fn path_contains(parent: &Path, child: &Path) -> io::Result<bool> {
    let canonical_parent = canonicalize_existing_path(parent)
        .map_err(|e| io::Error::new(e.kind(), format!("failed to canonicalize parent: {e}")))?;
    let canonical_child = canonicalize_with_nearest_existing_parent(child)
        .map_err(|e| io::Error::new(e.kind(), format!("failed to canonicalize child: {e}")))?;
    Ok(lexical_path_starts_with(
        &canonical_parent,
        &canonical_child,
    ))
}

fn ensure_destination_absent(dest: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dest) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn temp_dir_path_for(dest: &Path, seq: u64) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "copy".into());
    name.push(format!(".lc-dir-copy-{}-{}.tmp", std::process::id(), seq));
    dest.with_file_name(name)
}

fn reserve_temp_dir_for(dest: &Path) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = temp_dir_path_for(dest, seq);
        match fs::create_dir(&temp) {
            Ok(()) => return Ok(temp),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve temporary copy directory",
    ))
}

struct DestBackup {
    container: PathBuf,
    entry: PathBuf,
}

fn publish_temp_dir(
    temp_dest: &Path,
    dest: &Path,
    overwrite: bool,
    src_perms: &fs::Permissions,
) -> io::Result<()> {
    fs::set_permissions(temp_dest, src_perms.clone())?;
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
                fs::remove_dir(&backup.container)?;
            }
            Err(err)
        }
    }
}

fn move_existing_dest_to_backup(dest: &Path) -> io::Result<Option<DestBackup>> {
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
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "copy".into());
    name.push(format!(".lc-dir-backup-{}-{}.tmp", std::process::id(), seq));
    dest.with_file_name(name)
}

#[cfg(unix)]
fn reject_same_file(src: &Path, dest: &Path) -> io::Result<()> {
    let src_meta = fs::symlink_metadata(src)?;
    let dest_meta = match fs::symlink_metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    use std::os::unix::fs::MetadataExt;
    if src_meta.dev() == dest_meta.dev() && src_meta.ino() == dest_meta.ino() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "source and destination are the same file",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_same_file(src: &Path, dest: &Path) -> io::Result<()> {
    let same = match (src.canonicalize().ok(), dest.canonicalize().ok()) {
        (Some(s), Some(d)) => s == d,
        _ => src == dest,
    };
    if same {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "source and destination are the same file",
        ));
    }

    Ok(())
}

fn check_canceled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
    }

    Ok(())
}

fn canonicalize_existing_path(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize()
}

fn canonicalize_with_nearest_existing_parent(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = path;

    loop {
        if let Ok(canonical_ancestor) = ancestor.canonicalize() {
            let suffix = path
                .strip_prefix(ancestor)
                .unwrap_or_else(|_| Path::new(""));
            return normalize_suffix(canonical_ancestor, suffix);
        }

        match ancestor.parent() {
            Some(parent) if parent != ancestor => ancestor = parent,
            _ => return normalize_suffix(std::env::current_dir()?, path),
        }
    }
}

fn normalize_suffix(mut base: PathBuf, suffix: &Path) -> io::Result<PathBuf> {
    for component in suffix.components() {
        match component {
            Component::Normal(name) => base.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !base.pop() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "path escapes filesystem root",
                    ));
                }
            }
            Component::RootDir => base = PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
            Component::Prefix(prefix) => base = PathBuf::from(prefix.as_os_str()),
        }
    }

    Ok(base)
}

fn remove_any(path: &Path) -> io::Result<()> {
    if path.is_symlink() {
        std::fs::remove_file(path)
    } else if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn apply_metadata(target: &Path, src_meta: &fs::Metadata) -> io::Result<()> {
    let mode = src_meta.permissions();
    fs::set_permissions(target, mode)?;
    let atime = filetime::FileTime::from_last_access_time(src_meta);
    let mtime = filetime::FileTime::from_last_modification_time(src_meta);
    if let Err(e) = filetime::set_file_times(target, atime, mtime) {
        debug_log!("set_file_times failed for {}: {e}", target.display());
    }
    Ok(())
}

pub(super) fn replace_file_with_temp(temp: &Path, dest: &Path) -> io::Result<()> {
    let need_remove = match fs::symlink_metadata(dest) {
        Ok(meta) if meta.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("cannot overwrite directory with file: {}", dest.display()),
            ));
        }
        Ok(_) => true,
        Err(_) => false,
    };
    // On Windows, fs::rename fails if dest exists — remove first.
    // On Unix, rename(2) atomically replaces the target.
    #[cfg(windows)]
    if need_remove {
        fs::remove_file(dest)?;
    }
    let _ = need_remove; // unused on Unix
    fs::rename(temp, dest)
}

fn swap_temp_to_dest(temp: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
    if overwrite {
        let need_remove = match fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    "cannot replace a directory with a file",
                ));
            }
            Ok(_) => true,
            Err(_) => false,
        };
        #[cfg(windows)]
        if need_remove {
            let mut os = dest.as_os_str().to_os_string();
            os.push(".lc_bak");
            let backup = PathBuf::from(os);
            if backup.exists() {
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
        let _ = need_remove;
    }
    std::fs::rename(temp, dest)
}

fn reserve_temp_file_for(dest: &Path) -> io::Result<PathBuf> {
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

#[cfg(test)]
#[allow(dead_code)]
fn temp_file_path_for(dest: &Path, seq: u64) -> PathBuf {
    let dir = dest.parent().unwrap_or(Path::new("."));
    let name = dest.file_name().unwrap_or_default();
    dir.join(format!("{}.{}.tmp", name.to_string_lossy(), seq))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir() -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "lc_fileops_{}_{}_{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_copy_file_basic() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"hello world").unwrap();

        let bytes = copy_file(&src, &dest, false).unwrap();
        let _dest_mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(bytes, 11);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "hello world");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_same_location() {
        let tmp = unique_temp_dir();
        let src = tmp.join("same.txt");
        fs::write(&src, b"data").unwrap();

        let result = copy_file(&src, &src, false);
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_preserves_permissions() {
        let tmp = unique_temp_dir();
        let src = tmp.join("exec.sh");
        let dest = tmp.join("exec_copy.sh");
        fs::write(&src, b"#!/bin/bash").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();

        copy_file(&src, &dest, false).unwrap();
        let dest_mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(dest_mode, 0o755);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_with_progress_reports_bytes() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        let content = b"progress copy";
        fs::write(&src, content).unwrap();

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let bytes = copy_file_with_progress(&src, &dest, &progress_tx, &cancel, false).unwrap();

        assert_eq!(bytes, content.len() as u64);
        assert_eq!(fs::read(&dest).unwrap(), content);
        assert_eq!(progress_rx.try_iter().collect::<Vec<_>>(), vec![bytes]);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_with_progress_cancel_before_start_preserves_source() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"keep source").unwrap();

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = move_entry_with_progress(&src, &dest, &progress_tx, &cancel, false).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert_eq!(fs::read_to_string(&src).unwrap(), "keep source");
        assert!(!dest.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file1.txt"), b"content1").unwrap();
        fs::create_dir(src.join("subdir")).unwrap();
        fs::write(src.join("subdir").join("file2.txt"), b"content2").unwrap();

        let dest = tmp.join("dest_dir");
        let bytes = copy_dir_recursive(&src, &dest, false).unwrap();
        assert!(bytes > 0);
        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("subdir").join("file2.txt").exists());
        assert!(!tmp.read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".lc-dir-copy-")
        }));

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_reserves_unique_temp_when_collision_exists() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"content").unwrap();

        let dest = tmp.join("dest_dir");
        let seq = TEMP_DIR_COUNTER.load(Ordering::Relaxed);
        let collision = temp_dir_path_for(&dest, seq);
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("sentinel.txt"), b"keep").unwrap();

        let bytes = copy_dir_recursive(&src, &dest, false).unwrap();

        assert!(bytes > 0);
        assert!(dest.join("file.txt").exists());
        assert_eq!(
            fs::read_to_string(collision.join("sentinel.txt")).unwrap(),
            "keep"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_before_start_leaves_no_dest() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"content").unwrap();
        let dest = tmp.join("dest_dir");
        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = copy_dir_recursive_with_progress(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert!(!tmp.read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".lc-dir-copy-")
        }));

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_keeps_existing_temp_collision() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"content").unwrap();
        let dest = tmp.join("dest_dir");

        let seq = TEMP_DIR_COUNTER.load(Ordering::Relaxed);
        let collision = temp_dir_path_for(&dest, seq);
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("sentinel.txt"), b"keep").unwrap();

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = copy_dir_recursive_with_progress(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert_eq!(
            fs::read_to_string(collision.join("sentinel.txt")).unwrap(),
            "keep"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_file() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_me.txt");
        let dest = tmp.join("moved.txt");
        fs::write(&src, b"moving").unwrap();

        move_entry(&src, &dest, false).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "moving");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_dir() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("inside.txt"), b"inside").unwrap();

        let dest = tmp.join("moved_dir");
        move_entry(&src, &dest, false).unwrap();
        assert!(!src.exists());
        assert!(dest.join("inside.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_existing_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"new content").unwrap();
        fs::write(&dest, b"existing content").unwrap();

        let err = copy_file(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "existing content");
        assert_eq!(fs::read_to_string(&src).unwrap(), "new content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_existing_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"new content").unwrap();
        fs::write(&dest, b"existing content").unwrap();

        let err = move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "existing content");
        assert_eq!(fs::read_to_string(&src).unwrap(), "new content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_overwrite_true() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"new content").unwrap();
        fs::write(&dest, b"old content").unwrap();

        let bytes = copy_file(&src, &dest, true).unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_overwrite_true() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"new content").unwrap();
        fs::write(&dest, b"old content").unwrap();

        move_entry(&src, &dest, true).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_removes_existing_file() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"new content").unwrap();
        let dest = tmp.join("dest.txt");
        fs::write(&dest, b"old content").unwrap();

        copy_dir_recursive(&src, &dest, true).unwrap();
        assert!(dest.is_dir());
        assert_eq!(
            fs::read_to_string(dest.join("file.txt")).unwrap(),
            "new content"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_removes_existing_dir() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"new").unwrap();
        let dest = tmp.join("dest_dir");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("stale.txt"), b"old").unwrap();

        copy_dir_recursive(&src, &dest, true).unwrap();
        assert!(dest.join("file.txt").exists());
        assert!(!dest.join("stale.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_preserves_existing_dir_on_publish_error() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"new").unwrap();
        let dest = tmp.join("dest_dir");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("stale.txt"), b"old").unwrap();

        let blocked_parent = tmp.join("blocked_parent");
        let blocked_dest = blocked_parent.join("dest_dir");
        let temp = tmp.join("temp_dir");
        fs::create_dir(&temp).unwrap();
        fs::write(temp.join("file.txt"), b"new").unwrap();
        fs::write(&blocked_parent, b"not a directory").unwrap();

        let perms = fs::metadata(&temp).unwrap().permissions();
        let err = publish_temp_dir(&temp, &blocked_dest, true, &perms).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            fs::read_to_string(&blocked_parent).unwrap(),
            "not a directory"
        );
        assert_eq!(fs::read_to_string(dest.join("stale.txt")).unwrap(), "old");
        assert!(temp.join("file.txt").exists());

        fs::remove_dir_all(&temp).unwrap();
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_publish_temp_dir_restores_backup_when_replace_fails() {
        let tmp = unique_temp_dir();
        let dest = tmp.join("dest_dir");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("stale.txt"), b"old").unwrap();

        let temp = dest.join("nested_temp");
        fs::create_dir(&temp).unwrap();
        fs::write(temp.join("file.txt"), b"new").unwrap();

        let perms = fs::metadata(&temp).unwrap().permissions();
        let err = publish_temp_dir(&temp, &dest, true, &perms).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_to_string(dest.join("stale.txt")).unwrap(), "old");
        assert_eq!(fs::read_to_string(temp.join("file.txt")).unwrap(), "new");
        assert!(!tmp.read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".lc-dir-backup-")
        }));

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_rejects_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();

        let dest = src.join("nested");
        let err = copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_rejects_parent_component_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        let subdir = src.join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        let dest = subdir.join("..").join("nested");
        let err = copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_existing_file_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file.txt"), b"new content").unwrap();
        let dest = tmp.join("dest.txt");
        fs::write(&dest, b"existing content").unwrap();

        let err = copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "existing content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_rejects_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        fs::create_dir(&src).unwrap();

        let dest = src.join("nested");
        let err = move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_rejects_parent_component_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        let subdir = src.join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        let dest = subdir.join("..").join("nested");
        let err = move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(src.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        let linked = tmp.join("linked_dir");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&linked).unwrap();
        fs::write(linked.join("outside.txt"), b"outside").unwrap();
        symlink(&linked, src.join("symlink_dir")).unwrap();

        let dest = tmp.join("dest_dir");
        copy_dir_recursive(&src, &dest, false).unwrap();
        assert!(
            dest.join("symlink_dir")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(dest.join("symlink_dir")).unwrap(), linked);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_file() {
        let tmp = unique_temp_dir();
        let file = tmp.join("delete_me.txt");
        fs::write(&file, b"bye").unwrap();

        delete_file(&file).unwrap();
        assert!(!file.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_dir_recursive() {
        let tmp = unique_temp_dir();
        let dir = tmp.join("delete_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("file.txt"), b"data").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("nested.txt"), b"nested").unwrap();

        delete_dir_recursive(&dir).unwrap();
        assert!(!dir.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_removes_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let dir = tmp.join("delete_dir");
        let target = tmp.join("target_dir");
        fs::create_dir(&dir).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, dir.join("linked_dir")).unwrap();

        delete_dir_recursive(&dir).unwrap();

        assert!(!dir.exists());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_removes_top_level_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let link = tmp.join("linked_dir");
        let target = tmp.join("target_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &link).unwrap();

        delete_dir_recursive(&link).unwrap();

        assert!(!link.exists());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_cancelable_removes_top_level_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let link = tmp.join("linked_dir");
        let target = tmp.join("target_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &link).unwrap();
        let cancel = AtomicBool::new(false);

        delete_dir_recursive_cancelable(&link, &cancel).unwrap();

        assert!(!link.exists());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_create_directory() {
        let tmp = unique_temp_dir();
        let new_dir = tmp.join("new_folder");
        create_directory(&new_dir).unwrap();
        assert!(new_dir.is_dir());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_create_directory_rejects_parent_component() {
        let tmp = unique_temp_dir();
        let base = tmp.join("base");
        fs::create_dir(&base).unwrap();
        let path = base.join("..").join("escaped");

        let err = create_directory(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!tmp.join("escaped").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_chmod_does_not_follow_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let target = tmp.join("target.txt");
        let link = tmp.join("link.txt");
        fs::write(&target, b"target").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &link).unwrap();

        let err = chmod(&link, 0o777).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        let target_mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(target_mode, 0o600);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry() {
        let tmp = unique_temp_dir();
        let old = tmp.join("old_name.txt");
        fs::write(&old, b"rename me").unwrap();

        rename_entry(&old, "new_name.txt").unwrap();
        assert!(!old.exists());
        assert!(tmp.join("new_name.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_nonexistent() {
        let result = delete_file(Path::new("/tmp/lc_nonexistent_file_xyz"));
        assert!(result.is_err());
    }

    /// Helper: simulate the multi-copy loop from execute_confirmed_action
    fn batch_copy(srcs: &[std::path::PathBuf], dest_dir: &std::path::Path) -> Vec<String> {
        let mut errors = Vec::new();
        for src in srcs {
            let file_name = src.file_name().unwrap_or_default();
            let dest = dest_dir.join(file_name);
            let result = if src.is_dir() {
                copy_dir_recursive(src, &dest, false).map(|_| ())
            } else {
                copy_file(src, &dest, false).map(|_| ())
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        errors
    }

    /// Helper: simulate the multi-delete loop from execute_confirmed_action
    fn batch_delete(paths: &[std::path::PathBuf]) -> Vec<String> {
        let mut errors = Vec::new();
        for path in paths {
            let result = if path.is_dir() {
                delete_dir_recursive(path)
            } else {
                delete_file(path)
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", path.display(), e));
            }
        }
        errors
    }

    #[test]
    fn test_batch_copy_multiple_files() {
        let tmp = unique_temp_dir();
        let src_dir = tmp.join("src");
        let dest_dir = tmp.join("dest");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&dest_dir).unwrap();

        let files: Vec<std::path::PathBuf> = (1..=3)
            .map(|i| {
                let p = src_dir.join(format!("file{}.txt", i));
                fs::write(&p, format!("content{}", i).as_bytes()).unwrap();
                p
            })
            .collect();

        let errors = batch_copy(&files, &dest_dir);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        for i in 1..=3 {
            assert!(dest_dir.join(format!("file{}.txt", i)).exists());
            assert_eq!(
                fs::read_to_string(dest_dir.join(format!("file{}.txt", i))).unwrap(),
                format!("content{}", i)
            );
        }
        // Originals still exist (copy, not move)
        for f in &files {
            assert!(f.exists());
        }

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_copy_mixed_files_and_dirs() {
        let tmp = unique_temp_dir();
        let src_dir = tmp.join("src");
        let dest_dir = tmp.join("dest");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&dest_dir).unwrap();

        let file = src_dir.join("plain.txt");
        fs::write(&file, b"hello").unwrap();

        let dir = src_dir.join("subdir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("nested.txt"), b"nested").unwrap();

        let srcs = vec![file, dir];
        let errors = batch_copy(&srcs, &dest_dir);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert!(dest_dir.join("plain.txt").exists());
        assert!(dest_dir.join("subdir").join("nested.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_delete_multiple_files() {
        let tmp = unique_temp_dir();

        let files: Vec<std::path::PathBuf> = (1..=3)
            .map(|i| {
                let p = tmp.join(format!("del{}.txt", i));
                fs::write(&p, b"bye").unwrap();
                p
            })
            .collect();

        let errors = batch_delete(&files);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        for f in &files {
            assert!(!f.exists(), "file should be deleted: {}", f.display());
        }

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_delete_continues_on_error() {
        let tmp = unique_temp_dir();
        let real_file = tmp.join("real.txt");
        fs::write(&real_file, b"data").unwrap();
        let missing = tmp.join("nonexistent_xyz.txt");

        let paths = vec![missing, real_file.clone()];
        let errors = batch_delete(&paths);
        // One error for missing file, real file still deleted
        assert_eq!(errors.len(), 1);
        assert!(!real_file.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_copy_same_dir_overwrites() {
        let tmp = unique_temp_dir();
        let file = tmp.join("same.txt");
        fs::write(&file, b"original").unwrap();

        let errors = batch_copy(std::slice::from_ref(&file), &tmp);
        assert_eq!(errors.len(), 1);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_mid_copy() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();
        for i in 0..50 {
            fs::write(src.join(format!("file_{}.txt", i)), b"some content").unwrap();
        }

        let dest = tmp.join("dest_dir");
        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let cancel_clone = std::sync::Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            cancel_clone.store(true, Ordering::Relaxed);
        });
        handle.join().unwrap();

        let err = copy_dir_recursive_with_progress(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());

        assert!(!tmp.read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".lc-dir-copy-")
        }));

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_dir_recursive_rejects_critical_dir() {
        if fs::symlink_metadata("/etc").is_ok_and(|m| !m.permissions().readonly()) {
            return;
        }
        let err = delete_dir_recursive(Path::new("/etc")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_delete_dir_recursive_rejects_critical_dir_prefix() {
        if fs::symlink_metadata("/etc").is_ok_and(|m| !m.permissions().readonly()) {
            return;
        }
        let err = delete_dir_recursive(Path::new("/etc/hosts")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_rename_entry_same_name_is_existing_dest() {
        let tmp = unique_temp_dir();
        let file = tmp.join("myfile.txt");
        fs::write(&file, b"data").unwrap();

        rename_entry(&file, "myfile.txt").unwrap();
        assert!(file.exists());
        assert_eq!(fs::read_to_string(&file).unwrap(), "data");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry_existing_destination() {
        let tmp = unique_temp_dir();
        let a = tmp.join("a.txt");
        let b = tmp.join("b.txt");
        fs::write(&a, b"alpha").unwrap();
        fs::write(&b, b"beta").unwrap();

        rename_entry(&a, "b.txt").unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read_to_string(&b).unwrap(), "alpha");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry_rejects_path_separator() {
        let tmp = unique_temp_dir();
        let file = tmp.join("file.txt");
        fs::write(&file, b"data").unwrap();

        let err = rename_entry(&file, "sub/new.txt").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_chmod_normal_file() {
        let tmp = unique_temp_dir();
        let file = tmp.join("file.txt");
        fs::write(&file, b"test").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();

        chmod(&file, 0o644).unwrap();
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_apply_metadata_via_copy_preserves_mode_and_times() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        fs::write(&src, b"metadata test").unwrap();

        #[cfg(unix)]
        fs::set_permissions(&src, fs::Permissions::from_mode(0o750)).unwrap();

        let past_mtime = filetime::FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(&src, past_mtime).unwrap();

        copy_file(&src, &dest, false).unwrap();

        let dest_meta = fs::metadata(&dest).unwrap();
        #[cfg(unix)]
        {
            let dest_mode = dest_meta.permissions().mode() & 0o777;
            assert_eq!(dest_mode, 0o750);
        }
        let dest_mtime = filetime::FileTime::from_last_modification_time(&dest_meta);
        assert_eq!(dest_mtime.unix_seconds(), past_mtime.unix_seconds());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_exceeds_depth_limit() {
        let tmp = unique_temp_dir();
        let src = tmp.join("deep");
        fs::create_dir(&src).unwrap();

        let mut current = src.clone();
        for _ in 0..257 {
            current.push("d");
        }
        fs::create_dir_all(&current).unwrap();

        let dest = tmp.join("dest");
        let err = copy_dir_recursive(&src, &dest, false).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains(&format!(">{}", MAX_RECURSION_DEPTH)));
        assert!(!dest.exists());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
