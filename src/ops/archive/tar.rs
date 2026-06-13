use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::{
    ArchiveEntry, ArchiveError, ArchiveFormat, MAX_FILE_SIZE, MAX_LIST_ENTRIES, cleanup_extracted,
    copy_with_progress,
};
use crate::debug_log;

const MAX_CREATE_ENTRIES: usize = 100_000;

/// Appends the contents of `src` into the tar builder under `dest_name`,
/// counting entries against `count`. Returns an error if `MAX_CREATE_ENTRIES`
/// would be exceeded, making the limit check and the append a single pass.
fn append_dir_counted(
    builder: &mut tar::Builder<impl Write>,
    dest_name: &Path,
    src: &Path,
    count: &mut usize,
    progress: &Sender<u64>,
) -> Result<(), ArchiveError> {
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), dest_name.to_path_buf())];
    while let Some((src_dir, arch_dir)) = stack.pop() {
        for entry in fs::read_dir(&src_dir).map_err(ArchiveError::Io)? {
            let entry = entry.map_err(ArchiveError::Io)?;
            let src_path = entry.path();
            let arch_path = arch_dir.join(entry.file_name());

            if super::is_symlink_source(&src_path) {
                continue;
            }
            let ft = entry.file_type().map_err(ArchiveError::Io)?;

            *count = count.saturating_add(1);
            if *count > MAX_CREATE_ENTRIES {
                return Err(ArchiveError::InvalidArchive(format!(
                    "too many entries in directory tree (limit {MAX_CREATE_ENTRIES})"
                )));
            }

            if ft.is_dir() {
                builder
                    .append_dir(&arch_path, &src_path)
                    .map_err(ArchiveError::Io)?;
                stack.push((src_path, arch_path));
            } else {
                builder
                    .append_path_with_name(&src_path, &arch_path)
                    .map_err(ArchiveError::Io)?;
                // Report real file bytes so create progress is on the same scale
                // as extract (per-entry uncompressed size), not inode sizes.
                let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let _ = progress.send(bytes);
            }
        }
    }
    Ok(())
}

pub fn list_tar(path: &Path, format: ArchiveFormat) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = wrap_decompress(file, format)?;
    let mut archive = tar::Archive::new(reader);

    let mut entries = Vec::new();
    for entry in archive.entries()?.take(MAX_LIST_ENTRIES) {
        let entry = entry?;
        let header = entry.header();

        let path_bytes = entry.path_bytes();
        let path_str = std::str::from_utf8(&path_bytes).unwrap_or_else(|e| {
            debug_log!("list_tar: invalid UTF-8 in tar entry path: {e}");
            std::str::from_utf8(&path_bytes[..e.valid_up_to()]).unwrap_or("<invalid>")
        });
        entries.push(ArchiveEntry {
            name: path_str.to_owned().into_boxed_str(),
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
    file: std::fs::File,
    dest: &Path,
    format: ArchiveFormat,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let reader: Box<dyn Read> = wrap_decompress(file, format)?;
    let mut archive = tar::Archive::new(reader);

    let mut extracted_paths: Vec<PathBuf> = Vec::new();

    let result = (|| -> Result<(), ArchiveError> {
        let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
        let mut last_parent: Option<PathBuf> = None;
        let mut total_size = super::TotalSizeGuard::default();
        for entry in archive.entries()? {
            super::check_cancel(cancel)?;

            let mut entry = entry?;
            let header = entry.header();
            let is_dir = header.entry_type().is_dir();
            let is_symlink = header.entry_type().is_symlink();
            let size = entry.size();

            if size > MAX_FILE_SIZE {
                let path_bytes = entry.path_bytes();
                let path_str = std::str::from_utf8(&path_bytes).unwrap_or_else(|e| {
                    debug_log!("extract_tar: invalid UTF-8 in tar entry path: {e}");
                    std::str::from_utf8(&path_bytes[..e.valid_up_to()]).unwrap_or("<invalid>")
                });
                return Err(ArchiveError::InvalidArchive(format!(
                    "entry '{path_str}' size {size} exceeds maximum {MAX_FILE_SIZE}",
                )));
            }

            #[cfg(unix)]
            let unix_mode = header.mode().ok();

            if is_symlink {
                let _ = progress.send(size);
                continue;
            }

            let entry_path = entry.path()?;
            let outpath =
                super::sanitize_entry_path(&canonical_dest, &entry_path.to_string_lossy())?;

            if is_dir {
                fs::create_dir_all(&outpath)?;
                super::verify_within_dest(&canonical_dest, &outpath)?;
                extracted_paths.push(outpath);
                let _ = progress.send(size);
            } else {
                if let Some(parent) = outpath.parent()
                    && last_parent.as_deref() != Some(parent)
                {
                    fs::create_dir_all(parent)?;
                    super::verify_within_dest(&canonical_dest, parent)?;
                    last_parent = Some(parent.to_path_buf());
                }
                // MITIGATION: TOCTOU symlink race — an attacker could replace a
                // regular file with a symlink between the symlink_metadata check below
                // and the subsequent open() call. On Unix, O_NOFOLLOW causes open() to
                // fail with ELOOP if the final path component is a symlink, closing the
                // race window. On platforms without O_NOFOLLOW support, the inherent
                // TOCTOU window remains; the symlink_metadata check serves as a
                // best-effort defense only.
                super::check_symlink_at_dest(&outpath)?;
                let mut outfile = {
                    let mut opts = OpenOptions::new();
                    opts.write(true).create_new(true);
                    #[cfg(unix)]
                    opts.custom_flags(libc::O_NOFOLLOW);
                    opts.open(&outpath).or_else(|e| {
                        if e.kind() == io::ErrorKind::AlreadyExists {
                            debug_log!(
                                "extract_tar: destination exists, overwriting: {}",
                                outpath.display()
                            );
                            super::open_outfile(&outpath)
                        } else {
                            Err(e)
                        }
                    })?
                };
                let written = copy_with_progress(&mut entry, &mut outfile, progress, cancel)?;
                total_size.add(written)?;
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

    let result = build_tar_into(writer, sources, progress, cancel);

    match result {
        Ok(()) => {
            fs::rename(&tmp_dest, dest)?;
            Ok(())
        }
        Err(e) => {
            cleanup_temp_file(&tmp_dest);
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
    let (tar_file, tmp_tar) =
        super::create_archive_temp_file("lc-xz-tar", ".tar").map_err(ArchiveError::Io)?;

    let result = build_tar_into(tar_file, sources, progress, cancel);

    if result.is_err() {
        cleanup_temp_file(&tmp_tar);
        return result;
    }

    let tmp_dest = dest.with_extension("tar.xz.tmp");
    let compress_result = (|| -> Result<(), ArchiveError> {
        let mut input = io::BufReader::with_capacity(super::IO_BUFFER_SIZE, File::open(&tmp_tar)?);
        let mut output = File::create(&tmp_dest)?;
        lzma_rs::xz_compress(&mut input, &mut output)
            .map_err(|e| ArchiveError::Io(io::Error::other(e)))?;
        Ok(())
    })();

    cleanup_temp_file(&tmp_tar);

    if compress_result.is_err() {
        cleanup_temp_file(&tmp_dest);
        return compress_result;
    }
    fs::rename(&tmp_dest, dest)?;
    Ok(())
}

fn build_tar_into<W: Write>(
    writer: W,
    sources: &[PathBuf],
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut builder = tar::Builder::new(writer);
    append_sources(&mut builder, sources, progress, cancel)?;
    builder.finish().map_err(ArchiveError::Io)
}

fn cleanup_temp_file(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        debug_log!("cleanup temp {} failed: {e}", path.display());
    }
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
            let dir_name = source
                .file_name()
                .ok_or_else(|| ArchiveError::InvalidArchive("Invalid dir name".into()))?;
            builder
                .append_dir(dir_name, source)
                .map_err(ArchiveError::Io)?;
            entry_count = entry_count.saturating_add(1);
            if entry_count > MAX_CREATE_ENTRIES {
                return Err(ArchiveError::InvalidArchive(format!(
                    "too many entries (limit {MAX_CREATE_ENTRIES})"
                )));
            }
            // Progress for the tree is reported per-file inside the recursion,
            // keeping the create scale comparable to extract.
            append_dir_counted(
                builder,
                Path::new(dir_name),
                source,
                &mut entry_count,
                progress,
            )?;
        } else {
            // Open with O_NOFOLLOW to close the TOCTOU window between the
            // create-side symlink filter and this open; `None` means the source
            // became a symlink, so skip it consistently with the filter policy.
            let Some(mut file) = super::open_source_nofollow(source)? else {
                continue;
            };
            entry_count = entry_count.saturating_add(1);
            if entry_count > MAX_CREATE_ENTRIES {
                return Err(ArchiveError::InvalidArchive(format!(
                    "too many entries (limit {MAX_CREATE_ENTRIES})"
                )));
            }
            let name = source
                .file_name()
                .ok_or_else(|| ArchiveError::InvalidArchive("Invalid file name".into()))?;
            let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
            builder
                .append_file(name, &mut file)
                .map_err(ArchiveError::Io)?;
            let _ = progress.send(bytes);
        }
    }
    Ok(())
}

fn wrap_decompress(file: File, format: ArchiveFormat) -> Result<Box<dyn Read>, ArchiveError> {
    match format {
        ArchiveFormat::Tar => Ok(Box::new(io::BufReader::with_capacity(
            super::IO_BUFFER_SIZE,
            file,
        ))),
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
                super::create_archive_temp_file("lc-xz-decompress", ".tar")
                    .map_err(ArchiveError::Io)?;
            let mut reader = io::BufReader::with_capacity(super::IO_BUFFER_SIZE, file);
            if let Err(e) = lzma_rs::xz_decompress(&mut reader, &mut tmp_file) {
                cleanup_temp_file(&tmp_path);
                return Err(ArchiveError::Io(io::Error::other(e)));
            }
            drop(tmp_file);
            let tmp_file = File::open(&tmp_path)?;
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
