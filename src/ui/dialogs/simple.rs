use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Gauge, Paragraph, Wrap},
};

use crate::ui::dialogs::PropertiesInfo;
use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;
use super::text::truncate_path;

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

    let msg_min = if inner.height <= 3 { 1 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(msg_min),
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
        Line::from(format!("Name: {display_name}")),
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
