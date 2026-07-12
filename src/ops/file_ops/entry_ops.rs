use super::common::MSG_DEST_EXISTS;
#[cfg(unix)]
use super::common::MSG_SYMLINK_CHMOD;

use std::fs;
use std::io;
use std::path::{Component, Path};

/// Creates a directory at `path`, including any missing parent components.
///
/// Uses `create_dir_all` (not `create_dir`) because the caller is building
/// directory trees non-interactively (dest dir for copy, batch target dir).
/// A missing parent is expected for "copy to new directory" workflows;
/// failing with "no such file or directory" would force every caller to
/// implement their own ancestor creation loop.
///
/// Rejects paths containing `..` components to prevent directory traversal.
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

/// Validates that `new_name` is a single filename component (no separators, no `..`).
fn validate_entry_name(new_name: &str) -> io::Result<()> {
    if new_name.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "new name must not contain null bytes",
        ));
    }
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
    Ok(())
}

/// Renames a filesystem entry within its parent directory.
///
/// Detects same-inode renames (e.g., case-only rename on case-insensitive FS).
/// On POSIX, the `symlink_metadata` existence guard is best-effort — `fs::rename`
/// atomically replaces the destination; true atomic no-replace requires
/// `RENAME_NOREPLACE` or `renamex_np`, which are out of stdlib.
pub fn rename_entry(old: &Path, new_name: &str) -> io::Result<()> {
    validate_entry_name(new_name)?;
    let parent = old.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot determine parent directory",
        )
    })?;
    let new_path = parent.join(new_name);
    if new_path == old {
        return Ok(());
    }
    // Use `symlink_metadata` (not `try_exists`) for the existence check: a
    // dangling symlink at `new_path` must count as "present" so we don't silently
    // clobber it, but `try_exists` follows the broken link and reports `false`.
    // This mirrors `common::ensure_destination_absent`.
    let new_meta = match fs::symlink_metadata(&new_path) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let same_file = match (fs::symlink_metadata(old), new_meta.as_ref()) {
        (Ok(old_meta), Some(new_meta)) => super::common::same_inode(&old_meta, new_meta),
        _ => false,
    };
    // TOCTOU: this check + `fs::rename` is non-atomic. On POSIX, rename
    // atomically replaces the destination regardless; on Windows it errors.
    // This check is best-effort — atomic no-replace requires OS-specific APIs.
    if !same_file && new_meta.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            MSG_DEST_EXISTS,
        ));
    }
    fs::rename(old, new_path)
}

#[cfg(unix)]
/// Changes file permissions. Refuses to operate on symlinks — uses
/// `symlink_metadata` to detect them before calling `set_permissions`.
/// On macOS, `EFTYPE` is mapped to `InvalidInput` with a descriptive message.
pub fn chmod(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            MSG_SYMLINK_CHMOD,
        ));
    }

    let permissions = fs::Permissions::from_mode(mode & 0o7777);
    #[cfg(target_os = "macos")]
    let result = fs::set_permissions(path, permissions).map_err(|e| {
        if e.raw_os_error() == Some(libc::EFTYPE) {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported file type for chmod",
            )
        } else {
            e
        }
    });
    #[cfg(not(target_os = "macos"))]
    let result = fs::set_permissions(path, permissions);
    result
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // Regression: a dangling symlink at the destination must count as "present"
    // so `rename_entry` refuses rather than silently clobbering it. The old
    // `try_exists()` guard followed the broken link and reported `false`.
    #[test]
    fn rename_refuses_to_clobber_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        fs::write(&src, b"data").unwrap();

        let dangling = dir.path().join("dest.txt");
        std::os::unix::fs::symlink(dir.path().join("missing-target"), &dangling).unwrap();

        let err = rename_entry(&src, "dest.txt").expect_err("should refuse to clobber");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        // Source untouched and the dangling symlink still there.
        assert!(src.exists());
        assert!(
            fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn rename_to_fresh_name_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        fs::write(&src, b"data").unwrap();

        rename_entry(&src, "renamed.txt").unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read(dir.path().join("renamed.txt")).unwrap(), b"data");
    }
}
