use std::borrow::Cow;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;
use super::text::truncate_path;

const LIST_PADDING: &str = "  ";

fn render_file_list(
    f: &mut Frame,
    area: Rect,
    files: &[impl AsRef<str>],
    max_name_width: usize,
    colors: &ColorPalette,
) {
    let max_visible = area.height as usize;
    if max_visible == 0 {
        return;
    }
    let mut lines: Vec<Line> = Vec::with_capacity(max_visible);
    if files.len() <= max_visible {
        for name in files {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(
                Line::from(vec![Span::raw(LIST_PADDING), Span::raw(display)])
                    .style(Theme::warning_with_colors(colors)),
            );
        }
    } else {
        let file_slots = max_visible.saturating_sub(1);
        for name in files.iter().take(file_slots) {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(
                Line::from(vec![Span::raw(LIST_PADDING), Span::raw(display)])
                    .style(Theme::warning_with_colors(colors)),
            );
        }
        let remaining = files.len() - file_slots;
        lines.push(Line::from(format!("{LIST_PADDING}... +{remaining} more")));
    }
    let file_paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(file_paragraph, area);
}

pub(super) fn render_confirmation_dialog_inner(
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
    let max_rows = inner.height.saturating_sub(3);
    let file_rows = if has_files {
        (files.len() as u16).min(max_rows)
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
        let max_name_width = inner.width.saturating_sub(LIST_PADDING.len() as u16) as usize;
        render_file_list(f, chunks[1], files, max_name_width, colors);
    }

    let mut spans: Vec<Span> = Vec::with_capacity(buttons.len() * 2);
    for (i, (style, label)) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*label, *style));
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
    let btn_style = |idx: usize| -> Style {
        if selection == idx {
            Theme::highlight_bold_with_colors(colors)
        } else {
            Theme::dialog_with_colors(colors)
        }
    };
    let buttons = [(btn_style(0), "[ Yes ]"), (btn_style(1), "[ No ]")];
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

    let msg: Cow<'_, str> = if files.len() == 1 {
        Cow::Borrowed("File already exists at destination:")
    } else {
        Cow::Owned(format!(
            "{} files already exist at destination:",
            files.len()
        ))
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
