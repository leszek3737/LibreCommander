use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::sevenz::extract_7z;
use super::tar::extract_tar;
use super::zip::extract_zip;
use super::{ArchiveError, ArchiveFormat, detect_format};

/// Reuses the file handle opened by `detect_format` when present, otherwise
/// opens `path` fresh. Centralizes the handle-or-open fallback shared by the
/// Zip and Tar extraction arms.
fn reuse_or_open(opt: Option<File>, path: &Path) -> io::Result<File> {
    match opt {
        Some(f) => Ok(f),
        None => File::open(path),
    }
}

pub fn extract_archive(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let (format, file_opt) = detect_format(path)?;
    match format {
        ArchiveFormat::Zip => {
            let file = reuse_or_open(file_opt, path)?;
            extract_zip(file, dest, progress, cancel)
        }
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => {
            let file = reuse_or_open(file_opt, path)?;
            extract_tar(file, dest, format, progress, cancel)
        }
        ArchiveFormat::SevenZ => {
            // sevenz_rust reader API requires a path — cannot reuse the file handle
            drop(file_opt);
            extract_7z(path, dest, progress, cancel)
        }
    }
}
