use crate::debug_log;
use crate::ops::helpers::lexical_path_starts_with;

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) use crate::ops::helpers::MAX_RECURSION_DEPTH;

#[cfg(unix)]
pub(super) fn same_inode(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
pub(super) fn same_inode(_a: &fs::Metadata, _b: &fs::Metadata) -> bool {
    // Windows' file_index()/volume_serial_number() need the unstable
    // `windows_by_handle` feature (rust-lang/rust#63010); without a stable
    // identity, conservatively report "not the same file".
    false
}

#[cfg(not(windows))]
pub(super) fn is_dir_meta(meta: &fs::Metadata) -> bool {
    meta.is_dir()
}

#[cfg(windows)]
pub(super) fn is_dir_meta(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

/// Mandatory cancel check for callers with a required cancel token.
pub(super) fn check_canceled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation canceled",
        ));
    }
    Ok(())
}

/// Optional cancel check — used by functions that may or may not have a cancel token.
pub(super) fn check_optional_canceled(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation canceled",
        ));
    }
    Ok(())
}

pub const MSG_DEST_EXISTS: &str = "destination already exists";

pub(super) fn ensure_destination_absent(dest: &Path) -> io::Result<()> {
    // ponytail: check-then-rename TOCTOU; renameat2(RENAME_NOREPLACE) if needed.
    match fs::symlink_metadata(dest) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{MSG_DEST_EXISTS}: {}", dest.display()),
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Checks if `child` is lexically contained within `parent` after canonicalization.
///
/// Note: errors are wrapped via `format!` which drops the `Error::source()` chain.
/// This is an accepted limitation of the `io::Result` error model used throughout
/// the project; a custom error type would be needed to preserve the full chain.
pub(super) fn path_contains(parent: &Path, child: &Path) -> io::Result<bool> {
    let canonical_parent = canonicalize_existing_path(parent).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("failed to canonicalize parent '{}': {e}", parent.display()),
        )
    })?;
    let canonical_child = canonicalize_with_nearest_existing_parent(child).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("failed to canonicalize child '{}': {e}", child.display()),
        )
    })?;
    Ok(lexical_path_starts_with(
        &canonical_parent,
        &canonical_child,
    ))
}

#[cfg(any(unix, windows))]
pub(super) fn reject_same_file(src: &Path, dest: &Path) -> io::Result<()> {
    let src_meta = fs::symlink_metadata(src)?;
    let dest_meta = match fs::symlink_metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if same_inode(&src_meta, &dest_meta) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "source and destination are the same file",
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn reject_same_file(src: &Path, dest: &Path) -> io::Result<()> {
    fs::symlink_metadata(src)?;
    match fs::symlink_metadata(dest) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    let literal_match = src == dest;
    let canon_match = src
        .canonicalize()
        .and_then(|s| dest.canonicalize().map(|d| (s, d)))
        .is_ok_and(|(s, d)| s == d);
    if literal_match || canon_match {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "source and destination are the same file",
        ));
    }
    Ok(())
}

pub(super) fn validate_copy_targets(src: &Path, dest: &Path, overwrite: bool) -> io::Result<()> {
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

/// Canonicalizes a path that is expected to already exist.
/// Serves as a named entry point for potential future validation (e.g., symlink policy).
pub(super) fn canonicalize_existing_path(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize()
}

pub(super) fn canonicalize_with_nearest_existing_parent(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = path;

    loop {
        if let Ok(canonical_ancestor) = ancestor.canonicalize() {
            let suffix = path.strip_prefix(ancestor).unwrap_or_else(|_| {
                // Ancestor was derived by walking up `path`'s parents, so
                // `strip_prefix` should always succeed. If it fails, the
                // canonical suffix is unknowable — fall back to relative path.
                debug_log!(
                    "BUG: strip_prefix failed on ancestor-anchored path walk: {} vs {}",
                    path.display(),
                    ancestor.display()
                );
                Path::new("")
            });
            return normalize_suffix(canonical_ancestor, suffix);
        }

        match ancestor.parent() {
            Some(parent) if parent != ancestor => ancestor = parent,
            _ => return normalize_suffix(std::env::current_dir()?, path),
        }
    }
}

pub(super) fn normalize_suffix(mut base: PathBuf, suffix: &Path) -> io::Result<PathBuf> {
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
            Component::RootDir => base = PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            Component::Prefix(prefix) => base = PathBuf::from(prefix.as_os_str()),
        }
    }

    Ok(base)
}

pub const MSG_CRITICAL_DIR: &str = "refusing to delete critical system directory: ";
#[cfg(unix)]
pub const MSG_SYMLINK_CHMOD: &str = "cannot chmod a symlink, refuse to follow symlinks";

/// Removes a filesystem entry by dispatching on file type.
///
/// Uses `symlink_metadata` for the initial stat to avoid dereferencing
/// symlinks. Branches: directory → `remove_dir_all`, directory symlink
/// → `remove_dir`, file/symlink-to-file → `remove_file`.
///
/// # Non-adversarial contract
///
/// This function assumes a non-adversarial filesystem — no concurrent
/// process is actively replacing the entry or its parent directory during
/// the operation. The single `symlink_metadata` call at the top
/// determines the dispatch target; a TOCTOU substitution after the stat
/// could cause the wrong removal function to be called (e.g., removing
/// a file that replaced a directory). Full TOCTOU hardening would require
/// platform-specific `openat`+`unlinkat` patterns which are gated behind
/// `forbid(unsafe_code)`.
pub(super) fn remove_any(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_dir() {
        return fs::remove_dir_all(path);
    }
    // Windows-only: directory symlinks/junctions have is_symlink() + is_dir_meta().
    // On Unix this branch is unreachable — symlink_metadata symlinks are !is_dir().
    if meta.is_symlink() && is_dir_meta(&meta) {
        return fs::remove_dir(path);
    }
    fs::remove_file(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_suffix_simple() {
        let base = Path::new("/home/user").to_path_buf();
        let suffix = Path::new("docs");
        let result = normalize_suffix(base, suffix).unwrap();
        assert_eq!(result, Path::new("/home/user/docs"));
    }

    #[test]
    fn normalize_suffix_parent_dir() {
        let base = Path::new("/home/user").to_path_buf();
        let suffix = Path::new("../other");
        let result = normalize_suffix(base, suffix).unwrap();
        assert_eq!(result, Path::new("/home/other"));
    }

    #[test]
    fn normalize_suffix_parent_dir_above_root() {
        let base = Path::new("/home").to_path_buf();
        let suffix = Path::new("../../etc");
        let err = normalize_suffix(base, suffix).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn normalize_suffix_absolute_path_uses_root() {
        let base = Path::new("/anything").to_path_buf();
        let suffix = Path::new("/usr/local/bin");
        let result = normalize_suffix(base, suffix).unwrap();
        assert_eq!(result, Path::new("/usr/local/bin"));
    }

    #[test]
    fn normalize_suffix_current_dir_ignored() {
        let base = Path::new("/dir").to_path_buf();
        let suffix = Path::new("./sub");
        let result = normalize_suffix(base, suffix).unwrap();
        assert_eq!(result, Path::new("/dir/sub"));
    }
}
