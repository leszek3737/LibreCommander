use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use super::sevenz::create_7z;
use super::tar::create_tar;
use super::zip::create_zip;
use super::{ArchiveError, ArchiveFormat};

pub fn create_archive(
    sources: &[PathBuf],
    dest: &Path,
    format: ArchiveFormat,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    match format {
        ArchiveFormat::Zip => create_zip(sources, dest, progress, cancel),
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => create_tar(sources, dest, format, progress, cancel),
        ArchiveFormat::SevenZ => create_7z(sources, dest, progress, cancel),
    }
}
