use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use unicode_width::UnicodeWidthStr;

use crate::fs::cha::Cha;

/// Strip C0 controls / DEL from filenames for TUI display.
///
/// - `\n` → ⏎, `\t` → two spaces, `\r` dropped, other C0/DEL → ·
/// - **Not** a full ANSI CSI/OSC stripper: `ESC` becomes · and the following
///   payload (`[31m…`) stays visible. Filenames with real escape sequences are
///   rare; a full state machine was removed deliberately (ponytail audit).
pub(crate) fn sanitize_for_display(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|b| b <= 0x1F || b == 0x7F) {
        return Cow::Borrowed(s);
    }
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\n' => result.push('\u{23ce}'),
            '\r' => {}
            '\t' => result.push_str("  "),
            c if (c as u32) <= 0x1F || c == '\u{7F}' => result.push('\u{00b7}'),
            c => result.push(c),
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

const BYTES_PER_UNIT: f64 = 1024.0;

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
        format!("{} {}", size, units[unit_idx])
    } else {
        format!("{:.1} {}", size_f, units[unit_idx])
    }
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
    pub time_str: String,
    pub size_str: String,
    pub name_width: usize,
    pub size_width: usize,
    pub time_width: usize,
    pub category: FileCategory,
    pub sanitized_name: Option<String>,
}

impl FileEntry {
    /// Build a fully cached listing entry from core fields.
    pub fn new(
        name: String,
        path: PathBuf,
        cha: Cha,
        owner: impl AsRef<str>,
        group: impl AsRef<str>,
        selected: bool,
    ) -> Self {
        let (time_str, size_str, name_width, size_width, time_width) =
            Self::cached_fields(&cha, &name);
        let category = compute_category(&cha, &name);
        let sanitized_name = sanitize_name(&name);
        Self {
            name,
            path,
            cha,
            owner: Arc::from(owner.as_ref()),
            group: Arc::from(group.as_ref()),
            selected,
            time_str,
            size_str,
            name_width,
            size_width,
            time_width,
            category,
            sanitized_name,
        }
    }

    pub fn cached_fields(cha: &Cha, name: &str) -> (String, String, usize, usize, usize) {
        let time_str = format_time(cha.mtime.unwrap_or(std::time::UNIX_EPOCH));
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

    pub fn size(&self) -> u64 {
        self.cha.len
    }

    pub fn mtime(&self) -> SystemTime {
        self.cha.mtime.unwrap_or(std::time::UNIX_EPOCH)
    }

    pub fn btime(&self) -> SystemTime {
        self.cha.btime.unwrap_or(std::time::UNIX_EPOCH)
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
        // Right-align to width 6 for the properties dialog.
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
