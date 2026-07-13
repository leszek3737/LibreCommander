use crate::app::types::FileEntry;
use crate::fs::cha::{Cha, ChaKind, ChaMode};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MODE_FILE: u32 = 0o100000;
const MODE_DIR: u32 = 0o040000;
const MODE_SYMLINK: u32 = 0o120000;
const MODE_TYPE_MASK: u32 = 0o170000;
const DEFAULT_FILE_MODE: u32 = 0o100644;

enum EntryKind {
    Directory,
    File(u64),
}

pub struct TestEntry {
    name: String,
    path: Option<PathBuf>,
    kind: EntryKind,
    selected: bool,
    symlink: bool,
    permissions: Option<u32>,
    hidden: Option<bool>,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    owner: Option<String>,
    group: Option<String>,
    raw_mode: Option<u32>,
    /// Override `cha.len` after kind is applied (e.g. directory size in compare).
    len: Option<u64>,
}

// Setters used à la carte across the suite — any given one can be unused here.
#[allow(dead_code)]
impl TestEntry {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.is_empty(), "TestEntry name must not be empty");
        Self {
            name,
            path: None,
            kind: EntryKind::Directory,
            selected: false,
            symlink: false,
            permissions: None,
            hidden: None,
            modified: None,
            created: None,
            owner: None,
            group: None,
            raw_mode: None,
            len: None,
        }
    }

    pub fn path(mut self, p: impl Into<PathBuf>) -> Self {
        self.path = Some(p.into());
        self
    }

    /// Marks the entry as a regular file of `size` bytes.
    pub fn file(mut self, size: u64) -> Self {
        self.kind = EntryKind::File(size);
        self
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn symlink(mut self) -> Self {
        self.symlink = true;
        self
    }

    pub fn permissions(mut self, perms: u32) -> Self {
        self.permissions = Some(perms);
        self
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = Some(true);
        self
    }

    pub fn modified(mut self, t: SystemTime) -> Self {
        self.modified = Some(t);
        self
    }

    pub fn created(mut self, t: SystemTime) -> Self {
        self.created = Some(t);
        self
    }

    pub fn owner(mut self, o: impl Into<String>) -> Self {
        self.owner = Some(o.into());
        self
    }

    pub fn group(mut self, g: impl Into<String>) -> Self {
        self.group = Some(g.into());
        self
    }

    pub fn raw_mode(mut self, mode: u32) -> Self {
        self.raw_mode = Some(mode);
        self
    }

    pub fn len(mut self, size: u64) -> Self {
        self.len = Some(size);
        self
    }

    pub fn build(self) -> FileEntry {
        let path = self.path.unwrap_or_else(|| {
            #[allow(clippy::panic)]
            {
                panic!("TestEntry path must be set explicitly for '{}'", self.name)
            }
        });

        let default_mtime = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let mtime = self.modified.unwrap_or(default_mtime);

        let mut cha = Cha {
            kind: ChaKind::default(),
            mode: ChaMode::new(DEFAULT_FILE_MODE),
            len: 0,
            mtime: Some(mtime),
            // Only set birth time when the test asks for it — leave `None` so
            // sort-by-btime tests can distinguish "unknown" from "epoch".
            btime: self.created,
            ctime: None,
            atime: None,
            uid: 0,
            gid: 0,
            dev: 0,
            nlink: 0,
        };

        if let Some(mode) = self.raw_mode {
            let is_link = (mode & MODE_TYPE_MASK) == MODE_SYMLINK;
            let is_directory = (mode & MODE_TYPE_MASK) == MODE_DIR;
            let perms = mode & 0o7777;
            let type_bits = if is_link {
                MODE_SYMLINK
            } else if is_directory {
                MODE_DIR
            } else {
                MODE_FILE
            };
            cha.mode = ChaMode::new(type_bits | perms);
        } else {
            match self.kind {
                EntryKind::File(_) => {
                    cha.mode = ChaMode::new(MODE_FILE | cha.mode.permissions());
                }
                EntryKind::Directory => {
                    cha.mode = ChaMode::new(MODE_DIR | cha.mode.permissions());
                }
            }
            if let Some(perms) = self.permissions {
                let file_type = cha.mode.mode_u32() & MODE_TYPE_MASK;
                cha.mode = ChaMode::new(file_type | (perms & 0o7777));
            }
        }

        if let EntryKind::File(size) = self.kind {
            cha.len = size;
        }
        if let Some(size) = self.len {
            cha.len = size;
        }

        if self.symlink {
            let perms = cha.mode.permissions();
            cha.mode = ChaMode::new(MODE_SYMLINK | perms);
            cha.kind.dir_target = false;
            cha.kind.follow = false;
        }

        let hidden = self.hidden.unwrap_or_else(|| self.name.starts_with('.'));
        cha.set_hidden(hidden);

        let owner = self.owner.as_deref().unwrap_or("");
        let group = self.group.as_deref().unwrap_or("");

        FileEntry::new(self.name, path, cha, owner, group, self.selected, None)
    }
}
