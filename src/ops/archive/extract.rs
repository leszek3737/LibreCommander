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
    let format = detect_format(path)?;
    match format {
        ArchiveFormat::Zip => extract_zip(path, dest, progress, cancel),
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => extract_tar(path, dest, format, progress, cancel),
        ArchiveFormat::SevenZ => extract_7z(path, dest, progress, cancel),
    }
}
