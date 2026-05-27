use std::io;
use std::path::Path;
use std::time::SystemTime;

pub mod create;
pub mod extract;
pub mod list;
pub mod sevenz;
pub mod tar;
pub mod zip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Tar,
    SevenZ,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
    pub method: String,
}

#[derive(Debug)]
pub enum ArchiveError {
    Io(io::Error),
    UnsupportedFormat,
    InvalidArchive(String),
}

impl Clone for ArchiveError {
    fn clone(&self) -> Self {
        match self {
            ArchiveError::Io(e) => ArchiveError::Io(io::Error::new(e.kind(), e.to_string())),
            ArchiveError::UnsupportedFormat => ArchiveError::UnsupportedFormat,
            ArchiveError::InvalidArchive(msg) => ArchiveError::InvalidArchive(msg.clone()),
        }
    }
}

impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        ArchiveError::Io(e)
    }
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::Io(e) => write!(f, "IO error: {e}"),
            ArchiveError::UnsupportedFormat => write!(f, "Unsupported archive format"),
            ArchiveError::InvalidArchive(msg) => write!(f, "Invalid archive: {msg}"),
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArchiveError::Io(e) => Some(e),
            ArchiveError::UnsupportedFormat | ArchiveError::InvalidArchive(_) => None,
        }
    }
}

pub fn detect_format(path: &Path) -> Result<ArchiveFormat, ArchiveError> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut header = [0u8; 8];
        if f.read(&mut header).is_ok() && header.iter().any(|&b| b != 0) {
            if header[..4] == [0x50, 0x4b, 0x03, 0x04] {
                return Ok(ArchiveFormat::Zip);
            }
            if header[..6] == [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c] {
                return Ok(ArchiveFormat::SevenZ);
            }
            if header[..2] == [0x1f, 0x8b] {
                return Ok(ArchiveFormat::TarGz);
            }
            if header[..3] == [0x42, 0x5a, 0x68] {
                return Ok(ArchiveFormat::TarBz2);
            }
            if header[..6] == [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00] {
                return Ok(ArchiveFormat::TarXz);
            }
            if header[..4] == [0x28, 0xb5, 0x2f, 0xfd] {
                return Ok(ArchiveFormat::TarZst);
            }
        }
    }

    if name.ends_with(".zip") {
        Ok(ArchiveFormat::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Ok(ArchiveFormat::TarGz)
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz") || name.ends_with(".tbz2") {
        Ok(ArchiveFormat::TarBz2)
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        Ok(ArchiveFormat::TarXz)
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        Ok(ArchiveFormat::TarZst)
    } else if name.ends_with(".tar") {
        Ok(ArchiveFormat::Tar)
    } else if name.ends_with(".7z") {
        Ok(ArchiveFormat::SevenZ)
    } else {
        Err(ArchiveError::UnsupportedFormat)
    }
}
