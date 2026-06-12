use crate::app::types::FileEntry;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
}

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
        }
    }

    pub fn path(mut self, p: impl Into<PathBuf>) -> Self {
        self.path = Some(p.into());
        self
    }

    pub fn file(mut self, size: u64) -> Self {
        self.kind = EntryKind::File(size);
        self
    }

    #[allow(dead_code)]
    pub fn dir(mut self) -> Self {
        self.kind = EntryKind::Directory;
        self
    }

    #[allow(dead_code)]
    pub fn size(mut self, size: u64) -> Self {
        self.kind = EntryKind::File(size);
        self
    }

    #[allow(dead_code)]
    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    #[allow(dead_code)]
    pub fn symlink(mut self) -> Self {
        self.symlink = true;
        self
    }

    #[allow(dead_code)]
    pub fn permissions(mut self, perms: u32) -> Self {
        self.permissions = Some(perms);
        self
    }

    #[allow(dead_code)]
    pub fn hidden(mut self) -> Self {
        self.hidden = Some(true);
        self
    }

    #[allow(dead_code)]
    pub fn modified(mut self, t: SystemTime) -> Self {
        self.modified = Some(t);
        self
    }

    #[allow(dead_code)]
    pub fn created(mut self, t: SystemTime) -> Self {
        self.created = Some(t);
        self
    }

    #[allow(dead_code)]
    pub fn owner(mut self, o: impl Into<String>) -> Self {
        self.owner = Some(o.into());
        self
    }

    #[allow(dead_code)]
    pub fn group(mut self, g: impl Into<String>) -> Self {
        self.group = Some(g.into());
        self
    }

    #[allow(dead_code)]
    pub fn raw_mode(mut self, mode: u32) -> Self {
        self.raw_mode = Some(mode);
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
        let btime = self.created.unwrap_or(default_mtime);

        let mut builder = FileEntry::builder()
            .name(&self.name)
            .path(path)
            .modified(mtime)
            .created(btime);

        if let Some(owner) = self.owner {
            builder = builder.owner(owner);
        }
        if let Some(group) = self.group {
            builder = builder.group(group);
        }

        if let Some(mode) = self.raw_mode {
            let is_link = (mode & 0o170000) == 0o120000;
            let is_directory = (mode & 0o170000) == 0o040000;
            let perms = mode & 0o7777;
            builder = builder
                .is_dir(is_directory)
                .is_symlink(is_link)
                .permissions(perms);
        } else {
            match self.kind {
                EntryKind::File(size) => {
                    builder = builder.is_dir(false).size(size);
                }
                EntryKind::Directory => {
                    builder = builder.is_dir(true);
                }
            }
            if let Some(perms) = self.permissions {
                builder = builder.permissions(perms);
            }
        }

        if self.symlink {
            builder = builder.is_symlink(true);
        }

        let hidden = self.hidden.unwrap_or_else(|| self.name.starts_with('.'));
        builder = builder.is_hidden(hidden).selected(self.selected);

        builder.build()
    }
}
