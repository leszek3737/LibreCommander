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
    // ctime_nsec should always fit in u32, but clamp to 0 if the OS returns garbage.
    let nsecs = u32::try_from(meta.ctime_nsec()).unwrap_or(0);
    if secs >= 0 {
        // Cap at 999ms to stay within Duration's valid nanosecond range.
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
        $vis fn $name(_: &fs::Metadata) -> $ret {
            0
        }
    };
}

cfg_trivial!(fn metadata_uid(meta: &fs::Metadata) -> u32, uid);
cfg_trivial!(fn metadata_gid(meta: &fs::Metadata) -> u32, gid);
cfg_trivial!(fn metadata_dev(meta: &fs::Metadata) -> u64, dev);
cfg_trivial!(fn metadata_nlink(meta: &fs::Metadata) -> u64, nlink);

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChaKind: u8 {
        const FOLLOW = 0b0000_0001;
        const HIDDEN = 0b0000_0010;
        const DIR_TARGET = 0b0001_0000;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChaType {
    File,
    Dir,
    Link,
    Block,
    Char,
    Socket,
    Fifo,
    Unknown,
}

impl ChaType {
    #[inline]
    fn from_mode(mode: u32) -> Self {
        match mode & 0o170000 {
            0o100000 => Self::File,
            0o040000 => Self::Dir,
            0o120000 => Self::Link,
            0o060000 => Self::Block,
            0o020000 => Self::Char,
            0o140000 => Self::Socket,
            0o010000 => Self::Fifo,
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
    pub(crate) fn is_block(&self) -> bool {
        self.typ() == ChaType::Block
    }

    #[inline]
    pub(crate) fn is_char(&self) -> bool {
        self.typ() == ChaType::Char
    }

    #[inline]
    pub(crate) fn is_socket(&self) -> bool {
        self.typ() == ChaType::Socket
    }

    #[inline]
    pub(crate) fn is_fifo(&self) -> bool {
        self.typ() == ChaType::Fifo
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
    pub atime: Option<SystemTime>,
    pub uid: u32,
    pub gid: u32,
    pub dev: u64,
    pub nlink: u64,
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
            atime: meta.accessed().ok(),
            uid: metadata_uid(meta),
            gid: metadata_gid(meta),
            dev: metadata_dev(meta),
            nlink: metadata_nlink(meta),
        }
    }

    pub fn new(meta: &fs::Metadata) -> Self {
        Self::from_meta_base(meta, ChaKind::empty(), ChaMode::new(file_mode(meta)))
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
            let mut kind = ChaKind::FOLLOW;
            if target.is_dir() {
                kind.insert(ChaKind::DIR_TARGET);
            }
            Self::from_meta_base(target, kind, link_mode)
        } else {
            // Orphan/broken symlink: fall back to link's own metadata and strip
            // execute bits — we can't know the target's true permissions.
            Self::from_meta_base(
                link_meta,
                ChaKind::empty(),
                ChaMode::new(link_mode.mode_u32() & !0o111),
            )
        }
    }

    pub fn regular_file(size: u64) -> Self {
        Self {
            kind: ChaKind::empty(),
            mode: ChaMode::new(0o100644),
            len: size,
            mtime: Some(UNIX_EPOCH),
            btime: Some(UNIX_EPOCH),
            ctime: None,
            atime: None,
            uid: 0,
            gid: 0,
            dev: 0,
            nlink: 1,
        }
    }

    pub fn dummy_dir() -> Self {
        Self {
            kind: ChaKind::empty(),
            mode: ChaMode::new(0o040755),
            len: 0,
            mtime: Some(DIR_SENTINEL_MTIME),
            btime: Some(DIR_SENTINEL_MTIME),
            ctime: None,
            atime: None,
            uid: 0,
            gid: 0,
            dev: 0,
            nlink: 0,
        }
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        self.mode.is_dir() || (self.mode.is_link() && self.kind.contains(ChaKind::DIR_TARGET))
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        self.mode.is_file()
    }

    #[inline]
    pub fn is_link(&self) -> bool {
        self.mode.is_link()
    }

    #[inline]
    pub fn is_block(&self) -> bool {
        self.mode.is_block()
    }

    #[inline]
    pub fn is_char(&self) -> bool {
        self.mode.is_char()
    }

    #[inline]
    pub fn is_socket(&self) -> bool {
        self.mode.is_socket()
    }

    #[inline]
    pub fn is_fifo(&self) -> bool {
        self.mode.is_fifo()
    }

    #[inline]
    pub fn is_orphan(&self) -> bool {
        self.is_link() && !self.kind.contains(ChaKind::FOLLOW)
    }

    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.kind.contains(ChaKind::HIDDEN)
    }

    #[inline]
    pub fn is_executable(&self) -> bool {
        self.mode.is_executable()
    }

    #[inline]
    pub fn len(&self) -> u64 {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    #[inline]
    pub fn btime(&self) -> Option<SystemTime> {
        self.btime
    }

    #[inline]
    pub fn atime(&self) -> Option<SystemTime> {
        self.atime
    }

    #[inline]
    pub fn ctime(&self) -> Option<SystemTime> {
        self.ctime
    }

    #[inline]
    pub fn dev(&self) -> u64 {
        self.dev
    }

    #[inline]
    pub fn nlink(&self) -> u64 {
        self.nlink
    }

    /// Compares file metadata for change detection (cache invalidation).
    ///
    /// Intentionally excludes:
    /// - `atime` — unstable, changes on read access
    /// - `dev` — device ID, irrelevant for content changes
    /// - `nlink` — link count, irrelevant for content changes
    ///
    /// For full field equality, use `PartialEq` instead.
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

    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.kind.set(ChaKind::HIDDEN, hidden);
        self
    }

    pub fn with_executable(mut self, executable: bool) -> Self {
        if executable {
            self.mode = ChaMode::new(self.mode.mode_u32() | 0o111);
        } else {
            self.mode = ChaMode::new(self.mode.mode_u32() & !0o111);
        }
        self
    }
}

struct PermBits {
    read_bit: u32,
    write_bit: u32,
    exec_bit: u32,
    special_bit: u32,
    special_exec: char,
    special_noexec: char,
}

fn write_perm_triple(f: &mut fmt::Formatter<'_>, m: u32, bits: &PermBits) -> fmt::Result {
    f.write_char(if m & bits.read_bit != 0 { 'r' } else { '-' })?;
    f.write_char(if m & bits.write_bit != 0 { 'w' } else { '-' })?;
    f.write_char(if m & bits.special_bit != 0 {
        if m & bits.exec_bit != 0 {
            bits.special_exec
        } else {
            bits.special_noexec
        }
    } else if m & bits.exec_bit != 0 {
        'x'
    } else {
        '-'
    })?;
    Ok(())
}

const OWNER_BITS: PermBits = PermBits {
    read_bit: 0o400,
    write_bit: 0o200,
    exec_bit: 0o100,
    special_bit: 0o4000,
    special_exec: 's',
    special_noexec: 'S',
};

const GROUP_BITS: PermBits = PermBits {
    read_bit: 0o040,
    write_bit: 0o020,
    exec_bit: 0o010,
    special_bit: 0o2000,
    special_exec: 's',
    special_noexec: 'S',
};

const OTHERS_BITS: PermBits = PermBits {
    read_bit: 0o004,
    write_bit: 0o002,
    exec_bit: 0o001,
    special_bit: 0o1000,
    special_exec: 't',
    special_noexec: 'T',
};

impl fmt::Display for ChaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.0;
        write_perm_triple(f, m, &OWNER_BITS)?;
        write_perm_triple(f, m, &GROUP_BITS)?;
        write_perm_triple(f, m, &OTHERS_BITS)
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
        let mut kind = ChaKind::empty();
        assert!(!kind.contains(ChaKind::HIDDEN));
        kind.insert(ChaKind::HIDDEN);
        assert!(kind.contains(ChaKind::HIDDEN));
        kind.insert(ChaKind::FOLLOW);
        assert!(kind.contains(ChaKind::FOLLOW | ChaKind::HIDDEN));
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
        assert!(cha.is_orphan());

        cha.kind.insert(ChaKind::FOLLOW);
        assert!(!cha.is_orphan());
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
        assert!(cha.kind.contains(ChaKind::FOLLOW));
        assert!(!cha.is_orphan());
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
        assert!(cha.is_orphan());
        assert!(!cha.kind.contains(ChaKind::FOLLOW));
        assert!(!cha.is_executable());
        assert_eq!(cha.mode.permissions(), link_meta.mode() & 0o7777 & !0o111);
    }

    #[test]
    fn cha_with_hidden() {
        let cha = Cha::dummy_dir().with_hidden(true);
        assert!(cha.is_hidden());
        let cha = Cha::dummy_dir().with_hidden(false);
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
    fn cha_mode_block_char_socket_fifo() {
        assert!(ChaMode::new(0o060000).is_block());
        assert!(ChaMode::new(0o020000).is_char());
        assert!(ChaMode::new(0o140000).is_socket());
        assert!(ChaMode::new(0o010000).is_fifo());
        assert!(!ChaMode::new(0o060000).is_file());
        assert!(!ChaMode::new(0o020000).is_dir());
    }

    #[test]
    fn cha_with_executable() {
        let cha = Cha::dummy_dir().with_executable(true);
        assert!(cha.is_executable());
        let cha = Cha::dummy_dir().with_executable(false);
        assert!(!cha.is_executable());
    }

    #[test]
    fn cha_accessors_for_dead_fields() {
        let cha = Cha::dummy_dir();
        assert_eq!(cha.dev(), 0);
        assert_eq!(cha.nlink(), 0);
        assert!(cha.atime().is_none());
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
        assert!(cha.kind.contains(ChaKind::DIR_TARGET));
        assert!(cha.kind.contains(ChaKind::FOLLOW));
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
        assert!(!cha.kind.contains(ChaKind::DIR_TARGET));
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
        assert!(cha.is_orphan());
    }
}
