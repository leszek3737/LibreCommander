use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::menu::{MENU_ITEMS, MENU_TITLES, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::theme::Theme;

const MIN_DROPDOWN_ITEM_WIDTH: usize = 10;

pub fn render_menu_dropdown(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    selected_item: usize,
) {
    for (i, title) in MENU_TITLES.iter().enumerate() {
        let title_width = menu_title_width(title);
        let style = if i == selected_menu {
            Theme::highlight_bold()
        } else {
            Theme::menu_bar()
        };
        let label = format!(" {title} ");
        let p = Paragraph::new(label).style(style);
        let area = Rect::new(
            menu_bar_area.x + menu_title_x(menu_bar_area.width, i),
            menu_bar_area.y,
            title_width,
            1,
        );
        f.render_widget(p, area);
    }

    if MENU_ITEMS.is_empty() {
        return;
    }

    let selected_menu = selected_menu.min(MENU_ITEMS.len().saturating_sub(1));
    let items = MENU_ITEMS[selected_menu];
    let dropdown_width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(*s))
        .max()
        .unwrap_or(MIN_DROPDOWN_ITEM_WIDTH) as u16
        + 4;
    let dropdown_height = (items.len().min(u16::MAX as usize - 2)) as u16 + 2;

    let dropdown_y = menu_bar_area.y + 1;
    let dropdown_x = menu_dropdown_x(menu_bar_area, selected_menu, dropdown_width);
    let dropdown_area = Rect::new(dropdown_x, dropdown_y, dropdown_width, dropdown_height);

    f.render_widget(Clear, dropdown_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::panel_fg())
        .style(Theme::panel_bg());
    let inner = block.inner(dropdown_area);
    f.render_widget(block, dropdown_area);

    for (i, item) in items.iter().enumerate() {
        if i >= inner.height as usize {
            break;
        }
        let style = if i == selected_item {
            Theme::highlight()
        } else {
            Theme::panel()
        };
        let item_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let p = Paragraph::new(format!(" {item} ")).style(style);
        f.render_widget(p, item_area);
    }
}
