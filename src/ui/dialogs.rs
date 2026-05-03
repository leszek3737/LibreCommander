use super::theme::Theme;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
    },
};

#[derive(Debug, Clone)]
pub enum DialogKind {
    Confirm {
        title: String,
        message: String,
        selection: usize,
        files: Option<Vec<String>>,
    },
    Input {
        title: String,
        prompt: String,
        value: String,
        cursor_pos: usize,
    },
    Error {
        title: String,
        message: String,
    },
    Help {
        title: String,
        message: String,
    },
    Progress {
        title: String,
        message: String,
        percent: f32,
    },
    Properties {
        name: String,
        size: String,
        mtime: String,
        permissions: String,
        owner: String,
        group: String,
        file_type: String,
    },
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind) {
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
        DialogKind::Help { title, message } => {
            render_help_dialog(f, dialog_area, title, message);
        }
        DialogKind::Progress {
            title,
            message,
            percent,
        } => {
            render_progress_dialog(f, dialog_area, title, message, *percent);
        }
        DialogKind::Properties {
            name,
            size,
            mtime,
            permissions,
            owner,
            group,
            file_type,
        } => {
            render_properties_dialog(
                f,
                dialog_area,
                name,
                size,
                mtime,
                permissions,
                owner,
                group,
                file_type,
            );
        }
    }
}

pub fn render_confirm_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    selection: usize,
    files: &[String],
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_files = !files.is_empty();
    let file_rows = if has_files {
        (files.len() as u16 + 1).min(6)
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

    let msg_paragraph = Paragraph::new(message.to_string())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    if has_files {
        let max_visible = chunks[1].height as usize;
        let show_count = files.len().min(max_visible.saturating_sub(1).max(1));
        let mut lines: Vec<Line> = Vec::with_capacity(show_count + 1);
        if files.len() <= show_count {
            for name in files {
                lines.push(Line::from(format!("  {name}")).style(Theme::warning()));
            }
        } else {
            for name in files.iter().take(show_count.saturating_sub(1)) {
                lines.push(Line::from(format!("  {name}")).style(Theme::warning()));
            }
            let remaining = files.len() - show_count + 1;
            lines.push(Line::from(format!("  ... +{remaining} more")));
        }
        let file_paragraph = Paragraph::new(lines).alignment(Alignment::Left);
        f.render_widget(file_paragraph, chunks[1]);
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

pub fn render_input_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    prompt: &str,
    value: &str,
    cursor_pos: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::dialog());
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

    let prompt_paragraph = Paragraph::new(prompt.to_string());
    f.render_widget(prompt_paragraph, chunks[0]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain);
    let input_inner = input_block.inner(chunks[1]);

    let visible_width = input_inner.width as usize;
    if visible_width == 0 || input_inner.height == 0 {
        let input_paragraph = Paragraph::new(value.to_string()).block(input_block);
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::error_dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message.to_string())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Theme::error());
    f.render_widget(message_paragraph, chunks[0]);

    let ok_btn = Paragraph::new("[ OK ]")
        .style(Theme::selected_error())
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, chunks[1]);
}

pub fn render_help_dialog(f: &mut Frame, area: Rect, title: &str, message: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::help_dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message.to_string())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left)
        .style(Theme::info());
    f.render_widget(message_paragraph, chunks[0]);

    let ok_btn = Paragraph::new("[ Press any key ]")
        .style(Theme::highlight_bold())
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, chunks[1]);
}

pub fn render_progress_dialog(f: &mut Frame, area: Rect, title: &str, message: &str, percent: f32) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::dialog());
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

    let msg_paragraph = Paragraph::new(message.to_string())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    let clamped = percent.clamp(0.0, 100.0) as u16;
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

#[allow(clippy::too_many_arguments)]
pub fn render_properties_dialog(
    f: &mut Frame,
    area: Rect,
    name: &str,
    size: &str,
    mtime: &str,
    permissions: &str,
    owner: &str,
    group: &str,
    file_type: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("File Properties".to_string())
        .border_type(BorderType::Thick)
        .style(Theme::warning_dialog());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(format!("Name: {}", name)),
        Line::from(format!("Type: {}", file_type)),
        Line::from(format!("Size: {}", size)),
        Line::from(format!("Modified: {}", mtime)),
        Line::from(format!("Permissions: {}", permissions)),
        Line::from(format!("Owner: {}:{}", owner, group)),
        Line::from(""),
        Line::from("[ Press Enter or Esc to close ]").style(Theme::info()),
    ];

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}

pub fn render_list_picker(
    f: &mut Frame,
    title: &str,
    items: &[String],
    selected: usize,
    hint: &str,
) {
    let area = f.area();
    let picker_area = centered_rect(60, 70, area);

    // Fill picker area with blue background
    f.render_widget(Clear, picker_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog());
    f.render_widget(bg_block, picker_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_type(BorderType::Thick)
        .style(Theme::dialog());
    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if items.is_empty() {
        let empty = Paragraph::new("(empty)")
            .style(Style::default().fg(Theme::HIDDEN_FILE))
            .alignment(Alignment::Center);
        f.render_widget(empty, chunks[0]);
    } else {
        let visible_height = chunks[0].height as usize;
        let selected = selected.min(items.len().saturating_sub(1));
        let start_idx = if visible_height == 0 {
            selected
        } else {
            selected.saturating_sub(visible_height.saturating_sub(1))
        };
        let end_idx = (start_idx + visible_height).min(items.len());
        let list_items: Vec<ListItem> = items[start_idx..end_idx]
            .iter()
            .map(|s| ListItem::new(s.as_str()))
            .collect();
        let list = List::new(list_items)
            .highlight_style(Theme::highlight_bold())
            .highlight_symbol("> ");
        let mut list_state = ListState::default();
        list_state.select(Some(selected - start_idx));
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
