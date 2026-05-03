use filetime::FileTime;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const BUFFER_SIZE: usize = 64 * 1024;

pub fn copy_with_progress(
    src: &Path,
    dest: &Path,
    progress_tx: &std::sync::mpsc::Sender<u64>,
    cancel: &AtomicBool,
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
            if let Err(err) = fs::rename(&temp_dest, dest) {
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
        total_written += bytes_read as u64;
        let _ = progress_tx.send(total_written);

        if cancel.load(Ordering::Relaxed) {
            let _ = writer.flush();
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy canceled"));
        }
    }

    writer.flush()?;

    preserve_permissions(temp_dest, metadata)?;

    Ok(total_written)
}

fn temp_path_for(dest: &Path) -> std::path::PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("copy"));
    name.push(format!(".lc-copy-{}.tmp", std::process::id()));
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

        let copied = copy_with_progress(&src, &dest, &progress_tx, &cancel).expect("copy file");

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

        let copied = copy_with_progress(&src, &dest, &progress_tx, &cancel).expect("copy file");

        assert_eq!(copied, content.len() as u64);
        assert_eq!(fs::read(&dest).expect("read dest file"), content);
        assert_eq!(
            progress_rx.try_iter().collect::<Vec<_>>(),
            vec![BUFFER_SIZE as u64, copied]
        );
    }

    #[test]
    fn cancel_before_start_returns_interrupted() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        fs::write(&src, b"content").expect("write source file");

        let (progress_tx, _progress_rx) = mpsc::channel();
        let cancel = AtomicBool::new(true);

        let err = copy_with_progress(&src, &dest, &progress_tx, &cancel).expect_err("cancel copy");

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

        let err = copy_with_progress(&src, &dest, &progress_tx, &cancel).expect_err("copy fails");

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(!dest.exists());
    }
}
