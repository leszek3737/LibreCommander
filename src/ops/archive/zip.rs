use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::SystemTime;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;

use super::{ArchiveEntry, ArchiveError};

const MAX_LIST_ENTRIES: usize = 100_000;

pub fn list_zip(path: &Path) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let file = File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

    let mut entries = Vec::new();
    for i in 0..archive.len() {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        let entry = archive
            .by_index(i)
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

        entries.push(ArchiveEntry {
            name: entry.name().to_string(),
            size: entry.size(),
            compressed_size: entry.compressed_size(),
            modified: entry.last_modified().map(zip_datetime_to_system_time),
            is_dir: entry.is_dir(),
            method: format!("{:?}", entry.compression()),
        });
    }
    Ok(entries)
}

fn sanitize_entry_path(entry_name: &str, dest: &Path) -> Result<PathBuf, ArchiveError> {
    let entry_path = Path::new(entry_name);
    if entry_path.is_absolute() {
        return Err(ArchiveError::InvalidArchive(format!(
            "absolute path detected: {entry_name}"
        )));
    }
    // Reject any entry containing parent-directory components
    for component in entry_path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(ArchiveError::InvalidArchive(format!(
                "path traversal detected: {entry_name}"
            )));
        }
    }
    let outpath = dest.join(entry_name);
    let canonical_dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    let canonical_out = outpath
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(outpath.file_name().unwrap_or_default()))
        .unwrap_or_else(|| outpath.clone());

    if !canonical_out.starts_with(&canonical_dest) {
        return Err(ArchiveError::InvalidArchive(format!(
            "path traversal detected: {entry_name}"
        )));
    }
    Ok(outpath)
}

pub fn extract_zip(
    path: &Path,
    dest: &Path,
    progress: &Sender<u64>,
    cancel: &AtomicBool,
) -> Result<(), ArchiveError> {
    let file = File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

    let mut extracted_paths: Vec<PathBuf> = Vec::new();

    let result = (|| -> Result<(), ArchiveError> {
        for i in 0..archive.len() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ArchiveError::Io(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation canceled",
                )));
            }

            let mut entry = archive
                .by_index(i)
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;

            #[cfg(unix)]
            if entry.is_symlink() {
                continue;
            }

            let outpath = sanitize_entry_path(entry.name(), dest)?;

            if entry.is_dir() {
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
                    if let Some(mode) = entry.unix_mode() {
                        let safe_mode = mode & !0o7000;
                        fs::set_permissions(&outpath, fs::Permissions::from_mode(safe_mode))?;
                    }
                }
            }

            let _ = progress.send(entry.size());
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
        .compression_level(Some(6));

    let result = (|| -> Result<(), ArchiveError> {
        for source in sources {
            if cancel.load(Ordering::Relaxed) {
                return Err(ArchiveError::Io(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation canceled",
                )));
            }

            if source.is_dir() {
                add_dir_to_zip(&mut zip, source, source, &options, progress, cancel)?;
            } else {
                let name = source
                    .file_name()
                    .ok_or_else(|| ArchiveError::InvalidArchive("Invalid file name".into()))?
                    .to_string_lossy();
                zip.start_file(&name, options)
                    .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
                let mut file = File::open(source)?;
                let bytes = io::copy(&mut file, &mut zip)?;
                let _ = progress.send(bytes);
            }
        }

        zip.finish()
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        Ok(())
    })();

    if let Err(e) = &result {
        let _ = fs::remove_file(&tmp_dest);
        return Err(e.clone());
    }
    fs::rename(&tmp_dest, dest)?;
    Ok(())
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
        if cancel.load(Ordering::Relaxed) {
            return Err(ArchiveError::Io(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation canceled",
            )));
        }

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
            zip.add_directory(&name, *options)
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
            add_dir_to_zip(zip, base, &path, options, progress, cancel)?;
        } else {
            zip.start_file(&name, *options)
                .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
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
