use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use super::{
    ArchiveEntry, ArchiveError, MAX_FILE_SIZE, MAX_LIST_ENTRIES, cleanup_extracted,
    copy_with_progress,
};

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
    let mut extracted_paths: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<(), ArchiveError> {
        fs::create_dir_all(dest)?;
        let mut reader = sevenz_rust::SevenZReader::open(path, sevenz_rust::Password::empty())
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

        let mut last_parent: Option<PathBuf> = None;
        reader
            .for_each_entries(|entry, reader| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Canceled.into());
                }

                let outpath = match super::sanitize_entry_path(entry.name(), dest) {
                    Ok(p) => p,
                    Err(e) => return Err(PathTraversal(e.to_string()).into()),
                };

                if entry.size() > MAX_FILE_SIZE {
                    return Err(PathTraversal(format!(
                        "entry '{}' size {} exceeds maximum {MAX_FILE_SIZE}",
                        entry.name(),
                        entry.size()
                    ))
                    .into());
                }

                if entry.is_directory() {
                    fs::create_dir_all(&outpath).map_err(sevenz_rust::Error::io)?;
                    extracted_paths.push(outpath);
                } else {
                    if let Some(parent) = outpath.parent()
                        && last_parent.as_deref() != Some(parent)
                    {
                        fs::create_dir_all(parent).map_err(sevenz_rust::Error::io)?;
                        last_parent = Some(parent.to_path_buf());
                    }
                    let mut outfile = File::create(&outpath).map_err(sevenz_rust::Error::io)?;
                    copy_with_progress(reader, &mut outfile, progress)
                        .map_err(sevenz_rust::Error::io)?;
                    extracted_paths.push(outpath);
                }

                Ok(true)
            })
            .map_err(|e| {
                if cancel.load(Ordering::Relaxed) {
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
        cleanup_extracted(&extracted_paths);
    }

    result
}

/// 7z archive creation is not supported.
///
/// The `sevenz_rust` crate provides read-only access to 7z archives.
/// Use zip or tar format for creating archives instead.
pub fn create_7z(
    _sources: &[PathBuf],
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
