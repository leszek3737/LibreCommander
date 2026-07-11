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
    InvalidArchive(String),
    Io(io::Error),
}

struct SevenzEntryExtractor<'a> {
    canonical_dest: &'a Path,
    progress: &'a Sender<u64>,
    cancel: &'a AtomicBool,
    error_slot: &'a Cell<Option<SevenzExtractError>>,
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
        // Re-verify the parent on EVERY entry rather than caching the last one: a
        // cached parent could be swapped for a symlink between entries (TOCTOU),
        // and skipping `verify_inside` on a cache hit would let a later entry be
        // written outside `canonical_dest`.
        if let Some(parent) = outpath.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                self.error_slot.set(Some(SevenzExtractError::Io(e)));
                return Err(sevenz_rust::Error::Other("create_dir_all failed".into()));
            }
            self.verify_inside(parent)?;
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

        let outpath = match super::sanitize_entry_path(self.canonical_dest, Path::new(entry.name()))
        {
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
            // Only track directories THIS operation actually creates, so a
            // rollback never `remove_dir_all`s a pre-existing user directory that
            // `create_dir_all` merely succeeded on idempotently.
            let newly_created = fs::symlink_metadata(&outpath).is_err();
            if let Err(e) = fs::create_dir_all(&outpath) {
                self.error_slot.set(Some(SevenzExtractError::Io(e)));
                return Err(sevenz_rust::Error::Other("create_dir_all failed".into()));
            }
            self.verify_inside(&outpath)?;
            if newly_created {
                self.extracted_paths.push(outpath);
            }
        } else {
            self.create_parent_if_needed(&outpath)?;
            super::check_symlink_at_dest(&outpath).map_err(|e| {
                self.error_slot
                    .set(Some(SevenzExtractError::InvalidArchive(e.to_string())));
                sevenz_rust::Error::Other("symlink check failed".into())
            })?;
            let mut outfile = match super::open_outfile(&outpath) {
                Ok(f) => f,
                Err(e) => {
                    self.error_slot.set(Some(SevenzExtractError::Io(e)));
                    return Err(sevenz_rust::Error::Other("open_outfile failed".into()));
                }
            };
            // Register the file for rollback BEFORE copying: a mid-copy failure
            // (cancel / size-limit / IO error) must still clean up the partial
            // file, which pushing only after the copy would miss.
            self.extracted_paths.push(outpath);
            match copy_with_progress(reader, &mut outfile, self.progress, self.cancel) {
                Ok(written) => {
                    self.total_size.add(written).map_err(|e| {
                        self.error_slot
                            .set(Some(SevenzExtractError::InvalidArchive(e.to_string())));
                        sevenz_rust::Error::Other("total size limit exceeded".into())
                    })?;
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
        Some(SevenzExtractError::InvalidArchive(msg)) => ArchiveError::InvalidArchive(msg),
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
        let result = super::super::sanitize_entry_path(&canonical, Path::new("/etc/passwd"));
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_rejects_parent_dir() {
        let dest = PathBuf::from("/tmp");
        let canonical = dest.canonicalize().unwrap();
        let result = super::super::sanitize_entry_path(&canonical, Path::new("../passwd"));
        assert!(result.is_err());
    }

    // Real-archive round trip: `sevenz_rust` can WRITE (LZMA2) so we build a
    // genuine `.7z` fixture in-test rather than embedding opaque bytes. This
    // exercises `extract_7z` -> `for_each_entries` -> `process_entry` end to end,
    // including nested directories. Traversal/symlink/cancel/rollback below drive
    // `process_entry` directly with synthetic entries because a filesystem-sourced
    // fixture cannot carry `../` or symlink-escape names.
    #[test]
    fn extract_7z_roundtrip_preserves_contents() {
        let work = tempfile::tempdir().unwrap();
        let src = work.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"alpha").unwrap();
        let sub = src.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.txt"), b"beta").unwrap();

        let archive = work.path().join("out.7z");
        sevenz_rust::compress_to_path(&src, &archive).unwrap();

        let dest = work.path().join("extract");
        let (tx, _rx) = mpsc::channel();
        extract_7z(&archive, &dest, &tx, &AtomicBool::new(false)).unwrap();

        assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(fs::read(dest.join("sub").join("b.txt")).unwrap(), b"beta");
    }

    /// Builds an extractor over `canonical_dest` sharing `extracted`/`slot`.
    /// Inlined per test elsewhere would repeat five borrows; this keeps the
    /// synthetic-entry tests readable.
    fn run_entry(
        canonical_dest: &Path,
        slot: &Cell<Option<SevenzExtractError>>,
        cancel: &AtomicBool,
        extracted: &mut Vec<PathBuf>,
        entry: &sevenz_rust::SevenZArchiveEntry,
        reader: &mut dyn io::Read,
    ) -> Result<bool, sevenz_rust::Error> {
        let (tx, _rx) = mpsc::channel();
        let mut extractor = SevenzEntryExtractor {
            canonical_dest,
            progress: &tx,
            cancel,
            error_slot: slot,
            total_size: super::super::TotalSizeGuard::default(),
            extracted_paths: extracted,
        };
        extractor.process_entry(entry, reader)
    }

    fn file_entry(name: &str, size: u64) -> sevenz_rust::SevenZArchiveEntry {
        let mut entry = sevenz_rust::SevenZArchiveEntry::new();
        entry.name = name.to_string();
        entry.has_stream = true;
        entry.size = size;
        entry
    }

    #[test]
    fn process_entry_rejects_path_traversal() {
        let dest = tempfile::tempdir().unwrap();
        let canonical = dest.path().canonicalize().unwrap();
        let slot = Cell::new(None);
        let cancel = AtomicBool::new(false);
        let mut extracted = Vec::new();
        let entry = file_entry("../escape.txt", 3);
        let mut reader = io::Cursor::new(b"abc".to_vec());
        let res = run_entry(
            &canonical,
            &slot,
            &cancel,
            &mut extracted,
            &entry,
            &mut reader,
        );
        assert!(res.is_err());
        assert!(matches!(
            slot.take(),
            Some(SevenzExtractError::PathTraversal(_))
        ));
        assert!(extracted.is_empty());
    }

    #[test]
    fn process_entry_cancel_aborts() {
        let dest = tempfile::tempdir().unwrap();
        let canonical = dest.path().canonicalize().unwrap();
        let slot = Cell::new(None);
        let cancel = AtomicBool::new(true);
        let mut extracted = Vec::new();
        let entry = file_entry("f.txt", 3);
        let mut reader = io::Cursor::new(b"abc".to_vec());
        let res = run_entry(
            &canonical,
            &slot,
            &cancel,
            &mut extracted,
            &entry,
            &mut reader,
        );
        assert!(res.is_err());
        assert!(matches!(slot.take(), Some(SevenzExtractError::Canceled)));
    }

    // P0.1: a directory entry that already exists on disk must NOT be scheduled
    // for rollback, so a later failure never `remove_dir_all`s a user's
    // pre-existing directory (and its unrelated contents).
    #[test]
    fn rollback_preserves_preexisting_dir() {
        let dest = tempfile::tempdir().unwrap();
        let canonical = dest.path().canonicalize().unwrap();
        let keep = canonical.join("keep");
        fs::create_dir(&keep).unwrap();
        let data = keep.join("data.txt");
        fs::write(&data, b"precious").unwrap();

        let slot = Cell::new(None);
        let cancel = AtomicBool::new(false);
        let mut extracted = Vec::new();
        let mut entry = sevenz_rust::SevenZArchiveEntry::new();
        entry.name = "keep".to_string();
        entry.is_directory = true;
        let mut empty = io::Cursor::new(Vec::new());
        run_entry(
            &canonical,
            &slot,
            &cancel,
            &mut extracted,
            &entry,
            &mut empty,
        )
        .unwrap();

        assert!(
            !extracted.contains(&keep),
            "pre-existing dir must not be tracked for rollback"
        );
        // Simulate the failure-path rollback; pre-existing data must survive.
        super::super::cleanup_extracted(&extracted);
        assert!(data.exists(), "rollback deleted pre-existing data");
    }

    #[cfg(unix)]
    #[test]
    fn process_entry_rejects_symlink_at_dest() {
        let dest = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canonical = dest.path().canonicalize().unwrap();
        // Pre-plant a symlink where the entry would be written.
        std::os::unix::fs::symlink(outside.path().join("target"), canonical.join("f.txt")).unwrap();

        let slot = Cell::new(None);
        let cancel = AtomicBool::new(false);
        let mut extracted = Vec::new();
        let entry = file_entry("f.txt", 3);
        let mut reader = io::Cursor::new(b"abc".to_vec());
        let res = run_entry(
            &canonical,
            &slot,
            &cancel,
            &mut extracted,
            &entry,
            &mut reader,
        );
        assert!(res.is_err());
        assert!(
            !outside.path().join("target").exists(),
            "extraction escaped through a pre-planted symlink"
        );
    }
}
