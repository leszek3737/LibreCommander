use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::sevenz::extract_7z;
use super::tar::extract_tar;
use super::zip::extract_zip;
use super::{ArchiveError, ArchiveFormat, detect_format};

pub fn extract_archive(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let (format, file_opt) = detect_format(path)?;
    match format {
        ArchiveFormat::Zip => {
            let file = match file_opt {
                Some(f) => f,
                None => std::fs::File::open(path)?,
            };
            extract_zip(file, dest, progress, cancel)
        }
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => {
            let file = match file_opt {
                Some(f) => f,
                None => std::fs::File::open(path)?,
            };
            extract_tar(file, dest, format, progress, cancel)
        }
        ArchiveFormat::SevenZ => {
            // sevenz_rust reader API requires a path — cannot reuse the file handle
            drop(file_opt);
            extract_7z(path, dest, progress, cancel)
        }
    }
}
