use std::fmt::{self, Write};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn file_mode(meta: &fs::Metadata) -> u32 {
    meta.mode()
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChaKind: u8 {
        const FOLLOW = 0b0000_0001;
        const HIDDEN = 0b0000_0010;
        const SYSTEM = 0b0000_0100;
        const DUMMY  = 0b0000_1000;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChaType {
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
pub struct ChaMode(u16);

impl ChaMode {
    pub fn new(mode: u32) -> Self {
        debug_assert!(mode <= 0o177777, "Unix mode exceeds 16 bits");
        Self(mode as u16)
    }

    pub fn raw(&self) -> u16 {
        self.0
    }

    pub fn mode_u32(&self) -> u32 {
        self.0 as u32
    }

    pub fn typ(&self) -> ChaType {
        ChaType::from_mode(self.0 as u32)
    }

    pub fn is_file(&self) -> bool {
        self.typ() == ChaType::File
    }

    pub fn is_dir(&self) -> bool {
        self.typ() == ChaType::Dir
    }

    pub fn is_link(&self) -> bool {
        self.typ() == ChaType::Link
    }

    pub fn is_block(&self) -> bool {
        self.typ() == ChaType::Block
    }

    pub fn is_char(&self) -> bool {
        self.typ() == ChaType::Char
    }

    pub fn is_socket(&self) -> bool {
        self.typ() == ChaType::Socket
    }

    pub fn is_fifo(&self) -> bool {
        self.typ() == ChaType::Fifo
    }

    pub fn permissions(&self) -> u16 {
        self.0 & 0o7777
    }

    pub fn is_executable(&self) -> bool {
        let p = self.permissions();
        (p & 0o111) != 0
    }
}

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
    pub fn new(meta: &fs::Metadata) -> Self {
        Self {
            kind: ChaKind::empty(),
            mode: ChaMode::new(file_mode(meta)),
            len: meta.len(),
            mtime: meta.modified().ok(),
            btime: meta.created().ok(),
            ctime: {
                let secs = meta.ctime();
                let nsecs = meta.ctime_nsec() as u32;
                if secs >= 0 {
                    Some(UNIX_EPOCH + Duration::new(secs as u64, nsecs))
                } else {
                    None
                }
            },
            atime: meta.accessed().ok(),
            uid: meta.uid(),
            gid: meta.gid(),
            dev: meta.dev(),
            nlink: meta.nlink(),
        }
    }

    pub fn from_link_metadata(
        link_meta: &fs::Metadata,
        target_meta: Option<&fs::Metadata>,
    ) -> Self {
        let mut cha = if let Some(target) = target_meta {
            let mut c = Self::new(target);
            c.kind.insert(ChaKind::FOLLOW);
            c
        } else {
            Self::new(link_meta)
        };
        cha.mode = ChaMode::new(0o120000 | (file_mode(link_meta) & 0o7777));
        cha
    }

    pub fn dummy_dir() -> Self {
        Self {
            kind: ChaKind::DUMMY,
            mode: ChaMode::new(0o040755),
            len: 0,
            mtime: Some(UNIX_EPOCH),
            btime: Some(UNIX_EPOCH),
            ctime: None,
            atime: None,
            uid: 0,
            gid: 0,
            dev: 0,
            nlink: 0,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.mode.is_dir()
    }

    pub fn is_file(&self) -> bool {
        self.mode.is_file()
    }

    pub fn is_link(&self) -> bool {
        self.mode.is_link()
    }

    pub fn is_block(&self) -> bool {
        self.mode.is_block()
    }

    pub fn is_char(&self) -> bool {
        self.mode.is_char()
    }

    pub fn is_socket(&self) -> bool {
        self.mode.is_socket()
    }

    pub fn is_fifo(&self) -> bool {
        self.mode.is_fifo()
    }

    pub fn is_orphan(&self) -> bool {
        self.is_link() && !self.kind.contains(ChaKind::FOLLOW)
    }

    pub fn is_hidden(&self) -> bool {
        self.kind.contains(ChaKind::HIDDEN)
    }

    pub fn is_executable(&self) -> bool {
        self.mode.is_executable()
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    pub fn btime(&self) -> Option<SystemTime> {
        self.btime
    }

    pub fn atime(&self) -> Option<SystemTime> {
        self.atime
    }

    pub fn dev(&self) -> u64 {
        self.dev
    }

    pub fn nlink(&self) -> u64 {
        self.nlink
    }

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

impl fmt::Display for ChaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.0 as u32;
        f.write_char(if m & 0o400 != 0 { 'r' } else { '-' })?;
        f.write_char(if m & 0o200 != 0 { 'w' } else { '-' })?;
        f.write_char(if m & 0o4000 != 0 {
            if m & 0o100 != 0 { 's' } else { 'S' }
        } else if m & 0o100 != 0 {
            'x'
        } else {
            '-'
        })?;
        f.write_char(if m & 0o040 != 0 { 'r' } else { '-' })?;
        f.write_char(if m & 0o020 != 0 { 'w' } else { '-' })?;
        f.write_char(if m & 0o2000 != 0 {
            if m & 0o010 != 0 { 's' } else { 'S' }
        } else if m & 0o010 != 0 {
            'x'
        } else {
            '-'
        })?;
        f.write_char(if m & 0o004 != 0 { 'r' } else { '-' })?;
        f.write_char(if m & 0o002 != 0 { 'w' } else { '-' })?;
        f.write_char(if m & 0o1000 != 0 {
            if m & 0o001 != 0 { 't' } else { 'T' }
        } else if m & 0o001 != 0 {
            'x'
        } else {
            '-'
        })?;
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
        assert!(cha.kind.contains(ChaKind::DUMMY));
        assert_eq!(cha.mtime, Some(UNIX_EPOCH));
        assert_eq!(cha.btime, Some(UNIX_EPOCH));
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
    fn cha_mode_truncation_to_u16() {
        let raw: u32 = 0o100755;
        let mode = ChaMode::new(raw);
        assert_eq!(mode.mode_u32(), raw & 0xFFFF);
    }

    #[test]
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
        assert_eq!(u32::from(cha.mode.permissions()), link_meta.mode() & 0o7777);
    }

    #[test]
    fn cha_from_link_metadata_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let link_path = dir.path().join("dangling");
        std::os::unix::fs::symlink("/no/such/path", &link_path).unwrap();

        let link_meta = std::fs::symlink_metadata(&link_path).unwrap();
        let cha = Cha::from_link_metadata(&link_meta, None);

        assert!(cha.is_link());
        assert!(cha.is_orphan());
        assert!(!cha.kind.contains(ChaKind::FOLLOW));
        assert_eq!(u32::from(cha.mode.permissions()), link_meta.mode() & 0o7777);
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
}
