use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    pub name: Box<str>,
    pub size: u64,
    pub compressed_size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
    pub method: Box<str>,
}

#[derive(Debug)]
pub enum ArchiveError {
    Io(Arc<io::Error>),
    UnsupportedFormat,
    InvalidArchive(String),
}

impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        ArchiveError::Io(Arc::new(e))
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
            ArchiveError::Io(e) => Some(e.as_ref()),
            ArchiveError::UnsupportedFormat | ArchiveError::InvalidArchive(_) => None,
        }
    }
}

const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
const SEVENZ_MAGIC: [u8; 6] = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
const GZ_MAGIC: [u8; 2] = [0x1F, 0x8B];
const BZ2_MAGIC: [u8; 3] = [0x42, 0x5A, 0x68];
const XZ_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
const ZST_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

pub(crate) fn sanitize_entry_path(entry_name: &str, dest: &Path) -> Result<PathBuf, ArchiveError> {
    let entry_path = Path::new(entry_name);
    if entry_path.is_absolute() {
        return Err(ArchiveError::InvalidArchive(format!(
            "absolute path: {entry_name}"
        )));
    }
    for component in entry_path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(ArchiveError::InvalidArchive(format!(
                "path traversal: {entry_name}"
            )));
        }
    }
    let canonical_dest = dest
        .canonicalize()
        .map_err(|e| ArchiveError::Io(Arc::new(e)))?;
    let outpath = canonical_dest.join(entry_name);
    let file_name = outpath
        .file_name()
        .ok_or_else(|| ArchiveError::InvalidArchive(format!("invalid entry path: {entry_name}")))?;
    let canonical_out = outpath
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(file_name))
        .unwrap_or_else(|| outpath.clone());
    if !canonical_out.starts_with(&canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal: {entry_name}"
        )));
    }
    Ok(outpath)
}

fn has_ext_ignore_case(name: &str, ext: &str) -> bool {
    name.len() >= ext.len()
        && name
            .as_bytes()
            .iter()
            .rev()
            .zip(ext.as_bytes().iter().rev())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

pub fn detect_format(path: &Path) -> Result<ArchiveFormat, ArchiveError> {
    if path.is_file()
        && let Ok(mut f) = std::fs::File::open(path)
    {
        use std::io::Read;
        let mut header = [0u8; 8];
        if f.read_exact(&mut header).is_ok() {
            if header[..4] == ZIP_MAGIC {
                return Ok(ArchiveFormat::Zip);
            }
            if header[..6] == SEVENZ_MAGIC {
                return Ok(ArchiveFormat::SevenZ);
            }
            if header[..2] == GZ_MAGIC {
                return Ok(ArchiveFormat::TarGz);
            }
            if header[..3] == BZ2_MAGIC {
                return Ok(ArchiveFormat::TarBz2);
            }
            if header[..6] == XZ_MAGIC {
                return Ok(ArchiveFormat::TarXz);
            }
            if header[..4] == ZST_MAGIC {
                return Ok(ArchiveFormat::TarZst);
            }
        }
    }

    let name = path.file_name().unwrap_or_default().to_string_lossy();

    if has_ext_ignore_case(&name, ".zip") {
        Ok(ArchiveFormat::Zip)
    } else if has_ext_ignore_case(&name, ".tar.gz") || has_ext_ignore_case(&name, ".tgz") {
        Ok(ArchiveFormat::TarGz)
    } else if has_ext_ignore_case(&name, ".tar.bz2")
        || has_ext_ignore_case(&name, ".tbz")
        || has_ext_ignore_case(&name, ".tbz2")
    {
        Ok(ArchiveFormat::TarBz2)
    } else if has_ext_ignore_case(&name, ".tar.xz") || has_ext_ignore_case(&name, ".txz") {
        Ok(ArchiveFormat::TarXz)
    } else if has_ext_ignore_case(&name, ".tar.zst") || has_ext_ignore_case(&name, ".tzst") {
        Ok(ArchiveFormat::TarZst)
    } else if has_ext_ignore_case(&name, ".tar") {
        Ok(ArchiveFormat::Tar)
    } else if has_ext_ignore_case(&name, ".7z") {
        Ok(ArchiveFormat::SevenZ)
    } else {
        Err(ArchiveError::UnsupportedFormat)
    }
}
