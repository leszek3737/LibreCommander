use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{Local, TimeZone};
use unicode_width::UnicodeWidthStr;

use crate::fs::cha::Cha;

/// Strip C0 controls / DEL from filenames for TUI display.
///
/// - `\n` → ⏎, `\t` → two spaces, `\r` dropped, other C0/DEL → ·
/// - Also strips Unicode bidi-control and zero-width characters that can spoof
///   filename visual representation (e.g. `file.txt\u{202e}gpj` hiding an
///   extension, or zero-width chars making distinct names look identical):
///   U+200B–U+200D (zero-width space/joiner/non-joiner), U+200E–U+200F
///   (LRM/RLM), U+202A–U+202E (bidi embedding/override), U+2066–U+2069
///   (isolate), U+FEFF (BOM/zero-width no-break space).
/// - **Not** a full ANSI CSI/OSC stripper: `ESC` becomes · and the following
///   payload (`[31m…`) stays visible. Filenames with real escape sequences are
///   rare; a full state machine was removed deliberately (ponytail audit).
pub(crate) fn sanitize_for_display(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|b| b <= 0x1F || b == 0x7F) && !s.chars().any(is_unicode_spoofing_char) {
        return Cow::Borrowed(s);
    }
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if is_unicode_spoofing_char(ch) {
            continue;
        }
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

/// Returns `true` for Unicode bidi-control and zero-width characters that can
/// spoof a filename's visual representation in the TUI (extension hiding,
/// reordering, or invisible lookalikes).
const fn is_unicode_spoofing_char(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0x200B..=0x200D   // zero-width space / joiner / non-joiner
        | 0x200E..=0x200F // LRM / RLM
        | 0x202A..=0x202E // bidi embedding / override (incl. RLO U+202E)
        | 0x2066..=0x2069 // bidi isolate controls
        | 0xFEFF          // BOM / zero-width no-break space
    )
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
        // ponytail: at the largest unit (EB) rounding overflow cannot carry
        // up, and u64::MAX tops out at ~16 EB, so no clamp is needed here.
    }
    if unit_idx == 0 {
        format!("{} {}", size, units[unit_idx])
    } else {
        format!("{:.1} {}", size_f, units[unit_idx])
    }
}

pub(crate) fn format_system_time(modified: SystemTime) -> Option<String> {
    // Decompose to signed seconds so both pre- and post-epoch values convert
    // without `DateTime::from(SystemTime)` (which `.expect()`s on out-of-range
    // timestamps). `timestamp_opt` returns None outside chrono's range. The
    // format is minute-precision, so sub-second nanos are irrelevant.
    let secs = signed_epoch_secs(modified)?;
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|dt| dt.format("%d-%m-%y %H:%M").to_string())
}

/// Signed seconds from the Unix epoch, handling both pre- and post-1970 and
/// clamping out-of-i64 magnitudes to `i64::MAX` (which `timestamp_opt` then
/// rejects, yielding the None fallback).
pub(crate) fn signed_epoch_secs(t: SystemTime) -> Option<i64> {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => {
            let deficit = i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX);
            Some(deficit.checked_neg()?)
        }
    }
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
        // Width must match `display_name()`, which returns the sanitized form
        // (tabs→2 spaces, \n→⏎, etc.) — the raw name can have a different
        // visible width, misaligning columns.
        let display = sanitize_for_display(name);
        let name_width = UnicodeWidthStr::width(display.as_ref());
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
