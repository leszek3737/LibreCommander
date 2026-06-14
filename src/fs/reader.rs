//! Filesystem reader module.
//! Note: This module uses Unix-specific APIs (MetadataExt, uid/gid lookups)
//! and will only compile on Unix platforms.

#[cfg(test)]
use chrono::{DateTime, Local};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
#[cfg(test)]
use std::time::SystemTime;

use crate::app::types::{PanelListing, PanelState, compute_category, sanitize_name};
use crate::fs::cha::Cha;

/// Maximum number of uid/gid name mappings to keep per cache.
/// 1024 covers typical multi-user systems; oldest entries are evicted (FIFO) when exceeded.
const CACHE_MAX_SIZE: usize = 1024;

const INITIAL_DIR_CAPACITY: usize = 256;

#[cfg(test)]
use crate::app::types::format_permissions;

pub use crate::app::types::FileEntry;

#[cfg(unix)]
struct NameMapCache {
    map: HashMap<u32, Arc<str>>,
    order: VecDeque<u32>,
}

#[cfg(unix)]
impl NameMapCache {
    /// Reset to a consistent empty state, dropping any partially-mutated data.
    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

#[cfg(unix)]
struct UidCache {
    uid: NameMapCache,
    gid: NameMapCache,
}

#[cfg(unix)]
impl UidCache {
    /// Reset both id->name caches to a consistent empty state.
    fn clear(&mut self) {
        self.uid.clear();
        self.gid.clear();
    }
}

#[cfg(unix)]
static UID_CACHE: LazyLock<Mutex<UidCache>> = LazyLock::new(|| {
    Mutex::new(UidCache {
        uid: NameMapCache {
            map: HashMap::new(),
            order: VecDeque::new(),
        },
        gid: NameMapCache {
            map: HashMap::new(),
            order: VecDeque::new(),
        },
    })
});

#[cfg(unix)]
fn get_or_insert_name(
    cache: &mut NameMapCache,
    id: u32,
    lookup: impl FnOnce(u32) -> Option<Arc<str>>,
) -> Arc<str> {
    if let Some(name) = cache.map.get(&id) {
        return name.clone();
    }
    if cache.map.len() >= CACHE_MAX_SIZE
        && let Some(old) = cache.order.pop_front()
    {
        cache.map.remove(&old);
    }
    cache.order.push_back(id);
    let name = lookup(id).unwrap_or_else(|| Arc::from(id.to_string()));
    cache.map.insert(id, name.clone());
    name
}

#[cfg(unix)]
fn os_str_to_arc(s: &std::ffi::OsStr) -> Arc<str> {
    use std::os::unix::ffi::OsStrExt;
    // Borrow-free conversion when the bytes are valid UTF-8; otherwise fall
    // back to an explicit lossy conversion. The error is handled, not
    // suppressed, so no clippy::unwrap_used allow is needed.
    match std::str::from_utf8(s.as_bytes()) {
        Ok(valid) => Arc::from(valid),
        Err(_) => Arc::from(s.to_string_lossy().into_owned()),
    }
}

#[cfg(unix)]
fn lookup_owner_group(uid: u32, gid: u32) -> (Arc<str>, Arc<str>) {
    let mut cache = match UID_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // A previous holder panicked mid-mutation, so map/order may be out
            // of sync. Take ownership of the guard, dump the suspect data, and
            // rebuild from an empty, consistent state rather than trusting it.
            let mut guard = poisoned.into_inner();
            guard.clear();
            // Un-poison the lock so subsequent callers take the fast Ok path and
            // normal caching resumes; otherwise every later lock would re-clear
            // the (now-consistent) cache, degrading to no-cache mode forever.
            UID_CACHE.clear_poison();
            guard
        }
    };
    let owner = get_or_insert_name(&mut cache.uid, uid, |id| {
        users::get_user_by_uid(id).map(|u| os_str_to_arc(u.name()))
    });
    let group = get_or_insert_name(&mut cache.gid, gid, |id| {
        users::get_group_by_gid(id).map(|g| os_str_to_arc(g.name()))
    });
    (owner, group)
}

#[cfg(not(unix))]
fn lookup_owner_group(_uid: u32, _gid: u32) -> (Arc<str>, Arc<str>) {
    (Arc::from("-"), Arc::from("-"))
}

fn is_parent_entry(entry: &FileEntry) -> bool {
    entry.name == ".."
}

/// Lossy `OsStr` -> `String` conversion shared by the entry builders.
/// Centralizes the `to_string_lossy().into_owned()` pattern so file-name
/// handling stays consistent across callers.
fn os_str_to_string(s: &std::ffi::OsStr) -> String {
    s.to_string_lossy().into_owned()
}

fn file_name_from_path(path: &Path) -> String {
    os_str_to_string(path.file_name().unwrap_or_default())
}

fn build_file_entry(entry: &std::fs::DirEntry) -> io::Result<FileEntry> {
    let path = entry.path();
    let file_name = os_str_to_string(&entry.file_name());
    let metadata = entry.metadata()?;
    let is_symlink = metadata.is_symlink();
    let target_meta = if is_symlink {
        fs::metadata(&path).ok()
    } else {
        None
    };

    Ok(build_file_entry_from_metadata(
        path,
        file_name,
        &metadata,
        target_meta.as_ref(),
    ))
}

fn build_file_entry_from_metadata(
    path: PathBuf,
    file_name: String,
    metadata: &fs::Metadata,
    target_meta: Option<&fs::Metadata>,
) -> FileEntry {
    let is_symlink = metadata.is_symlink();
    let mut cha = if is_symlink {
        Cha::from_link_metadata(metadata, target_meta)
    } else {
        Cha::new(metadata)
    };
    cha.set_hidden(file_name.starts_with('.'));

    let (uid, gid) = (cha.uid, cha.gid);
    let (owner, group) = lookup_owner_group(uid, gid);

    let (time_str, size_str, name_width, size_width, time_width) =
        FileEntry::cached_fields(&cha, &file_name);
    let category = compute_category(&cha, &file_name);

    let sanitized = sanitize_name(&file_name);
    FileEntry {
        name: file_name,
        path,
        cha,
        owner,
        group,
        selected: false,
        mime_type: None,
        time_str,
        size_str,
        name_width,
        size_width,
        time_width,
        category,
        sanitized_name: sanitized,
    }
}

fn rebuild_path_index(listing: &mut PanelListing) {
    listing.path_index.clear();
    listing.path_index.reserve(listing.unfiltered_entries.len());
    for (i, entry) in listing.unfiltered_entries.iter().enumerate() {
        listing.path_index.insert(entry.path.clone(), i);
    }
}

pub fn ensure_path_index(panel: &mut PanelState) {
    if !panel.listing.path_index.is_empty() {
        return;
    }
    rebuild_path_index(&mut panel.listing);
}

pub fn read_directory(path: &Path) -> io::Result<(Vec<FileEntry>, Vec<io::Error>)> {
    let mut entries = Vec::with_capacity(INITIAL_DIR_CAPACITY);
    let mut errors = Vec::new();

    if path != Path::new("/") {
        let parent_buf;
        let parent_path = path.parent().filter(|p| !p.as_os_str().is_empty());
        let parent_path = match parent_path {
            Some(p) => p,
            None => {
                parent_buf = path.join("..");
                &parent_buf
            }
        };
        let (owner, group) = fs::symlink_metadata(parent_path)
            .ok()
            .map(|meta| {
                let cha = Cha::new(&meta);
                lookup_owner_group(cha.uid, cha.gid)
            })
            .unwrap_or_default();
        let dummy_cha = Cha::dummy_dir();
        let (time_str, size_str, name_width, size_width, time_width) =
            FileEntry::cached_fields(&dummy_cha, "..");
        let category = compute_category(&dummy_cha, "..");
        entries.push(FileEntry {
            name: "..".to_string(),
            path: parent_path.to_path_buf(),
            cha: dummy_cha,
            owner,
            group,
            selected: false,
            mime_type: None,
            time_str,
            size_str,
            name_width,
            size_width,
            time_width,
            category,
            sanitized_name: None,
        });
    }

    // read_dir is intentionally sequential. Parallel iteration via rayon
    // would require restructured error reporting (collecting per-entry
    // errors across threads). The sequential path is fast enough for
    // interactive directory browsing.
    for result in fs::read_dir(path)? {
        let entry = match result {
            Ok(entry) => entry,
            Err(e) => {
                errors.push(io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to read directory entry in '{}': {}",
                        path.display(),
                        e
                    ),
                ));
                continue;
            }
        };
        match build_file_entry(&entry) {
            Ok(file_entry) => entries.push(file_entry),
            Err(e) => errors.push(io::Error::new(
                e.kind(),
                format!("Failed to read '{}': {}", entry.path().display(), e),
            )),
        }
    }

    Ok((entries, errors))
}

pub fn get_file_info(path: &Path) -> io::Result<FileEntry> {
    let file_name = file_name_from_path(path);
    let metadata = fs::symlink_metadata(path)?;
    let is_symlink = metadata.is_symlink();
    let target_meta = if is_symlink {
        fs::metadata(path).ok()
    } else {
        None
    };

    Ok(build_file_entry_from_metadata(
        path.to_path_buf(),
        file_name,
        &metadata,
        target_meta.as_ref(),
    ))
}

/// Build a [`FileEntry`] from already-fetched, non-symlink metadata.
///
/// Lets callers that have just `stat`ed a path (e.g. the recursive name search,
/// which `lstat`s directories for cycle detection) avoid a second `stat` inside
/// [`get_file_info`]. `metadata` must be the entry's own (`lstat`) metadata and
/// the entry must not be a symlink — symlinks need their target metadata, which
/// this fast path does not resolve.
pub fn file_info_from_metadata(path: PathBuf, metadata: &fs::Metadata) -> FileEntry {
    debug_assert!(
        !metadata.is_symlink(),
        "file_info_from_metadata requires non-symlink metadata (symlinks need target metadata): {}",
        path.display()
    );
    let file_name = file_name_from_path(&path);
    build_file_entry_from_metadata(path, file_name, metadata, None)
}

pub fn upsert_entry(panel: &mut PanelState, mut entry: FileEntry) {
    if is_parent_entry(&entry) {
        return;
    }

    ensure_path_index(panel);

    if let Some(&idx) = panel.listing.path_index.get(&entry.path) {
        if let Some(existing) = panel.listing.unfiltered_entries.get_mut(idx) {
            entry.selected = existing.selected;
            *existing = entry;
        }
    } else {
        let new_idx = panel.listing.unfiltered_entries.len();
        panel.listing.path_index.insert(entry.path.clone(), new_idx);
        panel.listing.unfiltered_entries.push(entry);
    }
}

pub fn remove_entry(panel: &mut PanelState, path: &Path) {
    if panel.listing.unfiltered_entries.is_empty() {
        return;
    }

    ensure_path_index(panel);
    if let Some(idx) = panel.listing.path_index.remove(path) {
        let last = panel.listing.unfiltered_entries.len() - 1;
        if idx < last {
            let last_path = panel.listing.unfiltered_entries[last].path.clone();
            panel.listing.unfiltered_entries.swap_remove(idx);
            panel.listing.path_index.insert(last_path, idx);
        } else {
            panel.listing.unfiltered_entries.pop();
        }
    }
}

#[cfg(test)]
pub fn format_date(time: SystemTime) -> String {
    let datetime: DateTime<Local> = DateTime::from(time);
    let now = Local::now();
    let days = now.signed_duration_since(datetime).num_days();

    if (0..=365).contains(&days) {
        datetime.format("%b %d %H:%M").to_string()
    } else {
        datetime.format("%b %d  %Y").to_string()
    }
}

#[cfg(test)]
#[cfg(unix)]
pub fn is_executable(mode: u32) -> bool {
    (mode & 0o100) != 0 || (mode & 0o010) != 0 || (mode & 0o001) != 0
}

#[cfg(test)]
#[cfg(not(unix))]
pub fn is_executable(_mode: u32) -> bool {
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::types::FileEntry as CanonicalFileEntry;
    use std::fs::{self, File};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    fn test_entry(name: &str, selected: bool) -> FileEntry {
        FileEntry::builder()
            .name(name)
            .path(std::env::temp_dir().join(name))
            .size(10)
            .modified(SystemTime::now())
            .created(SystemTime::now())
            .is_hidden(name.starts_with('.'))
            .owner("user")
            .group("group")
            .selected(selected)
            .build()
    }

    fn parent_entry() -> FileEntry {
        FileEntry::builder()
            .name("..")
            .path(std::env::temp_dir())
            .is_dir(true)
            .is_executable(true)
            .modified(SystemTime::now())
            .permissions(0o755)
            .build()
    }

    fn test_panel(entries: Vec<FileEntry>) -> PanelState {
        let mut panel = PanelState::new(std::env::temp_dir());
        panel.listing.entries = entries;
        panel.recalculate_selection_stats();
        panel
    }

    #[test]
    fn test_format_size_zero() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(0)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        assert_eq!(entry.display_size(), "   0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(500)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        assert_eq!(entry.display_size(), " 500 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(1536)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        let result = entry.display_size();
        assert!(result.contains("KB"));
    }

    #[test]
    fn test_format_size_megabytes() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(1024 * 1024)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        let result = entry.display_size();
        assert!(result.contains("MB"));
    }

    #[test]
    fn test_format_size_gigabytes() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(1024 * 1024 * 1024)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        let result = entry.display_size();
        assert!(result.contains("GB"));
    }

    #[test]
    fn test_format_size_terabytes() {
        let entry = CanonicalFileEntry::builder()
            .name("test")
            .path(PathBuf::from("test"))
            .size(1024u64.pow(4))
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .build();
        let result = entry.display_size();
        assert!(result.contains("TB"));
    }

    #[test]
    fn test_format_permissions_rwx() {
        assert_eq!(format_permissions(0o755), "rwxr-xr-x");
        assert_eq!(format_permissions(0o644), "rw-r--r--");
        assert_eq!(format_permissions(0o700), "rwx------");
        assert_eq!(format_permissions(0o000), "---------");
        assert_eq!(format_permissions(0o777), "rwxrwxrwx");
    }

    #[test]
    fn test_is_executable() {
        assert!(is_executable(0o100));
        assert!(is_executable(0o010));
        assert!(is_executable(0o001));
        assert!(is_executable(0o755));
        assert!(!is_executable(0o644));
        assert!(!is_executable(0o000));
    }

    #[test]
    fn test_os_str_to_string_helper() {
        use std::ffi::OsStr;
        assert_eq!(os_str_to_string(OsStr::new("file.txt")), "file.txt");
        assert_eq!(os_str_to_string(OsStr::new("")), "");
        // Multi-byte but valid UTF-8 round-trips unchanged.
        assert_eq!(os_str_to_string(OsStr::new("łódź")), "łódź");
    }

    #[cfg(unix)]
    #[test]
    fn test_os_str_to_arc_valid_utf8() {
        use std::ffi::OsStr;
        // Pure ASCII and multi-byte valid UTF-8 both take the Ok path.
        assert_eq!(&*os_str_to_arc(OsStr::new("hello")), "hello");
        assert_eq!(&*os_str_to_arc(OsStr::new("héllo")), "héllo");
    }

    #[cfg(unix)]
    #[test]
    fn test_os_str_to_arc_invalid_utf8_falls_back_lossy() {
        use std::os::unix::ffi::OsStrExt;
        // 0xFF is never valid UTF-8; the Err arm must use the lossy fallback.
        let os = std::ffi::OsStr::from_bytes(b"bad\xFFname");
        let arc = os_str_to_arc(os);
        assert!(arc.starts_with("bad"));
        assert!(arc.ends_with("name"));
        assert!(arc.contains('\u{FFFD}'));
    }

    #[test]
    fn test_format_date_recent() {
        let recent = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 7);
        let result = format_date(recent);
        assert!(result.contains(':'));
    }

    #[test]
    fn test_format_date_old() {
        let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 400);
        let result = format_date(old);
        assert!(!result.contains(':'));
    }

    #[test]
    fn test_read_directory_basic() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        File::create(temp_dir.join("file1.txt")).unwrap();
        File::create(temp_dir.join("file2.txt")).unwrap();
        fs::create_dir(temp_dir.join("subdir")).unwrap();

        let (entries, errors) = read_directory(temp_dir).unwrap();
        assert!(errors.is_empty());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&".."));
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"file2.txt"));
        assert!(names.contains(&"subdir"));

        let subdir_entry = entries.iter().find(|e| e.name == "subdir").unwrap();
        assert!(subdir_entry.is_dir());
    }

    #[test]
    fn test_read_directory_hidden_files() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        File::create(temp_dir.join("visible.txt")).unwrap();
        File::create(temp_dir.join(".hidden")).unwrap();

        let (entries, errors) = read_directory(temp_dir).unwrap();
        assert!(errors.is_empty());
        assert!(entries.iter().any(|e| e.name == ".hidden"));
        assert!(entries.iter().any(|e| e.name == "visible.txt"));
    }

    #[test]
    fn test_get_file_info_file() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        let file_path = temp_dir.join("test.txt");
        File::create(&file_path).unwrap();

        let info = get_file_info(&file_path).unwrap();
        assert_eq!(info.name, "test.txt");
        assert!(!info.is_dir());
    }

    #[test]
    fn test_get_file_info_directory() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        let subdir = temp_dir.join("subdir");
        fs::create_dir(&subdir).unwrap();

        let info = get_file_info(&subdir).unwrap();
        assert_eq!(info.name, "subdir");
        assert!(info.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn test_get_file_info_executable() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        let file_path = temp_dir.join("script.sh");
        let file = File::create(&file_path).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms).unwrap();

        let info = get_file_info(&file_path).unwrap();
        assert!(info.is_executable());
    }

    #[cfg(unix)]
    #[test]
    fn test_read_directory_symlinks() {
        let temp = TempDir::new().unwrap();
        let temp_dir = temp.path();
        let target = temp_dir.join("target.txt");
        File::create(&target).unwrap();
        let link = temp_dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let (entries, errors) = read_directory(temp_dir).unwrap();
        assert!(errors.is_empty());
        if let Some(link_entry) = entries.iter().find(|e| e.name == "link.txt") {
            assert!(link_entry.is_symlink());
        }
    }

    #[test]
    fn test_read_directory_missing_path_still_returns_error() {
        let missing = std::env::temp_dir().join(format!(
            "lc_reader_missing_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let err = read_directory(&missing).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_upsert_entry_adds_new_entry() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("b.txt", false)]);
        panel.listing.unfiltered_entries = panel.listing.entries.clone();
        upsert_entry(&mut panel, test_entry("a.txt", false));

        assert!(
            panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.name == "a.txt")
        );
        assert_eq!(panel.listing.unfiltered_entries.len(), 3);
    }

    #[test]
    fn test_upsert_entry_updates_existing_and_preserves_selection() {
        let mut panel = test_panel(vec![test_entry("file.txt", true)]);
        panel.listing.unfiltered_entries = panel.listing.entries.clone();
        let mut updated = test_entry("file.txt", false);
        updated.cha.len = 99;

        upsert_entry(&mut panel, updated);

        assert_eq!(panel.listing.unfiltered_entries.len(), 1);
        assert_eq!(panel.listing.unfiltered_entries[0].cha.len, 99);
        assert!(panel.listing.unfiltered_entries[0].selected);
    }

    #[test]
    fn test_remove_entry_removes_matching_path() {
        let removed = test_entry("remove.txt", true);
        let mut panel = test_panel(vec![
            parent_entry(),
            removed.clone(),
            test_entry("keep.txt", false),
        ]);
        panel.listing.unfiltered_entries = panel.listing.entries.clone();

        remove_entry(&mut panel, &removed.path);

        assert!(
            !panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.name == "remove.txt")
        );
        assert!(
            panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.name == "keep.txt")
        );
    }

    #[test]
    fn test_upsert_adds_hidden_to_unfiltered() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("visible.txt", false)]);
        panel.listing.unfiltered_entries = panel.listing.entries.clone();
        panel.set_show_hidden(false);
        upsert_entry(&mut panel, test_entry(".hidden", false));

        assert!(
            panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.name == ".hidden")
        );
    }

    #[test]
    fn test_upsert_with_empty_unfiltered_inserts_entry() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("main.rs", false)]);
        panel.set_filter(Some("*.rs".to_string()));

        upsert_entry(&mut panel, test_entry("notes.txt", false));

        assert_eq!(panel.listing.unfiltered_entries.len(), 1);
        assert_eq!(panel.listing.unfiltered_entries[0].name, "notes.txt");
    }

    #[test]
    fn test_remove_entry_preserves_parent_entry() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("file.txt", false)]);
        panel.listing.unfiltered_entries = panel.listing.entries.clone();

        remove_entry(&mut panel, &std::env::temp_dir().join("file.txt"));

        assert!(
            panel
                .listing
                .unfiltered_entries
                .iter()
                .any(|entry| entry.name == "..")
        );
    }
}
