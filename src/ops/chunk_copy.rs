use filetime::FileTime;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

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

    let metadata = fs::metadata(src)?;
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
    let mut dest_file = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(dest)?);
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
    let perm_result = preserve_permissions(dest, src_metadata);
    let atime = filetime::FileTime::from_last_access_time(src_metadata);
    let mtime = filetime::FileTime::from_last_modification_time(src_metadata);
    let _ = fs::remove_file(temp_dest);
    perm_result?;
    let _ = filetime::set_file_times(dest, atime, mtime);

    Ok(())
}

fn temp_path_for(dest: &Path) -> std::path::PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("copy"));
    let tid = std::thread::current().id();
    name.push(format!(".lc-copy-{}-{:?}.tmp", std::process::id(), tid));
    dest.with_file_name(name)
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
}
