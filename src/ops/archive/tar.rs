use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::{
    ArchiveEntry, ArchiveError, ArchiveFormat, MAX_FILE_SIZE, MAX_LIST_ENTRIES, cleanup_extracted,
    copy_with_progress,
};
use crate::debug_log;

const MAX_CREATE_ENTRIES: usize = 100_000;

/// Write adapter that aborts once cumulative output exceeds
/// `MAX_TOTAL_ARCHIVE_SIZE`. Used to bound the xz -> temp-file materialization so
/// the decompression-bomb guard applies before the whole payload is on disk
/// (the streaming gz/bz2/zst decoders are already bounded lazily by the extract
/// loop's `TotalSizeGuard`).
struct CappedWriter<W: Write> {
    inner: W,
    written: u64,
}

impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Count the bytes the inner writer actually committed, not the full
        // `buf.len()`. On a short write the caller retries with the tail, so
        // counting up front would double-count the retried bytes and trip the
        // cap early. (`File` writes to a temp file essentially never short-write,
        // but the precise accounting is cheap and correct regardless.)
        let n = self.inner.write(buf)?;
        self.written = self.written.saturating_add(n as u64);
        if self.written > super::MAX_TOTAL_ARCHIVE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decompressed xz exceeds maximum archive size",
            ));
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Read adapter that aborts with `Interrupted` as soon as `cancel` is set, so a
/// single large file (or the whole xz compression pass) is interruptible
/// mid-stream instead of only between entries. Wraps the file reader that the
/// `tar` crate / `lzma_rs` copy from internally.
struct CancelReader<'a, R: Read> {
    inner: R,
    cancel: &'a AtomicBool,
}

impl<R: Read> Read for CancelReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            ));
        }
        self.inner.read(buf)
    }
}

/// Forwards `BufRead` so a `CancelReader` wrapping a buffered reader can feed
/// `lzma_rs::xz_compress` (which requires `BufRead`) while still aborting on
/// cancel at each buffer fill.
impl<R: BufRead> BufRead for CancelReader<'_, R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            ));
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
    }
}

/// Appends `file` into the tar builder under `arch_path` using a manually built
/// header so the data copy runs through a [`CancelReader`] (per-chunk
/// cancellation), unlike `Builder::append_file` whose internal copy is
/// uninterruptible.
fn append_file_cancellable(
    builder: &mut tar::Builder<impl Write>,
    arch_path: &Path,
    mut file: File,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let meta = file.metadata().map_err(ArchiveError::Io)?;
    let bytes = meta.len();
    let mut header = tar::Header::new_gnu();
    // Sets size (from `meta.len()`), mode, mtime and the regular-file entry type;
    // symlinks/dirs are filtered out before this is reached.
    header.set_metadata(&meta);
    let reader = CancelReader {
        inner: &mut file,
        cancel,
    };
    builder
        .append_data(&mut header, arch_path, reader)
        .map_err(ArchiveError::Io)?;
    let _ = progress.send(bytes);
    Ok(())
}

/// Appends the contents of `src` into the tar builder under `dest_name`,
/// counting entries against `count`. Returns an error if `MAX_CREATE_ENTRIES`
/// would be exceeded, making the limit check and the append a single pass.
fn append_dir_counted(
    builder: &mut tar::Builder<impl Write>,
    dest_name: &Path,
    src: &Path,
    count: &mut usize,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), dest_name.to_path_buf())];
    while let Some((src_dir, arch_dir)) = stack.pop() {
        for entry in fs::read_dir(&src_dir).map_err(ArchiveError::Io)? {
            // Check per entry so a huge directory tree is cancellable during the
            // walk, not only between top-level sources.
            super::check_cancel(cancel)?;

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
                // dir->symlink swap guard: `append_dir` and the recursion below
                // would follow a symlink, so re-check right before treating the
                // path as a directory (TOCTOU after the filter above).
                if super::is_symlink_source(&src_path) {
                    debug_log!(
                        "append_dir_counted: skipping path that became a symlink: {}",
                        src_path.display()
                    );
                    continue;
                }
                builder
                    .append_dir(&arch_path, &src_path)
                    .map_err(ArchiveError::Io)?;
                stack.push((src_path, arch_path));
            } else {
                // Open the nested file with O_NOFOLLOW to close the TOCTOU window
                // between the symlink filter above and this open, matching the zip
                // path. `None` means the entry became a symlink, so skip it.
                let Some(file) = super::open_source_nofollow(&src_path)? else {
                    continue;
                };
                // Reports real file bytes (per-entry uncompressed size) and runs
                // the copy through a CancelReader for mid-file cancellation.
                append_file_cancellable(builder, &arch_path, file, progress, cancel)?;
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
        // Preserve the full path structure on invalid UTF-8 by lossily replacing
        // only the bad bytes, instead of truncating at the first error.
        let path_str = match std::str::from_utf8(&path_bytes) {
            Ok(s) => std::borrow::Cow::Borrowed(s),
            Err(e) => {
                debug_log!("list_tar: invalid UTF-8 in tar entry path: {e}");
                String::from_utf8_lossy(&path_bytes)
            }
        };
        entries.push(ArchiveEntry {
            name: path_str.into_owned().into_boxed_str(),
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
        // Create the destination before canonicalizing it (matching zip/7z):
        // `canonicalize` fails on a path that does not yet exist.
        fs::create_dir_all(dest).map_err(ArchiveError::Io)?;
        let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
        let mut total_size = super::TotalSizeGuard::default();
        for entry in archive.entries()? {
            super::check_cancel(cancel)?;

            let mut entry = entry?;
            let header = entry.header();
            let is_dir = header.entry_type().is_dir();
            let is_symlink = header.entry_type().is_symlink();
            let is_hard_link = header.entry_type().is_hard_link();
            let size = entry.size();

            // Skip symlinks BEFORE the size check: a skipped entry type must not
            // be able to reject the whole archive via a bogus advertised size.
            if is_symlink {
                let _ = progress.send(size);
                continue;
            }

            if size > MAX_FILE_SIZE {
                let path_bytes = entry.path_bytes();
                // Preserve the full path structure on invalid UTF-8 by lossily
                // replacing only the bad bytes, instead of truncating at the first
                // error.
                let path_str = match std::str::from_utf8(&path_bytes) {
                    Ok(s) => std::borrow::Cow::Borrowed(s),
                    Err(e) => {
                        debug_log!("extract_tar: invalid UTF-8 in tar entry path: {e}");
                        String::from_utf8_lossy(&path_bytes)
                    }
                };
                return Err(ArchiveError::InvalidArchive(format!(
                    "entry '{path_str}' size {size} exceeds maximum {MAX_FILE_SIZE}",
                )));
            }

            #[cfg(unix)]
            let unix_mode = header.mode().ok();

            if is_hard_link {
                // A hard-link entry carries no data of its own; the tar crate
                // would materialize it as an empty regular file, silently
                // dropping the intended content. Reject it explicitly instead of
                // producing corrupt output.
                let path_bytes = entry.path_bytes();
                let path_str = String::from_utf8_lossy(&path_bytes);
                return Err(ArchiveError::InvalidArchive(format!(
                    "refusing to extract hard link entry '{path_str}' (unsupported)"
                )));
            }

            // Sanitize against the raw `Path` (no lossy UTF-8 conversion) so two
            // distinct non-UTF-8 names can't collapse to the same output path.
            let entry_path = entry.path()?;
            let outpath = super::sanitize_entry_path(&canonical_dest, &entry_path)?;

            if is_dir {
                // Only track directories THIS operation actually creates, so a
                // rollback never `remove_dir_all`s a pre-existing user directory
                // that `create_dir_all` merely succeeded on idempotently.
                let newly_created = fs::symlink_metadata(&outpath).is_err();
                fs::create_dir_all(&outpath)?;
                super::verify_within_dest(&canonical_dest, &outpath)?;
                if newly_created {
                    extracted_paths.push(outpath);
                }
                let _ = progress.send(size);
            } else {
                // Re-verify the parent on EVERY entry rather than caching the last
                // one: a cached parent could be swapped for a symlink between
                // entries (TOCTOU), and skipping `verify_within_dest` on a cache hit
                // would let a later entry be written outside `canonical_dest`.
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                    super::verify_within_dest(&canonical_dest, parent)?;
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
                // Register the file for rollback BEFORE copying: a mid-copy
                // failure (cancel / size-limit / IO error) must still clean up the
                // partial file, which pushing only after the copy would miss.
                extracted_paths.push(outpath.clone());
                let written = copy_with_progress(&mut entry, &mut outfile, progress, cancel)?;
                total_size.add(written)?;

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

    // Stage next to the destination (same filesystem for an atomic rename) using
    // `create_new` so we never truncate an existing file or follow a symlink
    // planted at a predictable `dest.tar.tmp` path.
    let dest_dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let (file, tmp_dest) =
        super::create_temp_file_in(dest_dir, ".lc-tar", ".tmp").map_err(ArchiveError::Io)?;
    let writer: Box<dyn Write> = wrap_compress(file, format)?;

    let result = build_tar_into(writer, sources, progress, cancel);

    match result {
        Ok(()) => {
            // Clean up the staged archive if the final rename fails, instead of
            // orphaning it next to the destination.
            if let Err(e) = fs::rename(&tmp_dest, dest) {
                cleanup_temp_file(&tmp_dest);
                return Err(ArchiveError::Io(e));
            }
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

    // Stage next to the destination (same filesystem for an atomic rename) using
    // `create_new` so we never truncate an existing file or follow a symlink
    // planted at a predictable `dest.tar.xz.tmp` path.
    let dest_dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let (mut output, tmp_dest) =
        super::create_temp_file_in(dest_dir, ".lc-tarxz", ".tmp").map_err(ArchiveError::Io)?;
    let compress_result = (|| -> Result<(), ArchiveError> {
        // Wrap the input in a CancelReader so a long compression pass aborts
        // promptly when the operation is canceled (xz_compress is otherwise an
        // uninterruptible whole-file read).
        let mut input = CancelReader {
            inner: io::BufReader::with_capacity(super::IO_BUFFER_SIZE, File::open(&tmp_tar)?),
            cancel,
        };
        lzma_rs::xz_compress(&mut input, &mut output)
            .map_err(|e| ArchiveError::Io(io::Error::other(e)))?;
        Ok(())
    })();
    // Close the staged file before the rename/cleanup below so the rename does
    // not race an open handle (matters on Windows).
    drop(output);

    cleanup_temp_file(&tmp_tar);

    if compress_result.is_err() {
        cleanup_temp_file(&tmp_dest);
        return compress_result;
    }
    // Clean up the staged archive if the final rename fails, instead of
    // orphaning it next to the destination.
    if let Err(e) = fs::rename(&tmp_dest, dest) {
        cleanup_temp_file(&tmp_dest);
        return Err(ArchiveError::Io(e));
    }
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
            // `is_dir()` follows symlinks, so re-check before treating the source
            // as a directory: skip if it became a symlink after the create-side
            // filter (dir->symlink swap TOCTOU).
            if super::is_symlink_source(source) {
                debug_log!(
                    "append_sources: skipping source that became a symlink: {}",
                    source.display()
                );
                continue;
            }
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
                cancel,
            )?;
        } else {
            // Open with O_NOFOLLOW to close the TOCTOU window between the
            // create-side symlink filter and this open; `None` means the source
            // became a symlink, so skip it consistently with the filter policy.
            let Some(file) = super::open_source_nofollow(source)? else {
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
            append_file_cancellable(builder, Path::new(name), file, progress, cancel)?;
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
            {
                // Inner scope so the `CappedWriter`'s `&mut tmp_file` borrow ends
                // before we drop the file and reopen it for reading.
                let mut capped = CappedWriter {
                    inner: &mut tmp_file,
                    written: 0,
                };
                if let Err(e) = lzma_rs::xz_decompress(&mut reader, &mut capped) {
                    cleanup_temp_file(&tmp_path);
                    return Err(ArchiveError::Io(io::Error::other(e)));
                }
            }
            drop(tmp_file);
            // Clean up the temp file if the reopen fails, matching the
            // xz_decompress error path above; otherwise it would be orphaned
            // (the cleanup-on-drop `TempFileReader` is only built after this).
            let tmp_file = File::open(&tmp_path).inspect_err(|_| cleanup_temp_file(&tmp_path))?;
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
            // `auto_finish()` so the zstd frame epilogue (and any buffered tail,
            // including the tar end-of-archive blocks) is written when the boxed
            // writer is dropped. A plain `Encoder` requires an explicit
            // `finish()` that `build_tar_into` never calls — it only drops the
            // builder — which silently produced truncated, corrupt `.tar.zst`
            // archives. Mirrors the drop-finalizing GzEncoder/BzEncoder branches.
            let encoder = zstd::stream::write::Encoder::new(file, 0)?;
            Ok(Box::new(encoder.auto_finish()))
        }
        ArchiveFormat::TarXz | ArchiveFormat::Zip | ArchiveFormat::SevenZ => {
            Err(ArchiveError::UnsupportedFormat)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    // --- Raw tar fixtures ------------------------------------------------
    // The `tar` crate deliberately refuses to WRITE `..`/absolute entry paths,
    // so a genuine zip-slip fixture must be assembled from raw 512-byte blocks.
    // `extract_raw_tar_roundtrip` extracts a benign raw entry to prove the block
    // encoding (octal fields + checksum) is valid, which in turn proves the
    // traversal test's rejection comes from `sanitize_entry_path`, not a parse
    // error.

    fn raw_tar_header(name: &str, data_len: usize, typeflag: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        let nb = name.as_bytes();
        assert!(nb.len() <= 100);
        h[..nb.len()].copy_from_slice(nb);
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{data_len:011o}\0").as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[156] = typeflag;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        // Checksum: the field is treated as 8 spaces while summing.
        for b in &mut h[148..156] {
            *b = b' ';
        }
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        h
    }

    fn raw_tar_regular(name: &str, data: &[u8]) -> Vec<u8> {
        let mut out = raw_tar_header(name, data.len(), b'0').to_vec();
        out.extend_from_slice(data);
        let rem = data.len() % 512;
        if rem != 0 {
            out.resize(out.len() + (512 - rem), 0);
        }
        out
    }

    fn raw_tar_archive(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut out: Vec<u8> = blocks.iter().flatten().copied().collect();
        out.resize(out.len() + 1024, 0); // two zero blocks = end of archive
        out
    }

    // --- tar-crate fixtures (valid, relative paths) ----------------------

    fn build_tar(path: &Path, entries: impl FnOnce(&mut tar::Builder<File>)) {
        let mut builder = tar::Builder::new(File::create(path).unwrap());
        entries(&mut builder);
        builder.finish().unwrap();
    }

    fn add_file(builder: &mut tar::Builder<File>, name: &str, data: &[u8]) {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(0o644);
        builder.append_data(&mut h, name, data).unwrap();
    }

    // Used only by the unix-gated rollback test below.
    #[cfg(unix)]
    fn add_dir(builder: &mut tar::Builder<File>, name: &str) {
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(0o755);
        builder.append_data(&mut h, name, io::empty()).unwrap();
    }

    fn extract(archive: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), ArchiveError> {
        let (tx, _rx) = mpsc::channel();
        extract_tar(
            File::open(archive).unwrap(),
            dest,
            ArchiveFormat::Tar,
            &tx,
            cancel,
        )
    }

    #[test]
    fn extract_raw_tar_roundtrip() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("ok.tar");
        fs::write(
            &archive,
            raw_tar_archive(&[raw_tar_regular("hello.txt", b"hi there")]),
        )
        .unwrap();
        let dest = work.path().join("dest");
        extract(&archive, &dest, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(dest.join("hello.txt")).unwrap(), b"hi there");
    }

    #[test]
    fn extract_rejects_dotdot_traversal() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("evil.tar");
        fs::write(
            &archive,
            raw_tar_archive(&[raw_tar_regular("../evil.txt", b"evil")]),
        )
        .unwrap();
        let dest = work.path().join("dest");
        let res = extract(&archive, &dest, &AtomicBool::new(false));
        assert!(
            matches!(res, Err(ArchiveError::InvalidArchive(ref m)) if m.contains("traversal")),
            "expected a path-traversal rejection, got {res:?}"
        );
        // Written outside the destination? Must not exist.
        assert!(!work.path().join("evil.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn extract_rejects_entry_through_symlinked_dir() {
        let work = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let archive = work.path().join("a.tar");
        build_tar(&archive, |b| add_file(b, "subdir/file.txt", b"pwned"));

        let dest = work.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(outside.path(), dest.join("subdir")).unwrap();

        let res = extract(&archive, &dest, &AtomicBool::new(false));
        assert!(res.is_err());
        assert!(!outside.path().join("file.txt").exists());
    }

    // P0.1 regression: a directory entry that already exists on disk must NOT be
    // tracked for rollback, so a failure on a LATER entry does not
    // `remove_dir_all` a pre-existing user directory and its unrelated contents.
    #[cfg(unix)]
    #[test]
    fn rollback_preserves_preexisting_dir() {
        let work = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dest = work.path().join("dest");
        fs::create_dir_all(&dest).unwrap();

        let keep = dest.join("keep");
        fs::create_dir(&keep).unwrap();
        fs::write(keep.join("precious.txt"), b"precious").unwrap();
        // Pre-plant a symlink so the second entry fails and triggers rollback.
        std::os::unix::fs::symlink(outside.path(), dest.join("escape")).unwrap();

        let archive = work.path().join("a.tar");
        build_tar(&archive, |b| {
            add_dir(b, "keep"); // references the pre-existing directory
            add_file(b, "escape/f.txt", b"x"); // fails via the symlinked parent
        });

        let res = extract(&archive, &dest, &AtomicBool::new(false));
        assert!(res.is_err(), "expected failure on the symlinked parent");
        assert!(
            keep.join("precious.txt").exists(),
            "rollback deleted pre-existing data"
        );
    }

    #[test]
    fn extract_canceled_returns_interrupted() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("a.tar");
        build_tar(&archive, |b| add_file(b, "f.txt", b"data"));
        let dest = work.path().join("dest");
        let res = extract(&archive, &dest, &AtomicBool::new(true));
        assert!(
            matches!(res, Err(ArchiveError::Io(ref e)) if e.kind() == io::ErrorKind::Interrupted),
            "expected Interrupted, got {res:?}"
        );
    }

    // P2.13 regression: a hard-link entry must be rejected explicitly rather than
    // silently materialized as an empty regular file.
    #[test]
    fn extract_rejects_hard_link_entry() {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("a.tar");
        build_tar(&archive, |b| {
            add_file(b, "target.txt", b"real");
            let mut h = tar::Header::new_gnu();
            h.set_size(0);
            h.set_entry_type(tar::EntryType::Link);
            h.set_mode(0o644);
            b.append_link(&mut h, "hardlink.txt", "target.txt").unwrap();
        });
        let dest = work.path().join("dest");
        let res = extract(&archive, &dest, &AtomicBool::new(false));
        assert!(
            matches!(res, Err(ArchiveError::InvalidArchive(ref m)) if m.contains("hard link")),
            "expected a hard-link rejection, got {res:?}"
        );
    }
}
