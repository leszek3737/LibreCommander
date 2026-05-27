use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use unicode_width::UnicodeWidthStr;

use crate::fs::cha::{Cha, ChaKind, ChaMode};

const MODE_FILE: u32 = 0o100000;
const MODE_DIR: u32 = 0o040000;
const MODE_SYMLINK: u32 = 0o120000;

const MODE_TYPE_MASK: u32 = 0o170000;

const DEFAULT_FILE_MODE: u32 = 0o100644;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSize(pub u64);

impl std::fmt::Display for FileSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self.0;
        let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;
        while size_f >= 1024.0 && unit_idx < units.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }
        if unit_idx > 0 {
            size_f = (size_f * 10.0).round() / 10.0;
            if size_f >= 1024.0 && unit_idx < units.len() - 1 {
                size_f /= 1024.0;
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

pub fn format_permissions(mode: u32) -> String {
    FileEntry::display_permissions_raw(mode)
}

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
    pub fn is_dir(mut self, v: bool) -> Self {
        let perms = self.cha.mode.permissions();
        if v {
            self.cha.mode = ChaMode::new(MODE_DIR | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        } else if self.cha.is_dir() {
            self.cha.mode = ChaMode::new(MODE_FILE | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        }
        self
    }
    pub fn is_symlink(mut self, v: bool) -> Self {
        let perms = self.cha.mode.permissions();
        if v {
            self.cha.mode = ChaMode::new(MODE_SYMLINK | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        } else if self.cha.is_link() {
            self.cha.mode = ChaMode::new(MODE_FILE | perms);
            self.cha.kind.remove(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
        }
        self
    }
    pub fn is_executable(mut self, v: bool) -> Self {
        self.cha = self.cha.with_executable(v);
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
    pub fn owner(mut self, v: impl Into<String>) -> Self {
        self.owner = Arc::from(v.into());
        self
    }
    pub fn group(mut self, v: impl Into<String>) -> Self {
        self.group = Arc::from(v.into());
        self
    }
    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }
    pub fn is_hidden(mut self, v: bool) -> Self {
        self.cha = self.cha.with_hidden(v);
        self
    }
    pub fn mime_type(mut self, v: Option<String>) -> Self {
        self.mime_type = v;
        self
    }
    pub fn build(self) -> FileEntry {
        let (time_str, size_str, name_width, size_width, time_width) =
            FileEntry::cached_fields(&self.cha, &self.name);
        let category = compute_category(&self.cha, &self.name);
        FileEntry {
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
        }
    }
}

impl FileEntry {
    pub fn cached_fields(cha: &Cha, name: &str) -> (String, String, usize, usize, usize) {
        let time_str = format_time(cha.mtime().unwrap_or(std::time::UNIX_EPOCH));
        let size_str = if cha.is_dir() {
            "     <DIR>".to_string()
        } else {
            format!("{:>10}", format_size(cha.len()))
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
        self.cha.len()
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

    pub fn display_size(&self) -> String {
        Self::format_size(self.size())
    }

    pub fn format_size(size: u64) -> String {
        format!("{:>6}", crate::app::types::format_size(size))
    }

    pub fn display_permissions(&self) -> String {
        Self::display_permissions_raw(self.mode_bits())
    }

    pub fn display_permissions_raw(mode: u32) -> String {
        use crate::fs::cha::ChaMode;
        ChaMode::new(mode).to_string()
    }

    pub fn display_modified(&self) -> String {
        let mtime = self.cha.mtime.unwrap_or(std::time::UNIX_EPOCH);
        format_time(mtime)
    }
}
