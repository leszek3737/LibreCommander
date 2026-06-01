use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::Style,
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::{centered_rect, dialog_block};

pub fn render_list_picker<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
) {
    render_list_picker_with_colors(f, title, items, selected, hint, &ColorPalette::default());
}

pub fn render_list_picker_with_colors<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
    colors: &ColorPalette,
) {
    let area = f.area();
    let picker_area = centered_rect(60, 70, area);

    f.render_widget(Clear, picker_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, picker_area);

    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if items.is_empty() {
        let empty = Paragraph::new("(empty)")
            .style(Style::default().fg(Theme::regular_file_with_colors(colors)))
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
        let list = List::new(
            items[start_idx..end_idx]
                .iter()
                .map(|s| ListItem::new(s.as_ref())),
        )
        .highlight_style(Theme::highlight_bold_with_colors(colors))
        .highlight_symbol("> ");
        let mut list_state = ListState::default();
        list_state.select(Some(clamped_selected - start_idx));
        f.render_stateful_widget(list, chunks[0], &mut list_state);
    }

    let hint_para = Paragraph::new(hint)
        .style(Theme::warning_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(hint_para, chunks[1]);
}
