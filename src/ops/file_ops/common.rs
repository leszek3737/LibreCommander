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

#[cfg(windows)]
pub(super) fn same_inode(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    let a_idx = a.file_index();
    let b_idx = b.file_index();
    let a_vol = a.volume_serial_number();
    let b_vol = b.volume_serial_number();
    a_vol.is_some() && a_vol == b_vol && a_idx.is_some() && a_idx == b_idx
}

#[cfg(not(any(unix, windows)))]
pub(super) fn same_inode(_a: &fs::Metadata, _b: &fs::Metadata) -> bool {
    false
}

pub(super) fn check_canceled(cancel: &AtomicBool) -> io::Result<()> {
    check_optional_canceled(Some(cancel))
}

pub(super) fn check_optional_canceled(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation canceled",
        ));
    }
    Ok(())
}

pub(super) fn ensure_destination_absent(dest: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dest) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

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

pub(super) fn canonicalize_existing_path(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize()
}

pub(super) fn canonicalize_with_nearest_existing_parent(path: &Path) -> io::Result<PathBuf> {
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

pub(super) fn remove_any(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::IsADirectory => std::fs::remove_dir_all(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}
