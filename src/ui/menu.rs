use ratatui::{
    Frame,
    layout::Rect,
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::menu::{MENU_ITEMS, MENU_TITLES, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::theme::{ColorPalette, Theme};

const MIN_DROPDOWN_ITEM_WIDTH: usize = 10;
const MENU_VERTICAL_OFFSET: u16 = 4;
const MENU_DROPDOWN_OFFSET: u16 = 2;

fn render_menu_title_bar(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    colors: &ColorPalette,
) {
    for (i, title) in MENU_TITLES.iter().enumerate() {
        let title_width = menu_title_width(title);
        let style = if i == selected_menu {
            Theme::highlight_bold(colors)
        } else {
            Theme::menu_bar(colors)
        };
        let label = Span::styled(format!(" {title} "), style);
        let p = Paragraph::new(label);
        let area = Rect::new(
            menu_bar_area.x + menu_title_x(menu_bar_area.width, i),
            menu_bar_area.y,
            title_width,
            1,
        );
        f.render_widget(p, area);
    }
}

fn render_menu_dropdown(
    f: &mut Frame,
    menu_bar_area: Rect,
    active_menu: usize,
    selected_item: usize,
    colors: &ColorPalette,
) {
    let items = MENU_ITEMS[active_menu];
    let dropdown_width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(*s))
        .max()
        .unwrap_or(MIN_DROPDOWN_ITEM_WIDTH) as u16
        + MENU_VERTICAL_OFFSET;
    let dropdown_y = menu_bar_area.y + 1;
    let max_visible = f.area().height.saturating_sub(dropdown_y);
    if max_visible < 2 {
        return;
    }
    let dropdown_height =
        ((items.len().min(u16::MAX as usize - 2)) as u16 + MENU_DROPDOWN_OFFSET).min(max_visible);
    let dropdown_x = menu_dropdown_x(menu_bar_area, active_menu, dropdown_width);
    let dropdown_area = Rect::new(dropdown_x, dropdown_y, dropdown_width, dropdown_height);

    f.render_widget(Clear, dropdown_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::panel_fg(colors))
        .style(Theme::panel_bg(colors));
    let inner = block.inner(dropdown_area);
    f.render_widget(block, dropdown_area);

    let clamped_selected = selected_item.min(items.len().saturating_sub(1));
    let visible_items = inner.height as usize;
    let scroll_offset = if items.len() <= visible_items {
        0
    } else {
        clamped_selected.saturating_sub(visible_items.saturating_sub(1))
    };

    for (i, item) in items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_items)
    {
        let row = i - scroll_offset;
        let style = if i == clamped_selected {
            Theme::highlight(colors)
        } else {
            Theme::panel(colors)
        };
        let item_area = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
        let label = Span::styled(format!(" {item} "), style);
        let p = Paragraph::new(label);
        f.render_widget(p, item_area);
    }
}

pub fn render_menu_bar(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    selected_item: usize,
    colors: &ColorPalette,
) {
    let selected_menu = selected_menu.min(MENU_ITEMS.len().saturating_sub(1));
    render_menu_title_bar(f, menu_bar_area, selected_menu, colors);
    if !MENU_ITEMS[selected_menu].is_empty() {
        render_menu_dropdown(f, menu_bar_area, selected_menu, selected_item, colors);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;

    fn render_with(menu: usize, item: usize) {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let menu_bar = Rect::new(0, 0, 80, 1);
                render_menu_bar(f, menu_bar, menu, item, &DEFAULT_COLORS);
            })
            .unwrap();
    }

    #[test]
    fn render_first_menu_first_item() {
        render_with(0, 0);
    }

    #[test]
    fn render_second_menu_last_item() {
        render_with(1, MENU_ITEMS[1].len() - 1);
    }

    #[test]
    fn render_out_of_bounds_menu() {
        render_with(99, 0);
    }

    #[test]
    fn render_out_of_bounds_item() {
        render_with(0, 999);
    }

    #[test]
    fn render_each_menu_each_item() {
        for (m, items) in MENU_ITEMS.iter().enumerate() {
            for it in 0..items.len() {
                render_with(m, it);
            }
        }
    }

    #[test]
    fn scroll_moves_visible_items() {
        let backend = TestBackend::new(30, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let completed = terminal
            .draw(|f| {
                let menu_bar = Rect::new(0, 0, 30, 1);
                render_menu_bar(f, menu_bar, 1, 8, &DEFAULT_COLORS);
            })
            .unwrap();
        let buf = completed.buffer;
        let area = buf.area();
        let mut rows = Vec::new();
        for y in area.y..area.y + area.height {
            let row: String = (area.x..area.x + area.width)
                .map(|x| {
                    buf.cell(ratatui::layout::Position::new(x, y))
                        .map_or(" ", |c| c.symbol())
                })
                .collect();
            rows.push(row);
        }
        let rendered = rows.join("\n");
        assert!(
            rendered.contains("Chmod"),
            "selected item should be visible"
        );
        assert!(
            rendered.contains("Rename"),
            "item before selected should be visible"
        );
        assert!(
            !rendered.contains("User menu"),
            "first item should be scrolled out"
        );
    }

    #[test]
    fn render_with_tiny_terminal() {
        let backend = TestBackend::new(20, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let menu_bar = Rect::new(0, 0, 20, 1);
                render_menu_bar(f, menu_bar, 1, 5, &DEFAULT_COLORS);
            })
            .unwrap();
    }
}
