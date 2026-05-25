use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
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
