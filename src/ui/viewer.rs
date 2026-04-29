use ratatui::{
    layout::Margin,
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::theme::Theme;

pub struct ViewerState {
    pub file_path: PathBuf,
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub line_count: usize,
    pub search_query: Option<String>,
    pub search_matches: Vec<(usize, usize, usize)>, // (line, col, highlight_len)
    pub current_match: usize,
    pub wrap_lines: bool,
    pub show_line_numbers: bool,
    pub hex_mode: bool,
    raw_bytes: Vec<u8>,
}

impl ViewerState {
    pub fn open(path: &Path) -> io::Result<Self> {
        let raw_bytes = fs::read(path)?;
        let content_str = String::from_utf8_lossy(&raw_bytes);
        let mut content: Vec<String> = content_str.lines().map(String::from).collect();
        if raw_bytes.last() == Some(&b'\n') {
            content.push(String::new());
        }
        let line_count = content.len();

        Ok(ViewerState {
            file_path: path.to_path_buf(),
            content,
            scroll_offset: 0,
            horizontal_offset: 0,
            line_count,
            search_query: None,
            search_matches: Vec::new(),
            current_match: 0,
            wrap_lines: true,
            show_line_numbers: false,
            hex_mode: false,
            raw_bytes,
        })
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn max_scroll(&self) -> usize {
        if self.hex_mode {
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
        let total = if self.hex_mode {
            self.raw_bytes.len().div_ceil(16)
        } else {
            self.line_count
        };
        self.scroll_offset = total.saturating_sub(page_height).min(self.max_scroll());
    }

    pub fn search(&mut self, query: &str, page_height: usize) {
        self.search_query = Some(query.to_string());
        self.search_matches.clear();
        self.current_match = 0;

        if query.is_empty() {
            return;
        }

        let lower_query: String = query.chars().flat_map(|c| c.to_lowercase()).collect();

        for (line_idx, line) in self.content.iter().enumerate() {
            let (lower_line, byte_map) = build_lowercase_mapping(line);
            let mut search_start = 0;
            while let Some(pos) = lower_line[search_start..].find(&lower_query) {
                let match_byte_start = search_start + pos;
                let match_byte_end = match_byte_start + lower_query.len();
                let orig_byte_start = byte_map[match_byte_start];
                let orig_byte_end = byte_map.get(match_byte_end).copied().unwrap_or(line.len());
                let char_pos = line[..orig_byte_start].chars().count();
                let match_char_len = line[orig_byte_start..orig_byte_end].chars().count().max(1);
                self.search_matches.push((line_idx, char_pos, match_char_len));
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
            self.scroll_offset = line_idx.saturating_sub(context);
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
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap_lines = !self.wrap_lines;
        if self.wrap_lines {
            self.horizontal_offset = 0;
        }
    }

    pub fn toggle_hex_mode(&mut self) {
        self.hex_mode = !self.hex_mode;
        self.scroll_offset = 0;
        self.horizontal_offset = 0;
    }

    pub fn scroll_left(&mut self, cols: usize) {
        self.horizontal_offset = self.horizontal_offset.saturating_sub(cols);
    }

    pub fn scroll_right(&mut self, cols: usize, visible_width: usize) {
        let line_num_width = if self.show_line_numbers { 6 } else { 0 };
        let effective_width = visible_width.saturating_sub(line_num_width);
        let max_line = if self.hex_mode {
            unicode_width::UnicodeWidthStr::width(format_hex_line(0, &[0u8; 16]).as_str())
        } else {
            self.content.iter().map(|l| unicode_width::UnicodeWidthStr::width(l.as_str())).max().unwrap_or(0)
        };
        let max_offset = if effective_width > 0 { max_line.saturating_sub(effective_width) } else { max_line };
        self.horizontal_offset = (self.horizontal_offset + cols).min(max_offset);
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

fn build_lowercase_mapping(original: &str) -> (String, Vec<usize>) {
    let mut lower = String::with_capacity(original.len());
    let mut byte_map = Vec::with_capacity(original.len());
    for (orig_byte_idx, ch) in original.char_indices() {
        let lower_ch: String = ch.to_lowercase().collect();
        for _ in 0..lower_ch.len() {
            byte_map.push(orig_byte_idx);
        }
        lower.push_str(&lower_ch);
    }
    (lower, byte_map)
}

// Helper to format a single line with highlighting
fn format_line_with_highlight<'a>(
    line: &'a str,
    line_idx: usize,
    search_matches: &[(usize, usize, usize)],
    current_match_idx: usize,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let matches_on_line: Vec<&(usize, usize, usize)> = search_matches
        .iter()
        .filter(|(l, _, _)| *l == line_idx)
        .collect();

    if matches_on_line.is_empty() {
        return vec![Span::raw(line)];
    }

    let mut last_end = 0;
    for match_ref in matches_on_line.iter() {
        let (match_line_idx, col, match_len) = match_ref;
        let col = *col;
        let match_len = *match_len;
        let match_line_idx = *match_line_idx;
        let start_byte = char_to_byte_idx(line, col);

        if col > last_end {
            let last_end_byte = char_to_byte_idx(line, last_end);
            spans.push(Span::raw(&line[last_end_byte..start_byte]));
        }

        let end_char = col + match_len.min(line.chars().count().saturating_sub(col));
        let end_byte = char_to_byte_idx(line, end_char);
        let match_text = &line[start_byte..end_byte];

        let is_current = search_matches
            .iter()
            .position(|m| m == &(match_line_idx, col, match_len))
            == Some(current_match_idx);

        let style = if is_current {
            Theme::highlight()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black).bg(Color::LightGreen)
        };

        spans.push(Span::styled(match_text, style));
        last_end = end_char;
    }

    if last_end < line.chars().count() {
        let last_end_byte = char_to_byte_idx(line, last_end);
        spans.push(Span::raw(&line[last_end_byte..]));
    }

    spans
}

pub fn render_viewer(f: &mut Frame, area: Rect, state: &ViewerState) {
    let bg_block = Block::default().style(Theme::panel());
    f.render_widget(bg_block, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(state.file_path.display().to_string())
        .title_style(Theme::title());
    f.render_widget(block, area);

    let inner_area = area.inner(Margin {
        horizontal: 1,
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

    for i in start_idx..end_idx {
        let line_content = &state.content[i];
        let spans: Vec<Span> = if state.show_line_numbers {
            let line_num = format!("{:>4}  ", i + 1);
            let mut line_spans = vec![Span::raw(line_num)];
            if state.search_matches.is_empty() {
                line_spans.push(Span::raw(line_content.clone()));
            } else {
                line_spans.extend(format_line_with_highlight(
                    line_content,
                    i,
                    &state.search_matches,
                    state.current_match,
                ));
            }
            line_spans
        } else if state.search_matches.is_empty() {
            vec![Span::raw(line_content.clone())]
        } else {
            format_line_with_highlight(
                line_content,
                i,
                &state.search_matches,
                state.current_match,
            )
        };

        lines.push(Line::from(spans));
    }

    let mut paragraph = Paragraph::new(lines);
    if state.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
    } else {
        paragraph = paragraph.scroll((0, state.horizontal_offset as u16));
    }

    f.render_widget(paragraph, content_area);

    // Reserve the last row inside the border for status text.
    let status_area = Rect {
        x: inner_area.x,
        y: inner_area.y + inner_area.height.saturating_sub(1),
        width: inner_area.width,
        height: 1,
    };

    let current_line = if state.line_count == 0 {
        0
    } else {
        state.scroll_offset + 1
    };
    let status_text = format!(
        " Line: {}/{}  {}  {}",
        current_line,
        state.line_count,
        state
            .file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        if state.wrap_lines { "Wrap" } else { "No Wrap" }
    );
    let status_paragraph = Paragraph::new(status_text)
        .style(Theme::status_bar());
    f.render_widget(status_paragraph, status_area);
}

pub fn render_hex_view(f: &mut Frame, area: Rect, state: &ViewerState) {
    let bg_block = Block::default().style(Theme::panel());
    f.render_widget(bg_block, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{} [Hex]", state.file_path.display()))
        .title_style(Theme::title());
    f.render_widget(block, area);

    let inner_area = area.inner(Margin {
        horizontal: 1,
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

    for line_idx in start_line..end_line {
        let offset = line_idx * bytes_per_line;
        let slice_len = (bytes.len() - offset).min(bytes_per_line);
        let slice = &bytes[offset..offset + slice_len];
        let hex_line = format_hex_line(offset, slice);
        lines.push(Line::from(Span::raw(hex_line)));
    }

    let paragraph = Paragraph::new(lines)
        .scroll((0, state.horizontal_offset as u16));
    f.render_widget(paragraph, content_area);

    let status_area = Rect {
        x: inner_area.x,
        y: inner_area.y + inner_area.height.saturating_sub(1),
        width: inner_area.width,
        height: 1,
    };

    let current_line = if total_lines == 0 {
        0
    } else {
        state.scroll_offset + 1
    };
    let status_text = format!(
        " Offset: {}/{}  {}  Hex",
        current_line,
        total_lines,
        state
            .file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
    );
    let status_paragraph = Paragraph::new(status_text)
        .style(Theme::status_bar());
    f.render_widget(status_paragraph, status_area);
}

pub fn format_hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut hex_part = String::with_capacity(49); // 16 * 3 + 1 padding
    for (i, b) in bytes.iter().enumerate() {
        if i == 8 {
            hex_part.push(' ');
        }
        use std::fmt::Write;
        let _ = write!(hex_part, "{b:02x} ");
    }

    // Pad the hex part to ensure consistent width (49 chars for 16 bytes + padding)
    let padding_needed = 49 - hex_part.len();
    hex_part.push_str(&" ".repeat(padding_needed));

    let ascii_part: String = bytes
        .iter()
        .map(|&b| {
            if (32..=126).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();

    format!("{offset:08x}: {hex_part} |{ascii_part}|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
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
    fn test_open_empty_file_has_zero_lines() {
        let file = create_test_file("");
        let state = ViewerState::open(file.path()).unwrap();

        assert_eq!(state.content.len(), 0);
        assert_eq!(state.line_count, 0);
        assert_eq!(state.scroll_offset, 0);
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
        let spans = format_line_with_highlight("zażółć gęślą jaźń", 0, &[(0, 7, 4)], 0);

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
    fn test_toggle_states() {
        let file = create_test_file("test");
        let mut state = ViewerState::open(file.path()).unwrap();

        assert!(!state.show_line_numbers);
        state.toggle_line_numbers();
        assert!(state.show_line_numbers);

        assert!(state.wrap_lines);
        state.toggle_wrap();
        assert!(!state.wrap_lines);

        assert!(!state.hex_mode);
        state.toggle_hex_mode();
        assert!(state.hex_mode);
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
    fn test_render_viewer_reserves_last_row_for_status_bar() {
        let file = create_test_file("line 1\nline 2\nline 3");
        let mut state = ViewerState::open(file.path()).unwrap();
        state.wrap_lines = false;

        let buffer = render_viewer_buffer(&state, 20, 5);

        assert!(buffer_line(&buffer, 1).contains("line 1"));
        assert!(buffer_line(&buffer, 2).contains("line 2"));
        assert!(buffer_line(&buffer, 3).contains("Line: 1/3"));
        assert!(!buffer_line(&buffer, 3).contains("line 3"));
    }
}
