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
    let widths: Vec<usize> = graphemes
        .iter()
        .map(|g| UnicodeWidthStr::width(*g))
        .collect();
    let clamped_cursor = cursor_pos.min(graphemes.len());

    let cursor_display: usize = widths[..clamped_cursor].iter().sum();

    let scroll_display = cursor_display.saturating_sub(visible_width.saturating_sub(1));

    if scroll_display == 0 {
        let (visible, vis_width) = build_visible(&graphemes, &widths, 0, visible_width);
        (visible, cursor_display, vis_width)
    } else {
        let start_idx = match widths
            .iter()
            .scan(0, |cum, &w| {
                let c = *cum;
                *cum += w;
                Some(c)
            })
            .position(|cum| cum >= scroll_display)
        {
            Some(idx) => idx,
            None => {
                let (visible, vis_width) = build_visible(&graphemes, &widths, 0, visible_width);
                return (visible, cursor_display, vis_width);
            }
        };
        let start_cum: usize = widths[..start_idx].iter().sum();
        let (visible, vis_width) = build_visible(&graphemes, &widths, start_idx, visible_width);
        let display_cursor_col = cursor_display - start_cum;
        (visible, display_cursor_col, vis_width)
    }
}

fn build_visible(
    graphemes: &[&str],
    widths: &[usize],
    start: usize,
    max_width: usize,
) -> (String, usize) {
    let mut visible = String::with_capacity(max_width * 4);
    let mut vis_width = 0;
    for i in start..graphemes.len() {
        if vis_width + widths[i] > max_width {
            break;
        }
        visible.push_str(graphemes[i]);
        vis_width += widths[i];
    }
    (visible, vis_width)
}
