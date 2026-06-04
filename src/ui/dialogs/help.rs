use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::{dialog_block, help_dialog_content_rect};
use super::text::wrapped_line_count;

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

    let narrow_width = content_area.width.saturating_sub(1);
    let likely_total = wrapped_line_count(message, narrow_width);
    let likely_scrollable = likely_total > max_lines;
    let message_area = if likely_scrollable && content_area.width > 1 {
        Rect::new(
            content_area.x,
            content_area.y,
            narrow_width,
            content_area.height,
        )
    } else {
        content_area
    };
    let total_lines = if message_area.width == narrow_width {
        likely_total
    } else {
        wrapped_line_count(message, message_area.width)
    };
    let clamped_offset = scroll_offset.min(total_lines.saturating_sub(max_lines));

    let message_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .scroll((u16::try_from(clamped_offset).unwrap_or(u16::MAX), 0))
        .alignment(Alignment::Left)
        .style(Theme::info_with_colors(colors));
    f.render_widget(message_paragraph, message_area);

    let has_scrollbar = total_lines > max_lines && content_area.width > 1;
    if has_scrollbar {
        let scrollbar_area = Rect::new(
            content_area.x + content_area.width.saturating_sub(1),
            content_area.y,
            1,
            content_area.height,
        );
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .viewport_content_length(max_lines)
            .position(clamped_offset);
        let scrollbar_color = Theme::scrollbar_active_with_colors(colors);
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
    let ok_btn = Paragraph::new("[ Press any key to exit, Arrows/PgUp/PgDn to scroll ]")
        .style(Theme::highlight_bold_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(ok_btn, button_area);
}
