mod common;
mod copy;
mod delete;
mod entry_ops;
pub(crate) mod move_ops;
mod temp;

pub(crate) use copy::preserve_timestamps;
// copy/delete/move are only consumed inside `ops::batch` (and the file_ops
// submodules); they are not part of the public `ops` facade. Kept crate-visible.
pub(crate) use copy::{copy_dir_recursive_with_progress, copy_file_with_progress, copy_symlink};
pub(crate) use delete::{
    delete_dir_recursive, delete_dir_recursive_cancelable, delete_file, ensure_entry_not_critical,
};
#[cfg(unix)]
pub use entry_ops::chmod;
pub use entry_ops::{create_directory, rename_entry};
pub(crate) use move_ops::move_entry_with_progress;

pub(super) use temp::replace_file_with_temp;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;

    const TAG_COPY: &str = "copy";
    const TAG_BACKUP: &str = "backup";

    fn assert_no_temp_leftovers(tmp_dir: &Path, tags: &[&str]) {
        let dir_name = tmp_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for tag in tags {
            let pattern = format!(".lc-dir-{tag}-");
            let mut found = vec![];
            if let Ok(entries) = std::fs::read_dir(tmp_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.contains(&pattern) {
                        found.push(name_str.into_owned());
                    }
                }
            }
            assert!(
                found.is_empty(),
                "temp leftovers found for tag '{tag}' in {dir_name}: {found:?}"
            );
        }
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir() -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "lc_fileops_{}_{}_{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn temp_dir_path_for(dest: &std::path::Path, seq: u64) -> std::path::PathBuf {
        temp::temp_dir_path_for(dest, seq)
    }

    #[test]
    fn test_copy_file_basic() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"hello world").unwrap();

        let bytes = copy::copy_file(&src, &dest, false).unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_same_location() {
        let tmp = unique_temp_dir();
        let src = tmp.join("same.txt");
        std::fs::write(&src, b"data").unwrap();

        let result = copy::copy_file(&src, &src, false);
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_preserves_permissions() {
        let tmp = unique_temp_dir();
        let src = tmp.join("exec.sh");
        let dest = tmp.join("exec_copy.sh");
        std::fs::write(&src, b"#!/bin/bash").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();

        copy::copy_file(&src, &dest, false).unwrap();
        let dest_mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(dest_mode, 0o755);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_with_progress_reports_bytes() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        let content = b"progress copy";
        std::fs::write(&src, content).unwrap();

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let bytes = copy_file_with_progress(&src, &dest, &progress_tx, &cancel, false).unwrap();

        assert_eq!(bytes, content.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), content);
        assert_eq!(progress_rx.try_iter().collect::<Vec<_>>(), vec![bytes]);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_with_progress_cancel_before_start_preserves_source() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"keep source").unwrap();

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = move_entry_with_progress(&src, &dest, &progress_tx, &cancel, false).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "keep source");
        assert!(!dest.exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file1.txt"), b"content1").unwrap();
        std::fs::create_dir(src.join("subdir")).unwrap();
        std::fs::write(src.join("subdir").join("file2.txt"), b"content2").unwrap();

        let dest = tmp.join("dest_dir");
        let bytes = copy::copy_dir_recursive(&src, &dest, false).unwrap();
        assert!(bytes > 0);
        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("subdir").join("file2.txt").exists());
        assert_no_temp_leftovers(&tmp, &[TAG_COPY]);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_reserves_unique_temp_when_collision_exists() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"content").unwrap();

        let dest = tmp.join("dest_dir");
        let seq = temp::TEMP_DIR_COUNTER.load(Ordering::Relaxed);
        let collision = temp_dir_path_for(&dest, seq);
        std::fs::create_dir(&collision).unwrap();
        std::fs::write(collision.join("sentinel.txt"), b"keep").unwrap();

        let bytes = copy::copy_dir_recursive(&src, &dest, false).unwrap();

        assert!(bytes > 0);
        assert!(dest.join("file.txt").exists());
        assert_eq!(
            std::fs::read_to_string(collision.join("sentinel.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_before_start_leaves_no_dest() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"content").unwrap();
        let dest = tmp.join("dest_dir");
        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = copy_dir_recursive_with_progress(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert_no_temp_leftovers(&tmp, &[TAG_COPY]);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_keeps_existing_temp_collision() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"content").unwrap();
        let dest = tmp.join("dest_dir");

        let seq = temp::TEMP_DIR_COUNTER.load(Ordering::Relaxed);
        let collision = temp_dir_path_for(&dest, seq);
        std::fs::create_dir(&collision).unwrap();
        std::fs::write(collision.join("sentinel.txt"), b"keep").unwrap();

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = copy_dir_recursive_with_progress(&src, &dest, &progress_tx, &cancel, false)
            .unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert!(!dest.exists());
        assert_eq!(
            std::fs::read_to_string(collision.join("sentinel.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_file() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_me.txt");
        let dest = tmp.join("moved.txt");
        std::fs::write(&src, b"moving").unwrap();

        move_ops::move_entry(&src, &dest, false).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "moving");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_dir() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("inside.txt"), b"inside").unwrap();

        let dest = tmp.join("moved_dir");
        move_ops::move_entry(&src, &dest, false).unwrap();
        assert!(!src.exists());
        assert!(dest.join("inside.txt").exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_existing_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dest, b"existing content").unwrap();

        let err = copy::copy_file(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "existing content");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "new content");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_existing_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dest, b"existing content").unwrap();

        let err = move_ops::move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "existing content");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "new content");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_file_overwrite_true() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dest, b"old content").unwrap();

        let bytes = copy::copy_file(&src, &dest, true).unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_overwrite_true() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dest, b"old content").unwrap();

        move_ops::move_entry(&src, &dest, true).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_removes_existing_file() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"new content").unwrap();
        let dest = tmp.join("dest.txt");
        std::fs::write(&dest, b"old content").unwrap();

        copy::copy_dir_recursive(&src, &dest, true).unwrap();
        assert!(dest.is_dir());
        assert_eq!(
            std::fs::read_to_string(dest.join("file.txt")).unwrap(),
            "new content"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_removes_existing_dir() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"new").unwrap();
        let dest = tmp.join("dest_dir");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("stale.txt"), b"old").unwrap();

        copy::copy_dir_recursive(&src, &dest, true).unwrap();
        assert!(dest.join("file.txt").exists());
        assert!(!dest.join("stale.txt").exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_overwrite_true_preserves_existing_dir_on_publish_error() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"new").unwrap();
        let dest = tmp.join("dest_dir");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("stale.txt"), b"old").unwrap();

        let blocked_parent = tmp.join("blocked_parent");
        let blocked_dest = blocked_parent.join("dest_dir");
        let temp = tmp.join("temp_dir");
        std::fs::create_dir(&temp).unwrap();
        std::fs::write(temp.join("file.txt"), b"new").unwrap();
        std::fs::write(&blocked_parent, b"not a directory").unwrap();

        let perms = std::fs::metadata(&temp).unwrap().permissions();
        let err = temp::publish_temp_dir(&temp, &blocked_dest, true, perms).unwrap_err();

        // Unix reports ENOTDIR; Windows maps the same condition to
        // ERROR_DIRECTORY/InvalidInput.
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::NotADirectory | std::io::ErrorKind::InvalidInput
            ),
            "unexpected error kind: {:?}",
            err.kind()
        );
        assert_eq!(
            std::fs::read_to_string(&blocked_parent).unwrap(),
            "not a directory"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("stale.txt")).unwrap(),
            "old"
        );
        assert!(temp.join("file.txt").exists());

        std::fs::remove_dir_all(&temp).unwrap();
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_publish_temp_dir_restores_backup_when_replace_fails() {
        let tmp = unique_temp_dir();
        let dest = tmp.join("dest_dir");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("stale.txt"), b"old").unwrap();

        let temp = dest.join("nested_temp");
        std::fs::create_dir(&temp).unwrap();
        std::fs::write(temp.join("file.txt"), b"new").unwrap();

        let perms = std::fs::metadata(&temp).unwrap().permissions();
        let err = temp::publish_temp_dir(&temp, &dest, true, perms).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            std::fs::read_to_string(dest.join("stale.txt")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(temp.join("file.txt")).unwrap(),
            "new"
        );
        assert_no_temp_leftovers(&tmp, &[TAG_BACKUP]);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_rejects_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();

        let dest = src.join("nested");
        let err = copy::copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_rejects_parent_component_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        let subdir = src.join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();

        let dest = subdir.join("..").join("nested");
        let err = copy::copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_existing_file_destination_does_not_overwrite() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"new content").unwrap();
        let dest = tmp.join("dest.txt");
        std::fs::write(&dest, b"existing content").unwrap();

        let err = copy::copy_dir_recursive(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "existing content");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_rejects_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        std::fs::create_dir(&src).unwrap();

        let dest = src.join("nested");
        let err = move_ops::move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_move_entry_rejects_parent_component_descendant_destination() {
        let tmp = unique_temp_dir();
        let src = tmp.join("move_dir");
        let subdir = src.join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();

        let dest = subdir.join("..").join("nested");
        let err = move_ops::move_entry(&src, &dest, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(src.exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        let linked = tmp.join("linked_dir");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&linked).unwrap();
        std::fs::write(linked.join("outside.txt"), b"outside").unwrap();
        symlink(&linked, src.join("symlink_dir")).unwrap();

        let dest = tmp.join("dest_dir");
        copy::copy_dir_recursive(&src, &dest, false).unwrap();
        assert!(
            dest.join("symlink_dir")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(dest.join("symlink_dir")).unwrap(),
            linked
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_file() {
        let tmp = unique_temp_dir();
        let file = tmp.join("delete_me.txt");
        std::fs::write(&file, b"bye").unwrap();

        delete_file(&file).unwrap();
        assert!(!file.exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_dir_recursive() {
        let tmp = unique_temp_dir();
        let dir = tmp.join("delete_dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("file.txt"), b"data").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("nested.txt"), b"nested").unwrap();

        delete_dir_recursive(&dir).unwrap();
        assert!(!dir.exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_removes_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let dir = tmp.join("delete_dir");
        let target = tmp.join("target_dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, dir.join("linked_dir")).unwrap();

        delete_dir_recursive(&dir).unwrap();

        assert!(!dir.exists());
        assert_eq!(
            std::fs::read_to_string(target.join("keep.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_removes_top_level_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let link = tmp.join("linked_dir");
        let target = tmp.join("target_dir");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &link).unwrap();

        delete_dir_recursive(&link).unwrap();

        assert!(!link.exists());
        assert_eq!(
            std::fs::read_to_string(target.join("keep.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_cancelable_removes_top_level_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let link = tmp.join("linked_dir");
        let target = tmp.join("target_dir");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &link).unwrap();
        let cancel = AtomicBool::new(false);

        delete_dir_recursive_cancelable(&link, &cancel).unwrap();

        assert!(!link.exists());
        assert_eq!(
            std::fs::read_to_string(target.join("keep.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_create_directory() {
        let tmp = unique_temp_dir();
        let new_dir = tmp.join("new_folder");
        create_directory(&new_dir).unwrap();
        assert!(new_dir.is_dir());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_create_directory_rejects_parent_component() {
        let tmp = unique_temp_dir();
        let base = tmp.join("base");
        std::fs::create_dir(&base).unwrap();
        let path = base.join("..").join("escaped");

        let err = create_directory(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!tmp.join("escaped").exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_chmod_does_not_follow_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_dir();
        let target = tmp.join("target.txt");
        let link = tmp.join("link.txt");
        std::fs::write(&target, b"target").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &link).unwrap();

        let err = chmod(&link, 0o777).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let target_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(target_mode, 0o600);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry() {
        let tmp = unique_temp_dir();
        let old = tmp.join("old_name.txt");
        std::fs::write(&old, b"rename me").unwrap();

        rename_entry(&old, "new_name.txt").unwrap();
        assert!(!old.exists());
        assert!(tmp.join("new_name.txt").exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_nonexistent() {
        let result = delete_file(std::path::Path::new("/tmp/lc_nonexistent_file_xyz"));
        assert!(result.is_err());
    }

    fn batch_copy(srcs: &[std::path::PathBuf], dest_dir: &std::path::Path) -> Vec<String> {
        let mut errors = Vec::new();
        for src in srcs {
            let file_name = src.file_name().unwrap_or_default();
            let dest = dest_dir.join(file_name);
            let result = if src.is_dir() {
                copy::copy_dir_recursive(src, &dest, false).map(|_| ())
            } else {
                copy::copy_file(src, &dest, false).map(|_| ())
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        errors
    }

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
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();

        let files: Vec<std::path::PathBuf> = (1..=3)
            .map(|i| {
                let p = src_dir.join(format!("file{}.txt", i));
                std::fs::write(&p, format!("content{}", i).as_bytes()).unwrap();
                p
            })
            .collect();

        let errors = batch_copy(&files, &dest_dir);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        for i in 1..=3 {
            assert!(dest_dir.join(format!("file{}.txt", i)).exists());
            assert_eq!(
                std::fs::read_to_string(dest_dir.join(format!("file{}.txt", i))).unwrap(),
                format!("content{}", i)
            );
        }
        for f in &files {
            assert!(f.exists());
        }

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_copy_mixed_files_and_dirs() {
        let tmp = unique_temp_dir();
        let src_dir = tmp.join("src");
        let dest_dir = tmp.join("dest");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();

        let file = src_dir.join("plain.txt");
        std::fs::write(&file, b"hello").unwrap();

        let dir = src_dir.join("subdir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("nested.txt"), b"nested").unwrap();

        let srcs = vec![file, dir];
        let errors = batch_copy(&srcs, &dest_dir);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert!(dest_dir.join("plain.txt").exists());
        assert!(dest_dir.join("subdir").join("nested.txt").exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_delete_multiple_files() {
        let tmp = unique_temp_dir();

        let files: Vec<std::path::PathBuf> = (1..=3)
            .map(|i| {
                let p = tmp.join(format!("del{}.txt", i));
                std::fs::write(&p, b"bye").unwrap();
                p
            })
            .collect();

        let errors = batch_delete(&files);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        for f in &files {
            assert!(!f.exists(), "file should be deleted: {}", f.display());
        }

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_delete_continues_on_error() {
        let tmp = unique_temp_dir();
        let real_file = tmp.join("real.txt");
        std::fs::write(&real_file, b"data").unwrap();
        let missing = tmp.join("nonexistent_xyz.txt");

        let paths = vec![missing, real_file.clone()];
        let errors = batch_delete(&paths);
        assert_eq!(errors.len(), 1);
        assert!(!real_file.exists());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_batch_copy_same_dir_overwrites() {
        let tmp = unique_temp_dir();
        let file = tmp.join("same.txt");
        std::fs::write(&file, b"original").unwrap();

        let errors = batch_copy(std::slice::from_ref(&file), &tmp);
        assert_eq!(errors.len(), 1);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_with_progress_cancel_before_start_via_thread() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src_dir");
        std::fs::create_dir(&src).unwrap();
        for i in 0..50 {
            std::fs::write(src.join(format!("file_{}.txt", i)), b"some content").unwrap();
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
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert!(!dest.exists());

        assert_no_temp_leftovers(&tmp, &[TAG_COPY]);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_delete_dir_recursive_rejects_root_directory() {
        let err = delete_dir_recursive(std::path::Path::new("/")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    // The critical-path list holds Unix system roots; on Windows the path
    // simply does not exist, so the guard is exercised on Unix only.
    #[cfg(unix)]
    #[test]
    fn test_delete_dir_recursive_rejects_critical_dir_prefix() {
        let err = delete_dir_recursive(std::path::Path::new("/usr/local/share")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_rename_entry_same_name_is_existing_dest() {
        let tmp = unique_temp_dir();
        let file = tmp.join("myfile.txt");
        std::fs::write(&file, b"data").unwrap();

        rename_entry(&file, "myfile.txt").unwrap();
        assert!(file.exists());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry_existing_destination() {
        let tmp = unique_temp_dir();
        let a = tmp.join("a.txt");
        let b = tmp.join("b.txt");
        std::fs::write(&a, b"alpha").unwrap();
        std::fs::write(&b, b"beta").unwrap();

        let err = rename_entry(&a, "b.txt").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(a.exists());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "beta");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_rename_entry_rejects_path_separator() {
        let tmp = unique_temp_dir();
        let file = tmp.join("file.txt");
        std::fs::write(&file, b"data").unwrap();

        let err = rename_entry(&file, "sub/new.txt").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_chmod_normal_file() {
        let tmp = unique_temp_dir();
        let file = tmp.join("file.txt");
        std::fs::write(&file, b"test").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();

        chmod(&file, 0o644).unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_apply_metadata_via_copy_preserves_mode_and_times() {
        let tmp = unique_temp_dir();
        let src = tmp.join("src.txt");
        let dest = tmp.join("dest.txt");
        std::fs::write(&src, b"metadata test").unwrap();

        #[cfg(unix)]
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o750)).unwrap();

        let past_mtime = filetime::FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(&src, past_mtime).unwrap();

        copy::copy_file(&src, &dest, false).unwrap();

        let dest_meta = std::fs::metadata(&dest).unwrap();
        #[cfg(unix)]
        {
            let dest_mode = dest_meta.permissions().mode() & 0o777;
            assert_eq!(dest_mode, 0o750);
        }
        let dest_mtime = filetime::FileTime::from_last_modification_time(&dest_meta);
        assert_eq!(dest_mtime.unix_seconds(), past_mtime.unix_seconds());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_copy_dir_recursive_exceeds_depth_limit() {
        // Run in a thread with a large stack to avoid SIGABRT from deep recursion
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let tmp = unique_temp_dir();
                let src = tmp.join("deep");
                std::fs::create_dir(&src).unwrap();

                let mut current = src.clone();
                for _ in 0..257 {
                    current.push("d");
                }
                std::fs::create_dir_all(&current).unwrap();

                let dest = tmp.join("dest");
                let err = copy::copy_dir_recursive(&src, &dest, false).unwrap_err();
                let msg = format!("{}", err);
                assert!(msg.contains(&format!(">={}", common::MAX_RECURSION_DEPTH)));
                assert!(!dest.exists());

                std::fs::remove_dir_all(&tmp).unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
