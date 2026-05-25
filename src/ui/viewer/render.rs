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
use super::hex::{HEX_BYTES_PER_LINE, format_hex_line_to_buffer};
use super::open::ViewerState;
use super::scroll::{line_number_column_width, line_number_digits, paragraph_horizontal_scroll};
use crate::ui::theme::{ColorPalette, Theme};

pub fn render_viewer(f: &mut Frame, area: Rect, state: &ViewerState) {
    render_viewer_with_colors(f, area, state, &ColorPalette::default());
}

pub(crate) fn compute_visible_range(
    state: &ViewerState,
    visible_height: usize,
) -> (usize, usize, usize) {
    if state.is_visual_scroll() {
        let heights = state.visual_heights.borrow();
        let (logical_start, sub) = state.visual_row_to_logical(state.scroll_offset);
        let mut visual_budget = visible_height.saturating_add(sub);
        let mut end = logical_start;
        while end < state.line_count && visual_budget > 0 {
            visual_budget = visual_budget.saturating_sub(heights[end]);
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
        let num_col_width = line_number_column_width(state.line_count) as u16;
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
                paragraph = paragraph.scroll((sub_row as u16, 0));
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
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel_with_colors(colors))
        .title(state.file_path.display().to_string())
        .title_style(Theme::title_with_colors(colors));
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

    if content_area.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut line_num_lines: Vec<Line<'_>> = Vec::new();
    let visible_height = content_area.height as usize;

    let use_visual = state.is_visual_scroll();

    let (start_idx, sub_row, end_idx) = compute_visible_range(state, visible_height);

    let separate_line_nums = !state.wrap_lines && state.show_line_numbers;
    let visible_matches = &state.search_matches_by_line;
    let mut match_start = visible_matches.partition_point(|line_match| line_match.line < start_idx);

    for i in start_idx..end_idx {
        let line = state.get_line(i);
        let line_match_start = match_start;
        while match_start < visible_matches.len() && visible_matches[match_start].line == i {
            match_start += 1;
        }
        let line_matches = &visible_matches[line_match_start..match_start];

        let text_spans: Vec<Span<'_>> = if line_matches.is_empty() {
            vec![Span::raw(line)]
        } else {
            format_line_with_highlight(&line, line_matches, state.current_match, colors)
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect()
        };

        if separate_line_nums {
            let line_num = format!(
                "{:>width$}  ",
                i + 1,
                width = line_number_digits(state.line_count)
            );
            line_num_lines.push(Line::from(Span::raw(line_num)));
            lines.push(Line::from(text_spans));
        } else if state.show_line_numbers {
            let line_num = format!(
                "{:>width$}  ",
                i + 1,
                width = line_number_digits(state.line_count)
            );
            let mut combined: Vec<Span<'_>> = Vec::with_capacity(text_spans.len() + 1);
            combined.push(Span::raw(line_num));
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
    } else if state.line_count == 0 {
        0
    } else {
        state.scroll_offset
    };
    let position_text = format!("Line: {}/{}", current_line + 1, state.line_count);
    render_viewer_status(f, inner_area, state, "Text", &position_text, colors);
}

pub fn render_hex_view(f: &mut Frame, area: Rect, state: &ViewerState) {
    render_hex_view_with_colors(f, area, state, &ColorPalette::default());
}

pub fn render_hex_view_with_colors(
    f: &mut Frame,
    area: Rect,
    state: &ViewerState,
    colors: &ColorPalette,
) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel_with_colors(colors))
        .title(format!("{} [Hex]", state.file_path.display()))
        .title_style(Theme::title_with_colors(colors));
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

    let line_count = end_line - start_line;
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(line_count);

    let visible_matches = &state.search_matches_by_line;
    let mut match_start =
        visible_matches.partition_point(|line_match| line_match.line < start_line);

    for line_idx in start_line..end_line {
        let offset = line_idx * bytes_per_line;
        let slice_len = (bytes.len() - offset).min(bytes_per_line);
        let slice = &bytes[offset..offset + slice_len];
        let mut hex_line = String::with_capacity(128);
        format_hex_line_to_buffer(offset, slice, &mut hex_line);

        let line_match_start = match_start;
        while match_start < visible_matches.len() && visible_matches[match_start].line == line_idx {
            match_start += 1;
        }
        let line_matches = &visible_matches[line_match_start..match_start];

        let spans: Vec<Span<'_>> = if line_matches.is_empty() {
            vec![Span::raw(hex_line)]
        } else {
            let highlighted =
                format_line_with_highlight(&hex_line, line_matches, state.current_match, colors);
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
    render_viewer_status(f, inner_area, state, "Hex", &position_text, colors);
}

pub fn render_image_view_with_colors(
    f: &mut Frame,
    area: Rect,
    state: &ViewerState,
    colors: &ColorPalette,
) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .style(Theme::panel_with_colors(colors))
        .title(format!("{} [Image]", state.file_path.display()))
        .title_style(Theme::title_with_colors(colors));
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

    if content_area.width > 0 && content_area.height > 0 {
        if let Some(text) = &state.cached_image_text {
            f.render_widget(text, content_area);
        } else {
            let paragraph = Paragraph::new("Generating preview\u{2026}");
            f.render_widget(paragraph, content_area);
        }
    }

    let position_text = format!("Size: {}", format_size(state.file_size as u64));
    render_viewer_status(f, inner_area, state, "Image", &position_text, colors);
}

pub fn render_loading(f: &mut Frame, area: Rect, path: &Path) {
    render_loading_with_colors(f, area, path, &ColorPalette::default());
}

pub fn render_loading_with_colors(f: &mut Frame, area: Rect, path: &Path, colors: &ColorPalette) {
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
    colors: &ColorPalette,
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
        Theme::status_bar_with_colors(colors).fg(Theme::warning_color_with_colors(colors))
    } else {
        Theme::status_bar_with_colors(colors)
    };
    let status_paragraph = Paragraph::new(status_text).style(status_style);
    f.render_widget(status_paragraph, status_area);
}

pub(crate) fn format_line_with_highlight<'a>(
    line: &'a str,
    line_matches: &[SearchLineMatch],
    current_match_idx: Option<usize>,
    colors: &ColorPalette,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    if line_matches.is_empty() {
        return vec![Span::raw(line)];
    }

    let regular_style = Style::default()
        .fg(Theme::search_match_fg_with_colors(colors))
        .bg(Theme::search_match_bg_with_colors(colors));
    let current_style = Style::default()
        .fg(Theme::search_match_current_fg_with_colors(colors))
        .bg(Theme::search_match_current_bg_with_colors(colors))
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
