use std::sync::OnceLock;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Borders, Clear},
};
use unicode_width::UnicodeWidthStr;

use crate::menu::{MENUS, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::theme::{ColorPalette, Theme};

/// Number of top-level menus (compile-time constant length of `MENUS`).
const MENU_COUNT: usize = MENUS.len();

/// Fallback content width for a dropdown whose menu has no items
/// (so `Iterator::max` yields `None`).
const MIN_DROPDOWN_ITEM_WIDTH: usize = 10;

/// Extra width added to the widest item to size a dropdown box:
/// 2 columns for the left/right borders + 2 for the one-space padding each side.
const MENU_PADDING_WIDTH: u16 = 4;

/// Extra height added to the item count to size a dropdown box:
/// the top and bottom border rows.
const MENU_DROPDOWN_OFFSET: u16 = 2;

/// Writes ` text ` (one leading + trailing space) at `(x, y)` in a single
/// `style`, clipped to `max_width` columns. Writing straight into the buffer
/// avoids the per-item `Vec<Span>` + `Paragraph` allocation that building a
/// `Line` would require (`Line` always wraps a heap `Vec<Span>`).
fn render_padded_text(buf: &mut Buffer, x: u16, y: u16, text: &str, max_width: u16, style: Style) {
    let end_x = x.saturating_add(max_width);
    let mut cx = buf.set_stringn(x, y, " ", max_width as usize, style).0;
    cx = buf
        .set_stringn(cx, y, text, end_x.saturating_sub(cx) as usize, style)
        .0;
    buf.set_stringn(cx, y, " ", end_x.saturating_sub(cx) as usize, style);
}

/// Widest dropdown item, per menu index, computed once and cached.
/// `UnicodeWidthStr::width` is not `const`, so the table is built lazily on
/// first use and then reused every frame the dropdown opens, instead of
/// rescanning all items each time.
fn dropdown_item_max_widths() -> &'static [usize; MENU_COUNT] {
    static WIDTHS: OnceLock<[usize; MENU_COUNT]> = OnceLock::new();
    WIDTHS.get_or_init(|| {
        let mut widths = [MIN_DROPDOWN_ITEM_WIDTH; MENU_COUNT];
        for (i, entry) in MENUS.iter().enumerate() {
            if let Some(max) = entry.items.iter().map(|s| UnicodeWidthStr::width(*s)).max() {
                widths[i] = max;
            }
        }
        widths
    })
}

fn render_menu_title_bar(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    colors: &ColorPalette,
) {
    let y = menu_bar_area.y;
    let base_x = menu_bar_area.x;
    let bar_width = menu_bar_area.width;
    let buf = f.buffer_mut();
    for (i, entry) in MENUS.iter().enumerate() {
        let title = entry.title;
        let title_width = menu_title_width(title);
        let style = if i == selected_menu {
            Theme::highlight_bold_with_colors(colors)
        } else {
            Theme::menu_bar_with_colors(colors)
        };
        let x = base_x + menu_title_x(bar_width, i);
        render_padded_text(buf, x, y, title, title_width, style);
    }
}

fn render_menu_dropdown(
    f: &mut Frame,
    menu_bar_area: Rect,
    active_menu: usize,
    selected_item: usize,
    colors: &ColorPalette,
) {
    let items = MENUS[active_menu].items;
    // Widest item width is precomputed once; `try_from(..).unwrap_or(u16::MAX)`
    // clamps a pathologically wide menu instead of overflowing u16.
    let dropdown_width = u16::try_from(dropdown_item_max_widths()[active_menu])
        .unwrap_or(u16::MAX)
        .saturating_add(MENU_PADDING_WIDTH);
    let dropdown_y = menu_bar_area.y + 1;
    let max_visible = f.area().height.saturating_sub(dropdown_y);
    if max_visible < 2 {
        return;
    }
    // Cap the item count before adding the two border rows so the `+ OFFSET`
    // cannot overflow u16, then clamp the box to the available screen height.
    let dropdown_height = ((items
        .len()
        .min(u16::MAX as usize - MENU_DROPDOWN_OFFSET as usize)) as u16
        + MENU_DROPDOWN_OFFSET)
        .min(max_visible);
    let dropdown_x = menu_dropdown_x(menu_bar_area, active_menu, dropdown_width);
    let dropdown_area = Rect::new(dropdown_x, dropdown_y, dropdown_width, dropdown_height);

    f.render_widget(Clear, dropdown_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::panel_fg_with_colors(colors))
        .style(Theme::panel_bg_with_colors(colors));
    let inner = block.inner(dropdown_area);
    f.render_widget(block, dropdown_area);

    let clamped_selected = selected_item.min(items.len().saturating_sub(1));
    let visible_items = inner.height as usize;
    // When the list is taller than the viewport, scroll just enough to keep the
    // selected item as the last visible row (it is the lowest item that must
    // stay on screen); otherwise show from the top.
    let scroll_offset = if items.len() <= visible_items {
        0
    } else {
        clamped_selected.saturating_sub(visible_items.saturating_sub(1))
    };

    let item_x = inner.x;
    let item_y = inner.y;
    let item_width = inner.width;
    let buf = f.buffer_mut();
    for (i, item) in items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_items)
    {
        let row = i - scroll_offset;
        let style = if i == clamped_selected {
            Theme::highlight_with_colors(colors)
        } else {
            Theme::panel_with_colors(colors)
        };
        render_padded_text(buf, item_x, item_y + row as u16, item, item_width, style);
    }
}

pub fn render_menu_bar_with_colors(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    selected_item: usize,
    colors: &ColorPalette,
) {
    let selected_menu = selected_menu.min(MENUS.len().saturating_sub(1));
    render_menu_title_bar(f, menu_bar_area, selected_menu, colors);
    if !MENUS[selected_menu].items.is_empty() {
        render_menu_dropdown(f, menu_bar_area, selected_menu, selected_item, colors);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    fn render_with(menu: usize, item: usize) {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let menu_bar = Rect::new(0, 0, 80, 1);
                render_menu_bar_with_colors(f, menu_bar, menu, item, &ColorPalette::default());
            })
            .unwrap();
    }

    #[test]
    fn render_first_menu_first_item() {
        render_with(0, 0);
    }

    #[test]
    fn render_second_menu_last_item() {
        render_with(1, MENUS[1].items.len() - 1);
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
        for (m, entry) in MENUS.iter().enumerate() {
            let items = entry.items;
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
                render_menu_bar_with_colors(f, menu_bar, 1, 8, &ColorPalette::default());
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
                render_menu_bar_with_colors(f, menu_bar, 1, 5, &ColorPalette::default());
            })
            .unwrap();
    }

    #[test]
    fn dropdown_item_widths_match_live_computation() {
        let cached = dropdown_item_max_widths();
        for (i, entry) in MENUS.iter().enumerate() {
            let live = entry
                .items
                .iter()
                .map(|s| UnicodeWidthStr::width(*s))
                .max()
                .unwrap_or(MIN_DROPDOWN_ITEM_WIDTH);
            assert_eq!(cached[i], live, "menu {i}");
        }
    }

    #[test]
    fn dropdown_width_overflow_saturates() {
        // Pathologically wide content must clamp to u16::MAX without overflowing.
        let width = u16::try_from(usize::MAX)
            .unwrap_or(u16::MAX)
            .saturating_add(MENU_PADDING_WIDTH);
        assert_eq!(width, u16::MAX);
    }
}
