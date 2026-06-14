use std::borrow::Cow;
use std::path::Path;

use ratatui::{
    Frame,
    layout::Margin,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::types::format_size;

use super::SearchLineMatch;
use super::hex::{HEX_BYTES_PER_LINE, HEX_LINE_WIDTH, format_hex_line_to_buffer};
use super::open::ViewerState;
use super::scroll::{line_number_digits, paragraph_horizontal_scroll};
use crate::ui::theme::{ColorPalette, Theme};

const VIEWER_MARGIN: Margin = Margin {
    horizontal: 0,
    vertical: 1,
};

fn viewer_title<'a>(path: &'a Path, suffix: &str) -> Cow<'a, str> {
    match path.to_str() {
        // Common case (valid-UTF-8 path, no suffix): borrow the path directly.
        Some(s) if suffix.is_empty() => Cow::Borrowed(s),
        _ if suffix.is_empty() => Cow::Owned(path.display().to_string()),
        _ => Cow::Owned(format!("{} [{suffix}]", path.display())),
    }
}

fn viewer_block<'a>(title: impl Into<Line<'a>>, colors: &ColorPalette) -> Block<'a> {
    Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel_with_colors(colors))
        .title(title)
        .title_style(Theme::title_with_colors(colors))
}

fn viewer_content_areas(area: Rect) -> (Rect, Rect) {
    let inner = area.inner(VIEWER_MARGIN);
    let content = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    (inner, content)
}

fn format_line_number(line_idx: usize, width: usize) -> String {
    format!("{:>width$}  ", line_idx + 1, width = width)
}

/// Walks the line-grouped search matches in render order.
///
/// `search_matches_by_line` is sorted by line, so both the text and hex views
/// can skip to the first visible line once (`partition_point`) and then, as
/// they render rows top-to-bottom, hand each row its contiguous slice of
/// matches without rescanning. Replaces the duplicated partition/advance loops
/// the two views used to carry.
struct LineMatchCursor<'a> {
    matches: &'a [SearchLineMatch],
    pos: usize,
}

impl<'a> LineMatchCursor<'a> {
    fn new(matches: &'a [SearchLineMatch], first_line: usize) -> Self {
        let pos = matches.partition_point(|m| m.line < first_line);
        Self { matches, pos }
    }

    /// Returns the matches on `line`, advancing the cursor past them. Callers
    /// must pass non-decreasing `line` values (rows render in order).
    fn matches_on(&mut self, line: usize) -> &'a [SearchLineMatch] {
        let start = self.pos;
        while self.pos < self.matches.len() && self.matches[self.pos].line == line {
            self.pos += 1;
        }
        &self.matches[start..self.pos]
    }
}

pub(crate) fn compute_visible_range(
    state: &ViewerState,
    visible_height: usize,
) -> (usize, usize, usize) {
    if state.is_visual_scroll() {
        let heights = state.render_cache.visual_heights.borrow();
        let (logical_start, sub) = state.visual_row_to_logical(state.scroll_offset);
        let mut visual_budget = visible_height.saturating_add(sub);
        let mut end = logical_start;
        while end < state.line_count && visual_budget > 0 {
            let h = heights.get(end).copied().unwrap_or(1);
            visual_budget = visual_budget.saturating_sub(h);
            end += 1;
        }
        (logical_start, sub, end)
    } else {
        let start = state.scroll_offset;
        let end = (start + visible_height).min(state.line_count);
        (start, 0, end)
    }
}

fn render_content_paragraph(
    f: &mut Frame,
    state: &ViewerState,
    lines: Vec<Line<'_>>,
    line_num_lines: Vec<Line<'_>>,
    content_area: Rect,
    use_visual: bool,
    sub_row: usize,
) {
    let separate_line_nums = !state.wrap_lines && state.show_line_numbers;

    if separate_line_nums {
        let num_col_width = state.render_cache.cached_line_num_col_width.get() as u16;
        let actual_num_width = num_col_width.min(content_area.width);
        let line_num_area = Rect {
            x: content_area.x,
            y: content_area.y,
            width: actual_num_width,
            height: content_area.height,
        };
        let text_area = Rect {
            x: content_area.x + actual_num_width,
            y: content_area.y,
            width: content_area.width.saturating_sub(actual_num_width),
            height: content_area.height,
        };

        let line_num_para = Paragraph::new(line_num_lines);
        f.render_widget(line_num_para, line_num_area);

        if text_area.width > 0 {
            let text_para = Paragraph::new(lines)
                .scroll((0, paragraph_horizontal_scroll(state.horizontal_offset)));
            f.render_widget(text_para, text_area);
        }
    } else {
        let mut paragraph = Paragraph::new(lines);
        if state.wrap_lines {
            paragraph = paragraph.wrap(Wrap { trim: false });
            if use_visual && sub_row > 0 {
                // Vertical sub-row offset into a partially-scrolled wrapped
                // line. ratatui's scroll unit is `u16`; a single logical line
                // wrapping past 65 535 visual rows is not representable (and
                // would need a viewport that tall), so clamp rather than wrap.
                let sub_row = sub_row.min(u16::MAX as usize) as u16;
                paragraph = paragraph.scroll((sub_row, 0));
            }
        } else {
            paragraph = paragraph.scroll((0, paragraph_horizontal_scroll(state.horizontal_offset)));
        }

        f.render_widget(paragraph, content_area);
    }
}

pub fn render_viewer_with_colors(
    f: &mut Frame,
    area: Rect,
    state: &ViewerState,
    colors: &ColorPalette,
) {
    let title = viewer_title(&state.file_path, "");
    f.render_widget(viewer_block(title, colors), area);

    let (inner_area, content_area) = viewer_content_areas(area);

    if inner_area.height == 0 {
        return;
    }

    if content_area.width == 0 {
        return;
    }

    let visible_height = content_area.height as usize;
    let use_visual = state.is_visual_scroll();
    let (start_idx, sub_row, end_idx) = compute_visible_range(state, visible_height);
    let capacity = end_idx - start_idx;

    // Decode the visible lines once and keep them alive for the whole render.
    // Valid UTF-8 borrows straight from `state` (zero-copy); invalid UTF-8 is
    // decoded lossily here. Highlight spans then borrow from these buffers
    // instead of cloning each span's substring into an owned `String`.
    let line_texts: Vec<Cow<'_, str>> = (start_idx..end_idx).map(|i| state.get_line(i)).collect();

    let separate_line_nums = !state.wrap_lines && state.show_line_numbers;
    let show_line_nums = state.show_line_numbers;
    let digit_width = line_number_digits(state.line_count);

    // Pre-rendered line-number strings, owned and kept alive for borrowing.
    let line_num_strings: Vec<String> = if show_line_nums {
        (start_idx..end_idx)
            .map(|i| format_line_number(i, digit_width))
            .collect()
    } else {
        Vec::new()
    };

    let mut cursor = LineMatchCursor::new(&state.search_matches_by_line, start_idx);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(capacity);
    let mut line_num_lines: Vec<Line<'_>> = Vec::with_capacity(capacity);

    for (k, i) in (start_idx..end_idx).enumerate() {
        let line: &str = line_texts[k].as_ref();
        let line_matches = cursor.matches_on(i);

        let text_spans: Vec<Span<'_>> = if line_matches.is_empty() {
            vec![Span::raw(line)]
        } else {
            format_line_with_highlight(line, line_matches, state.current_match, colors)
        };

        if separate_line_nums {
            line_num_lines.push(Line::from(Span::raw(line_num_strings[k].as_str())));
            lines.push(Line::from(text_spans));
        } else if show_line_nums {
            let mut combined: Vec<Span<'_>> = Vec::with_capacity(text_spans.len() + 1);
            combined.push(Span::raw(line_num_strings[k].as_str()));
            combined.extend(text_spans);
            lines.push(Line::from(combined));
        } else {
            lines.push(Line::from(text_spans));
        }
    }

    render_content_paragraph(
        f,
        state,
        lines,
        line_num_lines,
        content_area,
        use_visual,
        sub_row,
    );

    let current_line = if use_visual {
        state.visual_row_to_logical(state.scroll_offset).0
    } else {
        state.scroll_offset
    };
    // Guard the empty-content case so the footer reads "Line: 0/0" rather than
    // the nonsensical "Line: 1/0" (mirrors the hex view's `total_lines == 0`).
    let position_display = if state.line_count == 0 {
        0
    } else {
        current_line + 1
    };
    let position_text = format!("Line: {position_display}/{}", state.line_count);
    let size_label = format_size(state.file_size as u64);
    render_viewer_status(
        f,
        inner_area,
        state,
        "Text",
        &position_text,
        &size_label,
        colors,
    );
}

pub fn render_hex_view_with_colors(
    f: &mut Frame,
    area: Rect,
    state: &ViewerState,
    colors: &ColorPalette,
) {
    let title = viewer_title(&state.file_path, "Hex");
    f.render_widget(viewer_block(title, colors), area);

    let (inner_area, content_area) = viewer_content_areas(area);
    if inner_area.height == 0 {
        return;
    }

    let bytes = &state.raw_bytes;
    let bytes_per_line = HEX_BYTES_PER_LINE;
    let total_lines = bytes.len().div_ceil(bytes_per_line);

    let start_line = state.scroll_offset.min(total_lines.saturating_sub(1));
    let visible_lines = content_area.height as usize;
    let end_line = (start_line + visible_lines).min(total_lines);

    let line_count = end_line - start_line;

    // Build the visible hex lines once and keep them alive so highlight spans
    // can borrow them instead of cloning each line per frame.
    let hex_strings: Vec<String> = (start_line..end_line)
        .map(|line_idx| {
            let mut hex_line = String::with_capacity(HEX_LINE_WIDTH);
            let offset = line_idx * bytes_per_line;
            let slice_len = (bytes.len() - offset).min(bytes_per_line);
            let slice = &bytes[offset..offset + slice_len];
            format_hex_line_to_buffer(offset, slice, &mut hex_line);
            hex_line
        })
        .collect();

    let mut cursor = LineMatchCursor::new(&state.search_matches_by_line, start_line);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(line_count);

    for (k, line_idx) in (start_line..end_line).enumerate() {
        let hex_line: &str = hex_strings[k].as_str();
        let line_matches = cursor.matches_on(line_idx);

        let spans: Vec<Span<'_>> = if line_matches.is_empty() {
            vec![Span::raw(hex_line)]
        } else {
            format_line_with_highlight(hex_line, line_matches, state.current_match, colors)
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
    let size_label = format_size(state.file_size as u64);
    render_viewer_status(
        f,
        inner_area,
        state,
        "Hex",
        &position_text,
        &size_label,
        colors,
    );
}

pub fn render_image_view_with_colors(
    f: &mut Frame,
    area: Rect,
    state: &ViewerState,
    colors: &ColorPalette,
) {
    let title = viewer_title(&state.file_path, "Image");
    f.render_widget(viewer_block(title, colors), area);

    let (inner_area, content_area) = viewer_content_areas(area);
    if inner_area.height == 0 {
        return;
    }

    if content_area.width > 0 && content_area.height > 0 {
        if let Some(text) = &state.render_cache.cached_image_text {
            f.render_widget(text, content_area);
        } else {
            let paragraph = Paragraph::new("Generating preview\u{2026}");
            f.render_widget(paragraph, content_area);
        }
    }

    let size_label = format_size(state.file_size as u64);
    let position_text = format!("Size: {size_label}");
    render_viewer_status(
        f,
        inner_area,
        state,
        "Image",
        &position_text,
        &size_label,
        colors,
    );
}

pub fn render_loading_with_colors(
    f: &mut Frame,
    area: Rect,
    path: &Path,
    colors: &ColorPalette,
    spinner_frame: u64,
) {
    let spinner_chars = ['|', '/', '-', '\\'];
    let spinner = spinner_chars[spinner_frame as usize % spinner_chars.len()];

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let msg = format!("{spinner} Loading {name}...");

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Theme::panel_bg_with_colors(colors));
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

fn render_viewer_status(
    f: &mut Frame,
    inner_area: Rect,
    state: &ViewerState,
    mode_label: &str,
    position_text: &str,
    size_label: &str,
    colors: &ColorPalette,
) {
    let status_area = Rect {
        x: inner_area.x,
        y: inner_area.y + inner_area.height.saturating_sub(1),
        width: inner_area.width,
        height: 1,
    };

    let mime_label = state.detected_mime.as_deref().unwrap_or("—");
    let has_warning = state.has_invalid_utf8
        || (!state.is_hex_mode() && state.originally_binary)
        || state.file_truncated;
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
    let status_style = if has_warning {
        Theme::status_bar_with_colors(colors).fg(colors.warning)
    } else {
        Theme::status_bar_with_colors(colors)
    };
    let status_paragraph = Paragraph::new(status_text).style(status_style);
    f.render_widget(status_paragraph, status_area);
}

fn resolve_overlap<'a>(
    orig_start: usize,
    last_end: usize,
    is_current: bool,
    prev_match_start: Option<usize>,
    spans: &mut [Span<'a>],
    line: &'a str,
    regular_style: Style,
) -> (usize, Option<usize>) {
    if orig_start >= last_end {
        return (orig_start, None);
    }
    if is_current {
        if let Some(ps) = prev_match_start
            && ps < orig_start
            && !spans.is_empty()
        {
            let last_idx = spans.len() - 1;
            spans[last_idx] = Span::styled(&line[ps..orig_start], regular_style);
            return (orig_start, Some(last_end));
        }
        (orig_start, None)
    } else {
        (last_end, None)
    }
}

pub(crate) fn format_line_with_highlight<'a>(
    line: &'a str,
    line_matches: &[SearchLineMatch],
    current_match_idx: Option<usize>,
    colors: &ColorPalette,
) -> Vec<Span<'a>> {
    if line_matches.is_empty() {
        return vec![Span::raw(line)];
    }

    let mut spans = Vec::with_capacity(line_matches.len() * 2 + 1);

    let regular_style = Style::default()
        .fg(colors.search_match_fg)
        .bg(colors.search_match_bg);
    let current_style = Style::default()
        .fg(colors.search_match_current_fg)
        .bg(colors.search_match_current_bg)
        .add_modifier(Modifier::BOLD);

    let mut last_end = 0usize;
    let mut prev_match_start: Option<usize> = None;

    for line_match in line_matches {
        let orig_start = line_match.start_byte.min(line.len());
        let end_byte = line_match.end_byte.min(line.len());
        let is_current = Some(line_match.global_idx) == current_match_idx;

        let (start_byte, overlap_tail) = resolve_overlap(
            orig_start,
            last_end,
            is_current,
            prev_match_start,
            &mut spans,
            line,
            regular_style,
        );

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

        if let Some(pe) = overlap_tail
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
