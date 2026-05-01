use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

#[derive(Debug, Clone)]
pub enum DialogKind {
    Confirm {
        title: String,
        message: String,
        selection: usize,
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

#[derive(Debug, Clone, PartialEq)]
pub enum DialogResult {
    None,
    Confirmed,
    Cancelled,
    InputValue(String),
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind) {
    let rect = f.area();
    let dialog_area = centered_rect(50, 40, rect);

    // Fill dialog area with blue background
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog());
    f.render_widget(bg_block, dialog_area);

    match dialog {
        DialogKind::Confirm {
            title,
            message,
            selection,
        } => {
            render_confirm_dialog(f, dialog_area, title, message, *selection);
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
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message.to_string())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(message_paragraph, chunks[0]);

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
    f.render_widget(btn_paragraph, chunks[1]);
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
    let input_paragraph = Paragraph::new(value.to_string()).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);

    if cursor_pos <= value.chars().count() && input_inner.height > 0 {
        let prefix: String = value.chars().take(cursor_pos).collect();
        let display_col: usize = UnicodeWidthStr::width(prefix.as_str());
        let cursor_x = input_inner.x + display_col.min(input_inner.width as usize) as u16;
        let cursor_y = input_inner.y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
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
        .alignment(Alignment::Center)
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
        .constraints([Constraint::Min(2), Constraint::Length(1)])
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
}

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
        let list_items: Vec<ListItem> = items.iter().map(|s| ListItem::new(s.as_str())).collect();
        let list = List::new(list_items)
            .highlight_style(Theme::highlight_bold())
            .highlight_symbol("> ");
        let mut list_state = ListState::default();
        list_state.select(Some(selected));
        f.render_stateful_widget(list, chunks[0], &mut list_state);
    }

    let hint_para = Paragraph::new(hint)
        .style(Theme::warning())
        .alignment(Alignment::Center);
    f.render_widget(hint_para, chunks[1]);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let dialog_width = (area.width as u32 * percent_x as u32 / 100) as u16;
    let dialog_height = (area.height as u32 * percent_y as u32 / 100) as u16;

    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

    Rect::new(x, y, dialog_width, dialog_height)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_dialog_result_eq() {
        assert_eq!(DialogResult::None, DialogResult::None);
        assert_eq!(DialogResult::Confirmed, DialogResult::Confirmed);
        assert_ne!(DialogResult::Confirmed, DialogResult::Cancelled);
        assert_eq!(
            DialogResult::InputValue("test".to_string()),
            DialogResult::InputValue("test".to_string())
        );
    }
}
