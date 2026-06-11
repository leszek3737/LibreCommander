use std::sync::OnceLock;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Gauge, Paragraph, Wrap},
};

use crate::ui::dialogs::PropertiesInfo;
use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;
use super::text::truncate_path;

const OK_BUTTON_LABEL: &str = "[ OK ]";
const CLOSE_HINT_LABEL: &str = "[ Press Enter or Esc to close ]";
const CANCELING_PREFIX: &str = "Canceling:";
const PROPERTIES_NAME_MAX_WIDTH: usize = 30;

const PROP_NAME_PREFIX: &str = "Name: ";
const PROP_TYPE_PREFIX: &str = "Type: ";
const PROP_SIZE_PREFIX: &str = "Size: ";
const PROP_MODIFIED_PREFIX: &str = "Modified: ";
const PROP_PERMISSIONS_PREFIX: &str = "Permissions: ";
const PROP_OWNER_PREFIX: &str = "Owner: ";

static PERCENT_LABELS: OnceLock<[String; 101]> = OnceLock::new();

fn percent_label(n: u16) -> &'static str {
    let labels = PERCENT_LABELS.get_or_init(|| std::array::from_fn(|i| format!("{i}%")));
    &labels[n as usize]
}

fn centered_paragraph<'a>(text: &'a str, style: Style) -> Paragraph<'a> {
    Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Center)
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

    let ok_btn = centered_paragraph(OK_BUTTON_LABEL, Theme::selected_error_with_colors(colors));
    f.render_widget(ok_btn, chunks[1]);
}

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
        .label(percent_label(clamped));
    f.render_widget(gauge, chunks[1]);

    let hint_text = if !cancellable {
        ""
    } else if message.starts_with(CANCELING_PREFIX) {
        "Canceled"
    } else {
        "Esc: cancel after current item"
    };
    if !hint_text.is_empty() {
        let hint = centered_paragraph(hint_text, Theme::warning_with_colors(colors));
        f.render_widget(hint, chunks[2]);
    }
}

pub fn render_properties_dialog(
    f: &mut Frame,
    area: Rect,
    info: &PropertiesInfo<'_>,
    colors: &ColorPalette,
) {
    let display_name = truncate_path(&info.name, PROPERTIES_NAME_MAX_WIDTH);
    let title = format!("Properties — {display_name}");
    let block = dialog_block(&title, Theme::warning_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(format!("{PROP_NAME_PREFIX}{display_name}")),
        Line::from(format!("{PROP_TYPE_PREFIX}{}", info.file_type)),
        Line::from(format!("{PROP_SIZE_PREFIX}{}", info.size)),
        Line::from(format!("{PROP_MODIFIED_PREFIX}{}", info.mtime)),
        Line::from(format!("{PROP_PERMISSIONS_PREFIX}{}", info.permissions)),
        Line::from(format!("{PROP_OWNER_PREFIX}{}:{}", info.owner, info.group)),
        Line::from(""),
        Line::from(CLOSE_HINT_LABEL).style(Theme::info_with_colors(colors)),
    ];

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}
