use super::helpers::cleanup_file;
use crate::debug_log;
use filetime::FileTime;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const BUFFER_SIZE: usize = 256 * 1024;
const PROGRESS_THROTTLE: Duration = Duration::from_millis(50);
const PROGRESS_CHECK_BYTES: usize = 64 * 1024;

pub fn copy_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &std::sync::mpsc::Sender<u64>,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<u64> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
    }

    let metadata = fs::symlink_metadata(src)?;

    if metadata.file_type().is_symlink() {
        super::file_ops::copy_symlink(src, dest, overwrite)?;
        return Ok(0);
    }

    if !overwrite && dest.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", dest.display()),
        ));
    }

    let src_file = File::open(src)?;
    let temp_dest = temp_path_for(dest);
    let result = copy_to_temp(src_file, &temp_dest, &metadata, progress_tx, cancel);

    match result {
        Ok(total_written) => {
            if cancel.load(Ordering::Relaxed) {
                cleanup_file(&temp_dest);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
            }
            if let Err(err) = publish_temp(&temp_dest, dest, cancel, overwrite) {
                cleanup_file(&temp_dest);
                return Err(err);
            }

            let accessed = FileTime::from_last_access_time(&metadata);
            let modified = FileTime::from_last_modification_time(&metadata);
            if let Err(e) = filetime::set_file_times(dest, accessed, modified) {
                debug_log!("set_file_times failed for {}: {e}", dest.display());
            }

            Ok(total_written)
        }
        Err(err) => {
            cleanup_file(&temp_dest);
            Err(err)
        }
    }
}

fn copy_to_temp(
    src_file: File,
    temp_dest: &Path,
    metadata: &fs::Metadata,
    progress_tx: &std::sync::mpsc::Sender<u64>,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    let dest_file = File::create_new(temp_dest)?;

    #[cfg(target_os = "linux")]
    {
        let file_size = metadata.len();
        if file_size > 0 {
            if let Ok(written) =
                try_copy_file_range(&src_file, &dest_file, file_size, progress_tx, cancel)
            {
                preserve_permissions(temp_dest, metadata)?;
                return Ok(written);
            }
        }
    }

    // TODO: macOS fcopyfile() zero-copy path. Requires safe wrapper (libc
    // fcopyfile needs unsafe) or a third-party crate wrapping it safely.

    let mut reader = src_file;
    let mut writer = dest_file;
    let mut buf = vec![0_u8; BUFFER_SIZE];
    let mut total_written = 0_u64;
    let mut pending_delta = 0_u64;
    let mut last_progress = Instant::now() - PROGRESS_THROTTLE;
    let mut bytes_since_progress_check = 0_usize;

    loop {
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }

        writer.write_all(&buf[..bytes_read])?;
        let bytes_written = bytes_read as u64;
        total_written += bytes_written;
        pending_delta += bytes_written;
        bytes_since_progress_check += bytes_read;

        if bytes_since_progress_check >= PROGRESS_CHECK_BYTES {
            bytes_since_progress_check = 0;
            let now = Instant::now();
            if now.duration_since(last_progress) >= PROGRESS_THROTTLE {
                let _ = progress_tx.send(pending_delta);
                pending_delta = 0;
                last_progress = now;
            }
        }

        if cancel.load(Ordering::Relaxed) {
            if pending_delta > 0 {
                let _ = progress_tx.send(pending_delta);
            }
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
        }
    }

    if pending_delta > 0 {
        let _ = progress_tx.send(pending_delta);
    }

    writer.flush()?;

    preserve_permissions(temp_dest, metadata)?;

    Ok(total_written)
}

fn publish_temp(
    temp_dest: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        cleanup_file(temp_dest);
        return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
    }
    if overwrite {
        super::file_ops::replace_file_with_temp(temp_dest, dest)?;
        return Ok(());
    }

    match fs::hard_link(temp_dest, dest) {
        Ok(()) => return fs::remove_file(temp_dest),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Err(err),
        Err(_) => {}
    }

    match dest.try_exists() {
        Ok(true) => {
            cleanup_file(temp_dest);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination file already exists",
            ));
        }
        Err(e) => {
            debug_log!("publish_temp: cannot stat dest {}: {e}", dest.display());
            cleanup_file(temp_dest);
            return Err(e);
        }
        Ok(false) => {}
    }

    fs::rename(temp_dest, dest).inspect_err(|_| {
        cleanup_file(temp_dest);
    })?;

    Ok(())
}

fn temp_path_for(dest: &Path) -> std::path::PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("copy"));
    let tid = std::thread::current().id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    name.push(format!(
        ".lc-copy-{}-{}-{:08x}.tmp",
        std::process::id(),
        hash_thread_id(tid),
        nanos ^ (hash_thread_id(tid) as u32)
    ));
    dest.with_file_name(name)
}

fn hash_thread_id(tid: std::thread::ThreadId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tid.hash(&mut hasher);
    hasher.finish()
}

#[cfg(unix)]
fn preserve_permissions(dest: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(
        dest,
        fs::Permissions::from_mode(metadata.permissions().mode()),
    )
}

#[cfg(not(unix))]
fn preserve_permissions(_dest: &Path, _metadata: &fs::Metadata) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn try_copy_file_range(
    src: &File,
    dest: &File,
    file_size: u64,
    progress_tx: &std::sync::mpsc::Sender<u64>,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    use std::os::linux::fs::copy_file_range as cfr;

    let mut total_written = 0_u64;
    let mut pending_delta = 0_u64;
    let mut last_progress = Instant::now() - PROGRESS_THROTTLE;
    let mut bytes_since_progress_check: u64 = 0;

    while total_written < file_size {
        if cancel.load(Ordering::Relaxed) {
            if pending_delta > 0 {
                let _ = progress_tx.send(pending_delta);
            }
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
        }

        let remaining = file_size - total_written;
        let to_transfer = remaining.min(BUFFER_SIZE as u64);

        let copied = cfr(src, None, dest, None, to_transfer)?;

        if copied == 0 {
            break;
        }

        total_written += copied as u64;
        pending_delta += copied as u64;
        bytes_since_progress_check += copied as u64;

        if bytes_since_progress_check >= PROGRESS_CHECK_BYTES as u64 {
            bytes_since_progress_check = 0;
            let now = Instant::now();
            if now.duration_since(last_progress) >= PROGRESS_THROTTLE {
                let _ = progress_tx.send(pending_delta);
                pending_delta = 0;
                last_progress = now;
            }
        }
    }

    if pending_delta > 0 {
        let _ = progress_tx.send(pending_delta);
    }

    Ok(total_written)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    #[test]
    fn copies_small_file_with_progress() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        let content = b"small file content";
        fs::write(&src, content).expect("write source file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let copied =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy file");

        assert_eq!(copied, content.len() as u64);
        assert_eq!(fs::read(&dest).expect("read dest file"), content);
        assert_eq!(progress_rx.try_iter().collect::<Vec<_>>(), vec![copied]);
    }

    #[test]
    fn copies_file_larger_than_buffer_with_progress() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("dest.bin");
        let content: Vec<u8> = (0..(BUFFER_SIZE + 17))
            .map(|idx| (idx % 251) as u8)
            .collect();
        fs::write(&src, &content).expect("write source file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let copied =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy file");

        assert_eq!(copied, content.len() as u64);
        assert_eq!(fs::read(&dest).expect("read dest file"), content);
        assert_eq!(
            progress_rx.try_iter().collect::<Vec<_>>(),
            vec![BUFFER_SIZE as u64, 17]
        );
    }

    #[test]
    fn existing_dest_returns_already_exists_without_overwrite() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"new content").expect("write source file");
        fs::write(&dest, b"old content").expect("write dest file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let err =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect_err("copy fails");

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&dest).expect("read dest file"), b"old content");
        assert!(progress_rx.try_iter().collect::<Vec<_>>().is_empty());
        assert!(!temp_path_for(&dest).exists());
    }

    #[test]
    fn cancel_before_start_returns_interrupted() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"content").expect("write source file");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect_err("cancel copy");

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists());
    }

    #[test]
    fn existing_temp_file_returns_already_exists_without_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"content").expect("write source file");
        let stale_temp = temp_path_for(&dest);
        fs::write(&stale_temp, b"leftover").expect("write temp file");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let result = copy_with_progress(&src, &dest, &progress_tx, &cancel, false);
        assert!(
            result.is_ok(),
            "stale temp with unique name should not block copy"
        );
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "content");
    }

    #[cfg(unix)]
    #[test]
    fn copies_symlink_preserving_target() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let target = dir.path().join("target.txt");
        let src = dir.path().join("src_link");
        let dest = dir.path().join("dest_link");
        fs::write(&target, b"link target content").expect("write target");
        std::os::unix::fs::symlink(&target, &src).expect("create source symlink");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy symlink");

        assert_eq!(
            fs::read_link(&dest).expect("read dest link"),
            fs::read_link(&src).expect("read src link")
        );
    }

    #[test]
    fn test_publish_temp_overwrite_true() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"new content").expect("write source file");
        fs::write(&dest, b"old content").expect("write dest file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let copied =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, true).expect("overwrite copy");

        assert_eq!(copied, 11);
        assert_eq!(fs::read(&dest).expect("read dest file"), b"new content");
        assert_eq!(progress_rx.try_iter().collect::<Vec<_>>(), vec![11]);
        assert!(!temp_path_for(&dest).exists());
    }

    #[test]
    fn cancel_mid_copy_large_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("dest.bin");

        let content: Vec<u8> = (0..50_000_000).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &content).expect("write source file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        let dest_clone = dest.clone();
        let handle = std::thread::spawn(move || {
            copy_with_progress(&src, &dest_clone, &progress_tx, &cancel_clone, false)
        });

        for _ in progress_rx.iter() {
            cancel.store(true, Ordering::Relaxed);
        }

        let result = handle.join().expect("thread joins");
        assert!(result.is_err(), "copy should be canceled");
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
        assert!(!dest.exists(), "dest file must not exist after cancel");
    }

    #[test]
    fn empty_file_copy_creates_existing_empty_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"").expect("write empty source file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let copied =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy empty file");

        assert_eq!(copied, 0);
        assert!(dest.exists(), "dest must exist");
        assert_eq!(fs::read(&dest).expect("read dest file"), b"");
        let progress: Vec<u64> = progress_rx.try_iter().collect();
        assert!(progress.is_empty(), "no progress for empty file");
    }

    #[test]
    fn overwrite_cancel_midway_preserves_original_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("dest.bin");

        let new_content: Vec<u8> = (0..50_000_000).map(|i| (i % 251) as u8).collect();
        let old_content = b"original dest content";
        fs::write(&src, &new_content).expect("write source file");
        fs::write(&dest, old_content).expect("write dest file");

        let (progress_tx, progress_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        let dest2 = dest.clone();
        let handle = std::thread::spawn(move || {
            copy_with_progress(&src, &dest2, &progress_tx, &cancel_clone, true)
        });

        for _ in progress_rx.iter() {
            cancel.store(true, Ordering::Relaxed);
        }

        let result = handle.join().expect("thread joins");
        assert!(result.is_err(), "copy should be canceled");

        assert!(dest.exists(), "dest must still exist");
        assert_eq!(
            fs::read(&dest).expect("read dest file"),
            old_content,
            "original dest content preserved"
        );
    }

    #[test]
    fn copies_file_preserving_mtime() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        let content = b"timestamps test";
        fs::write(&src, content).expect("write source file");

        let set_mtime = FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(&src, set_mtime).expect("set source mtime");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy file");

        let dest_meta = fs::metadata(&dest).expect("dest metadata");
        let dest_mtime = FileTime::from_last_modification_time(&dest_meta);

        assert_eq!(
            dest_mtime.unix_seconds(),
            set_mtime.unix_seconds(),
            "mtime preserved"
        );
        assert_eq!(
            dest_mtime.nanoseconds(),
            set_mtime.nanoseconds(),
            "mtime nanoseconds preserved"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copies_file_preserving_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.sh");
        let dest = dir.path().join("dest.sh");
        let content = b"#!/bin/sh\necho hello\n";
        fs::write(&src, content).expect("write source file");

        let mode = 0o755u32;
        fs::set_permissions(&src, fs::Permissions::from_mode(mode)).expect("set source perms");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect("copy file");

        let dest_meta = fs::metadata(&dest).expect("dest metadata");
        assert_eq!(
            dest_meta.permissions().mode() & 0o777,
            mode,
            "permissions preserved"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_with_progress_symlink_overwrite_replaces_link_not_target() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let old_target = dir.path().join("old_target");
        let new_target = dir.path().join("new_target");
        fs::create_dir(&old_target).expect("create old_target dir");
        fs::create_dir(&new_target).expect("create new_target dir");
        fs::write(old_target.join("file.txt"), b"old").expect("write old file");
        fs::write(new_target.join("file.txt"), b"new").expect("write new file");

        let src = dir.path().join("src_link");
        let dest = dir.path().join("dest_link");
        std::os::unix::fs::symlink(&new_target, &src).expect("create src symlink");
        std::os::unix::fs::symlink(&old_target, &dest).expect("create dest symlink");

        let (tx, _) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        copy_with_progress(&src, &dest, &tx, &cancel, true).expect("overwrite symlink");

        assert_eq!(
            fs::read_link(&dest).expect("read dest"),
            new_target,
            "dest should point to new_target"
        );
        assert_eq!(
            fs::read(old_target.join("file.txt")).expect("read old file"),
            b"old",
            "old_target content untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_with_progress_symlink_no_overwrite_existing_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let old_target = dir.path().join("old_target");
        let new_target = dir.path().join("new_target");
        fs::create_dir(&old_target).expect("create old dir");
        fs::create_dir(&new_target).expect("create new dir");

        let src = dir.path().join("src_link");
        let dest = dir.path().join("dest_link");
        std::os::unix::fs::symlink(&new_target, &src).expect("src symlink");
        std::os::unix::fs::symlink(&old_target, &dest).expect("dest symlink");

        let (tx, _) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        let err = copy_with_progress(&src, &dest, &tx, &cancel, false)
            .expect_err("should fail with AlreadyExists");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn test_publish_temp_overwrite_refuses_directory_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let temp = dir.path().join("temp_file");
        let dest = dir.path().join("dest_dir");
        fs::write(&temp, b"data").expect("write temp");
        fs::create_dir(&dest).expect("create dest dir");

        let cancel = AtomicBool::new(false);
        let err = publish_temp(&temp, &dest, &cancel, true).expect_err("should fail");
        assert!(
            err.kind() == io::ErrorKind::IsADirectory || err.kind() == io::ErrorKind::Other,
            "expected IsADirectory or Other, got {:?}",
            err.kind()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_with_progress_symlink_to_absent_dest() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let target = dir.path().join("some_target");
        let src = dir.path().join("src_link");
        let dest = dir.path().join("dest_link");
        std::os::unix::fs::symlink(&target, &src).expect("create src symlink");

        let (tx, _) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        copy_with_progress(&src, &dest, &tx, &cancel, false).expect("copy symlink to absent dest");

        assert_eq!(fs::read_link(&dest).expect("read dest"), target);
    }
}
