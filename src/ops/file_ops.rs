use crate::ops::chunk_copy;

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

const MAX_RECURSION_DEPTH: usize = 256;
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
    "/private",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
];
const CRITICAL_DIR_PREFIXES: &[&str] = &[
    "/Applications",
    "/System",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib64",
    "/proc",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
];

pub fn copy_file(src: &Path, dest: &Path) -> io::Result<u64> {
    ensure_destination_absent(dest)?;

    reject_same_file(src, dest)?;
    let src_perms = fs::metadata(src)?.permissions();
    let bytes = fs::copy(src, dest)?;
    fs::set_permissions(dest, src_perms)?;
    Ok(bytes)
}

pub fn copy_file_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    check_canceled(cancel)?;
    ensure_destination_absent(dest)?;
    reject_same_file(src, dest)?;

    chunk_copy::copy_with_progress(src, dest, progress_tx, cancel)
}

pub fn copy_dir_recursive(src: &Path, dest: &Path) -> io::Result<u64> {
    let src_root = canonicalize_existing_path(src)?;
    let dest_root = canonicalize_with_nearest_existing_parent(dest)?;
    copy_dir_recursive_inner(src, dest, &src_root, &dest_root, 0)
}

pub fn copy_dir_recursive_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    check_canceled(cancel)?;
    let src_root = canonicalize_existing_path(src)?;
    let dest_root = canonicalize_with_nearest_existing_parent(dest)?;
    copy_dir_recursive_with_progress_inner(src, dest, &src_root, &dest_root, progress_tx, cancel, 0)
}

fn copy_dir_recursive_inner(
    src: &Path,
    dest: &Path,
    src_root: &Path,
    dest_root: &Path,
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
    if depth == 0 && path_contains_canonical(src_root, dest_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into its descendant",
        ));
    }
    ensure_destination_absent(dest)?;
    if depth == 0 {
        fs::create_dir_all(dest)?;
    } else {
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
            let copied =
                copy_dir_recursive_inner(&entry_path, &dest_path, src_root, dest_root, depth + 1)?;
            total_bytes = total_bytes.saturating_add(copied);
        } else if file_type.is_symlink() {
            let target = fs::read_link(&entry_path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &dest_path)?;
        } else {
            total_bytes = total_bytes.saturating_add(copy_file(&entry_path, &dest_path)?);
        }
    }

    fs::set_permissions(dest, src_perms)?;
    Ok(total_bytes)
}

fn copy_dir_recursive_with_progress_inner(
    src: &Path,
    dest: &Path,
    src_root: &Path,
    dest_root: &Path,
    progress_tx: &Sender<u64>,
    cancel: &AtomicBool,
    depth: usize,
) -> io::Result<u64> {
    check_canceled(cancel)?;
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
    if depth == 0 && path_contains_canonical(src_root, dest_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy directory into its descendant",
        ));
    }
    ensure_destination_absent(dest)?;
    if depth == 0 {
        fs::create_dir_all(dest)?;
    } else {
        fs::create_dir(dest)?;
    }
    let src_perms = fs::metadata(src)?.permissions();

    let mut total_bytes: u64 = 0;
    for entry in fs::read_dir(src)? {
        check_canceled(cancel)?;
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let copied = copy_dir_recursive_with_progress_inner(
                &entry_path,
                &dest_path,
                src_root,
                dest_root,
                progress_tx,
                cancel,
                depth + 1,
            )?;
            total_bytes = total_bytes.saturating_add(copied);
        } else if file_type.is_symlink() {
            copy_symlink(&entry_path, &dest_path)?;
        } else {
            total_bytes = total_bytes.saturating_add(copy_file_with_progress(
                &entry_path,
                &dest_path,
                progress_tx,
                cancel,
            )?);
        }
    }

    check_canceled(cancel)?;
    fs::set_permissions(dest, src_perms)?;
    Ok(total_bytes)
}

pub fn copy_symlink(src: &Path, dest: &Path) -> io::Result<()> {
    ensure_destination_absent(dest)?;

    let target = fs::read_link(src)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, dest)?;
    #[cfg(not(unix))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlinks not supported on this platform",
    ));
    Ok(())
}

pub fn move_entry(src: &Path, dest: &Path) -> io::Result<()> {
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
    ensure_destination_absent(dest)?;

    if src.is_dir() && path_contains(src, dest) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot move directory into its descendant",
        ));
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            let meta = src.symlink_metadata()?;
            if meta.file_type().is_symlink() {
                copy_symlink(src, dest)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
                    )));
                }
            } else if meta.is_dir() {
                copy_dir_recursive(src, dest)?;
                if !path_contains(src, dest)
                    && let Err(del_err) = delete_dir_recursive(src)
                {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
                    )));
                }
            } else {
                copy_file(src, dest)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
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
    ensure_destination_absent(dest)?;

    if src.is_dir() && path_contains(src, dest) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot move directory into its descendant",
        ));
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            check_canceled(cancel)?;
            let meta = src.symlink_metadata()?;
            if meta.file_type().is_symlink() {
                copy_symlink(src, dest)?;
                check_canceled(cancel)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
                    )));
                }
            } else if meta.is_dir() {
                copy_dir_recursive_with_progress(src, dest, progress_tx, cancel)?;
                check_canceled(cancel)?;
                if !path_contains(src, dest)
                    && let Err(del_err) = delete_dir_recursive(src)
                {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
                    )));
                }
            } else {
                copy_file_with_progress(src, dest, progress_tx, cancel)?;
                check_canceled(cancel)?;
                if let Err(del_err) = fs::remove_file(src) {
                    return Err(io::Error::other(format!(
                        "copied {:?} to {:?} but failed to remove source: {}",
                        src, dest, del_err
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

fn delete_dir_recursive_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    check_optional_canceled(cancel)?;
    if let Ok(canonical) = path.canonicalize() {
        if canonical.parent().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to delete root directory",
            ));
        }
        let canonical_str = canonical.to_string_lossy();
        for critical in CRITICAL_DIRS {
            if canonical_str == *critical {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing to delete critical system directory: {critical}"),
                ));
            }
        }
        for critical in CRITICAL_DIR_PREFIXES {
            if canonical_str.starts_with(&format!("{critical}/")) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing to delete critical system directory: {critical}"),
                ));
            }
        }
    }
    delete_dir_contents(path, cancel)?;
    check_optional_canceled(cancel)?;
    fs::remove_dir(path)
}

fn delete_dir_contents(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        check_optional_canceled(cancel)?;
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            delete_dir_contents(&entry_path, cancel)?;
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
    if new_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "file already exists",
        ));
    }
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

pub fn calculate_dir_size(path: &Path) -> io::Result<u64> {
    calculate_dir_size_inner(path, 0)
}

fn calculate_dir_size_inner(path: &Path, depth: usize) -> io::Result<u64> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(io::Error::other(format!(
            "directory too deeply nested (>{MAX_RECURSION_DEPTH} levels): {}",
            path.display()
        )));
    }

    let mut total: u64 = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                total = total.saturating_add(calculate_dir_size_inner(&entry_path, depth + 1)?);
            } else if file_type.is_symlink() {
                continue;
            } else {
                total = total.saturating_add(entry.metadata()?.len());
            }
        }
    } else {
        total = fs::metadata(path)?.len();
    }
    Ok(total)
}

fn path_contains(parent: &Path, child: &Path) -> bool {
    if let (Ok(canonical_parent), Ok(canonical_child)) = (
        canonicalize_existing_path(parent),
        canonicalize_with_nearest_existing_parent(child),
    ) {
        return path_contains_canonical(&canonical_parent, &canonical_child);
    }

    let parent_components = parent.components().peekable();
    let mut child_components = child.components().peekable();

    for parent_component in parent_components {
        match child_components.next() {
            Some(child_component) if components_equal(parent_component, child_component) => {}
            _ => return false,
        }
    }

    child_components.peek().is_some()
}

fn path_contains_canonical(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
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

fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    left == right
}

#[cfg(test)]
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

        let bytes = copy_file(&src, &dest).unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "hello world");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_same_location() {
        let tmp = unique_temp_dir();
        let src = tmp.join("same.txt");
        fs::write(&src, b"data").unwrap();

        let result = copy_file(&src, &src);
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

        copy_file(&src, &dest).unwrap();
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

        let bytes = copy_file_with_progress(&src, &dest, &progress_tx, &cancel).unwrap();

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

        let err = move_entry_with_progress(&src, &dest, &progress_tx, &cancel).unwrap_err();

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
        let bytes = copy_dir_recursive(&src, &dest).unwrap();
        assert!(bytes > 0);
        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("subdir").join("file2.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_file() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_me.txt");
        let dest = tmp.join("moved.txt");
        fs::write(&src, b"moving").unwrap();

        move_entry(&src, &dest).unwrap();
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
        move_entry(&src, &dest).unwrap();
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

        let err = copy_file(&src, &dest).unwrap_err();
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

        let err = move_entry(&src, &dest).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "existing content");
        assert_eq!(fs::read_to_string(&src).unwrap(), "new content");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_rejects_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        fs::create_dir(&src).unwrap();

        let dest = src.join("nested");
        let err = copy_dir_recursive(&src, &dest).unwrap_err();
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
        let err = copy_dir_recursive(&src, &dest).unwrap_err();
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

        let err = copy_dir_recursive(&src, &dest).unwrap_err();
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
        let err = move_entry(&src, &dest).unwrap_err();
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
        let err = move_entry(&src, &dest).unwrap_err();
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
        copy_dir_recursive(&src, &dest).unwrap();
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

    #[cfg(unix)]
    #[test]
    fn test_calculate_dir_size_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let dir = tmp.join("size_dir");
        let linked = tmp.join("linked_dir");
        fs::create_dir(&dir).unwrap();
        fs::create_dir(&linked).unwrap();
        fs::write(dir.join("local.txt"), b"abc").unwrap();
        fs::write(linked.join("outside.txt"), b"outside").unwrap();
        symlink(&linked, dir.join("symlink_dir")).unwrap();

        let size = calculate_dir_size(&dir).unwrap();
        assert_eq!(size, 3);

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
    fn test_calculate_dir_size() {
        let tmp = unique_temp_dir();
        let dir = tmp.join("size_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("small.txt"), b"abc").unwrap();
        fs::write(dir.join("medium.txt"), b"abcdefghij").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("nested.txt"), b"12345").unwrap();

        let size = calculate_dir_size(&dir).unwrap();
        assert_eq!(size, 18);

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
                copy_dir_recursive(src, &dest).map(|_| ())
            } else {
                copy_file(src, &dest).map(|_| ())
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

        let paths = vec![missing.clone(), real_file.clone()];
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
}
