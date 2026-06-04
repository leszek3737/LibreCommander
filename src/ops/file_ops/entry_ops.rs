use std::fs;
use std::io;
use std::path::{Component, Path};

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
    if new_path.try_exists().unwrap_or(true) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    fs::rename(old, new_path)
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot chmod a symlink, refuse to follow symlinks",
        ));
    }

    let permissions = fs::Permissions::from_mode(mode & 0o7777);
    #[cfg(target_os = "macos")]
    let result = fs::set_permissions(path, permissions).map_err(|e| {
        if e.raw_os_error() == Some(libc::EFTYPE) {
            return io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot chmod a symlink, refuse to follow symlinks",
            );
        }
        e
    });
    #[cfg(not(target_os = "macos"))]
    let result = fs::set_permissions(path, permissions);
    result
}
