use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

pub mod create;
pub mod extract;
pub mod list;
pub mod sevenz;
pub mod tar;
pub mod zip;

pub(crate) const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB
#[allow(dead_code)]
pub(crate) const MAX_TOTAL_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024 * 1024; // 256 GiB
pub(crate) const MAX_LIST_ENTRIES: usize = 100_000;

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
    Io(io::Error),
    UnsupportedFormat,
    InvalidArchive(String),
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

const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
const SEVENZ_MAGIC: [u8; 6] = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
const GZ_MAGIC: [u8; 2] = [0x1F, 0x8B];
const BZ2_MAGIC: [u8; 3] = [0x42, 0x5A, 0x68];
const XZ_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
const ZST_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const TAR_BLOCK_SIZE: usize = 512;
const USTAR_MAGIC_OFFSET: usize = 257;
const USTAR_MAGIC: &[u8] = b"ustar";

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<(), ArchiveError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ArchiveError::Io(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation canceled",
        )));
    }
    Ok(())
}

pub(crate) fn copy_with_progress(
    reader: &mut dyn Read,
    writer: &mut dyn io::Write,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    let mut buf = [0u8; 65536];
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            ));
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        total = total.saturating_add(n as u64);
        let _ = progress.send(n as u64);
    }
    writer.flush()?;
    Ok(total)
}

pub(crate) fn cleanup_extracted(paths: &[PathBuf]) {
    for p in paths.iter().rev() {
        if p.is_dir() {
            let _ = fs::remove_dir_all(p);
        } else {
            let _ = fs::remove_file(p);
        }
    }
}

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
    let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
    let outpath = canonical_dest.join(entry_name);

    let normalized_out = normalize_path(&outpath);
    if !normalized_out.starts_with(&canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal: {entry_name}"
        )));
    }
    Ok(outpath)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(comp) => result.push(comp),
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::RootDir => {
                result.push(component);
            }
            _ => {}
        }
    }
    result
}

fn has_ext_ignore_case(name: &str, ext: &str) -> bool {
    name.len() >= ext.len()
        && name.as_bytes()[name.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
}

fn verify_tar_inside(file: &mut fs::File, format: ArchiveFormat) -> bool {
    match format {
        ArchiveFormat::TarGz => {
            let mut reader = flate2::read::GzDecoder::new(file);
            let mut buf = [0u8; TAR_BLOCK_SIZE];
            if reader.read_exact(&mut buf).is_err() {
                return false;
            }
            buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
        }
        ArchiveFormat::TarBz2 => {
            let mut reader = bzip2::read::BzDecoder::new(file);
            let mut buf = [0u8; TAR_BLOCK_SIZE];
            if reader.read_exact(&mut buf).is_err() {
                return false;
            }
            buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
        }
        ArchiveFormat::TarXz => {
            let mut buf = [0u8; TAR_BLOCK_SIZE];
            let mut cursor = io::Cursor::new(&mut buf[..]);
            let mut buf_reader = io::BufReader::new(file);
            let _ = lzma_rs::xz_decompress(&mut buf_reader, &mut cursor);
            cursor.position() as usize >= TAR_BLOCK_SIZE
                && buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
        }
        ArchiveFormat::TarZst => {
            let mut reader = match zstd::stream::read::Decoder::new(file) {
                Ok(r) => r,
                Err(_) => return false,
            };
            let mut buf = [0u8; TAR_BLOCK_SIZE];
            if reader.read_exact(&mut buf).is_err() {
                return false;
            }
            buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
        }
        _ => false,
    }
}

pub fn detect_format(path: &Path) -> Result<ArchiveFormat, ArchiveError> {
    if path.is_file()
        && let Ok(mut f) = std::fs::File::open(path)
    {
        let mut header = [0u8; 8];
        if f.read_exact(&mut header).is_ok() {
            if header[..4] == ZIP_MAGIC {
                return Ok(ArchiveFormat::Zip);
            }
            if header[..6] == SEVENZ_MAGIC {
                return Ok(ArchiveFormat::SevenZ);
            }
            if header[..2] == GZ_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarGz) {
                return Ok(ArchiveFormat::TarGz);
            }
            if header[..3] == BZ2_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarBz2) {
                return Ok(ArchiveFormat::TarBz2);
            }
            if header[..6] == XZ_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarXz) {
                return Ok(ArchiveFormat::TarXz);
            }
            if header[..4] == ZST_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarZst) {
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
