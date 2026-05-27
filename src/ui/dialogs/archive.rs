use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;

#[allow(clippy::too_many_arguments)]
pub fn render_archive_extract_dialog(
    f: &mut Frame,
    area: Rect,
    source: &str,
    entry_count: usize,
    dest_value: &str,
    dest_cursor: usize,
    selection: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block("Extract Archive", Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let source_line = Line::from(vec![
        Span::styled("Source: ", Theme::dialog_with_colors(colors)),
        Span::styled(source, Theme::highlight_with_colors(colors)),
    ]);
    f.render_widget(Paragraph::new(source_line), chunks[0]);

    let entries_text = format!("{entry_count} entries");
    let entries_line = Line::from(vec![
        Span::styled("Entries: ", Theme::dialog_with_colors(colors)),
        Span::styled(entries_text, Theme::highlight_with_colors(colors)),
    ]);
    f.render_widget(Paragraph::new(entries_line), chunks[1]);

    let dest_label = Paragraph::new(Line::from(Span::styled(
        "Destination:",
        Theme::dialog_with_colors(colors),
    )));
    f.render_widget(dest_label, chunks[2]);

    render_input_field(f, chunks[3], dest_value, dest_cursor, colors);

    let buttons = [
        (
            if selection == 0 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ OK ]",
        ),
        (
            if selection == 1 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ Cancel ]",
        ),
    ];
    render_button_row(f, chunks[4], &buttons);
}

pub fn render_archive_create_dialog(
    f: &mut Frame,
    area: Rect,
    source_count: usize,
    dest_value: &str,
    dest_cursor: usize,
    selection: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block("Create Archive", Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let sources_text = format!("{source_count} files selected");
    let sources_line = Line::from(vec![
        Span::styled("Sources: ", Theme::dialog_with_colors(colors)),
        Span::styled(sources_text, Theme::highlight_with_colors(colors)),
    ]);
    f.render_widget(Paragraph::new(sources_line), chunks[0]);

    let dest_label = Paragraph::new(Line::from(Span::styled(
        "Archive name:",
        Theme::dialog_with_colors(colors),
    )));
    f.render_widget(dest_label, chunks[1]);

    render_input_field(f, chunks[2], dest_value, dest_cursor, colors);

    let buttons = [
        (
            if selection == 0 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ OK ]",
        ),
        (
            if selection == 1 {
                Theme::highlight_bold_with_colors(colors)
            } else {
                Theme::dialog_with_colors(colors)
            },
            "[ Cancel ]",
        ),
    ];
    render_button_row(f, chunks[3], &buttons);
}

fn render_input_field(
    f: &mut Frame,
    area: Rect,
    value: &str,
    cursor_pos: usize,
    colors: &ColorPalette,
) {
    use ratatui::widgets::{Block, BorderType, Borders};
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain);
    let input_block = if value.is_empty() {
        input_block.border_style(Theme::warning_with_colors(colors))
    } else {
        input_block
    };
    let input_inner = input_block.inner(area);

    let visible_width = input_inner.width as usize;
    if visible_width == 0 || input_inner.height == 0 {
        let input_paragraph = Paragraph::new(value).block(input_block);
        f.render_widget(input_paragraph, area);
        f.set_cursor_position((input_inner.x, input_inner.y));
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

    let mut visible = String::new();
    let mut vis_width = 0;
    let mut start_cum = 0usize;

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
    let cursor_x = input_inner.x + display_cursor_col.min(vis_width) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(visible).block(input_block);
    f.render_widget(input_paragraph, area);
    f.set_cursor_position((cursor_x, cursor_y));
}

fn render_button_row(f: &mut Frame, area: Rect, buttons: &[(ratatui::style::Style, &str)]) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, (style, label)) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*label, *style));
    }
    let btn_line = Line::from(spans);
    let btn_paragraph = Paragraph::new(btn_line).alignment(Alignment::Center);
    f.render_widget(btn_paragraph, area);
}
