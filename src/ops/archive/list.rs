use std::path::Path;

use super::sevenz::list_7z;
use super::tar::list_tar;
use super::zip::list_zip;
use super::{ArchiveEntry, ArchiveError, ArchiveFormat, detect_format};

/// List entries in an archive file.
///
/// Detects the archive format from magic bytes or file extension,
/// then returns metadata for every entry (name, size, is_dir, etc.).
///
/// # Errors
///
/// Returns [`ArchiveError::UnsupportedFormat`] if the format cannot be determined,
/// [`ArchiveError::InvalidArchive`] for corrupt archives, or [`ArchiveError::Io`]
/// on filesystem errors.
pub fn list_archive(path: &Path) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let format = detect_format(path)?;
    match format {
        ArchiveFormat::Zip => list_zip(path),
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst => list_tar(path, format),
        ArchiveFormat::SevenZ => list_7z(path),
    }
}
