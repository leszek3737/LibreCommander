use ratatui::{
    Frame,
    layout::Margin,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::borrow::Cow;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use crate::app::types::{ViewMode, format_size};

use super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchLineMatch {
    line: usize,
    global_idx: usize,
    start_byte: usize,
    end_byte: usize,
}

pub struct ViewerState {
    pub file_path: PathBuf,
    line_offsets: Vec<usize>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub line_count: usize,
    pub search_query: Option<String>,
    pub search_matches: Vec<(usize, usize, usize)>,
    search_matches_by_line: Vec<SearchLineMatch>,
    pub current_match: Option<usize>,
    pub wrap_lines: bool,
    pub show_line_numbers: bool,
    pub view_mode: ViewMode,
    raw_bytes: Vec<u8>,
    max_line_width: usize,
    pub detected_mime: Option<String>,
    pub file_size: usize,
    pub has_invalid_utf8: bool,
    originally_binary: bool,
    visual_heights: Vec<usize>,
    visual_offsets: Vec<usize>,
    cached_content_width: usize,
    file_truncated: bool,
}

pub struct ViewerLoader {
    pub receiver: mpsc::Receiver<io::Result<ViewerState>>,
    pub cancel: Arc<AtomicBool>,
    pub path: PathBuf,
    _handle: Option<JoinHandle<()>>,
}

impl ViewerLoader {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let owned_path = path.clone();
        let handle = thread::spawn(move || {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let result = ViewerState::open_with_cancel(&owned_path, Some(&cancel_flag));
            if !cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send(result);
            }
        });
        Self {
            receiver: rx,
            cancel,
            path,
            _handle: Some(handle),
        }
    }
}

impl Drop for ViewerLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl ViewerState {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_cancel(path, None)
    }

    fn compute_line_offsets(bytes: &[u8]) -> Vec<usize> {
        let mut offsets = Vec::new();
        if bytes.is_empty() {
            return offsets;
        }
        offsets.push(0);
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                let next = i + 1;
                if next < bytes.len() {
                    offsets.push(next);
                }
            }
            i += 1;
        }
        offsets
    }

    fn compute_max_line_width(line_offsets: &[usize], raw_bytes: &[u8]) -> usize {
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
        let start = *self.line_offsets.get(idx).unwrap_or(&0);
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

    fn open_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<Self> {
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

        let line_offsets = if raw_bytes.is_empty() || !open_as_text {
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
            view_mode: if open_as_text {
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
            visual_heights: Vec::new(),
            visual_offsets: Vec::new(),
            cached_content_width: 0,
            file_truncated,
        })
    }

    pub fn open_background(path: PathBuf) -> ViewerLoader {
        ViewerLoader::start(path)
    }

    #[must_use]
    pub fn is_hex_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Hex)
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    #[must_use]
    fn is_visual_scroll(&self) -> bool {
        self.wrap_lines && !self.is_hex_mode() && !self.visual_heights.is_empty()
    }

    #[must_use]
    fn total_visual_rows(&self) -> usize {
        self.visual_heights.iter().sum()
    }

    #[must_use]
    fn visual_row_to_logical(&self, visual_row: usize) -> (usize, usize) {
        const LINEAR_SEARCH_THRESHOLD: usize = 24;
        if self.visual_heights.len() <= LINEAR_SEARCH_THRESHOLD {
            let mut acc = 0usize;
            for (i, &h) in self.visual_heights.iter().enumerate() {
                if acc + h > visual_row {
                    return (i, visual_row - acc);
                }
                acc += h;
            }
            return (self.line_count.saturating_sub(1), 0);
        }
        let idx = self
            .visual_offsets
            .partition_point(|&offset| offset <= visual_row);
        if idx >= self.visual_offsets.len() {
            return (self.line_count.saturating_sub(1), 0);
        }
        let acc_before = if idx == 0 {
            0
        } else {
            self.visual_offsets[idx - 1]
        };
        (idx, visual_row - acc_before)
    }

    #[must_use]
    fn logical_to_visual_row(&self, logical_line: usize) -> usize {
        self.visual_heights.iter().take(logical_line).sum()
    }

    fn total_rows(&self) -> usize {
        if self.is_hex_mode() {
            self.raw_bytes.len().div_ceil(HEX_BYTES_PER_LINE)
        } else if self.is_visual_scroll() {
            self.total_visual_rows()
        } else {
            self.line_count
        }
    }

    #[must_use]
    fn max_scroll(&self) -> usize {
        self.total_rows().saturating_sub(1)
    }

    pub fn scroll_down(&mut self, lines: usize) {
        let max_scroll = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    pub fn page_up(&mut self, page_height: usize) {
        let step = page_height.max(1);
        self.scroll_up(step);
    }

    pub fn page_down(&mut self, page_height: usize) {
        let step = page_height.max(1);
        self.scroll_down(step);
    }

    pub fn go_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn go_to_bottom(&mut self, page_height: usize) {
        self.scroll_offset = self
            .total_rows()
            .saturating_sub(page_height)
            .min(self.max_scroll());
    }

    fn clear_search_results(&mut self) {
        if self.search_matches.capacity() > 1024 {
            self.search_matches = Vec::new();
        } else {
            self.search_matches.clear();
        }
        if self.search_matches_by_line.capacity() > 1024 {
            self.search_matches_by_line = Vec::new();
        } else {
            self.search_matches_by_line.clear();
        }
    }

    pub fn search(&mut self, query: &str, page_height: usize) {
        self.search_query = Some(query.to_string());
        self.clear_search_results();
        self.current_match = None;

        if query.is_empty() {
            return;
        }

        if self.is_hex_mode() {
            self.search_hex(query);
            return;
        }

        let lower_query: String = query.chars().flat_map(|c| c.to_lowercase()).collect();

        let mut lower_buf = String::new();
        let mut byte_map_buf = Vec::new();

        for line_idx in 0..self.line_count {
            let line = self.get_line(line_idx).into_owned();
            build_lowercase_mapping(&line, &mut lower_buf, &mut byte_map_buf);
            let mut search_start = 0;
            while let Some(pos) = lower_buf[search_start..].find(&lower_query) {
                let match_byte_start = search_start + pos;
                let match_byte_end = match_byte_start + lower_query.len();
                let orig_byte_start = byte_map_buf[match_byte_start];
                let mapped_end = byte_map_buf
                    .get(match_byte_end)
                    .copied()
                    .unwrap_or(line.len());
                let orig_byte_end = if mapped_end <= orig_byte_start && orig_byte_start < line.len()
                {
                    line[orig_byte_start..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| orig_byte_start + i)
                        .unwrap_or(line.len())
                } else {
                    mapped_end
                };
                let char_pos = line[..orig_byte_start].chars().count();
                let match_char_len = line[orig_byte_start..orig_byte_end].chars().count().max(1);
                let global_idx = self.search_matches.len();
                self.search_matches
                    .push((line_idx, char_pos, match_char_len));
                self.search_matches_by_line.push(SearchLineMatch {
                    line: line_idx,
                    global_idx,
                    start_byte: orig_byte_start,
                    end_byte: orig_byte_end,
                });
                search_start = match_byte_end;
            }
        }

        let current_logical = if self.is_visual_scroll() {
            self.visual_row_to_logical(self.scroll_offset).0
        } else {
            self.scroll_offset
        };
        for (i, &(line_idx, _, _)) in self.search_matches.iter().enumerate() {
            if line_idx >= current_logical {
                self.current_match = Some(i);
                self.scroll_to_current_match(page_height);
                return;
            }
        }
        if !self.search_matches.is_empty() {
            self.current_match = Some(0);
            self.scroll_to_current_match(page_height);
        }
    }

    fn search_hex(&mut self, query: &str) {
        let bpl = HEX_BYTES_PER_LINE;
        let lower_query: String = query.chars().flat_map(|c| c.to_lowercase()).collect();
        let query_bytes = Self::parse_hex_query(&lower_query);

        if let Some(ref needle) = query_bytes {
            let mut pos = 0;
            while let Some(idx) = find_bytes(&self.raw_bytes[pos..], needle) {
                let abs_offset = pos + idx;
                let line_idx = abs_offset / bpl;
                let byte_in_line = abs_offset % bpl;

                let hex_col = byte_in_line * 3 + if byte_in_line >= 8 { 1 } else { 0 };
                let match_len = needle.len().min(bpl - byte_in_line);

                let global_idx = self.search_matches.len();
                self.search_matches
                    .push((line_idx, hex_col, match_len * 3 - 1));
                self.search_matches_by_line.push(SearchLineMatch {
                    line: line_idx,
                    global_idx,
                    start_byte: hex_col,
                    end_byte: hex_col + match_len * 3 - 1,
                });

                pos = abs_offset + 1;
            }
        } else {
            let lossy = String::from_utf8_lossy(&self.raw_bytes);
            let mut lower_buf = String::with_capacity(lossy.len());
            let mut byte_map: Vec<usize> = Vec::with_capacity(lossy.len());
            for (byte_pos, ch) in lossy.char_indices() {
                for lc in ch.to_lowercase() {
                    for _ in 0..lc.len_utf8() {
                        byte_map.push(byte_pos);
                    }
                    lower_buf.push(lc);
                }
            }
            let mut search_start = 0;
            while let Some(pos) = lower_buf[search_start..].find(&lower_query) {
                let abs_pos = search_start + pos;
                let advance = lower_buf[abs_pos..]
                    .chars()
                    .next()
                    .map_or(1, |c| c.len_utf8());
                search_start = abs_pos + advance;

                let orig_byte = byte_map.get(abs_pos).copied().unwrap_or(abs_pos);
                let line_idx = orig_byte / bpl;
                let byte_in_line = orig_byte % bpl;
                let hex_col = byte_in_line * 3 + if byte_in_line >= 8 { 1 } else { 0 };
                let match_byte_len = lower_query
                    .len()
                    .min(self.raw_bytes.len().saturating_sub(orig_byte));
                if match_byte_len == 0 {
                    continue;
                }
                let match_hex_len = match_byte_len * 3 - 1;

                let global_idx = self.search_matches.len();
                self.search_matches.push((line_idx, hex_col, match_hex_len));
                self.search_matches_by_line.push(SearchLineMatch {
                    line: line_idx,
                    global_idx,
                    start_byte: hex_col,
                    end_byte: hex_col + match_hex_len,
                });
            }
        }

        if !self.search_matches.is_empty() {
            self.current_match = Some(0);
            self.scroll_offset = self.search_matches[0].0.min(self.max_scroll());
        }
    }

    fn parse_hex_query(query: &str) -> Option<Vec<u8>> {
        let cleaned: String = query.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.len() < 2 || !cleaned.len().is_multiple_of(2) {
            return None;
        }
        let lower: String = cleaned.chars().flat_map(|c| c.to_lowercase()).collect();
        (0..lower.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&lower[i..i + 2], 16).ok())
            .collect()
    }

    fn scroll_to_current_match(&mut self, page_height: usize) {
        let Some(current) = self.current_match else {
            return;
        };
        if let Some(&(line_idx, _, _)) = self.search_matches.get(current) {
            let context = 5usize.min(page_height.saturating_sub(1));
            if self.is_visual_scroll() {
                let visual_row = self.logical_to_visual_row(line_idx);
                self.scroll_offset = visual_row.saturating_sub(context).min(self.max_scroll());
            } else {
                self.scroll_offset = line_idx.saturating_sub(context).min(self.max_scroll());
            }
        }
    }

    pub fn next_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.current_match.unwrap_or(0);
        self.current_match = Some((current + 1) % self.search_matches.len());
        self.scroll_to_current_match(page_height);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
    }

    pub fn prev_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.current_match.unwrap_or(0);
        self.current_match = Some(if current == 0 {
            self.search_matches.len() - 1
        } else {
            current - 1
        });
        self.scroll_to_current_match(page_height);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        self.visual_heights.clear();
        self.visual_offsets.clear();
        self.cached_content_width = 0;
        if !self.is_hex_mode() {
            self.view_mode = ViewMode::Text;
        }
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap_lines = !self.wrap_lines;
        if self.wrap_lines {
            self.horizontal_offset = 0;
        }
        self.visual_heights.clear();
        self.visual_offsets.clear();
        self.cached_content_width = 0;
        if !self.is_hex_mode() {
            self.view_mode = ViewMode::Text;
        }
    }

    pub fn toggle_hex_mode(&mut self) {
        self.view_mode = if self.is_hex_mode() {
            ViewMode::Text
        } else {
            ViewMode::Hex
        };
        self.scroll_offset = 0;
        self.horizontal_offset = 0;

        if !self.is_hex_mode() && self.originally_binary {
            self.line_offsets = Self::compute_line_offsets(&self.raw_bytes);
            self.line_count = if self.raw_bytes.is_empty() {
                1
            } else {
                self.line_offsets.len()
            };
            self.max_line_width = if self.raw_bytes.is_empty() {
                0
            } else {
                Self::compute_max_line_width(&self.line_offsets, &self.raw_bytes)
            };
            self.clear_search_results();
            self.current_match = None;
            self.search_query = None;
            self.has_invalid_utf8 = std::str::from_utf8(&self.raw_bytes).is_err();
        }
    }

    pub fn update_wrap_layout(&mut self, content_width: usize) {
        if !self.wrap_lines || self.is_hex_mode() || self.line_count == 0 {
            if !self.visual_heights.is_empty() {
                self.visual_heights.clear();
                self.visual_offsets.clear();
                self.cached_content_width = 0;
            }
            return;
        }
        if self.cached_content_width == content_width && !self.visual_heights.is_empty() {
            return;
        }
        let line_num_width = if self.show_line_numbers {
            line_number_column_width(self.line_count)
        } else {
            0
        };
        let width = content_width.max(1);
        self.visual_heights = (0..self.line_count)
            .map(|i| {
                let line = self.get_line(i);
                let text_width = unicode_width::UnicodeWidthStr::width(line.as_ref());
                let total_width = line_num_width.saturating_add(text_width);
                total_width.div_ceil(width).max(1)
            })
            .collect();
        self.visual_offsets = Vec::with_capacity(self.visual_heights.len());
        let mut acc = 0usize;
        for &h in &self.visual_heights {
            acc += h;
            self.visual_offsets.push(acc);
        }
        self.cached_content_width = content_width;
        let max = self.max_scroll();
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }

    pub fn scroll_left(&mut self, cols: usize) {
        self.horizontal_offset = self.horizontal_offset.saturating_sub(cols);
    }

    pub fn scroll_right(&mut self, cols: usize, visible_width: usize) {
        let line_num_width = if self.show_line_numbers {
            line_number_column_width(self.line_count)
        } else {
            0
        };
        let effective_width = visible_width.saturating_sub(line_num_width);
        let max_line = if self.is_hex_mode() {
            HEX_LINE_WIDTH
        } else {
            self.max_line_width
        };
        let max_offset = if effective_width > 0 {
            max_line.saturating_sub(effective_width)
        } else {
            max_line
        };
        self.horizontal_offset = (self.horizontal_offset + cols).min(max_offset);
    }
}

fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).ilog10() as usize + 1
}

fn line_number_column_width(line_count: usize) -> usize {
    line_number_digits(line_count) + 2
}

fn paragraph_horizontal_scroll(horizontal_offset: usize) -> u16 {
    horizontal_offset.min(u16::MAX as usize) as u16
}

fn build_lowercase_mapping(original: &str, lower: &mut String, byte_map: &mut Vec<usize>) {
    lower.clear();
    byte_map.clear();
    for (orig_byte_idx, ch) in original.char_indices() {
        let len_before = lower.len();
        lower.extend(ch.to_lowercase());
        for _ in len_before..lower.len() {
            byte_map.push(orig_byte_idx);
        }
    }
}

fn should_open_as_text(path: &Path, mime: Option<&str>, bytes: &[u8]) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if crate::app::file_type::is_source_code(name) || crate::app::file_type::is_config(name) {
        return true;
    }

    if let Some(mime) = mime
        && is_known_binary_mime(mime)
    {
        return false;
    }

    if bytes.contains(&0) {
        return false;
    }

    if let Some(mime) = mime
        && (mime.starts_with("text/") || is_text_application_mime(mime))
    {
        return true;
    }

    true
}

fn is_text_application_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/json"
            | "application/toml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/xml"
            | "application/javascript"
            | "application/typescript"
            | "application/ecmascript"
            | "application/sql"
            | "application/x-httpd-php"
            | "application/x-sh"
            | "application/rtf"
    )
}

fn is_known_binary_mime(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime.starts_with("application/vnd.oasis.opendocument.")
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.ms-")
        || matches!(
            mime,
            "application/octet-stream"
                | "application/zip"
                | "application/x-tar"
                | "application/gzip"
                | "application/x-gzip"
                | "application/x-bzip2"
                | "application/x-xz"
                | "application/x-7z-compressed"
                | "application/vnd.rar"
                | "application/x-rar-compressed"
                | "application/zstd"
                | "application/pdf"
                | "application/msword"
                | "application/epub+zip"
                | "application/wasm"
                | "application/x-mach-binary"
                | "application/x-dosexec"
                | "application/x-executable"
                | "application/x-sharedlib"
                | "application/x-object"
        )
}

fn format_line_with_highlight<'a>(
    line: &'a str,
    line_matches: &[SearchLineMatch],
    current_match_idx: Option<usize>,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    if line_matches.is_empty() {
        return vec![Span::raw(line)];
    }

    let regular_style = Style::default()
        .fg(Theme::search_match_fg())
        .bg(Theme::search_match_bg());
    let current_style = Style::default()
        .fg(Theme::search_match_current_fg())
        .bg(Theme::search_match_current_bg())
        .add_modifier(Modifier::BOLD);

    let mut last_end = 0usize;
    let mut prev_match_start: Option<usize> = None;

    for line_match in line_matches {
        let orig_start = line_match.start_byte.min(line.len());
        let end_byte = line_match.end_byte.min(line.len());
        let is_current = Some(line_match.global_idx) == current_match_idx;

        let start_byte;
        let mut overlap_prev_end: Option<usize> = None;

        if orig_start < last_end && is_current {
            if let Some(ps) = prev_match_start
                && ps < orig_start
                && !spans.is_empty()
            {
                let last_idx = spans.len() - 1;
                spans[last_idx] = Span::styled(&line[ps..orig_start], regular_style);
                overlap_prev_end = Some(last_end);
            }
            start_byte = orig_start;
        } else if orig_start < last_end {
            start_byte = last_end;
        } else {
            start_byte = orig_start;
        }

        if end_byte <= start_byte {
            continue;
        }

        if start_byte > last_end {
            spans.push(Span::raw(&line[last_end..start_byte]));
        }

        let style = if is_current {
            current_style
        } else {
            regular_style
        };
        spans.push(Span::styled(&line[start_byte..end_byte], style));
        prev_match_start = Some(start_byte);
        last_end = end_byte;

        if let Some(pe) = overlap_prev_end
            && end_byte < pe
        {
            spans.push(Span::styled(&line[end_byte..pe], regular_style));
            prev_match_start = Some(end_byte);
            last_end = pe;
        }
    }

    if last_end < line.len() {
        spans.push(Span::raw(&line[last_end..]));
    }

    spans
}

fn render_viewer_status(
    f: &mut Frame,
    inner_area: Rect,
    state: &ViewerState,
    mode_label: &str,
    position_text: &str,
) {
    let status_area = Rect {
        x: inner_area.x,
        y: inner_area.y + inner_area.height.saturating_sub(1),
        width: inner_area.width,
        height: 1,
    };

    let mime_label = state.detected_mime.as_deref().unwrap_or("—");
    let size_label = format_size(state.file_size as u64);
    let utf8_warning = if state.has_invalid_utf8 {
        " \u{26a0} INVALID UTF-8"
    } else {
        ""
    };
    let binary_warning = if !state.is_hex_mode() && state.originally_binary {
        " \u{26a0} BINARY CONTENT"
    } else {
        ""
    };
    let truncated_warning = if state.file_truncated {
        " \u{26a0} TRUNCATED"
    } else {
        ""
    };
    let status_text = format!(
        " {mode_label}  {mime_label}  {size_label}  {position_text}{utf8_warning}{binary_warning}{truncated_warning}",
    );
    let status_style = if state.has_invalid_utf8
        || (!state.is_hex_mode() && state.originally_binary)
        || state.file_truncated
    {
        Theme::status_bar().fg(Theme::warning_color())
    } else {
        Theme::status_bar()
    };
    let status_paragraph = Paragraph::new(status_text).style(status_style);
    f.render_widget(status_paragraph, status_area);
}

pub fn render_viewer(f: &mut Frame, area: Rect, state: &ViewerState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel())
        .title(state.file_path.display().to_string())
        .title_style(Theme::title());
    f.render_widget(block, area);

    let inner_area = area.inner(Margin {
        horizontal: 0,
        vertical: 1,
    });

    if inner_area.height == 0 {
        return;
    }

    let content_area = Rect {
        x: inner_area.x,
        y: inner_area.y,
        width: inner_area.width,
        height: inner_area.height.saturating_sub(1),
    };

    let mut lines: Vec<Line> = Vec::new();
    let visible_height = content_area.height as usize;

    let use_visual = state.is_visual_scroll();

    let (start_idx, sub_row, end_idx) = if use_visual {
        let (logical_start, sub) = state.visual_row_to_logical(state.scroll_offset);
        let mut visual_budget = visible_height.saturating_add(sub);
        let mut end = logical_start;
        while end < state.line_count && visual_budget > 0 {
            visual_budget = visual_budget.saturating_sub(state.visual_heights[end]);
            end += 1;
        }
        (logical_start, sub, end)
    } else {
        let start = state.scroll_offset;
        let end = (start + visible_height).min(state.line_count);
        (start, 0, end)
    };

    let visible_matches = &state.search_matches_by_line;
    let mut match_start = visible_matches.partition_point(|line_match| line_match.line < start_idx);
    for i in start_idx..end_idx {
        let line_content = state.get_line(i).into_owned();
        let line_match_start = match_start;
        while match_start < visible_matches.len() && visible_matches[match_start].line == i {
            match_start += 1;
        }
        let line_matches = &visible_matches[line_match_start..match_start];
        let spans: Vec<Span> = if state.show_line_numbers {
            let line_num = format!(
                "{:>width$}  ",
                i + 1,
                width = line_number_digits(state.line_count)
            );
            let mut line_spans = vec![Span::raw(line_num)];
            if line_matches.is_empty() {
                line_spans.push(Span::raw(line_content));
            } else {
                line_spans.extend(
                    format_line_with_highlight(&line_content, line_matches, state.current_match)
                        .into_iter()
                        .map(|s| Span::styled(s.content.into_owned(), s.style)),
                );
            }
            line_spans
        } else if line_matches.is_empty() {
            vec![Span::raw(line_content)]
        } else {
            format_line_with_highlight(&line_content, line_matches, state.current_match)
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect()
        };

        lines.push(Line::from(spans));
    }

    let mut paragraph = Paragraph::new(lines);
    if state.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
        if use_visual && sub_row > 0 {
            paragraph = paragraph.scroll((sub_row as u16, 0));
        }
    } else {
        paragraph = paragraph.scroll((0, paragraph_horizontal_scroll(state.horizontal_offset)));
    }

    f.render_widget(paragraph, content_area);

    let current_line = if use_visual {
        state.visual_row_to_logical(state.scroll_offset).0
    } else if state.line_count == 0 {
        0
    } else {
        state.scroll_offset
    };
    let position_text = format!("Line: {}/{}", current_line + 1, state.line_count);
    render_viewer_status(f, inner_area, state, "Text", &position_text);
}

pub fn render_hex_view(f: &mut Frame, area: Rect, state: &ViewerState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel())
        .title(format!("{} [Hex]", state.file_path.display()))
        .title_style(Theme::title());
    f.render_widget(block, area);

    let inner_area = area.inner(Margin {
        horizontal: 0,
        vertical: 1,
    });
    if inner_area.height == 0 {
        return;
    }

    let content_area = Rect {
        x: inner_area.x,
        y: inner_area.y,
        width: inner_area.width,
        height: inner_area.height.saturating_sub(1),
    };

    let bytes = &state.raw_bytes;
    let bytes_per_line = HEX_BYTES_PER_LINE;
    let total_lines = bytes.len().div_ceil(bytes_per_line);

    let start_line = state.scroll_offset.min(total_lines.saturating_sub(1));
    let visible_lines = content_area.height as usize;
    let end_line = (start_line + visible_lines).min(total_lines);

    let mut lines: Vec<Line> = Vec::new();

    let visible_matches = &state.search_matches_by_line;
    let mut match_start =
        visible_matches.partition_point(|line_match| line_match.line < start_line);

    let mut hex_line_buffer = String::with_capacity(128);
    for line_idx in start_line..end_line {
        let offset = line_idx * bytes_per_line;
        let slice_len = (bytes.len() - offset).min(bytes_per_line);
        let slice = &bytes[offset..offset + slice_len];
        hex_line_buffer.clear();
        format_hex_line_to_buffer(offset, slice, &mut hex_line_buffer);

        let line_match_start = match_start;
        while match_start < visible_matches.len() && visible_matches[match_start].line == line_idx {
            match_start += 1;
        }
        let line_matches = &visible_matches[line_match_start..match_start];

        let spans: Vec<Span<'static>> = if line_matches.is_empty() {
            vec![Span::raw(std::mem::take(&mut hex_line_buffer))]
        } else {
            let highlighted =
                format_line_with_highlight(&hex_line_buffer, line_matches, state.current_match);
            highlighted
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect()
        };
        lines.push(Line::from(spans));
    }

    let paragraph =
        Paragraph::new(lines).scroll((0, paragraph_horizontal_scroll(state.horizontal_offset)));
    f.render_widget(paragraph, content_area);

    let current_line = if total_lines == 0 {
        0
    } else {
        state.scroll_offset + 1
    };
    let position_text = format!("Offset: {current_line}/{total_lines}");
    render_viewer_status(f, inner_area, state, "Hex", &position_text);
}

#[cfg(test)]
#[must_use]
fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut buf = String::with_capacity(128);
    format_hex_line_to_buffer(offset, bytes, &mut buf);
    buf
}

const HEX_BYTES_PER_LINE: usize = 16;
const HEX_PART_WIDTH: usize = HEX_BYTES_PER_LINE * 3 + 1;
const HEX_LINE_WIDTH: usize = 10 + HEX_PART_WIDTH + 2 + HEX_BYTES_PER_LINE + 1;

fn format_hex_line_to_buffer(offset: usize, bytes: &[u8], buf: &mut String) {
    use std::fmt::Write;
    let _ = write!(buf, "{offset:08x}: ");

    let hex_start = buf.len();
    for (i, b) in bytes.iter().enumerate() {
        if i == 8 {
            buf.push(' ');
        }
        let _ = write!(buf, "{b:02x} ");
    }

    let padding_needed = HEX_PART_WIDTH.saturating_sub(buf.len() - hex_start);
    let _ = write!(buf, "{:width$}", "", width = padding_needed);

    buf.push_str(" |");
    for &b in bytes {
        let c = if (32..=126).contains(&b) {
            b as char
        } else {
            '.'
        };
        buf.push(c);
    }
    buf.push('|');
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    if needle.len() == 1 {
        return haystack.iter().position(|&b| b == needle[0]);
    }
    let first = needle[0];
    let end = haystack.len() - needle.len() + 1;
    let mut i = 0;
    while i < end {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn render_loading(f: &mut Frame, area: Rect, path: &Path) {
    let spinner_chars = ['|', '/', '-', '\\'];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 200;
    let spinner = spinner_chars[idx as usize % spinner_chars.len()];

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let msg = format!("{spinner} Loading {name}...");

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Theme::panel_bg());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text_width = unicode_width::UnicodeWidthStr::width(msg.as_str()) as u16;
    let x = inner.x + inner.width.saturating_sub(text_width) / 2;
    let y = inner.y + inner.height.saturating_sub(1) / 2;
    f.render_widget(
        Paragraph::new(msg).style(Style::default().add_modifier(Modifier::BOLD)),
        Rect::new(
            x.min(inner.right().saturating_sub(text_width)),
            y.min(inner.bottom().saturating_sub(1)),
            text_width.min(inner.width),
            1,
        ),
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper to create a test file
    fn create_test_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        file
    }

    fn render_viewer_buffer(state: &ViewerState, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_viewer(frame, frame.area(), state))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_line(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn test_open_file() {
        let content = "Line 1\nLine 2\nLine 3";
        let file = create_test_file(content);
        let state = ViewerState::open(file.path()).unwrap();

        assert_eq!(state.line_count, 3);
        assert_eq!(state.get_line(0), "Line 1");
        assert_eq!(state.get_line(1), "Line 2");
        assert_eq!(state.get_line(2), "Line 3");
        assert_eq!(state.line_count, 3);
    }

    #[test]
    fn test_open_file_with_trailing_newline_omits_empty_tail() {
        let file = create_test_file("Line 1\nLine 2\n");
        let state = ViewerState::open(file.path()).unwrap();

        assert_eq!(state.line_count, 2);
        assert_eq!(state.get_line(0), "Line 1");
        assert_eq!(state.get_line(1), "Line 2");
    }

    #[test]
    fn test_scroll_up() {
        let content = "Line 1\nLine 2\nLine 3";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.scroll_offset = 5; // Simulate scroll down
        state.scroll_up(2);
        assert_eq!(state.scroll_offset, 3);

        state.scroll_up(10); // Should not go below 0
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_down() {
        let content = "Line 1\nLine 2\nLine 3";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 1);

        state.scroll_down(5); // Should clamp
        assert_eq!(state.scroll_offset, 2); // Max index is 2 for 3 lines
    }

    #[test]
    fn test_page_up_down() {
        let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.wrap_lines = false;

        state.scroll_offset = 10;
        let page_height = 5;
        state.page_up(page_height);
        assert_eq!(state.scroll_offset, 5);

        state.page_down(page_height);
        assert_eq!(state.scroll_offset, 4);
    }

    #[test]
    fn test_go_to_top_bottom() {
        let content = "Line 1\nLine 2\nLine 3";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.go_to_bottom(1);
        assert_eq!(state.scroll_offset, 2);

        state.go_to_top();
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_search() {
        let content = "apple\nbanana\ncherry\napple pie";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("apple", 20);

        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.search_matches[0], (0, 0, 5));
        assert_eq!(state.search_matches[1], (3, 0, 5));
        assert_eq!(state.current_match, Some(0));
    }

    #[test]
    fn test_next_prev_match() {
        let content = "apple\nbanana\napple pie";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("apple", 20);
        assert_eq!(state.current_match, Some(0));

        state.next_match(20);
        assert_eq!(state.current_match, Some(1));
        assert_eq!(state.scroll_offset, 0);

        state.next_match(20);
        assert_eq!(state.current_match, Some(0));

        state.prev_match(20);
        assert_eq!(state.current_match, Some(1));
    }

    #[test]
    fn test_search_case_insensitive() {
        let content = "Hello World\nfoo BAR\nhello world";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("hello", 20);

        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.search_matches[0], (0, 0, 5));
        assert_eq!(state.search_matches[1], (2, 0, 5));
    }

    #[test]
    fn test_open_empty_file_has_placeholder() {
        let file = create_test_file("");
        let state = ViewerState::open(file.path()).unwrap();

        assert_eq!(state.line_count, 1);
        assert_eq!(state.get_line(0), "[Empty file]");
        assert_eq!(state.line_count, 1);
        assert_eq!(state.file_size, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_should_open_as_text_allows_text_mime() {
        assert!(should_open_as_text(
            Path::new("README"),
            Some("text/plain"),
            b"hello"
        ));
    }

    #[test]
    fn test_should_open_as_text_allows_source_and_config_extensions() {
        assert!(should_open_as_text(
            Path::new("main.rs"),
            Some("application/octet-stream"),
            b"fn main() {}"
        ));
        assert!(should_open_as_text(
            Path::new("config.toml"),
            Some("application/octet-stream"),
            b"key = \"value\""
        ));
    }

    #[test]
    fn test_should_open_as_text_rejects_known_binary_mime() {
        assert!(!should_open_as_text(
            Path::new("archive.zip"),
            Some("application/zip"),
            b"PK\0\0"
        ));
        assert!(!should_open_as_text(
            Path::new("image.png"),
            Some("image/png"),
            b"\x89PNG\r\n"
        ));
    }

    #[test]
    fn test_should_open_as_text_rejects_unknown_nul_bytes() {
        assert!(!should_open_as_text(
            Path::new("unknown.bin"),
            None,
            b"abc\0def"
        ));
    }

    #[test]
    fn test_open_binary_file_defaults_to_hex_mode() {
        let mut file = NamedTempFile::with_suffix(".bin").unwrap();
        file.write_all(b"abc\0def").unwrap();

        let state = ViewerState::open(file.path()).unwrap();

        assert!(state.is_hex_mode());
        assert_eq!(state.raw_bytes, b"abc\0def");
    }

    #[test]
    fn test_source_code_ext_opens_as_text_even_with_nul_bytes() {
        let mut file = NamedTempFile::with_suffix(".rs").unwrap();
        file.write_all(b"fn main() {}\0\0\0\0binary").unwrap();
        let state = ViewerState::open(file.path()).unwrap();
        assert!(!state.is_hex_mode());
    }

    #[test]
    fn test_search_unicode_match_uses_char_columns() {
        let file = create_test_file("zażółć gęślą jaźń");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("gęśl", 20);

        assert_eq!(state.search_matches, vec![(0, 7, 4)]);
    }

    #[test]
    fn test_search_unicode_repeated_matches_keep_char_columns() {
        let file = create_test_file("żółw żółw");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("żółw", 20);

        assert_eq!(state.search_matches, vec![(0, 0, 4), (0, 5, 4)]);
    }

    #[test]
    fn test_format_line_with_highlight_handles_unicode() {
        let _spans = format_line_with_highlight(
            "zażółć gęślą jaźń",
            &[SearchLineMatch {
                line: 0,
                global_idx: 0,
                start_byte: 11,
                end_byte: 17,
            }],
            Some(0),
        );
    }

    #[test]
    fn test_format_line_with_highlight_overlapping_matches_no_duplicates() {
        let line = "0123456789abcdef";
        let regular_style = Style::default()
            .fg(Theme::search_match_fg())
            .bg(Theme::search_match_bg());
        let current_style = Style::default()
            .fg(Theme::search_match_current_fg())
            .bg(Theme::search_match_current_bg())
            .add_modifier(Modifier::BOLD);

        let spans = format_line_with_highlight(
            line,
            &[
                SearchLineMatch {
                    line: 0,
                    global_idx: 0,
                    start_byte: 3,
                    end_byte: 9,
                },
                SearchLineMatch {
                    line: 0,
                    global_idx: 1,
                    start_byte: 4,
                    end_byte: 10,
                },
            ],
            Some(1),
        );

        let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            rendered, line,
            "overlapping matches must not duplicate text"
        );

        assert_eq!(spans[0], Span::raw("012"));
        assert_eq!(spans[1], Span::styled("3", regular_style));
        assert_eq!(spans[2], Span::styled("456789", current_style));
        assert_eq!(spans[3], Span::raw("abcdef"));
    }

    #[test]
    fn test_format_line_with_highlight_fully_overlapping_skipped() {
        let line = "abcdefghij";
        let spans = format_line_with_highlight(
            line,
            &[
                SearchLineMatch {
                    line: 0,
                    global_idx: 0,
                    start_byte: 2,
                    end_byte: 8,
                },
                SearchLineMatch {
                    line: 0,
                    global_idx: 1,
                    start_byte: 3,
                    end_byte: 6,
                },
            ],
            None,
        );

        let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, line, "fully overlapped match must be skipped");
    }

    #[test]
    fn test_search_line_match_cache_stores_unicode_byte_ranges() {
        let file = create_test_file("zażółć gęślą jaźń");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("gęśl", 20);

        assert_eq!(
            state.search_matches_by_line,
            vec![SearchLineMatch {
                line: 0,
                global_idx: 0,
                start_byte: 11,
                end_byte: 17,
            }]
        );
    }

    #[test]
    fn test_search_replace_clears_line_match_cache() {
        let file = create_test_file("alpha\nbeta");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("alpha", 20);
        state.search("missing", 20);

        assert!(state.search_matches.is_empty());
        assert!(state.search_matches_by_line.is_empty());
    }

    #[test]
    fn test_horizontal_scroll_uses_cached_max_line_width() {
        let file = create_test_file("short\nabcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ");
        let mut state = ViewerState::open(file.path()).unwrap();

        assert_eq!(state.max_line_width, 46);
        state.scroll_right(100, 10);

        assert_eq!(state.horizontal_offset, 36);
    }

    #[test]
    fn test_format_hex_line() {
        let bytes = &[
            0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x00,
        ];
        let line = format_hex_line(0x1000, bytes);

        assert!(line.starts_with("00001000:"));
        assert!(line.contains("48 65 6c 6c 6f 20 57 6f  72 6c 64 00"));
        assert!(line.contains("|Hello World.|"));
    }

    #[test]
    fn test_open_valid_replacement_character_is_not_invalid_utf8() {
        let file = create_test_file("valid replacement: \u{FFFD}");

        let state = ViewerState::open(file.path()).unwrap();

        assert!(!state.has_invalid_utf8);
        assert!(state.get_line(0).contains('\u{FFFD}'));
    }

    #[test]
    fn test_open_invalid_utf8_sets_warning() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"valid\xffinvalid").unwrap();

        let state = ViewerState::open(file.path()).unwrap();

        assert!(state.has_invalid_utf8);
        assert!(state.get_line(0).contains('\u{FFFD}'));
    }

    #[test]
    fn test_format_hex_line_accepts_more_than_sixteen_bytes() {
        let bytes = [b'A'; 17];

        let line = format_hex_line(0, &bytes);

        assert!(line.starts_with("00000000:"));
        assert!(line.ends_with("|AAAAAAAAAAAAAAAAA|"));
    }

    #[test]
    fn test_toggle_states() {
        let file = create_test_file("test");
        let mut state = ViewerState::open(file.path()).unwrap();

        assert!(!state.show_line_numbers);
        state.toggle_line_numbers();
        assert!(state.show_line_numbers);

        assert!(state.wrap_lines);
        state.toggle_wrap();
        assert!(!state.wrap_lines);

        assert!(!state.is_hex_mode());
        state.toggle_hex_mode();
        assert!(state.is_hex_mode());
        assert_eq!(state.view_mode, ViewMode::Hex);
        state.toggle_hex_mode();
        assert_eq!(state.view_mode, ViewMode::Text);
    }

    #[test]
    fn test_horizontal_scroll() {
        let file = create_test_file("abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.scroll_right(5, 10);
        assert_eq!(state.horizontal_offset, 5);

        state.scroll_right(100, 10);
        assert_eq!(state.horizontal_offset, 36);

        state.scroll_left(2);
        assert_eq!(state.horizontal_offset, 34);

        state.scroll_left(100);
        assert_eq!(state.horizontal_offset, 0);
    }

    #[test]
    fn test_line_number_width_expands_for_large_files() {
        assert_eq!(line_number_column_width(9_999), 6);
        assert_eq!(line_number_column_width(10_000), 7);

        let content = (1..=10_000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.show_line_numbers = true;
        state.wrap_lines = false;
        state.scroll_offset = 9_999;

        let buffer = render_viewer_buffer(&state, 24, 4);

        assert!(buffer_line(&buffer, 1).contains("10000  line 10000"));
    }

    #[test]
    fn test_horizontal_scroll_uses_dynamic_line_number_width() {
        let content = (1..=10_000)
            .map(|_| "abcdefghijkl".to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.show_line_numbers = true;

        state.scroll_right(100, 10);

        assert_eq!(state.horizontal_offset, 9);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_paragraph_horizontal_scroll_clamps_to_u16() {
        assert_eq!(paragraph_horizontal_scroll(usize::MAX), u16::MAX);
    }

    #[test]
    fn test_render_viewer_reserves_last_row_for_status_bar() {
        let file = create_test_file("line 1\nline 2\nline 3");
        let mut state = ViewerState::open(file.path()).unwrap();
        state.wrap_lines = false;

        let buffer = render_viewer_buffer(&state, 60, 5);

        assert!(buffer_line(&buffer, 1).contains("line 1"));
        assert!(buffer_line(&buffer, 2).contains("line 2"));
        let status = buffer_line(&buffer, 3);
        assert!(status.contains("Line: 1/3"));
        assert!(status.contains("Text"));
        assert!(!status.contains("line 3"));
    }

    #[test]
    fn test_wrap_scroll_advances_by_visual_row() {
        let long_line = "a".repeat(200);
        let content = format!("short\n{long_line}\nend");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        assert!(state.wrap_lines);

        state.update_wrap_layout(80);
        assert!(!state.visual_heights.is_empty());

        let short_height = state.visual_heights[0];
        let long_height = state.visual_heights[1];
        assert_eq!(short_height, 1);
        assert!(
            long_height > 1,
            "long line should wrap to multiple visual rows"
        );

        let total_visual: usize = state.visual_heights.iter().sum();
        assert!(total_visual > state.line_count);

        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 1);

        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 2);

        state.scroll_up(1);
        assert_eq!(state.scroll_offset, 1);

        let max = state.max_scroll();
        assert_eq!(max, total_visual.saturating_sub(1));
        assert!(max > state.line_count);
    }

    #[test]
    fn test_wrap_scroll_with_narrow_width() {
        let content = "abcdefghij";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.update_wrap_layout(5);

        assert_eq!(state.visual_heights.len(), 1);
        assert_eq!(state.visual_heights[0], 2);

        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 1);

        let max = state.max_scroll();
        assert_eq!(max, 1);
    }

    #[test]
    fn test_wrap_go_to_bottom_uses_visual_rows() {
        let long_line = "x".repeat(160);
        let content = format!("a\nb\n{long_line}\nc");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.update_wrap_layout(80);

        let total_visual: usize = state.visual_heights.iter().sum();
        state.go_to_bottom(3);
        assert_eq!(
            state.scroll_offset,
            total_visual.saturating_sub(3).min(state.max_scroll())
        );
    }

    #[test]
    fn test_toggle_wrap_clears_visual_heights() {
        let content = "some text";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.update_wrap_layout(80);
        assert!(!state.visual_heights.is_empty());

        state.toggle_wrap();
        assert!(state.visual_heights.is_empty());
        assert_eq!(state.cached_content_width, 0);
    }

    #[test]
    fn test_no_wrap_uses_logical_lines() {
        let content = "Line 1\nLine 2\nLine 3";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.wrap_lines = false;

        assert!(!state.is_visual_scroll());
        assert_eq!(state.max_scroll(), 2);

        state.scroll_down(1);
        assert_eq!(state.scroll_offset, 1);
    }

    #[test]
    fn test_visual_row_to_logical_roundtrip() {
        let content = "short\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nend";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.update_wrap_layout(10);

        let total_visual: usize = state.visual_heights.iter().sum();
        for row in 0..total_visual {
            let (logical, sub) = state.visual_row_to_logical(row);
            let back = state.logical_to_visual_row(logical);
            assert_eq!(back + sub, row, "roundtrip failed for visual row {row}");
        }
    }

    #[test]
    fn search_deduplicates_matches_after_multi_char_lowercase() {
        let content = "Straße\nmessage\n";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("ss", 20);

        assert!(
            !state.search_matches.is_empty(),
            "expected at least one match"
        );
        let mut seen = std::collections::HashSet::new();
        for m in &state.search_matches {
            assert!(seen.insert(*m), "duplicate match tuple: {:?}", m);
        }
        assert!(
            state.search_matches.len() <= 4,
            "expected at most 4 matches, got {}",
            state.search_matches.len()
        );
    }

    #[test]
    fn test_hex_mode_search() {
        let mut file = NamedTempFile::with_suffix(".bin").unwrap();
        file.write_all(b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f")
            .unwrap();
        let mut state = ViewerState::open(file.path()).unwrap();
        assert!(state.is_hex_mode());

        state.search("01 02", 20);

        assert!(
            !state.search_matches.is_empty(),
            "hex search for '01 02' should find matches in hex data section"
        );
        assert_eq!(state.current_match, Some(0));
        assert!(state.search_matches[0].0 == 0);
    }

    #[test]
    fn test_search_scroll_clamp() {
        let content = (0..100)
            .map(|i| format!("Line {i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.wrap_lines = false;
        let page_height = 5usize;

        state.search("Line 000", page_height);
        assert!(!state.search_matches.is_empty());

        state.scroll_offset = state.line_count;
        state.search("Line 000", page_height);

        assert!(
            state.scroll_offset <= state.max_scroll(),
            "scroll_offset {} > max_scroll {}",
            state.scroll_offset,
            state.max_scroll()
        );
        assert!(!state.search_matches.is_empty());
        assert_eq!(state.current_match, Some(0));
    }

    #[test]
    fn test_visual_row_to_logical_binary_search() {
        let content = (0..30)
            .map(|i| format!("L{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_test_file(&content);
        let mut state = ViewerState::open(file.path()).unwrap();
        state.update_wrap_layout(10);

        assert!(
            state.visual_heights.len() > 24,
            "need > 24 visual heights to exercise binary search path"
        );

        let total_visual: usize = state.visual_heights.iter().sum();

        assert_eq!(state.visual_row_to_logical(0), (0, 0));

        let (last_logical, last_sub) = state.visual_row_to_logical(total_visual.saturating_sub(1));
        assert_eq!(last_logical, state.visual_heights.len() - 1);
        let expected_last_line = state.line_count - 1;
        assert_eq!(last_logical, expected_last_line);
        assert!(
            last_sub < state.visual_heights[last_logical],
            "sub-row should be within line height"
        );

        for &row in &[0usize, 1, 5, 10, 15, 20, 25] {
            if row < total_visual {
                let (logical, sub) = state.visual_row_to_logical(row);
                let back = state.logical_to_visual_row(logical);
                assert_eq!(
                    back + sub,
                    row,
                    "roundtrip failed for visual row {row}: logical={logical}, sub={sub}, back={back}"
                );
            }
        }

        let result = state.visual_row_to_logical(total_visual);
        assert_eq!(result.0, state.line_count.saturating_sub(1));
        assert_eq!(result.1, 0);
    }

    #[test]
    fn test_search_empty_query_noop() {
        let content = "alpha\nbeta\ngamma";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("alpha", 20);
        assert_eq!(state.search_matches.len(), 1);
        assert_eq!(state.current_match, Some(0));

        state.search("", 20);

        assert!(state.search_matches.is_empty());
        assert!(state.current_match.is_none());
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_search_no_match_returns_none() {
        let file = create_test_file("apple\nbanana\ncherry");
        let mut state = ViewerState::open(file.path()).unwrap();
        state.search("durian", 20);
        assert!(state.search_matches.is_empty());
        assert_eq!(state.current_match, None);
    }

    #[test]
    fn test_hex_search_skips_offset_prefix() {
        let mut file = NamedTempFile::with_suffix(".bin").unwrap();
        file.write_all(b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00")
            .unwrap();
        let mut state = ViewerState::open(file.path()).unwrap();
        assert!(state.is_hex_mode());

        state.search("0000000", 20);
        assert!(
            state.search_matches.is_empty(),
            "hex search should not match offset prefix"
        );
    }
}
