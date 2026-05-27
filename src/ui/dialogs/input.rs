use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;

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
    let cursor_x = input_inner.x + display_cursor_col.min(vis_width) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(visible).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
    f.set_cursor_position((cursor_x, cursor_y));
}
