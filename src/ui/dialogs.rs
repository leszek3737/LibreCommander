use std::borrow::Cow;
use std::path::{MAIN_SEPARATOR, Path};

use super::theme::{ColorPalette, Theme};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const DIALOG_WIDTH_PERCENT: u16 = 50;
const DIALOG_HEIGHT_PERCENT: u16 = 40;

#[derive(Debug, Clone)]
pub struct PropertiesInfo {
    pub name: String,
    pub size: String,
    pub mtime: String,
    pub permissions: String,
    pub owner: String,
    pub group: String,
    pub file_type: String,
}

#[derive(Debug, Clone)]
pub enum DialogKind<'a> {
    Confirm {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        selection: usize,
        files: Cow<'a, [String]>,
    },
    Input {
        title: Cow<'a, str>,
        prompt: Cow<'a, str>,
        value: Cow<'a, str>,
        cursor_pos: usize,
    },
    Error {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
    },
    Help {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        scroll_offset: usize,
    },
    Progress {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        percent: f32,
        cancellable: bool,
    },
    Properties {
        info: PropertiesInfo,
    },
    OverwriteConfirm {
        selection: usize,
        files: Cow<'a, [String]>,
    },
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind<'_>) {
    render_dialog_with_colors(f, dialog, &ColorPalette::default());
}

pub fn render_dialog_with_colors(f: &mut Frame, dialog: &DialogKind<'_>, colors: &ColorPalette) {
    if matches!(dialog, DialogKind::OverwriteConfirm { files, .. } if files.is_empty()) {
        return;
    }

    let rect = f.area();
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, rect);

    f.render_widget(Clear, dialog_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, dialog_area);

    match dialog {
        DialogKind::Confirm {
            title,
            message,
            selection,
            files,
        } => {
            render_confirm_dialog(
                f,
                dialog_area,
                title.as_ref(),
                message.as_ref(),
                *selection,
                files,
                colors,
            );
        }
        DialogKind::Input {
            title,
            prompt,
            value,
            cursor_pos,
        } => {
            render_input_dialog(
                f,
                dialog_area,
                title.as_ref(),
                prompt.as_ref(),
                value.as_ref(),
                *cursor_pos,
                colors,
            );
        }
        DialogKind::Error { title, message } => {
            render_error_dialog(f, dialog_area, title.as_ref(), message.as_ref(), colors);
        }
        DialogKind::Help {
            title,
            message,
            scroll_offset,
        } => {
            render_help_dialog(
                f,
                dialog_area,
                title.as_ref(),
                message.as_ref(),
                *scroll_offset,
                colors,
            );
        }
        DialogKind::Progress {
            title,
            message,
            percent,
            cancellable,
        } => {
            render_progress_dialog(
                f,
                dialog_area,
                title.as_ref(),
                message.as_ref(),
                *percent,
                *cancellable,
                colors,
            );
        }
        DialogKind::Properties { info } => {
            render_properties_dialog(f, dialog_area, info, colors);
        }
        DialogKind::OverwriteConfirm { selection, files } => {
            render_overwrite_dialog(f, dialog_area, *selection, files.as_ref(), colors);
        }
    }
}

fn help_dialog_content_rect(dialog_area: Rect) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick);
    let inner = block.inner(dialog_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    chunks[0]
}

pub fn help_visible_height(area: Rect) -> usize {
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    help_dialog_content_rect(dialog_area).height as usize
}

pub fn help_message_width(area: Rect) -> u16 {
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    let content = help_dialog_content_rect(dialog_area);
    if content.width > 1 {
        content.width.saturating_sub(1)
    } else {
        content.width
    }
}

fn dialog_block(title: &str, style: Style) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_type(BorderType::Thick)
        .style(style)
}

fn truncate_suffix(s: &str, max_width: usize) -> String {
    if max_width > 3 {
        let suffix_budget = max_width - 3;
        let mut width = 0;
        let mut split_idx = s.len();
        for (idx, ch) in s.char_indices().rev() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + cw > suffix_budget {
                split_idx = idx + ch.len_utf8();
                break;
            }
            width += cw;
            split_idx = idx;
        }
        format!("...{}", &s[split_idx..])
    } else {
        let mut out = String::new();
        let mut width = 0;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + cw > max_width {
                break;
            }
            width += cw;
            out.push(ch);
        }
        out
    }
}

fn truncate_path(path: &str, max_width: usize) -> String {
    let total_width = unicode_width::UnicodeWidthStr::width(path);
    if total_width <= max_width {
        return path.to_string();
    }
    let p = Path::new(path);
    let file = p
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_width = unicode_width::UnicodeWidthStr::width(file.as_str());
    if file_width >= max_width {
        return truncate_suffix(file.as_str(), max_width);
    }
    if dir.is_empty() {
        return truncate_suffix(path, max_width);
    }
    let budget = max_width - file_width - 1;
    let dir_part = truncate_suffix(dir.as_str(), budget);
    format!("{dir_part}{MAIN_SEPARATOR}{file}")
}

fn render_file_list(
    f: &mut Frame,
    area: Rect,
    files: &[impl AsRef<str>],
    max_name_width: usize,
    colors: &ColorPalette,
) {
    let max_visible = area.height as usize;
    let show_count = files.len().min(max_visible.saturating_sub(1).max(1));
    let mut lines: Vec<Line> = Vec::with_capacity(show_count + 1);
    if files.len() <= show_count {
        for name in files {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines
                .push(Line::from(format!("  {display}")).style(Theme::warning_with_colors(colors)));
        }
    } else {
        for name in files.iter().take(show_count.saturating_sub(1)) {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines
                .push(Line::from(format!("  {display}")).style(Theme::warning_with_colors(colors)));
        }
        let remaining = files.len() - show_count + 1;
        lines.push(Line::from(format!("  ... +{remaining} more")));
    }
    let file_paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(file_paragraph, area);
}

fn render_confirmation_dialog_inner(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    buttons: &[(Style, &str)],
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_files = !files.is_empty();
    let max_rows = inner.height.saturating_sub(5).max(3);
    let file_rows = if has_files {
        (files.len() as u16 + 1).min(max_rows)
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(file_rows),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    if has_files {
        let max_name_width = inner.width.saturating_sub(2) as usize;
        render_file_list(f, chunks[1], files, max_name_width, colors);
    }

    let mut spans: Vec<ratatui::text::Span> = Vec::new();
    for (i, (style, label)) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(ratatui::text::Span::raw("  "));
        }
        spans.push(ratatui::text::Span::styled(*label, *style));
    }
    let btn_line = Line::from(spans);
    let btn_paragraph = Paragraph::new(btn_line).alignment(Alignment::Center);
    f.render_widget(btn_paragraph, chunks[2]);
}

pub fn render_confirm_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    selection: usize,
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    let buttons = [
        (
            if selection == 0 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ Yes ]",
        ),
        (
            if selection == 1 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ No ]",
        ),
    ];
    render_confirmation_dialog_inner(f, area, title, message, &buttons, files, colors);
}

pub fn render_overwrite_dialog(
    f: &mut Frame,
    area: Rect,
    selection: usize,
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    if files.is_empty() {
        return;
    }

    let msg = if files.len() == 1 {
        "File already exists at destination:".to_string()
    } else {
        format!("{} files already exist at destination:", files.len())
    };

    let btn_style = |idx: usize| -> Style {
        if selection == idx {
            Theme::highlight_bold_with_colors(colors)
        } else {
            Theme::dialog_with_colors(colors)
        }
    };
    let buttons = [
        (btn_style(0), "[ Overwrite All ]"),
        (btn_style(1), "[ Cancel ]"),
    ];
    render_confirmation_dialog_inner(f, area, "Overwrite?", &msg, &buttons, files, colors);
}

pub fn input_dialog_rect(area: Rect) -> Rect {
    centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area)
}

pub fn render_input_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    prompt: &str,
    value: &str,
    cursor_pos: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    let prompt_paragraph = Paragraph::new(prompt);
    f.render_widget(prompt_paragraph, chunks[0]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain);
    let input_block = if value.is_empty() {
        input_block.border_style(Theme::warning_with_colors(colors))
    } else {
        input_block
    };
    let input_inner = input_block.inner(chunks[1]);

    let visible_width = input_inner.width as usize;
    if visible_width == 0 || input_inner.height == 0 {
        let input_paragraph = Paragraph::new(value).block(input_block);
        f.render_widget(input_paragraph, chunks[1]);
        return;
    }

    let grapheme_count = value.graphemes(true).count();
    let clamped_cursor = cursor_pos.min(grapheme_count);

    let cursor_display: usize = value
        .graphemes(true)
        .take(clamped_cursor)
        .map(UnicodeWidthStr::width)
        .sum();

    let scroll_display = cursor_display.saturating_sub(visible_width.saturating_sub(1));

    let mut start_cum = 0usize;
    let mut visible = String::new();
    let mut vis_width = 0;

    if scroll_display == 0 {
        for g in value.graphemes(true) {
            let gw = UnicodeWidthStr::width(g);
            if vis_width + gw > visible_width {
                break;
            }
            visible.push_str(g);
            vis_width += gw;
        }
    } else {
        let mut cum = 0usize;
        let mut found_start = false;
        for g in value.graphemes(true) {
            let gw = UnicodeWidthStr::width(g);
            if !found_start && cum + gw > scroll_display {
                found_start = true;
                start_cum = cum;
            }
            cum += gw;
            if found_start {
                if vis_width + gw > visible_width {
                    break;
                }
                visible.push_str(g);
                vis_width += gw;
            }
        }
        if !found_start {
            start_cum = 0;
            for g in value.graphemes(true) {
                let gw = UnicodeWidthStr::width(g);
                if vis_width + gw > visible_width {
                    break;
                }
                visible.push_str(g);
                vis_width += gw;
            }
        }
    }

    let display_cursor_col = cursor_display.saturating_sub(start_cum);
    let cursor_x = input_inner.x + display_cursor_col.min(visible_width.saturating_sub(1)) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(visible).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
    f.set_cursor_position((cursor_x, cursor_y));
}

pub fn render_error_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::error_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Theme::error_with_colors(colors));
    f.render_widget(message_paragraph, chunks[0]);

    let ok_btn = Paragraph::new("[ OK ]")
        .style(Theme::selected_error_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, chunks[1]);
}

pub fn wrapped_line_count(text: &str, available_width: u16) -> usize {
    let w = available_width.max(1) as usize;
    text.lines()
        .map(|line| {
            let display_w = UnicodeWidthStr::width(line);
            if display_w == 0 {
                1
            } else {
                display_w.div_ceil(w)
            }
        })
        .sum()
}

pub fn render_help_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    scroll_offset: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::help_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content_area = help_dialog_content_rect(area);
    let max_lines = content_area.height as usize;

    let likely_scrollable =
        wrapped_line_count(message, content_area.width.saturating_sub(1)) > max_lines;
    let message_area = if likely_scrollable && content_area.width > 1 {
        Rect::new(
            content_area.x,
            content_area.y,
            content_area.width.saturating_sub(1),
            content_area.height,
        )
    } else {
        content_area
    };

    let total_lines = wrapped_line_count(message, message_area.width);
    let clamped_offset = scroll_offset.min(total_lines.saturating_sub(max_lines));

    let message_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .scroll((clamped_offset.min(u16::MAX as usize) as u16, 0))
        .alignment(Alignment::Left)
        .style(Theme::info_with_colors(colors));
    f.render_widget(message_paragraph, message_area);

    let has_scrollbar = total_lines > max_lines && content_area.width > 1;
    if has_scrollbar {
        let scrollbar_area = Rect::new(
            content_area.x + content_area.width.saturating_sub(1),
            content_area.y,
            1,
            content_area.height,
        );
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .viewport_content_length(max_lines)
            .position(clamped_offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_symbol("█")
                .track_symbol(Some("░"))
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(Theme::scrollbar_active_with_colors(colors)))
                .track_style(Style::default().fg(Theme::scrollbar_active_with_colors(colors))),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }

    let button_area = Rect::new(
        inner.x,
        inner.y + inner.height.saturating_sub(1),
        inner.width,
        1,
    );
    let ok_btn = Paragraph::new("[ Press any key to exit, Arrows/PgUp/PgDn to scroll ]")
        .style(Theme::highlight_bold_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, button_area);
}

const CANCELING_PREFIX: &str = "Canceling:";

pub fn render_progress_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    percent: f32,
    cancellable: bool,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    let clamped = (percent.clamp(0.0, 100.0).round()) as u16;
    let gauge = Gauge::default()
        .gauge_style(Theme::progress_bar_with_colors(colors))
        .percent(clamped)
        .label(format!("{clamped}%"));
    f.render_widget(gauge, chunks[1]);

    let hint_text = if !cancellable {
        ""
    } else if message.starts_with(CANCELING_PREFIX) {
        "Canceled"
    } else {
        "Esc: cancel after current item"
    };
    if !hint_text.is_empty() {
        let hint = Paragraph::new(hint_text)
            .style(Theme::warning_with_colors(colors))
            .alignment(Alignment::Center);
        f.render_widget(hint, chunks[2]);
    }
}

pub fn render_properties_dialog(
    f: &mut Frame,
    area: Rect,
    info: &PropertiesInfo,
    colors: &ColorPalette,
) {
    let display_name = truncate_path(&info.name, 30);
    let title = format!("Properties — {display_name}");
    let block = dialog_block(&title, Theme::warning_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(format!("Name: {}", info.name)),
        Line::from(format!("Type: {}", info.file_type)),
        Line::from(format!("Size: {}", info.size)),
        Line::from(format!("Modified: {}", info.mtime)),
        Line::from(format!("Permissions: {}", info.permissions)),
        Line::from(format!("Owner: {}:{}", info.owner, info.group)),
        Line::from(""),
        Line::from("[ Press Enter or Esc to close ]").style(Theme::info_with_colors(colors)),
    ];

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}

pub fn render_list_picker<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
) {
    render_list_picker_with_colors(f, title, items, selected, hint, &ColorPalette::default());
}

pub fn render_list_picker_with_colors<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
    colors: &ColorPalette,
) {
    let area = f.area();
    let picker_area = centered_rect(60, 70, area);

    f.render_widget(Clear, picker_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, picker_area);

    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if items.is_empty() {
        let empty = Paragraph::new("(empty)")
            .style(Style::default().fg(Theme::regular_file_with_colors(colors)))
            .alignment(Alignment::Center);
        f.render_widget(empty, chunks[0]);
    } else {
        let visible_height = chunks[0].height as usize;
        let clamped_selected = selected.min(items.len().saturating_sub(1));
        let start_idx = if visible_height == 0 {
            clamped_selected
        } else {
            let half = visible_height / 2;
            if clamped_selected < half {
                0
            } else if clamped_selected + half >= items.len() {
                items.len().saturating_sub(visible_height)
            } else {
                clamped_selected - half
            }
        };
        let end_idx = (start_idx + visible_height).min(items.len());
        let list_items: Vec<ListItem> = items[start_idx..end_idx]
            .iter()
            .map(|s| ListItem::new(s.as_ref()))
            .collect();
        let list = List::new(list_items)
            .highlight_style(Theme::highlight_bold_with_colors(colors))
            .highlight_symbol("> ");
        let mut list_state = ListState::default();
        list_state.select(Some(clamped_selected - start_idx));
        f.render_stateful_widget(list, chunks[0], &mut list_state);
    }

    let hint_para = Paragraph::new(hint)
        .style(Theme::warning_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(hint_para, chunks[1]);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    const MIN_WIDTH: u16 = 30;
    const MIN_HEIGHT: u16 = 5;

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return area;
    }

    let dialog_width = ((area.width as u32 * percent_x as u32) / 100)
        .max(MIN_WIDTH as u32)
        .min(area.width as u32) as u16;
    let dialog_height = ((area.height as u32 * percent_y as u32) / 100)
        .max(MIN_HEIGHT as u32)
        .min(area.height as u32) as u16;

    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

    Rect::new(x, y, dialog_width, dialog_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    #[test]
    fn test_centered_rect_basic() {
        let area = Rect::new(0, 0, 100, 50);
        let rect = centered_rect(50, 50, area);
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 25);
        assert_eq!(rect.x, 25);
        assert_eq!(rect.y, 12);
    }

    #[test]
    fn test_centered_rect_full() {
        let area = Rect::new(0, 0, 100, 100);
        let rect = centered_rect(100, 100, area);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 100);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 0);
    }

    #[test]
    fn test_centered_rect_with_offset() {
        let area = Rect::new(10, 5, 80, 40);
        let rect = centered_rect(50, 50, area);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 20);
        assert_eq!(rect.x, 30);
        assert_eq!(rect.y, 15);
    }

    #[test]
    fn help_visible_height_matches_render_layout() {
        let area = Rect::new(0, 0, 80, 24);
        let visible = help_visible_height(area);
        let dialog_area = centered_rect(50, 40, area);
        let inner = Block::default().borders(Borders::ALL).inner(dialog_area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);

        assert_eq!(visible, chunks[0].height as usize);
    }

    #[test]
    fn help_scrollbar_does_not_overwrite_text_area() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let message = (0..20)
            .map(|i| format!("line-{i:02}-abcdef"))
            .collect::<Vec<_>>()
            .join("\n");

        terminal
            .draw(|f| {
                let area = centered_rect(50, 40, f.area());
                render_help_dialog(f, area, "Help", &message, 0, &DEFAULT_COLORS);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let area = centered_rect(50, 40, Rect::new(0, 0, 40, 12));
        let inner = Block::default().borders(Borders::ALL).inner(area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);
        let text_last_x = chunks[0].x + chunks[0].width - 2;
        let scrollbar_x = chunks[0].x + chunks[0].width - 1;

        assert_ne!(buffer[(text_last_x, chunks[0].y)].symbol(), "█");
        assert_ne!(buffer[(text_last_x, chunks[0].y)].symbol(), "░");
        assert!(matches!(
            buffer[(scrollbar_x, chunks[0].y)].symbol(),
            "█" | "░"
        ));
    }

    #[test]
    fn help_scrollbar_reserved_for_wrapped_single_line() {
        let message = "abcdefghijklmnopqrstuvwxyz".repeat(5);
        let area = centered_rect(50, 40, Rect::new(0, 0, 40, 12));
        let content = help_dialog_content_rect(area);
        let text_last_x = content.x + content.width - 2;
        let scrollbar_x = content.x + content.width - 1;

        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal
            .draw(|f| render_help_dialog(f, area, "Help", &message, 0, &DEFAULT_COLORS))
            .unwrap();
        let unscrolled_start = terminal.backend().buffer()[(content.x, content.y)]
            .symbol()
            .to_owned();

        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal
            .draw(|f| render_help_dialog(f, area, "Help", &message, 1, &DEFAULT_COLORS))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_ne!(buffer[(content.x, content.y)].symbol(), unscrolled_start);
        assert_ne!(buffer[(text_last_x, content.y)].symbol(), "█");
        assert_ne!(buffer[(text_last_x, content.y)].symbol(), "░");
        assert!(matches!(
            buffer[(scrollbar_x, content.y)].symbol(),
            "█" | "░"
        ));
    }

    #[test]
    fn wrapped_line_count_long_line_narrow_area() {
        let text = "abcdefghijklmnopqrstuvwxyz";
        assert_eq!(wrapped_line_count(text, 10), 3);
        assert!(wrapped_line_count(text, 1) > 1);
    }

    #[test]
    fn wrapped_line_count_short_line_wide_area() {
        assert_eq!(wrapped_line_count("abc", 80), 1);
    }

    #[test]
    fn wrapped_line_count_empty_text() {
        assert_eq!(wrapped_line_count("", 10), 0);
    }

    #[test]
    fn wrapped_line_count_multiline() {
        let text = "short\nthis is a much longer line that should wrap\nend";
        assert!(wrapped_line_count(text, 20) > text.lines().count());
    }

    #[test]
    fn help_scroll_uses_wrapped_lines() {
        let long_line: String = "x".repeat(200);
        let message = format!("header\n{long_line}\nfooter");
        let width: u16 = 20;
        let total = wrapped_line_count(&message, width);
        assert!(total > 3, "wrapped count {total} should exceed 3 raw lines");
    }

    #[test]
    fn truncate_path_keeps_short_utf8_path() {
        assert_eq!(truncate_path("zażółć", 6), "zażółć");
    }

    #[test]
    fn truncate_path_truncates_utf8_suffix_safely() {
        assert_eq!(truncate_path("/tmp/zażółć/plik", 9), "...ć/plik");
    }

    #[test]
    fn truncate_path_truncates_tiny_utf8_width_safely() {
        assert_eq!(truncate_path("żółć", 3), "żół");
    }

    #[test]
    fn truncate_path_preserves_filename() {
        assert_eq!(
            truncate_path("/very/long/directory/path/file.txt", 15),
            "...ath/file.txt"
        );
    }

    #[test]
    fn overwrite_dialog_empty_files_returns_early() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_dialog(
                    f,
                    &DialogKind::OverwriteConfirm {
                        selection: 0,
                        files: Cow::Borrowed(&[]),
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn list_picker_keeps_selected_visible() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();

        terminal
            .draw(|f| render_list_picker(f, "Pick", &items, 19, "hint"))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Item 19"));
        assert!(!rendered.contains("Item 0"));
    }

    #[test]
    fn test_render_confirmation_dialog_inner() {
        let backend = TestBackend::new(60, 25);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = centered_rect(50, 40, f.area());
                render_confirmation_dialog_inner(
                    f,
                    area,
                    "Confirm",
                    "Are you sure?",
                    &[
                        (Theme::highlight_bold(), "[ Yes ]"),
                        (Theme::dialog(), "[ No ]"),
                    ],
                    &["file1.txt", "file2.txt"] as &[&str],
                    &DEFAULT_COLORS,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(rendered.contains("Confirm"), "title should be rendered");
        assert!(
            rendered.contains("Are you sure?"),
            "message should be rendered"
        );
        assert!(
            rendered.contains("[ Yes ]"),
            "yes button should be rendered"
        );
        assert!(rendered.contains("[ No ]"), "no button should be rendered");
        assert!(
            rendered.contains("file1.txt"),
            "file list should show first file"
        );
        assert!(
            rendered.contains("file2.txt"),
            "file list should show second file"
        );
    }

    #[test]
    fn test_render_confirmation_dialog_inner_empty_title_no_files() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = centered_rect(50, 40, f.area());
                render_confirmation_dialog_inner(
                    f,
                    area,
                    "",
                    "msg",
                    &[(Theme::dialog(), "[ OK ]")],
                    &[] as &[&str],
                    &DEFAULT_COLORS,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(
            rendered.contains("msg"),
            "message should render with empty title"
        );
        assert!(rendered.contains("[ OK ]"), "single button should render");
    }

    #[test]
    fn test_help_dialog_content_rect() {
        let rect = help_dialog_content_rect(Rect::new(0, 0, 40, 20));
        assert_eq!(rect.x, 1, "x should be 1 after border");
        assert_eq!(rect.y, 1, "y should be 1 after border");
        assert_eq!(rect.width, 38, "width should be inner width of dialog");
        assert_eq!(rect.height, 17, "height accounts for border and bottom bar");

        let rect2 = help_dialog_content_rect(Rect::new(0, 0, 80, 40));
        assert_eq!(rect2.x, 1);
        assert_eq!(rect2.y, 1);
        assert_eq!(rect2.width, 78);
        assert_eq!(rect2.height, 37);
    }

    #[test]
    fn test_help_dialog_content_rect_small_terminal() {
        let rect = help_dialog_content_rect(Rect::new(0, 0, 8, 6));
        assert_eq!(rect.x, 1);
        assert_eq!(rect.y, 1);
        assert_eq!(rect.width, 6);
        assert_eq!(rect.height, 3);

        let rect2 = help_dialog_content_rect(Rect::new(0, 0, 3, 3));
        assert_eq!(rect2.x, 1);
        assert_eq!(rect2.y, 1);
        assert_eq!(rect2.width, 1);
        assert_eq!(rect2.height, 1);
    }

    #[test]
    fn test_input_dialog_empty_value_has_warning_border() {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = centered_rect(50, 40, f.area());
                render_input_dialog(f, area, "Input", "Enter value:", "", 0, &DEFAULT_COLORS);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let area = centered_rect(50, 40, Rect::new(0, 0, 60, 12));
        let inner = Block::default().borders(Borders::ALL).inner(area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(inner);

        let input_area = chunks[1];
        let top_left = buf[(input_area.x, input_area.y)].clone();
        let warning_color = Theme::warning().fg.unwrap_or(Color::Yellow);
        assert_eq!(
            top_left.fg, warning_color,
            "empty value input border should have warning color"
        );
    }

    #[test]
    fn test_properties_dialog_renders_content() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PropertiesInfo {
            name: "/very/long/path/to/some/file.txt".into(),
            size: "1.2 MB".into(),
            mtime: "2024-01-15 10:30".into(),
            permissions: "rw-r--r--".into(),
            owner: "user".into(),
            group: "staff".into(),
            file_type: "Regular File".into(),
        };

        terminal
            .draw(|f| {
                let area = centered_rect(50, 40, f.area());
                render_properties_dialog(f, area, &info, &DEFAULT_COLORS);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(
            rendered.contains("Name:"),
            "properties dialog should show file name label"
        );
        assert!(
            rendered.contains("Press Enter or Esc to close"),
            "properties dialog should show close hint"
        );
    }
}
