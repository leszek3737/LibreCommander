use std::fs::{self, File};
use std::io;
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
    extracted_paths.reserve(entry_count.min(MAX_LIST_ENTRIES));

    fs::create_dir_all(dest)?;
    let canonical_dest = dest.canonicalize().map_err(ArchiveError::Io)?;
    let mut last_parent: Option<PathBuf> = None;
    for i in 0..entry_count.min(MAX_LIST_ENTRIES) {
        check_cancel(cancel)?;

        let mut entry = archive.by_index(i).map_err(map_zip_err)?;

        #[cfg(unix)]
        if entry.is_symlink() {
            let _ = progress.send(entry.size());
            continue;
        }

        let outpath = super::sanitize_entry_path(&canonical_dest, entry.name())?;

        if entry.size() > MAX_FILE_SIZE {
            return Err(ArchiveError::InvalidArchive(format!(
                "entry '{}' size {} exceeds maximum {MAX_FILE_SIZE}",
                entry.name(),
                entry.size()
            )));
        }

        if entry.is_dir() {
            fs::create_dir_all(&outpath)?;
            extracted_paths.push(outpath);
            let _ = progress.send(entry.size());
        } else {
            if entry.compressed_size() > MAX_FILE_SIZE {
                return Err(ArchiveError::InvalidArchive(format!(
                    "entry '{}' compressed size {} exceeds maximum {MAX_FILE_SIZE}",
                    entry.name(),
                    entry.compressed_size()
                )));
            }
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
            let mut outfile = super::open_outfile(&outpath)?;
            copy_with_progress(&mut entry, &mut outfile, progress, cancel)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let safe_mode = mode & !0o7000;
                    fs::set_permissions(&outpath, fs::Permissions::from_mode(safe_mode))?;
                }
            }

            extracted_paths.push(outpath);
        }
    }
    Ok(())
}

pub fn extract_zip(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let file = File::open(path)?;
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
    for source in sources {
        check_cancel(cancel)?;

        if fs::symlink_metadata(source)?.is_symlink() {
            debug_log!("add_sources_to_zip: skipping symlink {}", source.display());
            continue;
        }

        if source.is_dir() {
            add_dir_to_zip(zip, source, source, options, progress, cancel)?;
        } else {
            let name = source
                .file_name()
                .ok_or_else(|| ArchiveError::InvalidArchive("Invalid file name".into()))?
                .to_string_lossy();
            zip.start_file(&*name, *options).map_err(map_zip_err)?;
            let mut file = File::open(source)?;
            let bytes = io::copy(&mut file, zip)?;
            let _ = progress.send(bytes);
        }
    }

    Ok(())
}

pub fn create_zip(
    sources: &[PathBuf],
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let tmp_dest = dest.with_extension("zip.tmp");
    let file = File::create(&tmp_dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(DEFAULT_COMPRESSION_LEVEL));

    let result = add_sources_to_zip(sources, &mut zip, &options, progress, cancel);

    match result {
        Err(e) => {
            let _ = fs::remove_file(&tmp_dest);
            Err(e)
        }
        Ok(()) => {
            if let Err(e) = zip.finish().map_err(map_zip_err) {
                let _ = fs::remove_file(&tmp_dest);
                return Err(e);
            }
            fs::rename(&tmp_dest, dest)?;
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
) -> Result<(), ArchiveError> {
    for entry in fs::read_dir(dir)? {
        check_cancel(cancel)?;

        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(base)
            .map_err(|_| ArchiveError::InvalidArchive("strip_prefix failed".into()))?
            .to_string_lossy()
            .replace('\\', "/");

        let meta = fs::symlink_metadata(&path)?;
        if meta.is_symlink() {
            continue;
        }
        if path.is_dir() {
            zip.add_directory(&name, *options).map_err(map_zip_err)?;
            add_dir_to_zip(zip, base, &path, options, progress, cancel)?;
        } else {
            zip.start_file(&name, *options).map_err(map_zip_err)?;
            let mut file = File::open(&path)?;
            let bytes = io::copy(&mut file, zip)?;
            let _ = progress.send(bytes);
        }
    }
    Ok(())
}

fn zip_datetime_to_system_time(dt: zip::DateTime) -> SystemTime {
    static DAYS_BEFORE_MONTH: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let year = dt.year() as u64;
    let month = dt.month() as u64;
    let day = dt.day() as u64;

    let year = year.max(1970);
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
