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
        .border_type(BorderType::Plain);
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

    // 1) Scroll-window calc.
    let (scroll_display, cursor_display) = input_scroll_offset(value, cursor_pos, visible_width);

    INPUT_BUF.with_borrow_mut(|buf| {
        buf.clear();
        // 2) Render: collect the visible window into the reusable buffer.
        let (_vis_width, start_cum) =
            collect_visible_graphemes(value, scroll_display, visible_width, buf);
        let input_paragraph = Paragraph::new(buf.as_str()).block(input_block);
        // `render_widget` consumes `input_paragraph`, dropping the shared borrow of `buf`
        // synchronously here — before `set_cursor_position` below touches the closure scope.
        f.render_widget(input_paragraph, area);

        // 3) Cursor calc (clamped to stay inside the inner area).
        let cursor_col = input_cursor_col(cursor_display, start_cum, visible_width);
        let cursor_x = input_inner.x + cursor_col as u16;
        f.set_cursor_position((cursor_x, input_inner.y));
    });
}

/// Computes the horizontal scroll offset (in display columns) and the cursor's
/// display column for an input field of the given visible width.
///
/// Single pass over graphemes: an out-of-range `cursor_pos` is implicitly
/// clamped because iteration stops at the end of the text.
fn input_scroll_offset(value: &str, cursor_pos: usize, visible_width: usize) -> (usize, usize) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut cursor_display = 0usize;
    for (i, g) in value.graphemes(true).enumerate() {
        if i >= cursor_pos {
            break;
        }
        cursor_display += UnicodeWidthStr::width(g);
    }
    // Keep the cursor within the last column of the visible window.
    let scroll_display = cursor_display.saturating_sub(visible_width.saturating_sub(1));
    (scroll_display, cursor_display)
}

/// Computes the cursor's column within the inner input area.
///
/// `start_cum` is the cumulative display width at the first visible grapheme; it
/// can be smaller than `scroll_display` when a wide (e.g. CJK, width-2)
/// grapheme straddles the scroll boundary, which would otherwise push the cursor
/// onto or past the right border column. Clamping to `visible_width - 1` keeps
/// it on the last interior column (`visible_width >= 1` is guaranteed by the
/// caller's guard).
fn input_cursor_col(cursor_display: usize, start_cum: usize, visible_width: usize) -> usize {
    let col = cursor_display.saturating_sub(start_cum);
    col.min(visible_width.saturating_sub(1))
}

/// Fills `buf` with the graphemes visible in the scroll window and returns
/// `(visible_width, start_cum)`, where `start_cum` is the cumulative display
/// width at the first visible grapheme. `buf` is assumed to be empty on entry.
///
/// When a wide grapheme straddles `scroll_display`, the whole grapheme is shown
/// and `start_cum` reflects its true (pre-grapheme) start, which the cursor
/// calc accounts for.
fn collect_visible_graphemes(
    value: &str,
    scroll_display: usize,
    visible_width: usize,
    buf: &mut String,
) -> (usize, usize) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    if scroll_display == 0 {
        let vis_width = collect_graphemes_up_to_width(value, visible_width, buf);
        return (vis_width, 0);
    }

    let mut vis_width = 0usize;
    let mut start_cum = 0usize;
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
            buf.push_str(g);
            vis_width += gw;
        }
    }
    if !found_start {
        // Cursor/scroll past the end of the text: fall back to the start.
        let vis_width = collect_graphemes_up_to_width(value, visible_width, buf);
        return (vis_width, 0);
    }

    (vis_width, start_cum)
}

fn collect_graphemes_up_to_width(value: &str, max_width: usize, buf: &mut String) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut width = 0;
    for g in value.graphemes(true) {
        let gw = UnicodeWidthStr::width(g);
        if width + gw > max_width {
            break;
        }
        buf.push_str(g);
        width += gw;
    }
    width
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

    // --- collect_graphemes_up_to_width: unicode width handling ---

    #[test]
    fn up_to_width_ascii_truncates_by_columns() {
        let mut buf = String::new();
        let w = collect_graphemes_up_to_width("hello", 3, &mut buf);
        assert_eq!(buf, "hel");
        assert_eq!(w, 3);
    }

    #[test]
    fn up_to_width_wide_cjk_does_not_split_grapheme() {
        // Each CJK char is width 2; a budget of 3 fits only one.
        let mut buf = String::new();
        let w = collect_graphemes_up_to_width("你好", 3, &mut buf);
        assert_eq!(buf, "你");
        assert_eq!(w, 2);
    }

    #[test]
    fn up_to_width_emoji_is_width_two() {
        let mut buf = String::new();
        let w = collect_graphemes_up_to_width("😀x", 2, &mut buf);
        assert_eq!(buf, "😀");
        assert_eq!(w, 2);
    }

    #[test]
    fn up_to_width_combining_mark_counts_as_one_column() {
        // "e" + combining acute is a single grapheme of display width 1.
        let mut buf = String::new();
        let w = collect_graphemes_up_to_width("e\u{0301}llo", 2, &mut buf);
        assert_eq!(buf, "e\u{0301}l");
        assert_eq!(w, 2);
    }

    // --- collect_visible_graphemes: scroll window ---

    #[test]
    fn visible_no_scroll_collects_from_start() {
        let mut buf = String::new();
        let (vis_width, start_cum) = collect_visible_graphemes("hello", 0, 3, &mut buf);
        assert_eq!(buf, "hel");
        assert_eq!((vis_width, start_cum), (3, 0));
    }

    #[test]
    fn visible_wide_grapheme_straddles_scroll_boundary() {
        // value widths: 2,2,2 (cum 0,2,4,6). scroll=3 falls inside the 2nd
        // grapheme, so start_cum (2) is < scroll_display (3).
        let mut buf = String::new();
        let (vis_width, start_cum) = collect_visible_graphemes("你好你", 3, 4, &mut buf);
        assert_eq!(buf, "好你");
        assert_eq!((vis_width, start_cum), (4, 2));
    }

    #[test]
    fn visible_scroll_past_end_falls_back_to_start() {
        let mut buf = String::new();
        let (vis_width, start_cum) = collect_visible_graphemes("ab", 100, 4, &mut buf);
        assert_eq!(buf, "ab");
        assert_eq!((vis_width, start_cum), (2, 0));
    }

    // --- input_scroll_offset ---

    #[test]
    fn scroll_offset_wide_cursor_at_end() {
        // cursor_display = 6, visible_width 4 -> scroll = 6 - 3 = 3.
        assert_eq!(input_scroll_offset("你好你", 3, 4), (3, 6));
    }

    #[test]
    fn scroll_offset_out_of_range_cursor_is_clamped() {
        assert_eq!(input_scroll_offset("ab", 99, 4), (0, 2));
    }

    #[test]
    fn scroll_offset_empty_value() {
        assert_eq!(input_scroll_offset("", 0, 4), (0, 0));
    }

    // --- input_cursor_col: wide-grapheme clamp at the scroll boundary (bug 1) ---

    #[test]
    fn cursor_col_clamped_off_the_border() {
        // Without clamping, col would be 4 == visible_width (the border column);
        // it must be clamped to visible_width - 1 = 3.
        assert_eq!(input_cursor_col(6, 2, 4), 3);
    }

    #[test]
    fn cursor_col_within_window_is_unchanged() {
        assert_eq!(input_cursor_col(2, 0, 4), 2);
    }

    #[test]
    fn cursor_col_single_column_window() {
        assert_eq!(input_cursor_col(0, 0, 1), 0);
    }

    // --- render-level integration: cursor never lands off the inner area ---

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
        assert_eq!(pos.x, last_interior);
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
