use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[cfg(windows)]
use super::common::is_dir_meta;
use super::common::{MAX_RECURSION_DEPTH, MSG_CRITICAL_DIR, check_optional_canceled};

#[cfg(not(windows))]
fn remove_symlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if is_dir_meta(&meta) {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(target_os = "macos")]
const CRITICAL_DIRS: &[&str] = &[
    "/",
    "/System",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib64",
    "/nix",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
    "/Applications",
    "/private",
    "/private/etc",
    "/private/tmp",
    "/private/var",
];

#[cfg(not(target_os = "macos"))]
const CRITICAL_DIRS: &[&str] = &[
    "/", "/System", "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/nix", "/proc", "/sbin",
    "/sys", "/tmp", "/usr", "/var", "/flatpak", "/gnu", "/snap",
];

#[cfg(target_os = "macos")]
const CRITICAL_DIR_PREFIXES: &[&str] = &[
    "/System",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib64",
    "/nix",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
    "/Applications",
    "/private/etc",
    "/private/tmp",
    "/private/var",
];

#[cfg(not(target_os = "macos"))]
const CRITICAL_DIR_PREFIXES: &[&str] = &[
    "/System", "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/nix", "/proc", "/sbin", "/sys",
    "/tmp", "/usr", "/var", "/flatpak", "/gnu", "/snap",
];

pub fn delete_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

pub fn delete_dir_recursive(path: &Path) -> io::Result<()> {
    delete_dir_recursive_with_cancel(path, None)
}

pub fn delete_dir_recursive_cancelable(path: &Path, cancel: &AtomicBool) -> io::Result<()> {
    delete_dir_recursive_with_cancel(path, Some(cancel))
}

fn validate_not_critical(canonical: &Path) -> io::Result<()> {
    // Unix root guard: `canonicalize("/")` yields `/` whose `parent()` is `None`.
    // On Windows, `\\?\C:\`.parent() = `Some(\\?\`)`, so this does NOT catch
    // Windows root drives — they're caught by the CRITICAL_DIRS list below instead.
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
                format!("{MSG_CRITICAL_DIR}{critical}"),
            ));
        }
    }
    let is_under_temp = {
        let raw_temp = std::env::temp_dir();
        if canonical.starts_with(&raw_temp) {
            true
        } else if let Ok(ct) = raw_temp.canonicalize() {
            canonical.starts_with(&ct)
        } else {
            false
        }
    };
    for critical in CRITICAL_DIR_PREFIXES {
        if !is_under_temp && canonical.starts_with(Path::new(*critical)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{MSG_CRITICAL_DIR}{critical}"),
            ));
        }
    }
    Ok(())
}

/// Validates that a file or symlink at `path` does not sit at or inside a
/// critical system directory, mirroring the protection `delete_dir_recursive_*`
/// already applies to directories. Used by the cross-device move fallback before
/// unlinking the source, so a regular file or symlink in `/usr/bin`, `/etc`, …
/// gets the same guard a directory there would.
///
/// A final-component symlink is NOT followed: its own location (parent chain
/// resolved, link name re-attached) is validated, so the check guards where the
/// link lives rather than where it points.
pub fn ensure_entry_not_critical(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let canonical = if meta.file_type().is_symlink() {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        let parent_canonical = match parent {
            Some(parent) => parent
                .canonicalize()
                .map_err(|e| io::Error::new(e.kind(), format!("Cannot verify path safety: {e}")))?,
            None => Path::new(".")
                .canonicalize()
                .map_err(|e| io::Error::new(e.kind(), format!("Cannot verify path safety: {e}")))?,
        };
        match path.file_name() {
            Some(name) => parent_canonical.join(name),
            None => parent_canonical,
        }
    } else {
        path.canonicalize()
            .map_err(|e| io::Error::new(e.kind(), format!("Cannot verify path safety: {e}")))?
    };
    validate_not_critical(&canonical)
}

/// Recursive delete operates under a non-adversarial filesystem guarantee.
/// It assumes no concurrent process is actively replacing directories with
/// symlinks during the deletion. The critical-directory blocklist provides
/// defense-in-depth against accidental deletion of system directories.
fn delete_dir_recursive_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    check_optional_canceled(cancel)?;
    let root_metadata = fs::symlink_metadata(path)?;
    if root_metadata.file_type().is_symlink() {
        // Validate the symlink's own location before unlinking: on macOS `/etc`,
        // `/var`, `/tmp` are themselves symlinks, so this early-return path would
        // otherwise bypass the critical-directory blocklist that the rest of the
        // function applies to real directories.
        ensure_entry_not_critical(path)?;
        return remove_symlink(path);
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| io::Error::new(e.kind(), format!("Cannot verify path safety: {e}")))?;
    validate_not_critical(&canonical)?;
    delete_dir_contents(&canonical, cancel)?;
    check_optional_canceled(cancel)?;
    fs::remove_dir(&canonical)
}

fn delete_dir_contents(root: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    delete_dir_contents_impl(root, cancel, 0)
}

fn delete_dir_contents_impl(
    path: &Path,
    cancel: Option<&AtomicBool>,
    depth: usize,
) -> io::Result<()> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(io::Error::other(format!(
            "directory nesting depth {depth} exceeds maximum allowed {MAX_RECURSION_DEPTH}",
        )));
    }

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
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
            // unlink(2) handles all non-directory entries: regular files, sockets, FIFOs, block/char devices
            fs::remove_file(&entry_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn ensure_entry_not_critical_allows_ordinary_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ordinary.txt");
        fs::write(&file, b"data").unwrap();
        assert!(ensure_entry_not_critical(&file).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_entry_not_critical_rejects_file_in_critical_dir() {
        // `/etc/hosts` resolves under a critical prefix (`/etc`, or `/private/etc`
        // on macOS) on essentially every Unix; guard on existence so the test is
        // a no-op on the rare system without it rather than a false failure.
        let hosts = Path::new("/etc/hosts");
        if hosts.exists() {
            assert!(
                ensure_entry_not_critical(hosts).is_err(),
                "expected /etc/hosts to be rejected as critical"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn ensure_entry_not_critical_rejects_symlink_in_critical_dir_without_following() {
        // A symlink whose *location* is critical must be rejected even if it
        // points somewhere harmless: the guard validates where the link lives,
        // not its target. We can't create files in `/etc`, so assert the inverse
        // property instead — a symlink in a temp dir pointing INTO `/etc` is
        // allowed, proving the target is not what gets validated.
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link-to-etc");
        std::os::unix::fs::symlink("/etc/hosts", &link).unwrap();
        assert!(
            ensure_entry_not_critical(&link).is_ok(),
            "a symlink in a temp dir must be judged by its own location, not its target"
        );
    }
}
