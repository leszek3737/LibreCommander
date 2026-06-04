use std::fs::{self, File};
use std::io::{self, BufWriter, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use super::{ArchiveEntry, ArchiveError};

const MAX_LIST_ENTRIES: usize = 100_000;
const READ_BUF_SIZE: usize = 65536;

pub fn list_7z(path: &Path) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let archive = sevenz_rust::Archive::open(path)
        .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

    let capacity = archive.files.len().min(MAX_LIST_ENTRIES);
    let mut entries = Vec::with_capacity(capacity);
    for entry in &archive.files {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        entries.push(ArchiveEntry {
            name: entry.name().to_string().into_boxed_str(),
            size: entry.size(),
            compressed_size: entry.compressed_size,
            modified: if entry.has_last_modified_date {
                Some(entry.last_modified_date.into())
            } else {
                None
            },
            is_dir: entry.is_directory(),
            method: if entry.has_stream {
                Box::<str>::from("Compressed")
            } else {
                Box::<str>::default()
            },
        });
    }
    Ok(entries)
}

struct Canceled;

impl From<Canceled> for sevenz_rust::Error {
    fn from(_: Canceled) -> Self {
        sevenz_rust::Error::Other("Operation canceled".into())
    }
}

struct PathTraversal(String);

impl From<PathTraversal> for sevenz_rust::Error {
    fn from(e: PathTraversal) -> Self {
        sevenz_rust::Error::Other(e.0.into())
    }
}

pub fn extract_7z(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut extracted_paths: Vec<std::path::PathBuf> = Vec::new();
    let result = (|| -> Result<(), ArchiveError> {
        fs::create_dir_all(dest)?;
        let mut reader = sevenz_rust::SevenZReader::open(path, sevenz_rust::Password::empty())
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

        reader
            .for_each_entries(|entry, reader| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Canceled.into());
                }

                let outpath = match super::sanitize_entry_path(entry.name(), dest) {
                    Ok(p) => p,
                    Err(e) => return Err(PathTraversal(e.to_string()).into()),
                };

                if entry.is_directory() {
                    fs::create_dir_all(&outpath).map_err(sevenz_rust::Error::io)?;
                    extracted_paths.push(outpath);
                } else {
                    if let Some(parent) = outpath.parent() {
                        fs::create_dir_all(parent).map_err(sevenz_rust::Error::io)?;
                    }
                    let file = File::create(&outpath).map_err(sevenz_rust::Error::io)?;
                    let mut writer = BufWriter::new(file);
                    let mut buf = [0u8; READ_BUF_SIZE];
                    let _bytes_written =
                        read_with_cancel(reader, &mut writer, cancel, progress, &mut buf)
                            .map_err(sevenz_rust::Error::io)?;
                    extracted_paths.push(outpath);
                }

                Ok(true)
            })
            .map_err(|e| {
                if cancel.load(Ordering::Relaxed) {
                    ArchiveError::Io(Arc::new(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Operation canceled",
                    )))
                } else {
                    ArchiveError::InvalidArchive(e.to_string())
                }
            })?;
        Ok(())
    })();

    if result.is_err() {
        for p in extracted_paths.iter().rev() {
            if p.is_dir() {
                let _ = fs::remove_dir_all(p);
            } else {
                let _ = fs::remove_file(p);
            }
        }
    }

    result
}

fn read_with_cancel(
    reader: &mut dyn Read,
    writer: &mut dyn io::Write,
    cancel: &AtomicBool,
    progress: &Sender<u64>,
    buf: &mut [u8],
) -> io::Result<u64> {
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            ));
        }
        let n = match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        total = total.saturating_add(n as u64);
        let _ = progress.send(n as u64);
    }
    writer.flush()?;
    Ok(total)
}

/// 7z archive creation is not supported.
///
/// The `sevenz_rust` crate provides read-only access to 7z archives.
/// Use zip or tar format for creating archives instead.
pub fn create_7z(
    _sources: &[std::path::PathBuf],
    _dest: &Path,
    _progress: &Sender<u64>,
    _cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    Err(ArchiveError::InvalidArchive(
        "7z archive creation is not supported. Use zip or tar format instead.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;

    #[test]
    fn create_7z_returns_unsupported() {
        let (tx, _rx) = mpsc::channel();
        let result = create_7z(
            &[],
            PathBuf::from("test.7z").as_path(),
            &tx,
            &AtomicBool::new(false),
        );
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_rejects_absolute_path() {
        let dest = PathBuf::from("/tmp");
        let result = super::super::sanitize_entry_path("/etc/passwd", &dest);
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_rejects_parent_dir() {
        let dest = PathBuf::from("/tmp");
        let result = super::super::sanitize_entry_path("../passwd", &dest);
        assert!(result.is_err());
    }
}
