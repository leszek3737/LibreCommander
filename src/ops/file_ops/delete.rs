use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use super::common::check_optional_canceled;

#[cfg(not(windows))]
fn remove_symlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> io::Result<()> {
    let target_is_dir = fs::metadata(path).is_ok_and(|m| m.is_dir());
    if target_is_dir {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

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

const DELETE_MAX_DEPTH: usize = 256;

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
        return remove_symlink(path);
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
    delete_dir_contents(&canonical, cancel)?;
    check_optional_canceled(cancel)?;
    fs::remove_dir(path)
}

fn delete_dir_contents(root: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    delete_dir_contents_impl(root, cancel, 0)
}

fn delete_dir_contents_impl(
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

    for entry in fs::read_dir(path)? {
        check_optional_canceled(cancel)?;
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            remove_symlink(&entry_path)?;
        } else if file_type.is_dir() {
            delete_dir_contents_impl(&entry_path, cancel, depth + 1)?;
            check_optional_canceled(cancel)?;
            fs::remove_dir(&entry_path)?;
        } else {
            fs::remove_file(&entry_path)?;
        }
    }
    Ok(())
}
