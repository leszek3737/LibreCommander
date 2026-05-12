use std::borrow::Cow;

use super::theme::Theme;

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
        files: Option<Vec<String>>,
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
    },
    Properties {
        info: PropertiesInfo,
    },
    OverwriteConfirm {
        selection: usize,
        files: Vec<String>,
    },
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind<'_>) {
    if matches!(dialog, DialogKind::OverwriteConfirm { files, .. } if files.is_empty()) {
        return;
    }

    let rect = f.area();
    let dialog_area = centered_rect(50, 40, rect);

    f.render_widget(Clear, dialog_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog());
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
                title,
                message,
                *selection,
                files.as_deref().unwrap_or_default(),
            );
        }
        DialogKind::Input {
            title,
            prompt,
            value,
            cursor_pos,
        } => {
            render_input_dialog(f, dialog_area, title, prompt, value, *cursor_pos);
        }
        DialogKind::Error { title, message } => {
            render_error_dialog(f, dialog_area, title, message);
        }
        DialogKind::Help {
            title,
            message,
            scroll_offset,
        } => {
            render_help_dialog(f, dialog_area, title, message, *scroll_offset);
        }
        DialogKind::Progress {
            title,
            message,
            percent,
        } => {
            render_progress_dialog(f, dialog_area, title, message, *percent);
        }
        DialogKind::Properties { info } => {
            render_properties_dialog(f, dialog_area, info);
        }
        DialogKind::OverwriteConfirm { selection, files } => {
            render_overwrite_dialog(f, dialog_area, *selection, files);
        }
    }
}

pub fn help_visible_height(area: Rect) -> usize {
    let dialog_area = centered_rect(50, 40, area);
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(dialog_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    chunks[0].height as usize
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
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let file_width = unicode_width::UnicodeWidthStr::width(file);
    if file_width >= max_width {
        return truncate_suffix(file, max_width);
    }
    if dir.is_empty() {
        return truncate_suffix(path, max_width);
    }
    let budget = max_width - file_width - 1;
    let dir_part = truncate_suffix(dir, budget);
    format!("{dir_part}/{file}")
}

fn render_file_list(f: &mut Frame, area: Rect, files: &[impl AsRef<str>], max_name_width: usize) {
    let max_visible = area.height as usize;
    let show_count = files.len().min(max_visible.saturating_sub(1).max(1));
    let mut lines: Vec<Line> = Vec::with_capacity(show_count + 1);
    if files.len() <= show_count {
        for name in files {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(Line::from(format!("  {display}")).style(Theme::warning()));
        }
    } else {
        for name in files.iter().take(show_count.saturating_sub(1)) {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(Line::from(format!("  {display}")).style(Theme::warning()));
        }
        let remaining = files.len() - show_count + 1;
        lines.push(Line::from(format!("  ... +{remaining} more")));
    }
    let file_paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(file_paragraph, area);
}

pub fn render_confirm_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    selection: usize,
    files: &[impl AsRef<str>],
) {
    let block = dialog_block(title, Theme::dialog());
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
        render_file_list(f, chunks[1], files, max_name_width);
    }

    let yes_style = if selection == 0 {
        Theme::highlight_bold()
    } else {
        Theme::dialog()
    };
    let no_style = if selection == 1 {
        Theme::highlight_bold()
    } else {
        Theme::dialog()
    };
    let buttons = Line::from(vec![
        ratatui::text::Span::styled("[ Yes ]", yes_style),
        ratatui::text::Span::raw("  "),
        ratatui::text::Span::styled("[ No ]", no_style),
    ]);
    let btn_paragraph = Paragraph::new(buttons).alignment(Alignment::Center);
    f.render_widget(btn_paragraph, chunks[2]);
}

pub fn render_overwrite_dialog(
    f: &mut Frame,
    area: Rect,
    selection: usize,
    files: &[impl AsRef<str>],
) {
    if files.is_empty() {
        return;
    }

    let block = dialog_block("Overwrite?", Theme::dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let msg = if files.len() == 1 {
        "File already exists at destination:".to_string()
    } else {
        format!("{} files already exist at destination:", files.len())
    };

    let max_rows = inner.height.saturating_sub(5).max(3);
    let file_rows = (files.len() as u16 + 1).min(max_rows);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(file_rows),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg_paragraph = Paragraph::new(msg)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    let max_name_width = inner.width.saturating_sub(2) as usize;
    render_file_list(f, chunks[1], files, max_name_width);

    let btn_style = |idx: usize| -> Style {
        if selection == idx {
            Theme::highlight_bold()
        } else {
            Theme::dialog()
        }
    };
    let buttons = Line::from(vec![
        ratatui::text::Span::styled("[ Overwrite All ]", btn_style(0)),
        ratatui::text::Span::raw("  "),
        ratatui::text::Span::styled("[ Cancel ]", btn_style(1)),
    ]);
    let btn_paragraph = Paragraph::new(buttons).alignment(Alignment::Center);
    f.render_widget(btn_paragraph, chunks[2]);
}

pub fn input_dialog_rect(area: Rect) -> Rect {
    centered_rect(50, 40, area)
}

pub fn render_input_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    prompt: &str,
    value: &str,
    cursor_pos: usize,
) {
    let block = dialog_block(title, Theme::dialog());
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
    let input_inner = input_block.inner(chunks[1]);

    let visible_width = input_inner.width as usize;
    if visible_width == 0 || input_inner.height == 0 {
        let input_paragraph = Paragraph::new(value).block(input_block);
        f.render_widget(input_paragraph, chunks[1]);
        return;
    }

    let chars: Vec<char> = value.chars().collect();
    let char_count = chars.len();
    let clamped_cursor = cursor_pos.min(char_count);

    let char_widths: Vec<usize> = chars
        .iter()
        .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
        .collect();

    let mut cum_widths = vec![0usize; char_count + 1];
    for i in 0..char_count {
        cum_widths[i + 1] = cum_widths[i] + char_widths[i];
    }

    let cursor_display = cum_widths[clamped_cursor];
    let scroll_display = if cursor_display >= visible_width {
        cursor_display.saturating_sub(visible_width.saturating_sub(1))
    } else {
        0
    };

    let start_idx = if scroll_display == 0 {
        0
    } else {
        cum_widths
            .iter()
            .position(|&w| w > scroll_display)
            .map(|p| p.saturating_sub(1))
            .unwrap_or(0)
    };

    let mut visible = String::new();
    let mut vis_width = 0;
    for i in start_idx..char_count {
        if vis_width + char_widths[i] > visible_width {
            break;
        }
        visible.push(chars[i]);
        vis_width += char_widths[i];
    }

    let display_cursor_col = cursor_display.saturating_sub(cum_widths[start_idx]);
    let cursor_x = input_inner.x + display_cursor_col.min(visible_width.saturating_sub(1)) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(visible).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
    f.set_cursor_position((cursor_x, cursor_y));
}

pub fn render_error_dialog(f: &mut Frame, area: Rect, title: &str, message: &str) {
    let block = dialog_block(title, Theme::error_dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Theme::error());
    f.render_widget(message_paragraph, chunks[0]);

    let ok_btn = Paragraph::new("[ OK ]")
        .style(Theme::selected_error())
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, chunks[1]);
}

pub fn render_help_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    scroll_offset: usize,
) {
    let block = dialog_block(title, Theme::help_dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let max_lines = chunks[0].height as usize;
    let all_lines: Vec<&str> = message.lines().collect();
    let total_lines = all_lines.len();

    let clamped_offset = scroll_offset.min(total_lines.saturating_sub(max_lines));
    let visible_lines: Vec<Line> = all_lines
        .iter()
        .skip(clamped_offset)
        .take(max_lines)
        .map(|l| Line::from(*l))
        .collect();

    let has_scrollbar = total_lines > max_lines && chunks[0].width > 1;
    let message_area = if has_scrollbar {
        Rect::new(
            chunks[0].x,
            chunks[0].y,
            chunks[0].width.saturating_sub(1),
            chunks[0].height,
        )
    } else {
        chunks[0]
    };

    let message_paragraph = Paragraph::new(visible_lines)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left)
        .style(Theme::info());
    f.render_widget(message_paragraph, message_area);

    if has_scrollbar {
        let scrollbar_area = Rect::new(
            chunks[0].x + chunks[0].width.saturating_sub(1),
            chunks[0].y,
            1,
            chunks[0].height,
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
                .thumb_style(Style::default().fg(Theme::scrollbar_active()))
                .track_style(Style::default().fg(Theme::scrollbar_active())),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }

    let ok_btn = Paragraph::new("[ Press any key to exit, Arrows/PgUp/PgDn to scroll ]")
        .style(Theme::highlight_bold())
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, chunks[1]);
}

pub fn render_progress_dialog(f: &mut Frame, area: Rect, title: &str, message: &str, percent: f32) {
    let block = dialog_block(title, Theme::dialog());
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
        .gauge_style(Theme::progress_bar())
        .percent(clamped)
        .label(format!("{clamped}%"));
    f.render_widget(gauge, chunks[1]);

    let hint = Paragraph::new("Esc: cancel after current item")
        .style(Theme::warning())
        .alignment(Alignment::Center);
    f.render_widget(hint, chunks[2]);
}

pub fn render_properties_dialog(f: &mut Frame, area: Rect, info: &PropertiesInfo) {
    let display_name = truncate_path(&info.name, 30);
    let title = format!("Properties — {display_name}");
    let block = dialog_block(&title, Theme::warning_dialog());
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
        Line::from("[ Press Enter or Esc to close ]").style(Theme::info()),
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
    let area = f.area();
    let picker_area = centered_rect(60, 70, area);

    f.render_widget(Clear, picker_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog());
    f.render_widget(bg_block, picker_area);

    let block = dialog_block(title, Theme::dialog());
    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if items.is_empty() {
        let empty = Paragraph::new("(empty)")
            .style(Style::default().fg(Theme::regular_file()))
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
            .highlight_style(Theme::highlight_bold())
            .highlight_symbol("> ");
        let mut list_state = ListState::default();
        list_state.select(Some(clamped_selected - start_idx));
        f.render_stateful_widget(list, chunks[0], &mut list_state);
    }

    let hint_para = Paragraph::new(hint)
        .style(Theme::warning())
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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
                render_help_dialog(f, area, "Help", &message, 0);
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
                        files: vec![],
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
}
