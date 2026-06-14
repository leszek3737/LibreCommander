use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use unicode_width::UnicodeWidthStr;

use crate::fs::cha::{Cha, ChaKind, ChaMode};

pub(crate) fn sanitize_for_display(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|b| b <= 0x1F || b == 0x7F) {
        return Cow::Borrowed(s);
    }

    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            0x1B => {
                if i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'[' => {
                            let mut j = i + 2;
                            while j < bytes.len() {
                                let b = bytes[j];
                                if (b'@'..=b'~').contains(&b) {
                                    j += 1;
                                    break;
                                }
                                if b <= 0x1F {
                                    break;
                                }
                                j += 1;
                            }
                            i = j;
                            continue;
                        }
                        b']' | b'P' | b'_' | b'^' => {
                            let mut j = i + 2;
                            while j < bytes.len() {
                                if bytes[j] == 0x07 {
                                    j += 1;
                                    break;
                                }
                                if bytes[j] == 0x1B && j + 1 < bytes.len() && bytes[j + 1] == b'\\'
                                {
                                    j += 2;
                                    break;
                                }
                                j += 1;
                            }
                            i = j;
                            continue;
                        }
                        _ => {}
                    }
                }
                result.push('\u{00b7}');
                i += 1;
            }
            b'\n' => {
                result.push('\u{23ce}');
                i += 1;
            }
            b'\r' => {
                i += 1;
            }
            b'\t' => {
                result.push_str("  ");
                i += 1;
            }
            0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F => {
                result.push('\u{00b7}');
                i += 1;
            }
            _ => {
                let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
                result.push(ch);
                i += ch.len_utf8();
            }
        }
    }

    Cow::Owned(result)
}

pub(crate) fn sanitize_name(name: &str) -> Option<String> {
    match sanitize_for_display(name) {
        Cow::Borrowed(_) => None,
        Cow::Owned(s) => Some(s),
    }
}

const MODE_FILE: u32 = 0o100000;
const MODE_DIR: u32 = 0o040000;
const MODE_SYMLINK: u32 = 0o120000;

const MODE_TYPE_MASK: u32 = 0o170000;

const DEFAULT_FILE_MODE: u32 = 0o100644;
const BYTES_PER_UNIT: f64 = 1024.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSize(pub u64);

impl std::fmt::Display for FileSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.0;
        let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;
        while size_f >= BYTES_PER_UNIT && unit_idx < units.len() - 1 {
            size_f /= BYTES_PER_UNIT;
            unit_idx += 1;
        }
        if unit_idx > 0 {
            size_f = (size_f * 10.0).round() / 10.0;
            // Rounding can push the value to at most exactly BYTES_PER_UNIT
            // (e.g. 1023.95 -> 1024.0), so a single extra step is sufficient.
            if size_f >= BYTES_PER_UNIT && unit_idx < units.len() - 1 {
                size_f /= BYTES_PER_UNIT;
                unit_idx += 1;
            }
        }
        if unit_idx == 0 {
            write!(f, "{} {}", size, units[unit_idx])
        } else {
            write!(f, "{:.1} {}", size_f, units[unit_idx])
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Dir,
    Archive,
    Image,
    Video,
    Audio,
    Document,
    Code,
    Config,
    Font,
    Executable,
    Symlink,
    Other,
}

pub fn format_size(size: u64) -> String {
    FileSize(size).to_string()
}

// PR-07 WS-B: the trivial free `format_permissions` wrapper was removed.
// Callers must call `FileEntry::display_permissions_raw(mode)` directly.
// Wave 2 migrates `ui/panels/mod.rs`, `fs/reader.rs`, and the
// `crate::app::types::mod.rs` re-export accordingly.

pub(crate) fn format_system_time(modified: SystemTime) -> Option<String> {
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    let ts = i64::try_from(duration.as_secs()).ok()?;
    let dt = DateTime::from_timestamp(ts, 0)?;
    Some(
        dt.with_timezone(&Local)
            .format("%d-%m-%y %H:%M")
            .to_string(),
    )
}

pub fn format_time(modified: SystemTime) -> String {
    format_system_time(modified).unwrap_or_else(|| "??-??-?? ??:??".to_string())
}

pub fn compute_category(cha: &Cha, name: &str) -> FileCategory {
    crate::app::file_type::category(name, cha.is_dir(), cha.is_executable(), cha.is_link())
}

/// Interns a string into an `Arc<str>`.
///
/// Taking `impl AsRef<str>` and building the `Arc` from the `&str` performs a
/// single allocation here (the `Arc` buffer). Compared to a `&str` -> `String`
/// -> `Arc` chain it avoids an intermediate owned `String`; when the caller
/// already owns a `String`, that buffer is simply dropped after the copy.
fn intern_str(value: impl AsRef<str>) -> Arc<str> {
    Arc::from(value.as_ref())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub cha: Cha,
    pub owner: Arc<str>,
    pub group: Arc<str>,
    pub selected: bool,
    pub mime_type: Option<String>,
    pub time_str: String,
    pub size_str: String,
    pub name_width: usize,
    pub size_width: usize,
    pub time_width: usize,
    pub category: FileCategory,
    pub sanitized_name: Option<String>,
}

#[derive(Debug)]
pub struct FileEntryBuilder {
    name: String,
    path: PathBuf,
    cha: Cha,
    owner: Arc<str>,
    group: Arc<str>,
    selected: bool,
    mime_type: Option<String>,
}

/// Error returned by [`FileEntryBuilder::build`] when the accumulated
/// configuration cannot produce a valid [`FileEntry`].
///
/// Validation is deferred to `build` so that chaining setters never panics on
/// partially-built input; the public builder therefore reports misuse as a
/// recoverable error instead of aborting the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// `name` was never set, or was set to an empty string. Every directory
    /// entry must carry a non-empty file name.
    EmptyName,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "FileEntry name must not be empty"),
        }
    }
}

impl std::error::Error for BuildError {}

impl FileEntryBuilder {
    pub fn name(mut self, v: impl Into<String>) -> Self {
        self.name = v.into();
        self
    }
    pub fn path(mut self, v: impl Into<PathBuf>) -> Self {
        self.path = v.into();
        self
    }
    pub fn cha(mut self, v: Cha) -> Self {
        self.cha = v;
        self
    }
    /// Applies file-type bits while preserving permission bits and clearing
    /// resolved-symlink-target flags. When `enable` is false and the entry is
    /// `currently` of this type, it is demoted back to a regular file.
    fn set_type(mut self, enable: bool, type_bits: u32, currently: bool) -> Self {
        let perms = self.cha.mode.permissions();
        if enable {
            self.cha.mode = ChaMode::new(type_bits | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        } else if currently {
            self.cha.mode = ChaMode::new(MODE_FILE | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        }
        self
    }
    pub fn is_dir(self, v: bool) -> Self {
        let currently = self.cha.is_dir();
        self.set_type(v, MODE_DIR, currently)
    }
    pub fn is_symlink(self, v: bool) -> Self {
        let currently = self.cha.is_link();
        self.set_type(v, MODE_SYMLINK, currently)
    }
    pub fn is_executable(mut self, v: bool) -> Self {
        self.cha.set_executable(v);
        self
    }
    pub fn size(mut self, v: u64) -> Self {
        self.cha.len = v;
        self
    }
    pub fn modified(mut self, v: SystemTime) -> Self {
        self.cha.mtime = Some(v);
        self
    }
    pub fn created(mut self, v: SystemTime) -> Self {
        self.cha.btime = Some(v);
        self
    }
    pub fn permissions(mut self, v: u32) -> Self {
        let file_type = self.cha.mode.mode_u32() & MODE_TYPE_MASK;
        self.cha.mode = ChaMode::new(file_type | (v & 0o7777));
        self
    }
    pub fn owner(mut self, v: impl AsRef<str>) -> Self {
        self.owner = intern_str(v);
        self
    }
    pub fn group(mut self, v: impl AsRef<str>) -> Self {
        self.group = intern_str(v);
        self
    }
    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }
    pub fn is_hidden(mut self, v: bool) -> Self {
        self.cha.set_hidden(v);
        self
    }
    pub fn mime_type(mut self, v: Option<String>) -> Self {
        self.mime_type = v;
        self
    }
    /// Finalizes the builder into a [`FileEntry`].
    ///
    /// Returns [`BuildError::EmptyName`] if `name` was never set (or set to an
    /// empty string) instead of panicking, so callers control how invalid input
    /// is handled.
    pub fn build(self) -> Result<FileEntry, BuildError> {
        if self.name.is_empty() {
            return Err(BuildError::EmptyName);
        }
        let (time_str, size_str, name_width, size_width, time_width) =
            FileEntry::cached_fields(&self.cha, &self.name);
        let category = compute_category(&self.cha, &self.name);
        let sanitized_name = sanitize_name(&self.name);
        Ok(FileEntry {
            name: self.name,
            path: self.path,
            cha: self.cha,
            owner: self.owner,
            group: self.group,
            selected: self.selected,
            mime_type: self.mime_type,
            time_str,
            size_str,
            name_width,
            size_width,
            time_width,
            category,
            sanitized_name,
        })
    }
}

impl FileEntry {
    // SCOPE-OUT (PR-07 WS-B): the precomputed display fields below
    // (`time_str`, `size_str`, `*_width`, `category`, `sanitized_name`) are an
    // eagerly materialized cache stored per entry. A field-cache/storage
    // redesign (e.g. lazy or columnar storage) is intentionally deferred to a
    // later PR and is NOT part of this change.
    pub fn cached_fields(cha: &Cha, name: &str) -> (String, String, usize, usize, usize) {
        let time_str = format_time(cha.mtime().unwrap_or(std::time::UNIX_EPOCH));
        let size_str = if cha.is_dir() {
            "     <DIR>".to_string()
        } else {
            format!("{:>10}", format_size(cha.len))
        };
        let name_width = UnicodeWidthStr::width(name);
        let size_width = UnicodeWidthStr::width(size_str.as_str());
        let time_width = UnicodeWidthStr::width(time_str.as_str());
        (time_str, size_str, name_width, size_width, time_width)
    }

    pub fn builder() -> FileEntryBuilder {
        FileEntryBuilder {
            name: String::new(),
            path: PathBuf::new(),
            cha: Cha {
                kind: ChaKind::empty(),
                mode: ChaMode::new(DEFAULT_FILE_MODE),
                len: 0,
                mtime: None,
                btime: None,
                ctime: None,
                atime: None,
                uid: 0,
                gid: 0,
                dev: 0,
                nlink: 0,
            },
            owner: Arc::from(""),
            group: Arc::from(""),
            selected: false,
            mime_type: None,
        }
    }

    pub fn size(&self) -> u64 {
        self.cha.len
    }

    pub fn mtime(&self) -> SystemTime {
        self.cha.mtime().unwrap_or(std::time::UNIX_EPOCH)
    }

    pub fn btime(&self) -> SystemTime {
        self.cha.btime().unwrap_or(std::time::UNIX_EPOCH)
    }

    pub fn mode_bits(&self) -> u32 {
        self.cha.mode.mode_u32()
    }

    pub fn uid(&self) -> u32 {
        self.cha.uid
    }

    pub fn gid(&self) -> u32 {
        self.cha.gid
    }

    pub fn is_dir(&self) -> bool {
        self.cha.is_dir()
    }

    pub fn is_symlink(&self) -> bool {
        self.cha.is_link()
    }

    pub fn is_executable(&self) -> bool {
        self.cha.is_executable()
    }

    pub fn is_hidden(&self) -> bool {
        self.cha.is_hidden()
    }

    pub fn category(&self) -> FileCategory {
        self.category
    }

    pub fn display_name(&self) -> &str {
        self.sanitized_name.as_deref().unwrap_or(&self.name)
    }

    pub fn display_size(&self) -> String {
        Self::format_size(self.size())
    }

    pub fn format_size(size: u64) -> String {
        // Right-align to width 6. The padding must be applied to the formatted
        // `String` (via the free `format_size`): `FileSize`'s Display impl writes
        // its parts directly and ignores the formatter's width/fill flags.
        format!("{:>6}", format_size(size))
    }

    pub fn display_permissions(&self) -> String {
        Self::display_permissions_raw(self.mode_bits())
    }

    pub fn display_permissions_raw(mode: u32) -> String {
        use crate::fs::cha::ChaMode;
        ChaMode::new(mode).to_string()
    }

    pub fn display_modified(&self) -> &str {
        &self.time_str
    }
}
