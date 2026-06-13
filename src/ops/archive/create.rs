use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use crate::debug_log;

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
    let non_symlink_sources: Vec<PathBuf> = sources
        .iter()
        .filter(|s| {
            if super::is_symlink_source(s) {
                debug_log!("create_archive: skipping symlink {}", s.display());
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();

    if non_symlink_sources.is_empty() {
        return Err(ArchiveError::NoValidSources);
    }

    match format {
        ArchiveFormat::Zip => create_zip(&non_symlink_sources, dest, progress, cancel),
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => create_tar(&non_symlink_sources, dest, format, progress, cancel),
        ArchiveFormat::SevenZ => create_7z(&non_symlink_sources, dest, progress, cancel),
    }
}
