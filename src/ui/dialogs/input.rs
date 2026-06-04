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

    let (visible, display_cursor_col, vis_width) =
        compute_visible_window(value, cursor_pos, visible_width);

    let cursor_x = input_inner.x + display_cursor_col.min(vis_width) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(visible).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
    f.set_cursor_position((cursor_x, cursor_y));
}

fn compute_visible_window(
    value: &str,
    cursor_pos: usize,
    visible_width: usize,
) -> (String, usize, usize) {
    let graphemes: Vec<&str> = value.graphemes(true).collect();
    let grapheme_count = graphemes.len();
    let clamped_cursor = cursor_pos.min(grapheme_count);

    let cursor_display: usize = graphemes
        .iter()
        .take(clamped_cursor)
        .map(|g| UnicodeWidthStr::width(*g))
        .sum();

    let scroll_display = cursor_display.saturating_sub(visible_width.saturating_sub(1));

    if scroll_display == 0 {
        let (visible, vis_width) = take_graphemes_up_to_width(&graphemes, visible_width);
        (visible, cursor_display, vis_width)
    } else {
        let (visible, start_cum, vis_width) =
            compute_scrolled_window(&graphemes, scroll_display, visible_width);
        let display_cursor_col = cursor_display.saturating_sub(start_cum);
        (visible, display_cursor_col, vis_width)
    }
}

fn take_graphemes_up_to_width(graphemes: &[&str], max_width: usize) -> (String, usize) {
    let mut visible = String::with_capacity(max_width);
    let mut vis_width = 0;
    for g in graphemes {
        let gw = UnicodeWidthStr::width(*g);
        if vis_width + gw > max_width {
            break;
        }
        visible.push_str(g);
        vis_width += gw;
    }
    (visible, vis_width)
}

fn compute_scrolled_window(
    graphemes: &[&str],
    scroll_display: usize,
    max_width: usize,
) -> (String, usize, usize) {
    let mut visible = String::with_capacity(max_width);
    let mut vis_width = 0;
    let mut cum = 0usize;
    let mut found_start = false;
    let mut start_cum = 0usize;

    for g in graphemes {
        let gw = UnicodeWidthStr::width(*g);
        if !found_start && cum + gw > scroll_display {
            found_start = true;
            start_cum = cum;
        }
        cum += gw;
        if found_start {
            if vis_width + gw > max_width {
                break;
            }
            visible.push_str(g);
            vis_width += gw;
        }
    }

    if !found_start {
        let (visible, vis_width) = take_graphemes_up_to_width(graphemes, max_width);
        (visible, 0, vis_width)
    } else {
        (visible, start_cum, vis_width)
    }
}
