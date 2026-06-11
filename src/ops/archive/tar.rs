use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use super::{
    ArchiveEntry, ArchiveError, ArchiveFormat, MAX_FILE_SIZE, MAX_LIST_ENTRIES, cleanup_extracted,
    copy_with_progress,
};
use crate::debug_log;

const MAX_CREATE_ENTRIES: usize = 100_000;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn create_temp_file(prefix: &str, suffix: &str) -> io::Result<(File, PathBuf)> {
    let pid = std::process::id();
    for _ in 0..100 {
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{pid}-{count}{suffix}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to create unique temp file after 100 attempts",
    ))
}

fn count_dir_entries(path: &Path) -> io::Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        count += 1;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            count += count_dir_entries(&entry.path())?;
        }
    }
    Ok(count)
}

pub fn list_tar(path: &Path, format: ArchiveFormat) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = wrap_decompress(file, format)?;
    let mut archive = tar::Archive::new(reader);

    let mut entries = Vec::new();
    for entry in archive.entries()? {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        let entry = entry?;
        let header = entry.header();

        entries.push(ArchiveEntry {
            name: String::from_utf8_lossy(&entry.path_bytes())
                .into_owned()
                .into_boxed_str(),
            size: entry.size(),
            compressed_size: 0,
            modified: header
                .mtime()
                .ok()
                .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t)),
            is_dir: header.entry_type().is_dir(),
            method: format!("{format:?}").into_boxed_str(),
        });
    }
    Ok(entries)
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

    let mut extracted_paths: Vec<PathBuf> = Vec::new();

    let result = (|| -> Result<(), ArchiveError> {
        let mut last_parent: Option<PathBuf> = None;
        for entry in archive.entries()? {
            super::check_cancel(cancel)?;

            let mut entry = entry?;
            let header = entry.header();
            let is_dir = header.entry_type().is_dir();
            let is_symlink = header.entry_type().is_symlink();
            let size = entry.size();

            if size > MAX_FILE_SIZE {
                return Err(ArchiveError::InvalidArchive(format!(
                    "entry '{}' size {size} exceeds maximum {MAX_FILE_SIZE}",
                    String::from_utf8_lossy(&entry.path_bytes()),
                )));
            }

            #[cfg(unix)]
            let unix_mode = header.mode().ok();

            if is_symlink {
                let _ = progress.send(size);
                continue;
            }

            let entry_path = entry.path()?;
            let outpath = super::sanitize_entry_path(&entry_path.to_string_lossy(), dest)?;

            if is_dir {
                fs::create_dir_all(&outpath)?;
                extracted_paths.push(outpath);
                let _ = progress.send(size);
            } else {
                if let Some(parent) = outpath.parent()
                    && last_parent.as_deref() != Some(parent)
                {
                    fs::create_dir_all(parent)?;
                    last_parent = Some(parent.to_path_buf());
                }
                if let Ok(meta) = fs::symlink_metadata(&outpath)
                    && meta.file_type().is_symlink()
                {
                    return Err(ArchiveError::InvalidArchive(format!(
                        "refusing to extract into existing symlink: {}",
                        outpath.display()
                    )));
                }
                let mut outfile = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&outpath)
                    .or_else(|e| {
                        if e.kind() == io::ErrorKind::AlreadyExists {
                            debug_log!(
                                "extract_tar: destination exists, overwriting: {}",
                                outpath.display()
                            );
                            File::create(&outpath)
                        } else {
                            Err(e)
                        }
                    })?;
                copy_with_progress(&mut entry, &mut outfile, progress)?;
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
        }
        Ok(())
    })();

    if result.is_err() {
        cleanup_extracted(&extracted_paths);
    }

    result
}

pub fn create_tar(
    sources: &[PathBuf],
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
    let finish_result = builder.finish().map_err(ArchiveError::Io);

    match (result, finish_result) {
        (Ok(()), Ok(())) => {
            fs::rename(&tmp_dest, dest)?;
            Ok(())
        }
        (Err(e), _) | (_, Err(e)) => {
            if let Err(e2) = fs::remove_file(&tmp_dest) {
                debug_log!(
                    "create_tar: cleanup temp {} failed: {e2}",
                    tmp_dest.display()
                );
            }
            Err(e)
        }
    }
}

fn create_tar_xz(
    sources: &[PathBuf],
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let (tar_file, tmp_tar) = create_temp_file("lc-xz-tar", ".tar").map_err(ArchiveError::Io)?;

    let result = (|| -> Result<(), ArchiveError> {
        let mut builder = tar::Builder::new(tar_file);
        append_sources(&mut builder, sources, progress, cancel)?;
        builder.finish().map_err(ArchiveError::Io)?;
        Ok(())
    })();

    if result.is_err() {
        if let Err(e) = fs::remove_file(&tmp_tar) {
            debug_log!(
                "create_tar_xz: cleanup tar temp {} failed: {e}",
                tmp_tar.display()
            );
        }
        return result;
    }

    let tmp_dest = dest.with_extension("tar.xz.tmp");
    let compress_result = (|| -> Result<(), ArchiveError> {
        let mut input = io::BufReader::new(File::open(&tmp_tar)?);
        let mut output = File::create(&tmp_dest)?;
        lzma_rs::xz_compress(&mut input, &mut output)
            .map_err(|e| ArchiveError::Io(io::Error::other(e)))?;
        Ok(())
    })();

    if let Err(e) = fs::remove_file(&tmp_tar) {
        debug_log!(
            "create_tar_xz: cleanup tar temp {} failed: {e}",
            tmp_tar.display()
        );
    }

    if compress_result.is_err() {
        if let Err(e) = fs::remove_file(&tmp_dest) {
            debug_log!(
                "create_tar_xz: cleanup xz temp {} failed: {e}",
                tmp_dest.display()
            );
        }
        return compress_result;
    }
    fs::rename(&tmp_dest, dest)?;
    Ok(())
}

fn append_sources(
    builder: &mut tar::Builder<impl Write>,
    sources: &[PathBuf],
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut entry_count: usize = 0;

    for source in sources {
        super::check_cancel(cancel)?;

        if source.is_dir() {
            let dir_entries = count_dir_entries(source)?;
            if entry_count.saturating_add(dir_entries) > MAX_CREATE_ENTRIES {
                return Err(ArchiveError::InvalidArchive(format!(
                    "too many entries in directory tree (limit {MAX_CREATE_ENTRIES}): {}",
                    source.display()
                )));
            }
            entry_count = entry_count.saturating_add(dir_entries);
            builder
                .append_dir_all(
                    source
                        .file_name()
                        .ok_or_else(|| ArchiveError::InvalidArchive("Invalid dir name".into()))?,
                    source,
                )
                .map_err(ArchiveError::Io)?;
        } else {
            entry_count = entry_count.saturating_add(1);
            if entry_count > MAX_CREATE_ENTRIES {
                return Err(ArchiveError::InvalidArchive(format!(
                    "too many entries (limit {MAX_CREATE_ENTRIES})"
                )));
            }
            let mut file = File::open(source)?;
            builder
                .append_file(
                    source
                        .file_name()
                        .ok_or_else(|| ArchiveError::InvalidArchive("Invalid file name".into()))?,
                    &mut file,
                )
                .map_err(ArchiveError::Io)?;
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
            let (mut tmp_file, tmp_path) =
                create_temp_file("lc-xz-decompress", ".tar").map_err(ArchiveError::Io)?;
            let mut reader = io::BufReader::new(file);
            if let Err(e) = lzma_rs::xz_decompress(&mut reader, &mut tmp_file) {
                if let Err(e2) = fs::remove_file(&tmp_path) {
                    debug_log!(
                        "wrap_decompress: cleanup decompress temp {} failed: {e2}",
                        tmp_path.display()
                    );
                }
                return Err(ArchiveError::Io(io::Error::other(e)));
            }
            drop(tmp_file);
            let mut tmp_file = File::open(&tmp_path)?;
            tmp_file.seek(io::SeekFrom::Start(0))?;
            struct TempFileReader {
                inner: File,
                path: PathBuf,
            }
            impl Read for TempFileReader {
                fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                    self.inner.read(buf)
                }
            }
            impl Drop for TempFileReader {
                fn drop(&mut self) {
                    if let Err(e) = fs::remove_file(&self.path) {
                        debug_log!(
                            "wrap_decompress: TempFileReader drop cleanup {} failed: {e}",
                            self.path.display()
                        );
                    }
                }
            }
            Ok(Box::new(TempFileReader {
                inner: tmp_file,
                path: tmp_path,
            }))
        }
        ArchiveFormat::TarZst => {
            let decoder = zstd::stream::read::Decoder::new(file)?;
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
            let encoder = zstd::stream::write::Encoder::new(file, 0)?;
            Ok(Box::new(encoder))
        }
        ArchiveFormat::TarXz | ArchiveFormat::Zip | ArchiveFormat::SevenZ => {
            Err(ArchiveError::UnsupportedFormat)
        }
    }
}
