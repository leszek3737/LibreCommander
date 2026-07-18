use std::fmt::{self, Write};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
pub(crate) fn file_mode(meta: &fs::Metadata) -> u32 {
    meta.mode()
}

// Non-Unix platforms lack Unix permission bits, so we synthesize a mode from the
// file type plus a reasonable default (rw-r--r--) for display consistency.
#[cfg(not(unix))]
pub(crate) fn file_mode(meta: &fs::Metadata) -> u32 {
    let type_bits = if meta.is_dir() {
        0o040000
    } else if meta.is_symlink() {
        0o120000
    } else {
        0o100000
    };
    type_bits | 0o644
}

#[cfg(unix)]
fn change_time(meta: &fs::Metadata) -> Option<SystemTime> {
    let secs = meta.ctime();
    // ctime_nsec() returns i64; negative values (broken OS) are clamped to 0.
    let nsecs = u32::try_from(meta.ctime_nsec()).unwrap_or(0);
    if secs >= 0 {
        // Cap at max Duration nanoseconds (999_999_999).
        let nsecs = nsecs.min(999_999_999);
        UNIX_EPOCH.checked_add(Duration::new(secs as u64, nsecs))
    } else {
        None
    }
}

// ctime (inode change time) is a Unix-only concept; other platforms have no equivalent.
#[cfg(not(unix))]
fn change_time(_meta: &fs::Metadata) -> Option<SystemTime> {
    None
}

macro_rules! cfg_trivial {
    ($vis:vis fn $name:ident($meta:ident: &fs::Metadata) -> $ret:ty, $call:ident) => {
        #[cfg(unix)]
        $vis fn $name($meta: &fs::Metadata) -> $ret {
            $meta.$call()
        }
        #[cfg(not(unix))]
        // These fields are Unix-only concepts (uid, gid, device ID, link count);
        // returning 0 lets the rest of the code compile without guarding every access.
        // Synthetic values on non-Unix platforms.
        $vis fn $name(_: &fs::Metadata) -> $ret {
            0
        }
    };
}

cfg_trivial!(fn metadata_uid(meta: &fs::Metadata) -> u32, uid);
cfg_trivial!(fn metadata_gid(meta: &fs::Metadata) -> u32, gid);

/// Symlink/hidden flags for a [`Cha`]. Plain bools — three flags do not need bitflags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ChaKind {
    pub follow: bool,
    pub hidden: bool,
    pub dir_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChaType {
    File,
    Dir,
    Link,
    Unknown,
}

impl ChaType {
    #[inline]
    fn from_mode(mode: u32) -> Self {
        match mode & 0o170000 {
            0o100000 => Self::File,
            0o040000 => Self::Dir,
            0o120000 => Self::Link,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChaMode(u32);

impl ChaMode {
    #[inline]
    pub fn new(mode: u32) -> Self {
        Self(mode)
    }

    #[inline]
    pub fn mode_u32(&self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn typ(&self) -> ChaType {
        ChaType::from_mode(self.0)
    }

    // Canonical file-type predicates: the single source of truth, derived purely
    // from the raw mode bits via `typ()`. `Cha`'s public predicates delegate here
    // (see `Cha::is_file`/`is_dir`/`is_link`); do not reimplement this logic.
    #[inline]
    pub(crate) fn is_file(&self) -> bool {
        self.typ() == ChaType::File
    }

    #[inline]
    pub(crate) fn is_dir(&self) -> bool {
        self.typ() == ChaType::Dir
    }

    #[inline]
    pub(crate) fn is_link(&self) -> bool {
        self.typ() == ChaType::Link
    }

    #[inline]
    pub fn permissions(&self) -> u32 {
        self.0 & 0o7777
    }

    #[inline]
    pub(crate) fn is_executable(&self) -> bool {
        let p = self.permissions();
        (p & 0o111) != 0
    }
}

// Recognizable sentinel so dummy dirs sort to the epoch and callers can detect them.
const DIR_SENTINEL_MTIME: SystemTime = UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cha {
    pub kind: ChaKind,
    pub mode: ChaMode,
    pub len: u64,
    pub mtime: Option<SystemTime>,
    pub btime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    pub uid: u32,
    pub gid: u32,
}

impl Cha {
    fn from_meta_base(meta: &fs::Metadata, kind: ChaKind, mode: ChaMode) -> Self {
        Self {
            kind,
            mode,
            len: meta.len(),
            // .ok() silently drops unsupported-FS errors — better to show no timestamp
            // than to crash or skip the entire entry.
            mtime: meta.modified().ok(),
            btime: meta.created().ok(),
            ctime: change_time(meta),
            uid: metadata_uid(meta),
            gid: metadata_gid(meta),
        }
    }

    pub fn new(meta: &fs::Metadata) -> Self {
        Self::from_meta_base(meta, ChaKind::default(), ChaMode::new(file_mode(meta)))
    }

    pub fn from_link_metadata(
        link_meta: &fs::Metadata,
        target_meta: Option<&fs::Metadata>,
    ) -> Self {
        // Symlink mode carries only the permission bits; the type is forced to
        // 0o120000 (symlink) regardless of the link's own stored type.
        let link_mode = ChaMode::new(0o120000 | (file_mode(link_meta) & 0o7777));

        if let Some(target) = target_meta {
            // Resolved link: use the *target's* metadata for size/times/ownership
            // but keep the symlink mode so the UI shows it as a link.
            Self::from_meta_base(
                target,
                ChaKind {
                    follow: true,
                    dir_target: target.is_dir(),
                    ..ChaKind::default()
                },
                link_mode,
            )
        } else {
            // Orphan/broken symlink: fall back to link's own metadata and strip
            // execute bits — we can't know the target's true permissions.
            Self::from_meta_base(
                link_meta,
                ChaKind::default(),
                ChaMode::new(link_mode.mode_u32() & !0o111),
            )
        }
    }

    pub fn regular_file(size: u64) -> Self {
        Self {
            kind: ChaKind::default(),
            mode: ChaMode::new(0o100644),
            len: size,
            mtime: Some(UNIX_EPOCH),
            btime: Some(UNIX_EPOCH),
            ctime: None,
            uid: 0,
            gid: 0,
        }
    }

    pub fn dummy_dir() -> Self {
        Self {
            kind: ChaKind::default(),
            mode: ChaMode::new(0o040755),
            len: 0,
            mtime: Some(DIR_SENTINEL_MTIME),
            btime: Some(DIR_SENTINEL_MTIME),
            ctime: None,
            uid: 0,
            gid: 0,
        }
    }

    /// Delegates to [`ChaMode::is_dir`], **plus** the one legitimate divergence:
    /// a followed symlink whose target is a directory counts as a directory. That
    /// case needs `kind` (the resolved-target flag), which `ChaMode` cannot see
    /// from mode bits alone — so this is intentionally *not* a bare passthrough.
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.mode.is_dir() || (self.mode.is_link() && self.kind.dir_target)
    }

    /// Delegates to [`ChaMode::is_file`] (the canonical type predicate).
    #[inline]
    pub fn is_file(&self) -> bool {
        self.mode.is_file()
    }

    /// Delegates to [`ChaMode::is_link`] (the canonical type predicate).
    #[inline]
    pub fn is_link(&self) -> bool {
        self.mode.is_link()
    }

    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.kind.hidden
    }

    #[inline]
    pub fn is_executable(&self) -> bool {
        self.mode.is_executable()
    }

    /// Compares file metadata for change detection (cache invalidation).
    pub fn hits(&self, other: &Self) -> bool {
        self.len == other.len
            && self.mtime == other.mtime
            && self.ctime == other.ctime
            && self.btime == other.btime
            && self.kind == other.kind
            && self.mode == other.mode
            && self.uid == other.uid
            && self.gid == other.gid
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.kind.hidden = hidden;
    }

    /// Sets execute permission on **all three** triples (owner, group, others).
    /// Use [`Cha::set_executable_owner`](Self::set_executable_owner) to toggle
    /// only the owner's execute bit.
    pub fn set_executable(&mut self, executable: bool) {
        if executable {
            self.mode = ChaMode::new(self.mode.mode_u32() | 0o111);
        } else {
            self.mode = ChaMode::new(self.mode.mode_u32() & !0o111);
        }
    }

    /// Toggles only the owner execute bit (`0o100`); group and others are left
    /// untouched.
    pub fn set_executable_owner(&mut self, executable: bool) {
        if executable {
            self.mode = ChaMode::new(self.mode.mode_u32() | 0o100);
        } else {
            self.mode = ChaMode::new(self.mode.mode_u32() & !0o100);
        }
    }
}

// One permission triple (owner / group / others). The r/w/x bits are the base
// bits 0o4/0o2/0o1 shifted left by `shift` (6/3/0), so the three triples differ
// only by `shift` plus the special bit and its glyphs. `special_bit` is the
// setuid/setgid/sticky bit; `special_exec`/`special_noexec` are the chars shown
// in the execute column when that special bit is set (with/without execute).
struct PermTriple {
    shift: u32,
    special_bit: u32,
    special_exec: char,
    special_noexec: char,
}

const PERM_TRIPLES: [PermTriple; 3] = [
    // owner
    PermTriple {
        shift: 6,
        special_bit: 0o4000,
        special_exec: 's',
        special_noexec: 'S',
    },
    // group
    PermTriple {
        shift: 3,
        special_bit: 0o2000,
        special_exec: 's',
        special_noexec: 'S',
    },
    // others
    PermTriple {
        shift: 0,
        special_bit: 0o1000,
        special_exec: 't',
        special_noexec: 'T',
    },
];

fn write_perm_triple(f: &mut fmt::Formatter<'_>, m: u32, t: &PermTriple) -> fmt::Result {
    let read_bit = 0o4 << t.shift;
    let write_bit = 0o2 << t.shift;
    let exec_bit = 0o1 << t.shift;
    f.write_char(if m & read_bit != 0 { 'r' } else { '-' })?;
    f.write_char(if m & write_bit != 0 { 'w' } else { '-' })?;
    f.write_char(if m & t.special_bit != 0 {
        if m & exec_bit != 0 {
            t.special_exec
        } else {
            t.special_noexec
        }
    } else if m & exec_bit != 0 {
        'x'
    } else {
        '-'
    })?;
    Ok(())
}

impl fmt::Display for ChaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.0;
        for triple in &PERM_TRIPLES {
            write_perm_triple(f, m, triple)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn cha_mode_file_type_detection() {
        assert!(ChaMode::new(0o100644).is_file());
        assert!(ChaMode::new(0o040755).is_dir());
        assert!(ChaMode::new(0o120777).is_link());
        assert!(!ChaMode::new(0o100644).is_dir());
        assert!(!ChaMode::new(0o040755).is_file());
    }

    #[test]
    fn cha_mode_permissions() {
        let mode = ChaMode::new(0o100755);
        assert!(mode.is_executable());
        assert_eq!(mode.permissions(), 0o755);

        let mode = ChaMode::new(0o100644);
        assert!(!mode.is_executable());
        assert_eq!(mode.permissions(), 0o644);
    }

    #[test]
    fn cha_kind_flags() {
        let mut kind = ChaKind::default();
        assert!(!kind.hidden);
        kind.hidden = true;
        assert!(kind.hidden);
        kind.follow = true;
        assert!(kind.follow && kind.hidden);
    }

    #[test]
    fn cha_dummy_dir() {
        let cha = Cha::dummy_dir();
        assert!(cha.is_dir());
        assert_eq!(cha.mtime, Some(DIR_SENTINEL_MTIME));
        assert_eq!(cha.btime, Some(DIR_SENTINEL_MTIME));
        assert_eq!(cha.len, 0);
    }

    #[test]
    fn cha_orphan_detection() {
        let mut cha = Cha::dummy_dir();
        cha.mode = ChaMode::new(0o120777);
        assert!(cha.is_link());
        assert!(!cha.kind.follow);

        cha.kind.follow = true;
        assert!(cha.kind.follow);
    }

    #[test]
    fn cha_hits_identity() {
        let a = Cha::dummy_dir();
        let b = a.clone();
        assert!(a.hits(&b));
    }

    #[test]
    fn cha_hits_different_mtime() {
        let a = Cha::dummy_dir();
        let mut b = a.clone();
        b.mtime = Some(SystemTime::now());
        assert!(!a.hits(&b));
    }

    #[test]
    fn cha_display_permissions() {
        let mode = ChaMode::new(0o100644);
        assert_eq!(format!("{mode}"), "rw-r--r--");

        let mode = ChaMode::new(0o100755);
        assert_eq!(format!("{mode}"), "rwxr-xr-x");
    }

    #[test]
    fn cha_mode_preserves_raw_bits() {
        let raw: u32 = 0x1_0000 | 0o100755;
        let mode = ChaMode::new(raw);
        assert_eq!(mode.mode_u32(), raw);
    }

    #[test]
    #[cfg(unix)]
    fn cha_from_link_metadata_with_target() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("target.txt");
        let link_path = dir.path().join("link");
        std::fs::write(&file_path, b"hello").unwrap();
        std::os::unix::fs::symlink(&file_path, &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let target_meta = std::fs::metadata(&file_path).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, Some(&target_meta));

        assert!(cha.is_link());
        assert!(cha.kind.follow);
        assert_eq!(cha.len, 5);
        assert_eq!(cha.mode.permissions(), link_meta.mode() & 0o7777);
    }

    #[test]
    #[cfg(unix)]
    fn cha_from_link_metadata_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let link_path = dir.path().join("dangling");
        std::os::unix::fs::symlink("/no/such/path", &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, None);

        assert!(cha.is_link());
        assert!(!cha.kind.follow);
        assert!(!cha.is_executable());
        assert_eq!(cha.mode.permissions(), link_meta.mode() & 0o7777 & !0o111);
    }

    #[test]
    fn cha_with_hidden() {
        let mut cha = Cha::dummy_dir();
        cha.set_hidden(true);
        assert!(cha.is_hidden());
        let mut cha = Cha::dummy_dir();
        cha.set_hidden(false);
        assert!(!cha.is_hidden());
    }

    #[test]
    fn cha_mode_special_permissions() {
        let mode = ChaMode::new(0o104755);
        assert_eq!(mode.permissions(), 0o4755);
        assert!(mode.is_executable());
        assert_eq!(format!("{mode}"), "rwsr-xr-x");

        let mode = ChaMode::new(0o104644);
        assert_eq!(format!("{mode}"), "rwSr--r--");

        let mode = ChaMode::new(0o102755);
        assert_eq!(format!("{mode}"), "rwxr-sr-x");

        let mode = ChaMode::new(0o101751);
        assert_eq!(format!("{mode}"), "rwxr-x--t");

        let mode = ChaMode::new(0o101750);
        assert_eq!(format!("{mode}"), "rwxr-x--T");
    }

    #[test]
    fn cha_mode_special_types_are_unknown() {
        // Block/char/socket/fifo are not first-class UI kinds; they map to Unknown.
        assert!(!ChaMode::new(0o060000).is_file());
        assert!(!ChaMode::new(0o020000).is_dir());
        assert!(!ChaMode::new(0o140000).is_link());
        assert!(!ChaMode::new(0o010000).is_file());
    }

    #[test]
    fn cha_with_executable() {
        let mut cha = Cha::dummy_dir();
        cha.set_executable(true);
        assert!(cha.is_executable());
        let mut cha = Cha::dummy_dir();
        cha.set_executable(false);
        assert!(!cha.is_executable());
    }

    #[test]
    #[cfg(unix)]
    fn cha_symlink_dir_is_dir_and_link() {
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("target_dir");
        std::fs::create_dir(&target_dir).unwrap();
        let link_path = dir.path().join("link_to_dir");
        std::os::unix::fs::symlink(&target_dir, &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let target_meta = std::fs::metadata(&target_dir).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, Some(&target_meta));

        assert!(cha.is_dir());
        assert!(cha.is_link());
        assert!(cha.kind.dir_target);
        assert!(cha.kind.follow);
    }

    #[test]
    #[cfg(unix)]
    fn cha_symlink_file_not_dir_but_link() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("target.txt");
        std::fs::write(&file_path, b"data").unwrap();
        let link_path = dir.path().join("link_to_file");
        std::os::unix::fs::symlink(&file_path, &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let target_meta = std::fs::metadata(&file_path).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, Some(&target_meta));

        assert!(!cha.is_dir());
        assert!(cha.is_link());
        assert!(!cha.kind.dir_target);
    }

    #[test]
    #[cfg(unix)]
    fn cha_broken_symlink_not_executable() {
        let dir = tempfile::tempdir().unwrap();
        let link_path = dir.path().join("dangling");
        std::os::unix::fs::symlink("/no/such/path", &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, None);

        assert!(cha.is_link());
        assert!(!cha.is_executable());
        assert!(!cha.kind.follow);
    }
}
