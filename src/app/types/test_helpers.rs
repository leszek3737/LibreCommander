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

// `#[allow(dead_code)]` is hoisted to the impl block (collapsed from 11
// per-method attributes): this shared test builder exposes setters that are used
// à la carte across the whole suite, so any given setter can be legitimately
// unused in a single compilation unit.
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
        }
    }

    pub fn path(mut self, p: impl Into<PathBuf>) -> Self {
        self.path = Some(p.into());
        self
    }

    /// Marks the entry as a regular file of `size` bytes. This is the canonical
    /// size setter; the former identical `.size()` alias and the no-op `.dir()`
    /// (the default kind is already `Directory`) were removed as redundant.
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

        // File type + permission bits.
        //
        // Precedence: an explicit `raw_mode` is the authoritative source for the
        // type (dir/symlink/regular) and the permission bits, since it mirrors a
        // real `stat()` `st_mode`. Without it, the type comes from `kind` (file vs
        // directory) and permissions from an explicit `.permissions(..)`.
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
                EntryKind::File(_) => {
                    builder = builder.is_dir(false);
                }
                EntryKind::Directory => {
                    builder = builder.is_dir(true);
                }
            }
            if let Some(perms) = self.permissions {
                builder = builder.permissions(perms);
            }
        }

        // Byte size composes independently of the type source: `raw_mode` carries
        // no length, so `.file(size)` is always honored, even alongside `raw_mode`
        // (WS-C fix: previously `.raw_mode(..).file(N)` silently dropped the size).
        // A `Directory` kind keeps size 0 (a listing's directory size is
        // meaningless here).
        if let EntryKind::File(size) = self.kind {
            builder = builder.size(size);
        }

        // Symlink precedence: an explicit `.symlink()` is a hard override that
        // upgrades the entry to a symlink even when `raw_mode` encoded a
        // non-symlink type. It only ever upgrades — it never downgrades a symlink
        // that `raw_mode` already established.
        if self.symlink {
            builder = builder.is_symlink(true);
        }

        let hidden = self.hidden.unwrap_or_else(|| self.name.starts_with('.'));
        builder = builder.is_hidden(hidden).selected(self.selected);

        // `TestEntry::build` keeps returning `FileEntry` (not `Result`): test code
        // always supplies a non-empty name, so the only `BuildError` is unreachable
        // here and is surfaced as a panic with a clear message.
        builder.build().expect("valid test entry")
    }
}
