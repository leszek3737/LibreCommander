use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use std::cell::RefCell;

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;

thread_local! {
    /// Reusable scratch buffer for the input field's visible window,
    /// avoiding a per-frame allocation. Safe because rendering is
    /// single-threaded and the buffer is cleared at the start of each use.
    static INPUT_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    /// Reusable scratch buffer for the "N files selected" sources line.
    static SOURCES_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Style for a single dialog button, highlighted when `selected`.
fn button_style(selected: bool, colors: &ColorPalette) -> ratatui::style::Style {
    if selected {
        Theme::highlight_bold_with_colors(colors)
    } else {
        Theme::dialog_with_colors(colors)
    }
}

pub fn render_archive_extract_dialog(
    f: &mut Frame,
    area: Rect,
    info: &str,
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
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let info_line = Line::from(Span::styled(info, Theme::highlight_with_colors(colors)));
    f.render_widget(Paragraph::new(info_line), chunks[0]);

    render_input_field(f, chunks[1], dest_value, dest_cursor, colors);

    let buttons = [
        (button_style(selection == 0, colors), "[ OK ]"),
        (button_style(selection == 1, colors), "[ Cancel ]"),
    ];
    render_button_row(f, chunks[2], &buttons);
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

    SOURCES_BUF.with_borrow_mut(|buf| {
        use std::fmt::Write as _;
        buf.clear();
        // Writing into a String never fails; the result is ignored.
        let _ = write!(buf, "{source_count} files selected");
        let sources_line = Line::from(vec![
            Span::styled("Sources: ", Theme::dialog_with_colors(colors)),
            Span::styled(buf.as_str(), Theme::highlight_with_colors(colors)),
        ]);
        f.render_widget(Paragraph::new(sources_line), chunks[0]);
    });

    let dest_label = Paragraph::new(Line::from(Span::styled(
        "Archive name:",
        Theme::dialog_with_colors(colors),
    )));
    f.render_widget(dest_label, chunks[1]);

    render_input_field(f, chunks[2], dest_value, dest_cursor, colors);

    let buttons = [
        (button_style(selection == 0, colors), "[ OK ]"),
        (button_style(selection == 1, colors), "[ Cancel ]"),
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

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
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
        // `Block::inner` can return coordinates outside `area` when the area is
        // too small to hold its borders (width/height <= 1), which would place
        // the terminal cursor off-frame. Clamp into `area`; skip entirely for a
        // degenerate (empty) area.
        if area.width > 0 && area.height > 0 {
            let cursor_x = input_inner.x.min(area.x + area.width - 1);
            let cursor_y = input_inner.y.min(area.y + area.height - 1);
            f.set_cursor_position((cursor_x, cursor_y));
        }
        return;
    }

    let window = super::input::compute_visible_window(value, cursor_pos, visible_width);
    // Keep the cursor on the last interior column when a wide grapheme would
    // otherwise land it on the border (`visible_width >= 1` here).
    let cursor_col = window.cursor_col.min(visible_width.saturating_sub(1));

    INPUT_BUF.with_borrow_mut(|buf| {
        buf.clear();
        buf.push_str(&window.text);
        let input_paragraph = Paragraph::new(buf.as_str()).block(input_block);
        // `render_widget` consumes `input_paragraph`, dropping the shared borrow of `buf`
        // synchronously here — before `set_cursor_position` below touches the closure scope.
        f.render_widget(input_paragraph, area);

        let cursor_x = input_inner.x + cursor_col as u16;
        f.set_cursor_position((cursor_x, input_inner.y));
    });
}

fn render_button_row(f: &mut Frame, area: Rect, buttons: &[(ratatui::style::Style, &str)]) {
    let mut spans: Vec<Span> = Vec::with_capacity(buttons.len());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::{Position, Rect};
    use ratatui::widgets::{Block, Borders};

    // Scroll/window math lives in `input::compute_visible_window`; archive keeps
    // render-level tests that the cursor never lands outside the input area.

    fn render_input_cursor(area: Rect, value: &str, cursor_pos: usize) -> Position {
        let w = area.x + area.width + 2;
        let h = area.y + area.height + 2;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_input_field(f, area, value, cursor_pos, &DEFAULT_COLORS))
            .unwrap();
        terminal.get_cursor_position().unwrap()
    }

    #[test]
    fn render_wide_grapheme_cursor_stays_inside_inner() {
        // inner width 4; CJK value of total width 6 with the cursor at the end.
        // Shared `compute_visible_window` anchors on whole graphemes, so the
        // cursor sits past the last visible cluster, still inside the inner area.
        let area = Rect::new(0, 0, 6, 3);
        let inner = Block::default().borders(Borders::ALL).inner(area);
        let pos = render_input_cursor(area, "你好你", 3);
        let last_interior = inner.x + inner.width - 1;
        assert!(
            pos.x >= inner.x && pos.x <= last_interior,
            "cursor x {} outside inner [{}, {}]",
            pos.x,
            inner.x,
            last_interior
        );
    }

    #[test]
    fn render_zero_width_inner_clamps_cursor_into_area() {
        // area.width == 1 -> inner width 0; cursor must be clamped into `area`
        // (column 0) instead of landing on the off-frame inner.x.
        let area = Rect::new(0, 0, 1, 3);
        let pos = render_input_cursor(area, "abc", 2);
        assert_eq!(pos.x, 0);
        assert!(pos.y >= area.y && pos.y < area.y + area.height);
    }
}
