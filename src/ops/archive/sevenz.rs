use std::fs::{self, File};
use std::io::{self, BufWriter, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use super::{ArchiveEntry, ArchiveError};

const MAX_LIST_ENTRIES: usize = 100_000;

pub fn list_7z(path: &Path) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let archive = sevenz_rust::Archive::open(path)
        .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

    let mut entries = Vec::new();
    for entry in &archive.files {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        entries.push(ArchiveEntry {
            name: entry.name().to_string(),
            size: entry.size(),
            compressed_size: entry.compressed_size,
            modified: if entry.has_last_modified_date {
                Some(entry.last_modified_date.into())
            } else {
                None
            },
            is_dir: entry.is_directory(),
            method: if entry.has_stream {
                "Compressed".to_string()
            } else {
                String::new()
            },
        });
    }
    Ok(entries)
}

fn sanitize_entry_path(entry_name: &str, dest: &Path) -> Result<std::path::PathBuf, ArchiveError> {
    let entry_path = Path::new(entry_name);
    if entry_path.is_absolute() {
        return Err(ArchiveError::InvalidArchive(format!(
            "absolute path detected: {entry_name}"
        )));
    }
    // Reject any entry containing parent-directory components
    for component in entry_path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(ArchiveError::InvalidArchive(format!(
                "path traversal detected: {entry_name}"
            )));
        }
    }
    let outpath = dest.join(entry_name);
    let canonical_dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    let canonical_out = outpath
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(outpath.file_name().unwrap_or_default()))
        .unwrap_or_else(|| outpath.clone());

    if !canonical_out.starts_with(&canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal detected: {entry_name}"
        )));
    }
    Ok(outpath)
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
    let dest_owned = dest.to_path_buf();
    let cancel_owned = cancel;
    let progress_owned = progress.clone();

    let mut extracted_paths: Vec<std::path::PathBuf> = Vec::new();
    let result = (|| -> Result<(), ArchiveError> {
        let mut reader = sevenz_rust::SevenZReader::open(path, sevenz_rust::Password::empty())
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

        reader
            .for_each_entries(|entry, reader| {
                if cancel_owned.load(Ordering::Relaxed) {
                    return Err(Canceled.into());
                }

                let outpath = match sanitize_entry_path(entry.name(), &dest_owned) {
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
                    let bytes_written =
                        read_with_cancel(reader, &mut writer, cancel_owned, entry.size())
                            .map_err(sevenz_rust::Error::io)?;
                    extracted_paths.push(outpath);

                    let _ = progress_owned.send(bytes_written);
                }

                Ok(true)
            })
            .map_err(|e| {
                if cancel_owned.load(Ordering::Relaxed) {
                    ArchiveError::Io(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Operation canceled",
                    ))
                } else {
                    ArchiveError::InvalidArchive(e.to_string())
                }
            })?;
        Ok(())
    })();

    if result.is_err() {
        for p in extracted_paths.iter().rev() {
            if p.is_dir() {
                let _ = fs::remove_dir(p);
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
    declared_size: u64,
) -> io::Result<u64> {
    let mut buf = [0u8; 8192];
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            ));
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        total = total.saturating_add(n as u64);
        if declared_size > 0 && total >= declared_size {
            break;
        }
    }
    writer.flush()?;
    Ok(total)
}

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
