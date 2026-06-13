use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use crate::debug_log;

pub mod create;
pub mod extract;
pub mod list;
pub mod sevenz;
pub mod tar;
pub mod zip;

pub(crate) const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB
pub(crate) const MAX_TOTAL_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024 * 1024; // 256 GiB
pub(crate) const MAX_LIST_ENTRIES: usize = 100_000;
pub(crate) const IO_BUFFER_SIZE: usize = 64 * 1024;

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
    NoValidSources,
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
            ArchiveError::NoValidSources => {
                write!(f, "No valid sources (all was symlinks or inaccessible)")
            }
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArchiveError::Io(e) => Some(e),
            ArchiveError::UnsupportedFormat
            | ArchiveError::InvalidArchive(_)
            | ArchiveError::NoValidSources => None,
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

static ARCHIVE_TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Creates a new temporary file with a unique name in the system temp directory.
/// Used by archive handlers for intermediate decompression buffers.
pub(crate) fn create_archive_temp_file(
    prefix: &str,
    suffix: &str,
) -> io::Result<(fs::File, PathBuf)> {
    let pid = std::process::id();
    for _ in 0..128 {
        let count = ARCHIVE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{pid}-{count}{suffix}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to create unique archive temp file after 128 attempts",
    ))
}

/// Checks whether `path` is already a symlink and returns an error if so.
/// Called before writing each archive entry to prevent extraction into pre-planted symlinks.
pub(crate) fn check_symlink_at_dest(path: &Path) -> Result<(), ArchiveError> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(ArchiveError::InvalidArchive(format!(
            "refusing to extract into existing symlink: {}",
            path.display()
        )));
    }
    Ok(())
}

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
    let mut buf = [0u8; IO_BUFFER_SIZE];
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
        // best-effort: receiver may have dropped
        let _ = progress.send(n as u64);
    }
    writer.flush()?;
    Ok(total)
}

pub(crate) fn cleanup_extracted(paths: &[PathBuf]) {
    for p in paths.iter().rev() {
        let is_dir = fs::symlink_metadata(p)
            .map(|m| m.is_dir() && !m.file_type().is_symlink())
            .unwrap_or(false);
        let result = if is_dir {
            fs::remove_dir_all(p)
        } else {
            fs::remove_file(p)
        };
        if let Err(e) = &result {
            debug_log!("cleanup_extracted: failed to remove {}: {e}", p.display());
        }
    }
}

pub(crate) fn open_outfile(path: &Path) -> io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    opts.open(path)
}

/// Single source of truth for the create-side symlink decision: returns `true`
/// when `path` is a symlink, so callers skip it rather than following it into a
/// location chosen by an attacker. Metadata errors (e.g. the path vanished) are
/// treated as "not a symlink" — the subsequent open is what ultimately fails or
/// is hardened with `O_NOFOLLOW`.
pub(crate) fn is_symlink_source(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Opens a top-level source file for archive creation. On Unix the file is
/// opened with `O_NOFOLLOW`, closing the TOCTOU window between the create-side
/// symlink filter (`is_symlink_source`) and this open: if the final component
/// has been swapped for a symlink, `open()` fails with `ELOOP` and we return
/// `Ok(None)`, signalling the caller to skip the source consistently with the
/// filter policy. Non-Unix platforms keep the plain open fallback.
pub(crate) fn open_source_nofollow(path: &Path) -> Result<Option<fs::File>, ArchiveError> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    match opts.open(path) {
        Ok(file) => Ok(Some(file)),
        #[cfg(unix)]
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            debug_log!(
                "open_source_nofollow: skipping symlinked source {}",
                path.display()
            );
            Ok(None)
        }
        Err(e) => Err(ArchiveError::Io(e)),
    }
}

pub(crate) fn sanitize_entry_path(
    canonical_dest: &Path,
    entry_name: &str,
) -> Result<PathBuf, ArchiveError> {
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
    let outpath = canonical_dest.join(entry_name);

    let normalized_out = normalize_path(&outpath);
    if !normalized_out.starts_with(canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal: {entry_name}"
        )));
    }
    Ok(outpath)
}

/// Verifies that `path` (which must already exist), after resolving symlinks,
/// is still inside `canonical_dest`. The lexical check in `sanitize_entry_path`
/// cannot see symlinks pre-planted inside the destination (e.g. `dest/subdir`
/// pointing outside), so callers must invoke this on every directory they are
/// about to write through, after creating it.
pub(crate) fn verify_within_dest(canonical_dest: &Path, path: &Path) -> Result<(), ArchiveError> {
    let canonical = path.canonicalize().map_err(ArchiveError::Io)?;
    if !canonical.starts_with(canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path escapes destination via symlink: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Tracks the cumulative number of bytes written during one extraction and
/// rejects archives whose decompressed total exceeds `MAX_TOTAL_ARCHIVE_SIZE`
/// (decompression-bomb guard).
#[derive(Default)]
pub(crate) struct TotalSizeGuard(u64);

impl TotalSizeGuard {
    pub(crate) fn add(&mut self, bytes: u64) -> Result<(), ArchiveError> {
        self.0 = self.0.saturating_add(bytes);
        if self.0 > MAX_TOTAL_ARCHIVE_SIZE {
            return Err(ArchiveError::InvalidArchive(format!(
                "total extracted size exceeds maximum {MAX_TOTAL_ARCHIVE_SIZE}"
            )));
        }
        Ok(())
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                result.push(prefix.as_os_str());
            }
            Component::RootDir => {
                result.push(std::path::MAIN_SEPARATOR.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            Component::Normal(comp) => {
                result.push(comp);
            }
        }
    }
    result
}

fn has_ext_ignore_case(name: &str, ext: &str) -> bool {
    name.len() >= ext.len()
        && name.as_bytes()[name.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
}

fn verify_tar_header(mut reader: impl Read) -> bool {
    let mut buf = [0u8; TAR_BLOCK_SIZE];
    if reader.read_exact(&mut buf).is_err() {
        return false;
    }
    buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
}

fn seek_to_start(mut f: std::fs::File) -> std::fs::File {
    let _ = std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0));
    f
}

fn verify_tar_inside(file: &mut fs::File, format: ArchiveFormat) -> bool {
    if std::io::Seek::seek(file, std::io::SeekFrom::Start(0)).is_err() {
        return false;
    }
    match format {
        ArchiveFormat::TarGz => verify_tar_header(flate2::read::GzDecoder::new(file)),
        ArchiveFormat::TarBz2 => verify_tar_header(bzip2::read::BzDecoder::new(file)),
        ArchiveFormat::TarXz => {
            let mut buf = [0u8; TAR_BLOCK_SIZE];
            let mut cursor = io::Cursor::new(&mut buf[..]);
            let mut buf_reader = io::BufReader::new(file);
            let _ = lzma_rs::xz_decompress(&mut buf_reader, &mut cursor);
            cursor.position() as usize >= TAR_BLOCK_SIZE
                && buf[USTAR_MAGIC_OFFSET..].starts_with(USTAR_MAGIC)
        }
        ArchiveFormat::TarZst => match zstd::stream::read::Decoder::new(file) {
            Ok(reader) => verify_tar_header(reader),
            Err(_) => false,
        },
        _ => false,
    }
}

pub fn detect_format(path: &Path) -> Result<(ArchiveFormat, Option<std::fs::File>), ArchiveError> {
    const EXT_TABLE: &[(&[&str], ArchiveFormat)] = &[
        (&[".zip"], ArchiveFormat::Zip),
        (&[".tar.gz", ".tgz"], ArchiveFormat::TarGz),
        (&[".tar.bz2", ".tbz", ".tbz2"], ArchiveFormat::TarBz2),
        (&[".tar.xz", ".txz"], ArchiveFormat::TarXz),
        (&[".tar.zst", ".tzst"], ArchiveFormat::TarZst),
        (&[".tar"], ArchiveFormat::Tar),
        (&[".7z"], ArchiveFormat::SevenZ),
    ];

    let mut open_file: Option<std::fs::File> = None;

    if path.is_file()
        && let Ok(mut f) = std::fs::File::open(path)
    {
        let mut header = [0u8; 8];
        if f.read_exact(&mut header).is_ok() {
            if header[..4] == ZIP_MAGIC {
                return Ok((ArchiveFormat::Zip, Some(seek_to_start(f))));
            }
            if header[..6] == SEVENZ_MAGIC {
                return Ok((ArchiveFormat::SevenZ, Some(seek_to_start(f))));
            }
            if header[..2] == GZ_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarGz) {
                return Ok((ArchiveFormat::TarGz, Some(seek_to_start(f))));
            }
            if header[..3] == BZ2_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarBz2) {
                return Ok((ArchiveFormat::TarBz2, Some(seek_to_start(f))));
            }
            if header[..6] == XZ_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarXz) {
                return Ok((ArchiveFormat::TarXz, Some(seek_to_start(f))));
            }
            if header[..4] == ZST_MAGIC && verify_tar_inside(&mut f, ArchiveFormat::TarZst) {
                return Ok((ArchiveFormat::TarZst, Some(seek_to_start(f))));
            }
        }
        open_file = Some(f);
    }

    let name = path.file_name().ok_or_else(|| {
        ArchiveError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot determine archive format: path has no file name: {}",
                path.display()
            ),
        ))
    })?;
    let name = name.to_string_lossy();

    for &(exts, fmt) in EXT_TABLE {
        if exts.iter().any(|ext| has_ext_ignore_case(&name, ext)) {
            return Ok((fmt, open_file));
        }
    }

    Err(ArchiveError::UnsupportedFormat)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn total_size_guard_rejects_over_limit() {
        let mut guard = TotalSizeGuard::default();
        assert!(guard.add(MAX_TOTAL_ARCHIVE_SIZE).is_ok());
        assert!(guard.add(1).is_err());
    }

    #[test]
    fn total_size_guard_saturates_on_overflow() {
        let mut guard = TotalSizeGuard::default();
        assert!(guard.add(u64::MAX).is_err());
        assert!(guard.add(u64::MAX).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn verify_within_dest_rejects_symlinked_subdir() {
        let dest = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canonical_dest = dest.path().canonicalize().unwrap();

        let link = dest.path().join("subdir");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();

        assert!(verify_within_dest(&canonical_dest, &link).is_err());

        let real = dest.path().join("real");
        fs::create_dir(&real).unwrap();
        assert!(verify_within_dest(&canonical_dest, &real).is_ok());
    }

    fn make_ext_file(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, b"").unwrap();
        (dir, path)
    }

    #[test]
    fn detect_format_zip_by_extension() {
        let (_dir, path) = make_ext_file("test.zip");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::Zip);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tar_gz_by_extension() {
        let (_dir, path) = make_ext_file("test.tar.gz");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarGz);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tar_bz2_by_extension() {
        let (_dir, path) = make_ext_file("test.tar.bz2");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarBz2);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tar_xz_by_extension() {
        let (_dir, path) = make_ext_file("test.tar.xz");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarXz);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tar_zst_by_extension() {
        let (_dir, path) = make_ext_file("test.tar.zst");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarZst);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tar_by_extension() {
        let (_dir, path) = make_ext_file("test.tar");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::Tar);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_7z_by_extension() {
        let (_dir, path) = make_ext_file("test.7z");
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::SevenZ);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_tgz_by_extension() {
        let (_dir, path) = make_ext_file("test.tgz");
        let (fmt, _file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarGz);
    }

    #[test]
    fn detect_format_tbz_by_extension() {
        let (_dir, path) = make_ext_file("test.tbz2");
        let (fmt, _file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarBz2);
    }

    #[test]
    fn detect_format_txz_by_extension() {
        let (_dir, path) = make_ext_file("test.txz");
        let (fmt, _file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarXz);
    }

    #[test]
    fn detect_format_unknown_extension_returns_error() {
        let (_dir, path) = make_ext_file("test.txt");
        assert!(matches!(
            detect_format(&path),
            Err(ArchiveError::UnsupportedFormat)
        ));
    }

    #[test]
    fn detect_format_magic_bytes_zip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.dat");
        let mut content = vec![0x50, 0x4B, 0x03, 0x04];
        content.extend_from_slice(&[0u8; 4]);
        fs::write(&path, &content).unwrap();
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::Zip);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_magic_bytes_7z() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.dat");
        let mut content = vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
        content.extend_from_slice(&[0u8; 2]);
        fs::write(&path, &content).unwrap();
        let (fmt, file) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::SevenZ);
        assert!(file.is_some());
    }

    #[test]
    fn detect_format_case_insensitive_extension() {
        let (_dir, path) = make_ext_file("test.ZIP");
        let (fmt, _) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::Zip);

        let (_dir, path) = make_ext_file("test.TAR.GZ");
        let (fmt, _) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::TarGz);

        let (_dir, path) = make_ext_file("test.7Z");
        let (fmt, _) = detect_format(&path).unwrap();
        assert_eq!(fmt, ArchiveFormat::SevenZ);
    }

    #[test]
    fn detect_format_path_without_filename_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(detect_format(root).is_err());
    }
}
