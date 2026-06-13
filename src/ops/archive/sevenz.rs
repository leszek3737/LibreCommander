use std::cell::Cell;
use std::fs::{self};
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
                // NOTE: `Box::<str>::from("Compressed")` still heap-allocates this
                // constant string. Avoiding it would require changing the shared
                // `ArchiveEntry::method` field type (in mod.rs) to something that can
                // hold a `&'static str` (e.g. `Cow<'static, str>`), because tar.rs
                // stores a dynamically built name there. Not worth the churn for now.
                Box::<str>::from("Compressed")
            } else {
                Box::<str>::default()
            },
        });
    }
    Ok(entries)
}

#[derive(Debug)]
enum SevenzExtractError {
    Canceled,
    PathTraversal(PathBuf),
    Io(io::Error),
}

struct SevenzEntryExtractor<'a> {
    canonical_dest: &'a Path,
    progress: &'a Sender<u64>,
    cancel: &'a AtomicBool,
    error_slot: &'a Cell<Option<SevenzExtractError>>,
    last_parent: Option<PathBuf>,
    total_size: super::TotalSizeGuard,
    extracted_paths: &'a mut Vec<PathBuf>,
}

impl<'a> SevenzEntryExtractor<'a> {
    fn verify_inside(&self, p: &Path) -> Result<(), sevenz_rust::Error> {
        super::verify_within_dest(self.canonical_dest, p).map_err(|e| {
            self.error_slot
                .set(Some(SevenzExtractError::PathTraversal(p.to_path_buf())));
            sevenz_rust::Error::Other(e.to_string().into())
        })
    }

    fn create_parent_if_needed(&mut self, outpath: &Path) -> Result<(), sevenz_rust::Error> {
        if let Some(parent) = outpath.parent()
            && self.last_parent.as_deref() != Some(parent)
        {
            if let Err(e) = fs::create_dir_all(parent) {
                self.error_slot.set(Some(SevenzExtractError::Io(e)));
                return Err(sevenz_rust::Error::Other("create_dir_all failed".into()));
            }
            self.verify_inside(parent)?;
            self.last_parent = Some(parent.to_path_buf());
        }
        Ok(())
    }

    fn process_entry(
        &mut self,
        entry: &sevenz_rust::SevenZArchiveEntry,
        reader: &mut dyn io::Read,
    ) -> Result<bool, sevenz_rust::Error> {
        if self.cancel.load(Ordering::Relaxed) {
            self.error_slot.set(Some(SevenzExtractError::Canceled));
            return Err(sevenz_rust::Error::Other("Operation canceled".into()));
        }

        let outpath = match super::sanitize_entry_path(self.canonical_dest, entry.name()) {
            Ok(p) => p,
            Err(e) => {
                self.error_slot
                    .set(Some(SevenzExtractError::PathTraversal(PathBuf::from(
                        entry.name(),
                    ))));
                return Err(sevenz_rust::Error::Other(e.to_string().into()));
            }
        };

        if entry.size() > MAX_FILE_SIZE {
            return Err(sevenz_rust::Error::Other(
                format!(
                    "entry '{}' size {} exceeds maximum {MAX_FILE_SIZE}",
                    entry.name(),
                    entry.size()
                )
                .into(),
            ));
        }

        if entry.is_directory() {
            if let Err(e) = fs::create_dir_all(&outpath) {
                self.error_slot.set(Some(SevenzExtractError::Io(e)));
                return Err(sevenz_rust::Error::Other("create_dir_all failed".into()));
            }
            self.verify_inside(&outpath)?;
            self.extracted_paths.push(outpath);
        } else {
            self.create_parent_if_needed(&outpath)?;
            super::check_symlink_at_dest(&outpath)
                .map_err(|e| sevenz_rust::Error::Other(e.to_string().into()))?;
            let mut outfile = match super::open_outfile(&outpath) {
                Ok(f) => f,
                Err(e) => {
                    self.error_slot.set(Some(SevenzExtractError::Io(e)));
                    return Err(sevenz_rust::Error::Other("open_outfile failed".into()));
                }
            };
            match copy_with_progress(reader, &mut outfile, self.progress, self.cancel) {
                Ok(written) => {
                    self.total_size
                        .add(written)
                        .map_err(|e| sevenz_rust::Error::Other(e.to_string().into()))?;
                    self.extracted_paths.push(outpath);
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted && self.cancel.load(Ordering::Relaxed)
                    {
                        self.error_slot.set(Some(SevenzExtractError::Canceled));
                    } else {
                        self.error_slot.set(Some(SevenzExtractError::Io(e)));
                    }
                    return Err(sevenz_rust::Error::Other("copy failed".into()));
                }
            }
        }

        // `Ok(true)` resumes iteration; `Ok(false)` aborts (sevenz_rust contract).
        Ok(true)
    }
}

fn translate_extract_error(
    err: &sevenz_rust::Error,
    slot: Option<SevenzExtractError>,
    cancel: &AtomicBool,
) -> ArchiveError {
    match slot {
        Some(SevenzExtractError::PathTraversal(p)) => {
            ArchiveError::InvalidArchive(format!("path traversal: {}", p.display()))
        }
        Some(SevenzExtractError::Io(e)) => ArchiveError::Io(e),
        Some(SevenzExtractError::Canceled) => ArchiveError::Io(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation canceled",
        )),
        None => {
            if cancel.load(Ordering::Relaxed) {
                ArchiveError::Io(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation canceled",
                ))
            } else {
                ArchiveError::InvalidArchive(err.to_string())
            }
        }
    }
}

pub fn extract_7z(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    // Pre-size for a typical small/medium archive to avoid early re-allocations;
    // the Vec still grows for larger archives.
    const EXTRACTED_PATHS_HINT: usize = 64;
    let mut extracted_paths: Vec<PathBuf> = Vec::with_capacity(EXTRACTED_PATHS_HINT);
    let result = (|| -> Result<(), ArchiveError> {
        fs::create_dir_all(dest)?;
        let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
        let mut reader = sevenz_rust::SevenZReader::open(path, sevenz_rust::Password::empty())
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

        let error_slot: Cell<Option<SevenzExtractError>> = Cell::new(None);
        let mut extractor = SevenzEntryExtractor {
            canonical_dest: &canonical_dest,
            progress,
            cancel,
            error_slot: &error_slot,
            last_parent: None,
            total_size: super::TotalSizeGuard::default(),
            extracted_paths: &mut extracted_paths,
        };

        reader
            .for_each_entries(|entry, reader| extractor.process_entry(entry, reader))
            .map_err(|e| translate_extract_error(&e, error_slot.take(), cancel))?;
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
        let canonical = dest.canonicalize().unwrap();
        let result = super::super::sanitize_entry_path(&canonical, "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_rejects_parent_dir() {
        let dest = PathBuf::from("/tmp");
        let canonical = dest.canonicalize().unwrap();
        let result = super::super::sanitize_entry_path(&canonical, "../passwd");
        assert!(result.is_err());
    }
}
