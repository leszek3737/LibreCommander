use filetime::FileTime;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::fs::symlink;

const BUFFER_SIZE: usize = 64 * 1024;

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
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        {
            if overwrite {
                let _ = fs::remove_file(dest);
            }
            symlink(&target, dest)?;
        }
        #[cfg(not(unix))]
        {
            let _ = (target, dest);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symlinks not supported on this platform",
            ));
        }
        return Ok(0);
    }

    let src_file = File::open(src)?;
    let temp_dest = temp_path_for(dest);
    let result = copy_to_temp(src_file, &temp_dest, &metadata, progress_tx, cancel);

    match result {
        Ok(total_written) => {
            if cancel.load(Ordering::Relaxed) {
                let _ = fs::remove_file(&temp_dest);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
            }
            if let Err(err) = publish_temp(&temp_dest, dest, &metadata, cancel, overwrite) {
                let _ = fs::remove_file(&temp_dest);
                return Err(err);
            }

            let accessed = FileTime::from_last_access_time(&metadata);
            let modified = FileTime::from_last_modification_time(&metadata);
            let _ = filetime::set_file_times(dest, accessed, modified);

            Ok(total_written)
        }
        Err(err) => {
            let _ = fs::remove_file(&temp_dest);
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
    let mut reader = BufReader::new(src_file);
    let mut writer = BufWriter::new(dest_file);
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut total_written = 0_u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        writer.write_all(&buffer[..bytes_read])?;
        let bytes_written = bytes_read as u64;
        total_written += bytes_written;
        let _ = progress_tx.send(bytes_written);

        if cancel.load(Ordering::Relaxed) {
            let _ = writer.flush();
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
        }
    }

    writer.flush()?;

    preserve_permissions(temp_dest, metadata)?;

    Ok(total_written)
}

fn publish_temp(
    temp_dest: &Path,
    dest: &Path,
    src_metadata: &fs::Metadata,
    cancel: &AtomicBool,
    overwrite: bool,
) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        let _ = fs::remove_file(temp_dest);
        return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
    }
    if overwrite {
        fs::rename(temp_dest, dest)?;
        return Ok(());
    }

    match fs::hard_link(temp_dest, dest) {
        Ok(()) => return fs::remove_file(temp_dest),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Err(err),
        Err(_) => {}
    }

    let mut src = BufReader::new(File::open(temp_dest)?);
    let dest_file = OpenOptions::new().write(true).create_new(true).open(dest)?;
    let result = (|| -> io::Result<()> {
        preserve_permissions(dest, src_metadata)?;
        let mut dest_file = BufWriter::new(dest_file);
        let mut buffer = [0_u8; BUFFER_SIZE];

        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(dest_file);
                let _ = fs::remove_file(dest);
                let _ = fs::remove_file(temp_dest);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
            }

            let bytes_read = src.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            dest_file.write_all(&buffer[..bytes_read])?;
        }

        dest_file.flush()?;
        dest_file.get_ref().sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result?;
    let atime = filetime::FileTime::from_last_access_time(src_metadata);
    let mtime = filetime::FileTime::from_last_modification_time(src_metadata);
    let _ = fs::remove_file(temp_dest);
    let _ = filetime::set_file_times(dest, atime, mtime);

    Ok(())
}

fn temp_path_for(dest: &Path) -> std::path::PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("copy"));
    let tid = std::thread::current().id();
    name.push(format!(
        ".lc-copy-{}-{}.tmp",
        std::process::id(),
        hash_thread_id(tid)
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
        assert_eq!(progress_rx.try_iter().collect::<Vec<_>>(), vec![11]);
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
        fs::write(temp_path_for(&dest), b"leftover").expect("write temp file");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);

        let err =
            copy_with_progress(&src, &dest, &progress_tx, &cancel, false).expect_err("copy fails");

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(!dest.exists());
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

        let new_content: Vec<u8> = (0..1_048_576).map(|i| (i % 251) as u8).collect();
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

        progress_rx.recv().expect("first progress tick");
        cancel.store(true, Ordering::Relaxed);

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
}
