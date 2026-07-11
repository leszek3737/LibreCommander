use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::SystemTime;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;

use super::{
    ArchiveEntry, ArchiveError, MAX_FILE_SIZE, MAX_LIST_ENTRIES, check_cancel, cleanup_extracted,
    copy_with_progress,
};
use crate::debug_log;

const DEFAULT_COMPRESSION_LEVEL: i64 = 6;

/// Upper bound on the number of entries written into a created archive. Mirrors
/// the tar create-side limit (`tar::MAX_CREATE_ENTRIES`) so both formats reject
/// pathologically large directory trees instead of exhausting memory/CPU.
const MAX_CREATE_ENTRIES: usize = 100_000;

/// Increments `count` for one archive entry and errors if the limit is crossed.
fn count_entry(count: &mut usize) -> Result<(), ArchiveError> {
    *count = count.saturating_add(1);
    if *count > MAX_CREATE_ENTRIES {
        return Err(ArchiveError::InvalidArchive(format!(
            "too many entries (limit {MAX_CREATE_ENTRIES})"
        )));
    }
    Ok(())
}

/// Removes a partially written temporary archive, logging any failure instead of
/// silently dropping the error (mirrors the tar create path).
fn cleanup_temp_file(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        debug_log!("create_zip: temp cleanup {} failed: {e}", path.display());
    }
}

fn map_zip_err(e: zip::result::ZipError) -> ArchiveError {
    match e {
        zip::result::ZipError::Io(io_err) => ArchiveError::Io(io_err),
        other => ArchiveError::InvalidArchive(other.to_string()),
    }
}

fn compression_method_name(method: CompressionMethod) -> &'static str {
    match method {
        CompressionMethod::Stored => "Stored",
        CompressionMethod::Deflated => "Deflated",
        CompressionMethod::Deflate64 => "Deflate64",
        CompressionMethod::Bzip2 => "Bzip2",
        CompressionMethod::Lzma => "Lzma",
        CompressionMethod::Ppmd => "Ppmd",
        CompressionMethod::Zstd => "Zstd",
        CompressionMethod::Xz => "Xz",
        CompressionMethod::Aes => "Aes",
        _ => "Unknown",
    }
}

pub fn list_zip(path: &Path) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(map_zip_err)?;

    let capacity = archive.len().min(MAX_LIST_ENTRIES);
    let mut entries = Vec::with_capacity(capacity);
    for i in (0..archive.len()).take(MAX_LIST_ENTRIES) {
        let entry = archive.by_index(i).map_err(map_zip_err)?;

        entries.push(ArchiveEntry {
            name: entry.name().to_string().into_boxed_str(),
            size: entry.size(),
            compressed_size: entry.compressed_size(),
            modified: entry.last_modified().map(zip_datetime_to_system_time),
            is_dir: entry.is_dir(),
            method: compression_method_name(entry.compression()).into(),
        });
    }
    Ok(entries)
}

fn extract_zip_entries(
    archive: &mut ZipArchive<File>,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
    extracted_paths: &mut Vec<PathBuf>,
) -> Result<(), ArchiveError> {
    let entry_count = archive.len();
    // No entry-count cap on extraction: it is bounded by the per-entry
    // `MAX_FILE_SIZE` check and the cumulative `TotalSizeGuard` byte cap below,
    // matching tar/7z. Capping at `MAX_LIST_ENTRIES` (a TUI listing limit) would
    // reject legitimately large archives — e.g. a `node_modules` tree with more
    // than 100k files — that the other formats extract without complaint. The
    // reserve is clamped so a crafted central directory advertising a huge entry
    // count can't force a giant up-front allocation.
    extracted_paths.reserve(entry_count.min(MAX_LIST_ENTRIES));

    fs::create_dir_all(dest)?;
    let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
    let mut total_size = super::TotalSizeGuard::default();
    for i in 0..entry_count {
        check_cancel(cancel)?;

        let mut entry = archive.by_index(i).map_err(map_zip_err)?;

        #[cfg(unix)]
        if entry.is_symlink() {
            let _ = progress.send(entry.size());
            continue;
        }

        let outpath = super::sanitize_entry_path(&canonical_dest, Path::new(entry.name()))?;

        if entry.size() > MAX_FILE_SIZE {
            return Err(ArchiveError::InvalidArchive(format!(
                "entry '{}' size {} exceeds maximum {MAX_FILE_SIZE}",
                entry.name(),
                entry.size()
            )));
        }

        if entry.is_dir() {
            // Only track directories THIS operation actually creates, so a later
            // rollback never `remove_dir_all`s a pre-existing user directory that
            // `create_dir_all` merely succeeded on idempotently.
            let newly_created = fs::symlink_metadata(&outpath).is_err();
            fs::create_dir_all(&outpath)?;
            super::verify_within_dest(&canonical_dest, &outpath)?;
            if newly_created {
                extracted_paths.push(outpath);
            }
            let _ = progress.send(entry.size());
        } else {
            if entry.compressed_size() > MAX_FILE_SIZE {
                return Err(ArchiveError::InvalidArchive(format!(
                    "entry '{}' compressed size {} exceeds maximum {MAX_FILE_SIZE}",
                    entry.name(),
                    entry.compressed_size()
                )));
            }
            // Re-verify the parent on EVERY entry rather than caching the last
            // one: a cached parent could be swapped for a symlink between entries
            // (TOCTOU), and skipping `verify_within_dest` on a cache hit would let
            // a later entry be written outside `canonical_dest`.
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
                super::verify_within_dest(&canonical_dest, parent)?;
            }
            super::check_symlink_at_dest(&outpath)?;
            let mut outfile = super::open_outfile(&outpath)?;
            // Register the file for rollback BEFORE copying: a mid-copy failure
            // (cancel / size-limit / IO error) must still clean up the partial
            // file, which pushing only after the copy would miss.
            extracted_paths.push(outpath.clone());
            let written = copy_with_progress(&mut entry, &mut outfile, progress, cancel)?;
            total_size.add(written)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let safe_mode = mode & !0o7000;
                    fs::set_permissions(&outpath, fs::Permissions::from_mode(safe_mode))?;
                }
            }
        }
    }
    Ok(())
}

pub fn extract_zip(
    file: std::fs::File,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut archive = ZipArchive::new(file).map_err(map_zip_err)?;

    let mut extracted_paths: Vec<PathBuf> = Vec::new();

    let result = extract_zip_entries(&mut archive, dest, progress, cancel, &mut extracted_paths);

    if result.is_err() {
        cleanup_extracted(&extracted_paths);
    }

    result
}

fn add_sources_to_zip(
    sources: &[PathBuf],
    zip: &mut zip::ZipWriter<File>,
    options: &SimpleFileOptions,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let mut count: usize = 0;
    for source in sources {
        check_cancel(cancel)?;

        // Top-level symlinks are filtered by create_archive; this metadata read
        // distinguishes dir vs file. Per-entry symlinks are filtered in the walk
        // (add_dir_to_zip) and the final open is hardened with O_NOFOLLOW.
        let meta = fs::symlink_metadata(source)?;
        if meta.is_dir() {
            add_dir_to_zip(zip, source, source, options, progress, cancel, &mut count)?;
        } else {
            count_entry(&mut count)?;
            let name = source
                .file_name()
                .ok_or_else(|| {
                    ArchiveError::InvalidArchive(format!(
                        "source '{}' has no file name (root paths have none)",
                        source.display()
                    ))
                })?
                .to_string_lossy();
            add_file_to_zip(zip, &name, source, options, progress, cancel)?;
        }
    }

    Ok(())
}

fn add_file_to_zip(
    zip: &mut zip::ZipWriter<File>,
    name: &str,
    source: &Path,
    options: &SimpleFileOptions,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    // Open with O_NOFOLLOW before creating the entry so a symlink swapped in
    // after the create-side filter (TOCTOU) is skipped without leaving an empty
    // entry behind. `None` means the final component became a symlink.
    let Some(mut file) = super::open_source_nofollow(source)? else {
        return Ok(());
    };
    zip.start_file(name, *options).map_err(map_zip_err)?;
    // `copy_with_progress` checks `cancel` every chunk, so a single large file is
    // interruptible mid-copy — plain `io::copy` would run to completion first.
    copy_with_progress(&mut file, zip, progress, cancel)?;
    Ok(())
}

pub fn create_zip(
    sources: &[PathBuf],
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    // Stage next to the destination (same filesystem for an atomic rename) using
    // `create_new` so we never truncate an existing file or follow a symlink
    // planted at a predictable `dest.zip.tmp` path.
    let dest_dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let (file, tmp_dest) =
        super::create_temp_file_in(dest_dir, ".lc-zip", ".tmp").map_err(ArchiveError::Io)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(DEFAULT_COMPRESSION_LEVEL));

    let result = add_sources_to_zip(sources, &mut zip, &options, progress, cancel);

    match result {
        Err(e) => {
            cleanup_temp_file(&tmp_dest);
            Err(e)
        }
        Ok(()) => {
            if let Err(e) = zip.finish().map_err(map_zip_err) {
                cleanup_temp_file(&tmp_dest);
                return Err(e);
            }
            // Clean up the staged archive if the final rename fails, instead of
            // orphaning it next to the destination.
            if let Err(e) = fs::rename(&tmp_dest, dest) {
                cleanup_temp_file(&tmp_dest);
                return Err(ArchiveError::Io(e));
            }
            Ok(())
        }
    }
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<File>,
    base: &Path,
    dir: &Path,
    options: &SimpleFileOptions,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
    count: &mut usize,
) -> Result<(), ArchiveError> {
    for entry in fs::read_dir(dir)? {
        check_cancel(cancel)?;

        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(base)
            .map_err(|e| {
                ArchiveError::InvalidArchive(format!(
                    "strip_prefix failed for {}: {e}",
                    path.display()
                ))
            })?
            .to_string_lossy()
            .replace('\\', "/");

        // Single symlink_metadata read: skip symlinks (create-side filter) and
        // reuse the same metadata to distinguish dir vs file, avoiding a second
        // syscall.
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            count_entry(count)?;
            zip.add_directory(&name, *options).map_err(map_zip_err)?;
            add_dir_to_zip(zip, base, &path, options, progress, cancel, count)?;
        } else {
            count_entry(count)?;
            add_file_to_zip(zip, &name, &path, options, progress, cancel)?;
        }
    }
    Ok(())
}

fn zip_datetime_to_system_time(dt: zip::DateTime) -> SystemTime {
    static DAYS_BEFORE_MONTH: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let year = dt.year() as u64;
    let month = dt.month() as u64;
    let day = dt.day() as u64;

    // year clamped to the ZIP/DOS epoch (1980): zip::DateTime's valid range is
    // 1980-2107, so earlier years cannot occur in a well-formed archive; the
    // clamp is a defensive no-op that keeps the arithmetic below non-negative.
    let year = year.max(1980);
    let month = month.clamp(1, 12);

    let leap_years = (year - 1) / 4 - (year - 1) / 100 + (year - 1) / 400;
    let is_leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let leap_adj = if is_leap && month > 2 { 1 } else { 0 };

    let days_since_epoch = (year - 1970) * 365 + leap_years - (1969 / 4 - 1969 / 100 + 1969 / 400)
        + DAYS_BEFORE_MONTH.get(month as usize - 1).unwrap_or(&0)
        + leap_adj
        + day
        - 1;

    let secs = days_since_epoch * 24 * 3600
        + dt.hour() as u64 * 3600
        + dt.minute() as u64 * 60
        + dt.second() as u64;
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io;
    // Used only by the unix-gated symlink tests below.
    #[cfg(unix)]
    use std::io::Write;
    use std::sync::mpsc;

    #[cfg(unix)]
    #[test]
    fn extract_rejects_entry_through_symlinked_dir() {
        let work = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let archive_path = work.path().join("evil.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("subdir/file.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"pwned").unwrap();
        writer.finish().unwrap();

        let dest = work.path().join("extract");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(outside.path(), dest.join("subdir")).unwrap();

        let (tx, _rx) = mpsc::channel();
        let result = extract_zip(
            File::open(&archive_path).unwrap(),
            &dest,
            &tx,
            &AtomicBool::new(false),
        );
        assert!(result.is_err());
        assert!(!outside.path().join("file.txt").exists());
    }

    #[test]
    fn create_zip_basic_with_multiple_files() {
        let work = tempfile::tempdir().unwrap();
        let src_dir = work.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let f1 = src_dir.join("a.txt");
        let f2 = src_dir.join("b.txt");
        fs::write(&f1, b"alpha").unwrap();
        fs::write(&f2, b"beta").unwrap();

        let archive_path = work.path().join("out.zip");
        let (tx, _rx) = mpsc::channel();
        let sources = vec![f1, f2];
        create_zip(&sources, &archive_path, &tx, &AtomicBool::new(false)).unwrap();
        assert!(archive_path.is_file());

        let file = File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2);

        let names: std::collections::HashSet<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "a.txt"));
        assert!(names.iter().any(|n| n == "b.txt"));
    }

    #[test]
    fn create_zip_canceled_returns_error_without_panicking() {
        let work = tempfile::tempdir().unwrap();
        let src_dir = work.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let f1 = src_dir.join("a.txt");
        fs::write(&f1, b"alpha").unwrap();

        let archive_path = work.path().join("canceled.zip");
        let (tx, _rx) = mpsc::channel();
        // Cancel flag already set: creation must bail out with an Interrupted
        // error and leave no archive (the temp file is cleaned up).
        let cancel = AtomicBool::new(true);
        let result = create_zip(&[f1], &archive_path, &tx, &cancel);
        assert!(
            matches!(result, Err(ArchiveError::Io(ref e)) if e.kind() == io::ErrorKind::Interrupted),
            "expected Interrupted error, got {result:?}"
        );
        assert!(!archive_path.exists());
    }

    #[test]
    fn create_zip_includes_nested_dir_entries() {
        let work = tempfile::tempdir().unwrap();
        let base = work.path().join("root");
        let sub = base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("inner.txt"), b"inner").unwrap();

        let archive_path = work.path().join("nested.zip");
        let (tx, _rx) = mpsc::channel();
        let sources = vec![base];
        create_zip(&sources, &archive_path, &tx, &AtomicBool::new(false)).unwrap();

        let file = File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with("inner.txt")),
            "expected inner.txt entry, got {names:?}"
        );
    }

    #[test]
    fn create_archive_empty_sources_returns_no_valid_sources() {
        let work = tempfile::tempdir().unwrap();
        let archive_path = work.path().join("empty.zip");
        let (tx, _rx) = mpsc::channel();
        let result = super::super::create::create_archive(
            &[],
            &archive_path,
            super::super::ArchiveFormat::Zip,
            &tx,
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(ArchiveError::NoValidSources)));
    }

    #[test]
    fn compression_method_name_covers_all_supported() {
        assert_eq!(compression_method_name(CompressionMethod::Stored), "Stored");
        assert_eq!(
            compression_method_name(CompressionMethod::Deflated),
            "Deflated"
        );
        assert_eq!(
            compression_method_name(CompressionMethod::Deflate64),
            "Deflate64"
        );
        assert_eq!(compression_method_name(CompressionMethod::Bzip2), "Bzip2");
        assert_eq!(compression_method_name(CompressionMethod::Lzma), "Lzma");
        assert_eq!(compression_method_name(CompressionMethod::Ppmd), "Ppmd");
        assert_eq!(compression_method_name(CompressionMethod::Zstd), "Zstd");
        assert_eq!(compression_method_name(CompressionMethod::Xz), "Xz");
        assert_eq!(compression_method_name(CompressionMethod::Aes), "Aes");
    }

    #[test]
    fn zip_datetime_handles_minimum_year() {
        let dt = zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap();
        let st = zip_datetime_to_system_time(dt);
        let elapsed = st.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        assert!(elapsed.as_secs() > 0);
    }
}
