//! Filesystem reader module.
//! Note: This module uses Unix-specific APIs (MetadataExt, uid/gid lookups)
//! and will only compile on Unix platforms.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;

#[cfg(test)]
use crate::app::types::format_permissions;

pub use crate::app::types::FileEntry;

struct UidCache {
    uid_to_name: HashMap<u32, String>,
    gid_to_name: HashMap<u32, String>,
}

thread_local! {
    static UID_CACHE: RefCell<UidCache> = RefCell::new(UidCache {
        uid_to_name: HashMap::new(),
        gid_to_name: HashMap::new(),
    });
}

pub fn read_directory(
    path: &Path,
    show_hidden: bool,
) -> io::Result<(Vec<FileEntry>, Vec<io::Error>)> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();

    if path != Path::new("/") {
        let parent_path = path.parent().unwrap_or(path);
        entries.push(FileEntry {
            name: "..".to_string(),
            path: parent_path.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            is_executable: true,
            size: 0,
            modified: SystemTime::now(),
            permissions: 0o755,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        });
    }

    for entry in fs::read_dir(path)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                errors.push(io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to read directory entry in '{}': {}",
                        path.display(),
                        e
                    ),
                ));
                continue;
            }
        };
        let entry_path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        let is_hidden = file_name.starts_with('.');

        if !show_hidden && is_hidden {
            continue;
        }

        match get_file_info(&entry_path) {
            Ok(file_entry) => entries.push(file_entry),
            Err(e) => errors.push(io::Error::new(
                e.kind(),
                format!("Failed to read '{}': {}", entry_path.display(), e),
            )),
        }
    }

    Ok((entries, errors))
}

pub fn get_file_info(path: &Path) -> io::Result<FileEntry> {
    let metadata = fs::symlink_metadata(path)?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let is_symlink = metadata.is_symlink();
    let target_meta = if is_symlink {
        fs::metadata(path).ok()
    } else {
        None
    };
    let is_dir = if is_symlink {
        target_meta.as_ref().is_some_and(|m| m.is_dir())
    } else {
        metadata.is_dir()
    };

    let (size, modified, permissions, is_exec, uid, gid) =
        if let Some(ref target_metadata) = target_meta {
            let size = target_metadata.len();
            let modified = target_metadata.modified()?;
            let mode = target_metadata.mode();
            let uid = target_metadata.uid();
            let gid = target_metadata.gid();
            (size, modified, mode, is_executable(mode), uid, gid)
        } else {
            let size = metadata.len();
            let modified = metadata.modified()?;
            let mode = metadata.mode();
            let uid = metadata.uid();
            let gid = metadata.gid();
            let is_exec = if is_symlink && target_meta.is_none() {
                false
            } else {
                is_executable(mode)
            };
            let display_mode = if is_symlink && target_meta.is_none() {
                0
            } else {
                mode
            };
            (size, modified, display_mode, is_exec, uid, gid)
        };

    let (owner, group) = {
        UID_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let owner = cache
                .uid_to_name
                .entry(uid)
                .or_insert_with(|| {
                    users::get_user_by_uid(uid)
                        .map(|u| u.name().to_string_lossy().to_string())
                        .unwrap_or_else(|| uid.to_string())
                })
                .clone();
            let group = cache
                .gid_to_name
                .entry(gid)
                .or_insert_with(|| {
                    users::get_group_by_gid(gid)
                        .map(|g| g.name().to_string_lossy().to_string())
                        .unwrap_or_else(|| gid.to_string())
                })
                .clone();
            (owner, group)
        })
    };

    Ok(FileEntry {
        name: file_name.clone(),
        path: path.to_path_buf(),
        is_dir,
        is_symlink,
        is_executable: is_exec,
        size,
        modified,
        permissions,
        owner,
        group,
        selected: false,
        is_hidden: file_name.starts_with('.'),
    })
}

pub fn format_date(time: SystemTime) -> String {
    use chrono::{DateTime, Local};

    let datetime: DateTime<Local> = DateTime::from(time);
    let now = Local::now();
    let duration = now - datetime;

    if duration.num_days() < 365 {
        datetime.format("%b %d %H:%M").to_string()
    } else {
        datetime.format("%b %d  %Y").to_string()
    }
}

pub fn is_executable(mode: u32) -> bool {
    (mode & 0o100) != 0 || (mode & 0o010) != 0 || (mode & 0o001) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::FileEntry as CanonicalFileEntry;
    use std::fs::{self, File};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    fn create_temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let id = CTR.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "lc_reader_{}_{}_{}",
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
    fn test_format_size_zero() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        assert_eq!(entry.display_size(), "     0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 500,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        assert_eq!(entry.display_size(), "   500 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 1536,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        let result = entry.display_size();
        assert!(result.contains("KB"));
    }

    #[test]
    fn test_format_size_megabytes() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 1024 * 1024,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        let result = entry.display_size();
        assert!(result.contains("MB"));
    }

    #[test]
    fn test_format_size_gigabytes() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 1024 * 1024 * 1024,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        let result = entry.display_size();
        assert!(result.contains("GB"));
    }

    #[test]
    fn test_format_size_terabytes() {
        let entry = CanonicalFileEntry {
            name: "test".to_string(),
            path: PathBuf::from("test"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 1024u64.pow(4),
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: false,
        };
        let result = entry.display_size();
        assert!(result.contains("TB"));
    }

    #[test]
    fn test_format_permissions_rwx() {
        assert_eq!(format_permissions(0o755), "rwxr-xr-x");
        assert_eq!(format_permissions(0o644), "rw-r--r--");
        assert_eq!(format_permissions(0o700), "rwx------");
        assert_eq!(format_permissions(0o000), "---------");
        assert_eq!(format_permissions(0o777), "rwxrwxrwx");
    }

    #[test]
    fn test_is_executable() {
        assert!(is_executable(0o100));
        assert!(is_executable(0o010));
        assert!(is_executable(0o001));
        assert!(is_executable(0o755));
        assert!(!is_executable(0o644));
        assert!(!is_executable(0o000));
    }

    #[test]
    fn test_format_date_recent() {
        let recent = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 7);
        let result = format_date(recent);
        assert!(result.contains(':'));
    }

    #[test]
    fn test_format_date_old() {
        let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 400);
        let result = format_date(old);
        assert!(!result.contains(':'));
    }

    #[test]
    fn test_read_directory_basic() {
        let temp_dir = create_temp_dir();
        File::create(temp_dir.join("file1.txt")).unwrap();
        File::create(temp_dir.join("file2.txt")).unwrap();
        fs::create_dir(temp_dir.join("subdir")).unwrap();

        let (entries, errors) = read_directory(&temp_dir, false).unwrap();
        assert!(errors.is_empty());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&".."));
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"file2.txt"));
        assert!(names.contains(&"subdir"));

        let subdir_entry = entries.iter().find(|e| e.name == "subdir").unwrap();
        assert!(subdir_entry.is_dir);

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_read_directory_hidden_files() {
        let temp_dir = create_temp_dir();
        File::create(temp_dir.join("visible.txt")).unwrap();
        File::create(temp_dir.join(".hidden")).unwrap();

        let (entries_no_hidden, errors) = read_directory(&temp_dir, false).unwrap();
        assert!(errors.is_empty());
        assert!(!entries_no_hidden.iter().any(|e| e.name == ".hidden"));

        let (entries_with_hidden, errors) = read_directory(&temp_dir, true).unwrap();
        assert!(errors.is_empty());
        assert!(entries_with_hidden.iter().any(|e| e.name == ".hidden"));

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_get_file_info_file() {
        let temp_dir = create_temp_dir();
        let file_path = temp_dir.join("test.txt");
        File::create(&file_path).unwrap();

        let info = get_file_info(&file_path).unwrap();
        assert_eq!(info.name, "test.txt");
        assert!(!info.is_dir);

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_get_file_info_directory() {
        let temp_dir = create_temp_dir();
        let subdir = temp_dir.join("subdir");
        fs::create_dir(&subdir).unwrap();

        let info = get_file_info(&subdir).unwrap();
        assert_eq!(info.name, "subdir");
        assert!(info.is_dir);

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_get_file_info_executable() {
        let temp_dir = create_temp_dir();
        let file_path = temp_dir.join("script.sh");
        let file = File::create(&file_path).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms).unwrap();

        let info = get_file_info(&file_path).unwrap();
        assert!(info.is_executable);

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_read_directory_symlinks() {
        let temp_dir = create_temp_dir();
        let target = temp_dir.join("target.txt");
        File::create(&target).unwrap();
        let link = temp_dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let (entries, errors) = read_directory(&temp_dir, false).unwrap();
        assert!(errors.is_empty());
        if let Some(link_entry) = entries.iter().find(|e| e.name == "link.txt") {
            assert!(link_entry.is_symlink);
        }

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_read_directory_missing_path_still_returns_error() {
        let missing = std::env::temp_dir().join(format!(
            "lc_reader_missing_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let err = read_directory(&missing, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
