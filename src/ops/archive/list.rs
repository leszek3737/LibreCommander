use std::path::Path;

use super::sevenz::list_7z;
use super::tar::list_tar;
use super::zip::list_zip;
use super::{ArchiveEntry, ArchiveError, ArchiveFormat, detect_format};

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
