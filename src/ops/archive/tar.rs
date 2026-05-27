use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use super::{ArchiveEntry, ArchiveError, ArchiveFormat};

const MAX_LIST_ENTRIES: usize = 100_000;

pub fn list_tar(path: &Path, format: ArchiveFormat) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = wrap_decompress(file, format)?;
    let mut archive = tar::Archive::new(reader);

    let mut entries = Vec::new();
    for entry in archive
        .entries()
        .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?
    {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        let entry = entry.map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        let header = entry.header();

        entries.push(ArchiveEntry {
            name: String::from_utf8_lossy(&entry.path_bytes()).to_string(),
            size: entry.size(),
            compressed_size: 0,
            modified: header
                .mtime()
                .ok()
                .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t)),
            is_dir: entry.header().entry_type().is_dir(),
            method: format!("{format:?}"),
        });
    }
    Ok(entries)
}

fn validate_entry_path(entry_path: &Path, dest: &Path) -> Result<std::path::PathBuf, ArchiveError> {
    if entry_path.is_absolute() {
        return Err(ArchiveError::InvalidArchive(format!(
            "absolute path detected: {}",
            entry_path.display()
        )));
    }
    // Reject any entry containing parent-directory components
    for component in entry_path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(ArchiveError::InvalidArchive(format!(
                "path traversal detected: {}",
                entry_path.display()
            )));
        }
    }
    let outpath = dest.join(entry_path);
    let canonical_dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    let canonical_out = outpath
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(outpath.file_name().unwrap_or_default()))
        .unwrap_or_else(|| outpath.clone());

    if !canonical_out.starts_with(&canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal detected: {}",
            entry_path.display()
        )));
    }
    Ok(outpath)
}

pub fn extract_tar(
    path: &Path,
    dest: &Path,
    format: ArchiveFormat,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = wrap_decompress(file, format)?;
    let mut archive = tar::Archive::new(reader);

    let mut extracted_paths: Vec<std::path::PathBuf> = Vec::new();

    let result = (|| -> Result<(), ArchiveError> {
        for entry in archive
            .entries()
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?
        {
            if cancel.load(Ordering::Relaxed) {
                return Err(ArchiveError::Io(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation canceled",
                )));
            }

            let mut entry = entry.map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
            let header = entry.header();
            let is_dir = header.entry_type().is_dir();
            let is_symlink = header.entry_type().is_symlink();
            let size = entry.size();

            #[cfg(unix)]
            let unix_mode = header.mode().ok();

            if is_symlink {
                continue;
            }

            let entry_path = entry
                .path()
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
            let outpath = validate_entry_path(&entry_path, dest)?;

            if is_dir {
                fs::create_dir_all(&outpath)?;
                extracted_paths.push(outpath);
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = File::create(&outpath)?;
                io::copy(&mut entry, &mut outfile)?;
                extracted_paths.push(outpath.clone());

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = unix_mode {
                        let safe_mode = mode & !0o7000;
                        fs::set_permissions(&outpath, fs::Permissions::from_mode(safe_mode))?;
                    }
                }
            }

            let _ = progress.send(size);
        }
        Ok(())
    })();

    if result.is_err() {
        for p in extracted_paths.iter().rev() {
            if p.is_dir() {
                let _ = fs::remove_dir(p);
            } else {
                let _ = fs::remove_file(p);
            }
        }
    }

    result
}

pub fn create_tar(
    sources: &[std::path::PathBuf],
    dest: &Path,
    format: ArchiveFormat,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    if format == ArchiveFormat::TarXz {
        return create_tar_xz(sources, dest, progress, cancel);
    }

    let tmp_dest = dest.with_extension("tar.tmp");
    let file = File::create(&tmp_dest)?;
    let writer: Box<dyn Write> = wrap_compress(file, format)?;
    let mut builder = tar::Builder::new(writer);

    let result = append_sources(&mut builder, sources, progress, cancel);
    let finish_result = builder
        .finish()
        .map_err(|e| ArchiveError::InvalidArchive(e.to_string()));

    match (result, finish_result) {
        (Ok(()), Ok(())) => {
            fs::rename(&tmp_dest, dest)?;
            Ok(())
        }
        (Err(e), _) | (_, Err(e)) => {
            let _ = fs::remove_file(&tmp_dest);
            Err(e)
        }
    }
}

fn create_tar_xz(
    sources: &[std::path::PathBuf],
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_tar = std::env::temp_dir().join(format!("lc-xz-tar-{}-{count}", std::process::id()));
    let tmp_dest = dest.with_extension("tar.xz.tmp");

    // Build tar to temp file
    {
        let tar_file = File::create(&tmp_tar)?;
        let mut builder = tar::Builder::new(tar_file);
        append_sources(&mut builder, sources, progress, cancel)?;
        builder
            .finish()
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
    }

    // Compress tar temp file to output
    let result = (|| -> Result<(), ArchiveError> {
        let mut input = std::io::BufReader::new(File::open(&tmp_tar)?);
        let mut output = File::create(&tmp_dest)?;
        lzma_rs::xz_compress(&mut input, &mut output)
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        Ok(())
    })();

    let _ = fs::remove_file(&tmp_tar);
    if result.is_err() {
        let _ = fs::remove_file(&tmp_dest);
        return result;
    }
    fs::rename(&tmp_dest, dest)?;
    Ok(())
}

fn append_sources(
    builder: &mut tar::Builder<impl Write>,
    sources: &[std::path::PathBuf],
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            return Err(ArchiveError::Io(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            )));
        }

        if source.is_dir() {
            builder
                .append_dir_all(
                    source
                        .file_name()
                        .ok_or_else(|| ArchiveError::InvalidArchive("Invalid dir name".into()))?,
                    source,
                )
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        } else {
            let mut file = File::open(source)?;
            builder
                .append_file(
                    source
                        .file_name()
                        .ok_or_else(|| ArchiveError::InvalidArchive("Invalid file name".into()))?,
                    &mut file,
                )
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        }

        let meta = fs::metadata(source)?;
        let _ = progress.send(meta.len());
    }
    Ok(())
}

fn wrap_decompress(file: File, format: ArchiveFormat) -> Result<Box<dyn Read>, ArchiveError> {
    match format {
        ArchiveFormat::Tar => Ok(Box::new(file)),
        ArchiveFormat::TarGz => {
            let decoder = flate2::read::GzDecoder::new(file);
            Ok(Box::new(decoder))
        }
        ArchiveFormat::TarBz2 => {
            let decoder = bzip2::read::BzDecoder::new(file);
            Ok(Box::new(decoder))
        }
        ArchiveFormat::TarXz => {
            static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);
            let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let tmp_path = std::env::temp_dir()
                .join(format!("lc-xz-decompress-{}-{count}", std::process::id()));
            let mut tmp_file = File::create(&tmp_path)?;
            let mut reader = std::io::BufReader::new(file);
            if let Err(e) = lzma_rs::xz_decompress(&mut reader, &mut tmp_file) {
                let _ = fs::remove_file(&tmp_path);
                return Err(ArchiveError::InvalidArchive(e.to_string()));
            }
            drop(tmp_file);
            let mut tmp_file = File::open(&tmp_path)?;
            tmp_file.seek(io::SeekFrom::Start(0))?;
            // Wrap in a reader that cleans up the temp file on drop
            struct TempFileReader {
                inner: File,
                path: std::path::PathBuf,
            }
            impl Read for TempFileReader {
                fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                    self.inner.read(buf)
                }
            }
            impl Drop for TempFileReader {
                fn drop(&mut self) {
                    let _ = fs::remove_file(&self.path);
                }
            }
            Ok(Box::new(TempFileReader {
                inner: tmp_file,
                path: tmp_path,
            }))
        }
        ArchiveFormat::TarZst => {
            let decoder = zstd::stream::read::Decoder::new(file)
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
            Ok(Box::new(decoder))
        }
        ArchiveFormat::Zip | ArchiveFormat::SevenZ => Err(ArchiveError::UnsupportedFormat),
    }
}

fn wrap_compress(file: File, format: ArchiveFormat) -> Result<Box<dyn Write>, ArchiveError> {
    match format {
        ArchiveFormat::Tar => Ok(Box::new(file)),
        ArchiveFormat::TarGz => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            Ok(Box::new(encoder))
        }
        ArchiveFormat::TarBz2 => {
            let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
            Ok(Box::new(encoder))
        }
        ArchiveFormat::TarZst => {
            let encoder = zstd::stream::write::Encoder::new(file, 0)
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
            Ok(Box::new(encoder))
        }
        ArchiveFormat::TarXz | ArchiveFormat::Zip | ArchiveFormat::SevenZ => {
            Err(ArchiveError::UnsupportedFormat)
        }
    }
}
