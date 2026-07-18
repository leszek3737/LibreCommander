//! Filesystem reader module.
//! Note: uid/gid owner-name lookups use Unix-specific APIs and are
//! `cfg(unix)`-gated; other platforms get empty owner/group names.

#[cfg(unix)]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(unix)]
use std::sync::{LazyLock, Mutex};

use crate::app::types::PanelState;
use crate::fs::cha::Cha;

/// Maximum number of uid/gid name mappings to keep per cache.
/// 1024 covers typical multi-user systems; oldest entries are evicted (FIFO) when exceeded.
#[cfg(unix)]
const CACHE_MAX_SIZE: usize = 1024;

const INITIAL_DIR_CAPACITY: usize = 256;

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

/// Locks the global id->name cache, recovering from poisoning by clearing the
/// (possibly inconsistent) cache and un-poisoning so normal caching resumes.
#[cfg(unix)]
fn lock_cache() -> std::sync::MutexGuard<'static, UidCache> {
    match UID_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // A previous holder panicked mid-mutation, so map/order may be out of
            // sync. Take ownership, dump the suspect data, and rebuild from an
            // empty, consistent state rather than trusting it.
            let mut guard = poisoned.into_inner();
            guard.clear();
            UID_CACHE.clear_poison();
            guard
        }
    }
}

#[cfg(unix)]
fn get_or_insert_name(
    select: impl Fn(&mut UidCache) -> &mut NameMapCache,
    id: u32,
    lookup: impl FnOnce(u32) -> Option<Arc<str>>,
) -> Arc<str> {
    // Phase 1: fast cache hit, then DROP the lock before any lookup. The lookup
    // (getpwuid/getgrgid via NSS) can block for hundreds of ms on networked
    // backends (LDAP/AD/SSSD); holding the single global mutex across it would
    // stall every other thread resolving metadata.
    {
        let mut cache = lock_cache();
        if let Some(name) = select(&mut cache).map.get(&id) {
            return name.clone();
        }
    }
    // Phase 2: possibly-slow lookup with NO lock held.
    let name = lookup(id).unwrap_or_else(|| Arc::from(id.to_string()));
    // Phase 3: re-acquire to insert; another thread may have resolved the same id
    // meanwhile — prefer the existing entry so the cache stays single-valued.
    let mut cache = lock_cache();
    let map = select(&mut cache);
    if let Some(existing) = map.map.get(&id) {
        return existing.clone();
    }
    if map.map.len() >= CACHE_MAX_SIZE
        && let Some(old) = map.order.pop_front()
    {
        map.map.remove(&old);
    }
    map.order.push_back(id);
    map.map.insert(id, name.clone());
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
    let owner = get_or_insert_name(
        |c| &mut c.uid,
        uid,
        |id| users::get_user_by_uid(id).map(|u| os_str_to_arc(u.name())),
    );
    let group = get_or_insert_name(
        |c| &mut c.gid,
        gid,
        |id| users::get_group_by_gid(id).map(|g| os_str_to_arc(g.name())),
    );
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
    // `Path::file_name()` returns `None` for the filesystem root (`/`) and for
    // paths ending in `..`; fall back to the whole path's display form so the
    // root renders as `/` rather than an empty name.
    match path.file_name() {
        Some(name) => os_str_to_string(name),
        None => path.as_os_str().to_string_lossy().into_owned(),
    }
}

fn build_file_entry(entry: &std::fs::DirEntry) -> io::Result<FileEntry> {
    let path = entry.path();
    let file_name = os_str_to_string(&entry.file_name());
    let is_symlink = entry.file_type()?.is_symlink();
    let metadata = fs::symlink_metadata(&path)?;
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

    FileEntry::new(file_name, path, cha, owner.as_ref(), group.as_ref(), false)
}

pub fn ensure_path_index(panel: &mut PanelState) {
    panel.listing.ensure_index();
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
        entries.push(FileEntry::new(
            "..".to_string(),
            parent_path.to_path_buf(),
            dummy_cha,
            owner.as_ref(),
            group.as_ref(),
            false,
        ));
    }

    // Sequential read_dir: interactive browsing is fast enough without
    // parallelizing entry collection (which would complicate per-entry errors).
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
    // The fast path (no target lookup) is only correct for non-symlink metadata.
    // If a caller violates the precondition, resolve the target metadata here
    // rather than silently rendering the symlink as a broken/orphan entry — the
    // old `debug_assert` only caught this in debug builds and was a no-op in
    // release, where the precondition matters most.
    let file_name = file_name_from_path(&path);
    if metadata.is_symlink() {
        let target_meta = fs::metadata(&path).ok();
        return build_file_entry_from_metadata(path, file_name, metadata, target_meta.as_ref());
    }
    build_file_entry_from_metadata(path, file_name, metadata, None)
}

pub fn upsert_entry(panel: &mut PanelState, entry: FileEntry) {
    // The `..` parent link is synthesized per directory read and must never be
    // tracked as a real entry; guard before delegating to the listing store.
    if is_parent_entry(&entry) {
        return;
    }
    panel.listing.upsert(entry);
}

pub fn remove_entry(panel: &mut PanelState, path: &Path) {
    panel.listing.remove(path);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::app::types::format_size;
    #[cfg(unix)]
    use crate::fs::cha::ChaMode;
    use std::fs::{self, File};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::time::SystemTime;
    use tempfile::TempDir;

    fn test_entry(name: &str, selected: bool) -> FileEntry {
        use crate::app::types::test_helpers::TestEntry;
        let mut e = TestEntry::new(name)
            .path(std::env::temp_dir().join(name))
            .file(10)
            .owner("user")
            .group("group")
            .modified(SystemTime::now())
            .created(SystemTime::now());
        if selected {
            e = e.selected();
        }
        e.build()
    }

    fn parent_entry() -> FileEntry {
        use crate::app::types::test_helpers::TestEntry;
        TestEntry::new("..")
            .path(std::env::temp_dir())
            .permissions(0o755)
            .modified(SystemTime::now())
            .build()
    }

    fn test_panel(entries: Vec<FileEntry>) -> PanelState {
        let mut panel = PanelState::new(std::env::temp_dir());
        panel.set_entries(entries);
        panel
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(FileEntry::format_size(0), "   0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(FileEntry::format_size(500), " 500 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert!(format_size(1536).contains("KB"));
    }

    #[test]
    fn test_format_size_megabytes() {
        assert!(format_size(1024 * 1024).contains("MB"));
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert!(format_size(1024 * 1024 * 1024).contains("GB"));
    }

    #[test]
    fn test_format_size_terabytes() {
        assert!(format_size(1024u64.pow(4)).contains("TB"));
    }

    #[test]
    fn test_format_permissions_rwx() {
        assert_eq!(FileEntry::display_permissions_raw(0o755), "rwxr-xr-x");
        assert_eq!(FileEntry::display_permissions_raw(0o644), "rw-r--r--");
        assert_eq!(FileEntry::display_permissions_raw(0o700), "rwx------");
        assert_eq!(FileEntry::display_permissions_raw(0o000), "---------");
        assert_eq!(FileEntry::display_permissions_raw(0o777), "rwxrwxrwx");
    }

    // Unix permission-bit semantics; on Windows ChaMode has no exec bits.
    #[cfg(unix)]
    #[test]
    fn test_is_executable() {
        assert!(ChaMode::new(0o100).is_executable());
        assert!(ChaMode::new(0o010).is_executable());
        assert!(ChaMode::new(0o001).is_executable());
        assert!(ChaMode::new(0o755).is_executable());
        assert!(!ChaMode::new(0o644).is_executable());
        assert!(!ChaMode::new(0o000).is_executable());
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
        upsert_entry(&mut panel, test_entry("a.txt", false));

        assert!(
            panel
                .listing
                .unfiltered()
                .iter()
                .any(|entry| entry.name == "a.txt")
        );
        assert_eq!(panel.listing.unfiltered().len(), 3);
    }

    #[test]
    fn test_upsert_entry_updates_existing_and_preserves_selection() {
        let mut panel = test_panel(vec![test_entry("file.txt", true)]);
        let mut updated = test_entry("file.txt", false);
        updated.cha.len = 99;

        upsert_entry(&mut panel, updated);

        assert_eq!(panel.listing.unfiltered().len(), 1);
        assert_eq!(panel.listing.unfiltered()[0].cha.len, 99);
        assert!(panel.listing.unfiltered()[0].selected);
    }

    #[test]
    fn test_remove_entry_removes_matching_path() {
        let removed = test_entry("remove.txt", true);
        let mut panel = test_panel(vec![
            parent_entry(),
            removed.clone(),
            test_entry("keep.txt", false),
        ]);

        remove_entry(&mut panel, &removed.path);

        assert!(
            !panel
                .listing
                .unfiltered()
                .iter()
                .any(|entry| entry.name == "remove.txt")
        );
        assert!(
            panel
                .listing
                .unfiltered()
                .iter()
                .any(|entry| entry.name == "keep.txt")
        );
    }

    #[test]
    fn test_upsert_adds_hidden_to_unfiltered() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("visible.txt", false)]);
        panel.set_show_hidden(false);
        upsert_entry(&mut panel, test_entry(".hidden", false));

        assert!(
            panel
                .listing
                .unfiltered()
                .iter()
                .any(|entry| entry.name == ".hidden")
        );
    }

    #[test]
    fn test_upsert_with_empty_unfiltered_inserts_entry() {
        // Single-store model: an empty backing store is the "empty unfiltered"
        // precondition this test exercises (the dual-store split is gone).
        let mut panel = test_panel(vec![]);
        panel.set_filter(Some("*.rs".to_string()));

        upsert_entry(&mut panel, test_entry("notes.txt", false));

        assert_eq!(panel.listing.unfiltered().len(), 1);
        assert_eq!(panel.listing.unfiltered()[0].name, "notes.txt");
    }

    #[test]
    fn test_remove_entry_preserves_parent_entry() {
        let mut panel = test_panel(vec![parent_entry(), test_entry("file.txt", false)]);

        remove_entry(&mut panel, &std::env::temp_dir().join("file.txt"));

        assert!(
            panel
                .listing
                .unfiltered()
                .iter()
                .any(|entry| entry.name == "..")
        );
    }
}
