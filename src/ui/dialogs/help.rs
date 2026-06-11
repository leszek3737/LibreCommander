use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::{dialog_block, help_dialog_content_rect};
use super::text::wrapped_line_count;

const HELP_SCROLL_HINT: &str = "[ Press any key to exit, Arrows/PgUp/PgDn to scroll ]";

pub fn render_help_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    scroll_offset: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::help_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content_area = help_dialog_content_rect(area);
    let max_lines = content_area.height as usize;
    let has_room = content_area.width > 1;
    let narrow_width = content_area.width.saturating_sub(1);

    let narrow_line_count = wrapped_line_count(message, narrow_width);
    let show_scrollbar = narrow_line_count > max_lines && has_room;

    let message_area = if show_scrollbar {
        Rect::new(
            content_area.x,
            content_area.y,
            narrow_width,
            content_area.height,
        )
    } else {
        content_area
    };

    let total_lines = if show_scrollbar || has_room {
        narrow_line_count
    } else {
        wrapped_line_count(message, content_area.width)
    };

    let clamped_offset = scroll_offset
        .min(total_lines.saturating_sub(max_lines))
        .min(u16::MAX as usize);

    let message_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .scroll((clamped_offset as u16, 0))
        .alignment(Alignment::Left)
        .style(Theme::info_with_colors(colors));
    f.render_widget(message_paragraph, message_area);

    if show_scrollbar {
        let scrollbar_area = Rect::new(
            content_area.x + content_area.width.saturating_sub(1),
            content_area.y,
            1,
            content_area.height,
        );
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .viewport_content_length(max_lines)
            .position(clamped_offset);
        let scrollbar_color = colors.scrollbar_active;
        let sb_style = Style::default().fg(scrollbar_color);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_symbol("█")
                .track_symbol(Some("░"))
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(sb_style)
                .track_style(sb_style),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }

    let button_area = Rect::new(
        inner.x,
        inner.y + inner.height.saturating_sub(1),
        inner.width,
        1,
    );
    let ok_btn = Paragraph::new(HELP_SCROLL_HINT)
        .style(Theme::highlight_bold_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, button_area);
}
