use std::borrow::Cow;
use std::cell::RefCell;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::ViewMode;

use super::hex::HEX_BYTES_PER_LINE;
use super::mime::{is_image_mime, should_open_as_text};

pub struct ViewerState {
    pub file_path: PathBuf,
    pub(crate) line_offsets: Vec<usize>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub line_count: usize,
    pub search_query: Option<String>,
    pub search_matches: Vec<(usize, usize, usize)>,
    pub(crate) search_matches_by_line: Vec<super::SearchLineMatch>,
    pub current_match: Option<usize>,
    pub wrap_lines: bool,
    pub show_line_numbers: bool,
    pub view_mode: ViewMode,
    pub(crate) raw_bytes: Vec<u8>,
    pub(crate) max_line_width: usize,
    pub detected_mime: Option<String>,
    pub file_size: usize,
    pub has_invalid_utf8: bool,
    pub(crate) originally_binary: bool,
    pub(crate) visual_heights: RefCell<Vec<usize>>,
    pub(crate) visual_offsets: RefCell<Vec<usize>>,
    pub(crate) cached_content_width: RefCell<usize>,
    pub(crate) file_truncated: bool,
    pub cached_image_size: Option<(u16, u16)>,
    pub cached_image_text: Option<ratatui::text::Text<'static>>,
}

impl ViewerState {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_cancel(path, None)
    }

    pub(crate) fn compute_line_offsets(bytes: &[u8]) -> Vec<usize> {
        if bytes.is_empty() {
            return vec![0];
        }
        let mut offsets = vec![0];
        offsets.extend(
            memchr::memchr_iter(b'\n', bytes)
                .filter(|&pos| pos + 1 < bytes.len())
                .map(|pos| pos + 1),
        );
        offsets
    }

    pub(crate) fn compute_max_line_width(line_offsets: &[usize], raw_bytes: &[u8]) -> usize {
        let mut max_w = 0;
        for (i, &start) in line_offsets.iter().enumerate() {
            let end = line_offsets.get(i + 1).copied().unwrap_or(raw_bytes.len());
            let line_end = if end > 0 && raw_bytes.get(end - 1) == Some(&b'\n') {
                end - 1
            } else {
                end
            };
            let w = match std::str::from_utf8(&raw_bytes[start..line_end]) {
                Ok(s) => unicode_width::UnicodeWidthStr::width(s),
                Err(_) => {
                    let cow = String::from_utf8_lossy(&raw_bytes[start..line_end]);
                    unicode_width::UnicodeWidthStr::width(cow.as_ref())
                }
            };
            if w > max_w {
                max_w = w;
            }
        }
        max_w
    }

    pub fn get_line(&self, idx: usize) -> Cow<'_, str> {
        if self.raw_bytes.is_empty() && idx == 0 {
            return Cow::Borrowed("[Empty file]");
        }
        let Some(&start) = self.line_offsets.get(idx) else {
            return Cow::Borrowed("");
        };
        let end = self
            .line_offsets
            .get(idx + 1)
            .copied()
            .unwrap_or(self.raw_bytes.len());
        let line_end = if end > start && self.raw_bytes.get(end - 1) == Some(&b'\n') {
            end - 1
        } else {
            end
        };
        if line_end <= start {
            return Cow::Borrowed("");
        }
        match std::str::from_utf8(&self.raw_bytes[start..line_end]) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => {
                Cow::Owned(String::from_utf8_lossy(&self.raw_bytes[start..line_end]).into_owned())
            }
        }
    }

    pub(crate) fn open_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<Self> {
        const MAX_VIEW_SIZE: usize = 100 * 1024 * 1024;
        const READ_CHUNK: usize = 64 * 1024;

        let file = fs::File::open(path)?;
        let mut raw_bytes = Vec::new();
        let mut reader = file.take((MAX_VIEW_SIZE + 1) as u64);
        let mut buf = [0u8; READ_CHUNK];
        loop {
            if let Some(c) = cancel
                && c.load(Ordering::Relaxed)
            {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            raw_bytes.extend_from_slice(&buf[..n]);
        }
        let file_truncated = raw_bytes.len() > MAX_VIEW_SIZE;
        if file_truncated {
            raw_bytes.truncate(MAX_VIEW_SIZE);
        }

        let file_size = fs::metadata(path)
            .map(|m| m.len() as usize)
            .unwrap_or(raw_bytes.len());
        let mime =
            crate::app::mime::detect_mime_from_bytes(path, &raw_bytes[..raw_bytes.len().min(8192)]);
        let open_as_text = should_open_as_text(path, mime.as_deref(), &raw_bytes);

        let has_invalid_utf8 =
            !raw_bytes.is_empty() && open_as_text && std::str::from_utf8(&raw_bytes).is_err();

        let line_offsets = if !open_as_text {
            Vec::new()
        } else {
            Self::compute_line_offsets(&raw_bytes)
        };
        let line_count = if raw_bytes.is_empty() {
            1
        } else if !open_as_text {
            raw_bytes.len().div_ceil(HEX_BYTES_PER_LINE)
        } else {
            line_offsets.len()
        };
        let max_line_width = if open_as_text && !raw_bytes.is_empty() {
            Self::compute_max_line_width(&line_offsets, &raw_bytes)
        } else {
            0
        };

        let is_image = is_image_mime(mime.as_deref());

        Ok(ViewerState {
            file_path: path.to_path_buf(),
            line_offsets,
            scroll_offset: 0,
            horizontal_offset: 0,
            line_count,
            search_query: None,
            search_matches: Vec::new(),
            search_matches_by_line: Vec::new(),
            current_match: None,
            wrap_lines: true,
            show_line_numbers: false,
            view_mode: if is_image {
                ViewMode::Image
            } else if open_as_text {
                ViewMode::Text
            } else {
                ViewMode::Hex
            },
            raw_bytes,
            max_line_width,
            detected_mime: mime,
            file_size,
            has_invalid_utf8,
            originally_binary: !open_as_text,
            visual_heights: RefCell::new(Vec::new()),
            visual_offsets: RefCell::new(Vec::new()),
            cached_content_width: RefCell::new(0),
            file_truncated,
            cached_image_size: None,
            cached_image_text: None,
        })
    }

    pub fn open_background(path: PathBuf) -> super::loader::ViewerLoader {
        super::loader::ViewerLoader::start(path)
    }

    #[must_use]
    pub fn is_hex_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Hex)
    }

    pub fn image_content_size(area_width: u16, area_height: u16) -> (u16, u16) {
        (area_width, area_height.saturating_sub(3))
    }
}
