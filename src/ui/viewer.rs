use ratatui::{
    Frame,
    layout::Margin,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

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
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub line_count: usize,
    pub search_query: Option<String>,
    pub search_matches: Vec<(usize, usize, usize)>, // (line, col, highlight_len)
    search_matches_by_line: Vec<SearchLineMatch>,
    pub current_match: usize,
    pub wrap_lines: bool,
    pub show_line_numbers: bool,
    pub view_mode: ViewMode,
    raw_bytes: Vec<u8>,
    max_line_width: usize,
    pub detected_mime: Option<String>,
    pub file_size: usize,
    pub has_invalid_utf8: bool,
    originally_binary: bool,
}

impl ViewerState {
    pub fn open(path: &Path) -> io::Result<Self> {
        const MAX_VIEW_SIZE: usize = 64 * 1024 * 1024; // 64 MB

        let file = fs::File::open(path)?;
        let mut raw_bytes = Vec::new();
        file.take((MAX_VIEW_SIZE + 1) as u64)
            .read_to_end(&mut raw_bytes)?;
        if raw_bytes.len() > MAX_VIEW_SIZE {
            return Err(io::Error::other(format!(
                "File too large to view ({} bytes, max 64 MB)",
                raw_bytes.len()
            )));
        }

        let file_size = raw_bytes.len();
        let mime =
            crate::app::mime::detect_mime_from_bytes(path, &raw_bytes[..raw_bytes.len().min(8192)]);
        let open_as_text = should_open_as_text(path, mime.as_deref(), &raw_bytes);

        let (content, has_invalid_utf8) = if raw_bytes.is_empty() {
            (vec!["[Empty file]".to_string()], false)
        } else if open_as_text {
            let has_invalid = std::str::from_utf8(&raw_bytes).is_err();
            let content_str = String::from_utf8_lossy(&raw_bytes);
            let mut content: Vec<String> = content_str.lines().map(String::from).collect();
            if raw_bytes.last() == Some(&b'\n') {
                content.push(String::new());
            }
            (content, has_invalid)
        } else {
            let mime_label = mime.as_deref().unwrap_or("unknown MIME");
            let msg = vec![format!(
                "Binary file ({mime_label}, {file_size} bytes). Opened in Hex mode."
            )];
            (msg, false)
        };
        let line_count = content.len();
        let max_line_width = content
            .iter()
            .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
            .max()
            .unwrap_or(0);

        Ok(ViewerState {
            file_path: path.to_path_buf(),
            content,
            scroll_offset: 0,
            horizontal_offset: 0,
            line_count,
            search_query: None,
            search_matches: Vec::new(),
            search_matches_by_line: Vec::new(),
            current_match: 0,
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
        })
    }

    pub fn is_hex_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Hex)
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn max_scroll(&self) -> usize {
        if self.is_hex_mode() {
            self.raw_bytes.len().div_ceil(16).saturating_sub(1)
        } else {
            self.line_count.saturating_sub(1)
        }
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
        let total = if self.is_hex_mode() {
            self.raw_bytes.len().div_ceil(16)
        } else {
            self.line_count
        };
        self.scroll_offset = total.saturating_sub(page_height).min(self.max_scroll());
    }

    pub fn search(&mut self, query: &str, page_height: usize) {
        self.search_query = Some(query.to_string());
        self.search_matches.clear();
        self.search_matches_by_line.clear();
        self.current_match = 0;

        if query.is_empty() || self.is_hex_mode() {
            return;
        }

        let lower_query: String = query.chars().flat_map(|c| c.to_lowercase()).collect();

        let mut lower_buf = String::new();
        let mut byte_map_buf = Vec::new();

        for (line_idx, line) in self.content.iter().enumerate() {
            build_lowercase_mapping(line, &mut lower_buf, &mut byte_map_buf);
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

        for (i, &(line_idx, _, _)) in self.search_matches.iter().enumerate() {
            if line_idx >= self.scroll_offset {
                self.current_match = i;
                self.scroll_to_current_match(page_height);
                return;
            }
        }
        if !self.search_matches.is_empty() {
            self.current_match = 0;
            self.scroll_to_current_match(page_height);
        }
    }

    fn scroll_to_current_match(&mut self, page_height: usize) {
        if let Some(&(line_idx, _, _)) = self.search_matches.get(self.current_match) {
            let context = 5usize.min(page_height.saturating_sub(1));
            self.scroll_offset = line_idx.saturating_sub(context).min(self.max_scroll());
        }
    }

    pub fn next_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.search_matches.len();
        self.scroll_to_current_match(page_height);
    }

    pub fn prev_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        self.current_match = if self.current_match == 0 {
            self.search_matches.len() - 1
        } else {
            self.current_match - 1
        };
        self.scroll_to_current_match(page_height);
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        if !self.is_hex_mode() {
            self.view_mode = ViewMode::Text;
        }
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap_lines = !self.wrap_lines;
        if self.wrap_lines {
            self.horizontal_offset = 0;
        }
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
            let content_str = String::from_utf8_lossy(&self.raw_bytes);
            self.content = content_str.lines().map(String::from).collect();
            if self.raw_bytes.last() == Some(&b'\n') {
                self.content.push(String::new());
            }
            self.line_count = self.content.len();
            self.max_line_width = self
                .content
                .iter()
                .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
                .max()
                .unwrap_or(0);
            self.search_matches.clear();
            self.search_matches_by_line.clear();
            self.current_match = 0;
            self.search_query = None;
            self.has_invalid_utf8 = std::str::from_utf8(&self.raw_bytes).is_err();
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
        let lower_ch: String = ch.to_lowercase().collect();
        for _ in 0..lower_ch.len() {
            byte_map.push(orig_byte_idx);
        }
        lower.push_str(&lower_ch);
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
                | "application/rtf"
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
    current_match_idx: usize,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    if line_matches.is_empty() {
        return vec![Span::raw(line)];
    }

    let mut last_end = 0usize;
    for line_match in line_matches {
        let start_byte = line_match.start_byte.min(line.len());

        if start_byte > last_end {
            spans.push(Span::raw(&line[last_end..start_byte]));
        }

        let end_byte = line_match.end_byte.min(line.len());
        let match_text = &line[start_byte..end_byte];

        let is_current = line_match.global_idx == current_match_idx;

        let style = if is_current {
            Style::default()
                .fg(Theme::search_match_current_fg())
                .bg(Theme::search_match_current_bg())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Theme::search_match_fg())
                .bg(Theme::search_match_bg())
        };

        spans.push(Span::styled(match_text, style));
        last_end = end_byte;
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
    let status_text = format!(
        " {mode_label}  {mime_label}  {size_label}  {position_text}{utf8_warning}{binary_warning}",
    );
    let status_style =
        if state.has_invalid_utf8 || (!state.is_hex_mode() && state.originally_binary) {
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
    let start_idx = state.scroll_offset;
    let end_idx = (start_idx + visible_height).min(state.content.len());

    let visible_matches = &state.search_matches_by_line;
    let mut match_start = visible_matches.partition_point(|line_match| line_match.line < start_idx);
    for i in start_idx..end_idx {
        let line_content = &state.content[i];
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
                line_spans.push(Span::raw(line_content.as_str()));
            } else {
                line_spans.extend(format_line_with_highlight(
                    line_content,
                    line_matches,
                    state.current_match,
                ));
            }
            line_spans
        } else if line_matches.is_empty() {
            vec![Span::raw(line_content.as_str())]
        } else {
            format_line_with_highlight(line_content, line_matches, state.current_match)
        };

        lines.push(Line::from(spans));
    }

    let mut paragraph = Paragraph::new(lines);
    if state.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
    } else {
        paragraph = paragraph.scroll((0, paragraph_horizontal_scroll(state.horizontal_offset)));
    }

    f.render_widget(paragraph, content_area);

    let current_line = if state.line_count == 0 {
        0
    } else {
        state.scroll_offset + 1
    };
    let position_text = format!("Line: {current_line}/{}", state.line_count);
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
    let bytes_per_line = 16;
    let total_lines = bytes.len().div_ceil(bytes_per_line);

    let start_line = state.scroll_offset.min(total_lines.saturating_sub(1));
    let visible_lines = content_area.height as usize;
    let end_line = (start_line + visible_lines).min(total_lines);

    let mut lines: Vec<Line> = Vec::new();

    let mut hex_line_buffer = String::with_capacity(128);
    for line_idx in start_line..end_line {
        let offset = line_idx * bytes_per_line;
        let slice_len = (bytes.len() - offset).min(bytes_per_line);
        let slice = &bytes[offset..offset + slice_len];
        hex_line_buffer.clear();
        format_hex_line_to_buffer(offset, slice, &mut hex_line_buffer);
        lines.push(Line::from(Span::raw(std::mem::take(&mut hex_line_buffer))));
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

pub fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut buf = String::with_capacity(128);
    format_hex_line_to_buffer(offset, bytes, &mut buf);
    buf
}

const HEX_BYTES_PER_LINE: usize = 16;
const HEX_PART_WIDTH: usize = HEX_BYTES_PER_LINE * 3 + 1;
const HEX_LINE_WIDTH: usize = 78;

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

        assert_eq!(state.content.len(), 3);
        assert_eq!(state.content[0], "Line 1");
        assert_eq!(state.content[1], "Line 2");
        assert_eq!(state.content[2], "Line 3");
        assert_eq!(state.line_count, 3);
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
        assert_eq!(state.current_match, 0);
    }

    #[test]
    fn test_next_prev_match() {
        let content = "apple\nbanana\napple pie";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("apple", 20);
        assert_eq!(state.current_match, 0);

        state.next_match(20);
        assert_eq!(state.current_match, 1);
        assert_eq!(state.scroll_offset, 0);

        state.next_match(20);
        assert_eq!(state.current_match, 0);

        state.prev_match(20);
        assert_eq!(state.current_match, 1);
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

        assert_eq!(state.content.len(), 1);
        assert_eq!(state.content[0], "[Empty file]");
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
        let spans = format_line_with_highlight(
            "zażółć gęślą jaźń",
            &[SearchLineMatch {
                line: 0,
                global_idx: 0,
                start_byte: 11,
                end_byte: 17,
            }],
            0,
        );

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "zażółć ");
        assert_eq!(spans[1].content, "gęśl");
        assert_eq!(spans[2].content, "ą jaźń");
    }

    #[test]
    fn test_search_find_next_wraps() {
        // "target" appears on line 0 and line 2; line 1 has no match
        let content = "target one\nno hit here\ntarget two";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("target", 20);
        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.current_match, 0);
        assert_eq!(state.scroll_offset, 0);

        state.next_match(20);
        assert_eq!(state.current_match, 1);
        assert_eq!(state.scroll_offset, 0);

        state.next_match(20);
        assert_eq!(state.current_match, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_search_find_prev_wraps() {
        // "target" appears on line 0 and line 2; line 1 has no match
        let content = "target one\nno hit here\ntarget two";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("target", 20);
        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.current_match, 0);

        state.prev_match(20);
        assert_eq!(state.current_match, 1);
        assert_eq!(state.scroll_offset, 0);

        state.prev_match(20);
        assert_eq!(state.current_match, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_search_no_match() {
        let content = "apple\nbanana\ncherry";
        let file = create_test_file(content);
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("durian", 20);

        assert!(state.search_matches.is_empty());
        assert_eq!(state.current_match, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_search_keeps_line_match_cache_ordered() {
        let file = create_test_file("alpha beta alpha\nbeta\nalpha");
        let mut state = ViewerState::open(file.path()).unwrap();

        state.search("alpha", 20);

        assert_eq!(state.search_matches, vec![(0, 0, 5), (0, 11, 5), (2, 0, 5)]);
        assert_eq!(
            state.search_matches_by_line,
            vec![
                SearchLineMatch {
                    line: 0,
                    global_idx: 0,
                    start_byte: 0,
                    end_byte: 5,
                },
                SearchLineMatch {
                    line: 0,
                    global_idx: 1,
                    start_byte: 11,
                    end_byte: 16,
                },
                SearchLineMatch {
                    line: 2,
                    global_idx: 2,
                    start_byte: 0,
                    end_byte: 5,
                },
            ]
        );
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
        assert!(state.content[0].contains('\u{FFFD}'));
    }

    #[test]
    fn test_open_invalid_utf8_sets_warning() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"valid\xffinvalid").unwrap();

        let state = ViewerState::open(file.path()).unwrap();

        assert!(state.has_invalid_utf8);
        assert!(state.content[0].contains('\u{FFFD}'));
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
}
