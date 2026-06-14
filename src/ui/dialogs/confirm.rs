use std::borrow::Cow;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;
use super::text::truncate_path;

const LIST_PADDING: &str = "  ";

/// Minimum rows reserved for the wrapped message paragraph.
const MESSAGE_MIN_ROWS: u16 = 2;

/// Rows reserved for the single button line at the bottom.
const BUTTON_ROWS: u16 = 1;

/// Rows of the dialog body that are always present (message + button line).
/// Whatever height remains after these may be used by the file list.
const RESERVED_NON_FILE_ROWS: u16 = MESSAGE_MIN_ROWS + BUTTON_ROWS;

/// Build a padded list row with the given style.
///
/// Centralizes the `LIST_PADDING + content` layout shared by every row of the
/// file list (named entries and the trailing "+N more" summary).
fn padded_line<'a>(content: impl Into<Cow<'a, str>>, style: Style) -> Line<'a> {
    Line::from(vec![Span::raw(LIST_PADDING), Span::raw(content)]).style(style)
}

/// Number of rows the file list may occupy, clamped to `max_rows`.
///
/// Uses a saturating conversion so that huge file counts (>= 65536) cannot wrap
/// to zero via `as u16` and make the whole list disappear.
fn clamp_file_rows(len: usize, max_rows: u16) -> u16 {
    u16::try_from(len).unwrap_or(u16::MAX).min(max_rows)
}

/// Style of a dialog button: highlighted when selected, plain otherwise.
fn button_style(selection: usize, idx: usize, colors: &ColorPalette) -> Style {
    if selection == idx {
        Theme::highlight_bold_with_colors(colors)
    } else {
        Theme::dialog_with_colors(colors)
    }
}

fn render_file_list(
    f: &mut Frame,
    area: Rect,
    files: &[impl AsRef<str>],
    max_name_width: usize,
    colors: &ColorPalette,
) {
    let max_visible = area.height as usize;
    if max_visible == 0 {
        return;
    }
    // Hoisted out of the per-file loop: the style is identical for every row.
    let warning_style = Theme::warning_with_colors(colors);
    let mut lines: Vec<Line> = Vec::with_capacity(max_visible);
    if files.len() <= max_visible {
        for name in files {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(padded_line(display, warning_style));
        }
    } else {
        let file_slots = max_visible.saturating_sub(1);
        for name in files.iter().take(file_slots) {
            let display = truncate_path(name.as_ref(), max_name_width);
            lines.push(padded_line(display, warning_style));
        }
        let remaining = files.len() - file_slots;
        lines.push(padded_line(format!("... +{remaining} more"), warning_style));
    }
    let file_paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(file_paragraph, area);
}

pub(super) fn render_confirmation_dialog_inner(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    buttons: &[(Style, &str)],
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_files = !files.is_empty();
    let max_rows = inner.height.saturating_sub(RESERVED_NON_FILE_ROWS);
    let file_rows = if has_files {
        clamp_file_rows(files.len(), max_rows)
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(MESSAGE_MIN_ROWS),
            Constraint::Length(file_rows),
            Constraint::Length(BUTTON_ROWS),
        ])
        .split(inner);

    let msg_paragraph = Paragraph::new(message)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    if has_files {
        let max_name_width = inner.width.saturating_sub(LIST_PADDING.len() as u16) as usize;
        render_file_list(f, chunks[1], files, max_name_width, colors);
    }

    let mut spans: Vec<Span> = Vec::with_capacity(buttons.len() * 2);
    for (i, (style, label)) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*label, *style));
    }
    let btn_line = Line::from(spans);
    let btn_paragraph = Paragraph::new(btn_line).alignment(Alignment::Center);
    f.render_widget(btn_paragraph, chunks[2]);
}

pub fn render_confirm_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    selection: usize,
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    let buttons = [
        (button_style(selection, 0, colors), "[ Yes ]"),
        (button_style(selection, 1, colors), "[ No ]"),
    ];
    render_confirmation_dialog_inner(f, area, title, message, &buttons, files, colors);
}

pub fn render_overwrite_dialog(
    f: &mut Frame,
    area: Rect,
    selection: usize,
    files: &[impl AsRef<str>],
    colors: &ColorPalette,
) {
    if files.is_empty() {
        return;
    }

    let msg: Cow<'_, str> = if files.len() == 1 {
        Cow::Borrowed("File already exists at destination:")
    } else {
        Cow::Owned(format!(
            "{} files already exist at destination:",
            files.len()
        ))
    };

    let buttons = [
        (button_style(selection, 0, colors), "[ Overwrite All ]"),
        (button_style(selection, 1, colors), "[ Cancel ]"),
    ];
    render_confirmation_dialog_inner(f, area, "Overwrite?", &msg, &buttons, files, colors);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const W: u16 = 40;
    const H: u16 = 12;

    /// Render a confirm dialog into a valid backend; the supplied `area` is the
    /// dialog rect under test. Returns the flattened buffer text.
    fn draw_confirm(area: Rect, files: &[&str]) -> String {
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal
            .draw(|f| {
                render_confirm_dialog(f, area, "Title", "Message", 0, files, &DEFAULT_COLORS);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..H {
            for x in 0..W {
                rendered.push_str(buffer[(x, y)].symbol());
            }
        }
        rendered
    }

    #[test]
    fn clamp_file_rows_saturates_instead_of_wrapping() {
        // Within range: passed through, clamped to the available rows.
        assert_eq!(clamp_file_rows(0, 10), 0);
        assert_eq!(clamp_file_rows(5, 10), 5);
        assert_eq!(clamp_file_rows(20, 10), 10);
        // 65536 wraps to 0 under `as u16`; the saturating path must not.
        assert_eq!(clamp_file_rows(65_536, 10), 10);
        assert_eq!(clamp_file_rows(70_000, u16::MAX), u16::MAX);
        assert_eq!(clamp_file_rows(usize::MAX, 100), 100);
        // Zero available rows always yields zero, regardless of count.
        assert_eq!(clamp_file_rows(usize::MAX, 0), 0);
    }

    #[test]
    fn empty_file_list_renders_without_panic() {
        let files: &[&str] = &[];
        let rendered = draw_confirm(Rect::new(0, 0, W, H), files);
        // Message and a button label must both appear even with no file list.
        assert!(
            rendered.contains("Message"),
            "message paragraph must render"
        );
        assert!(
            rendered.contains("[ Yes ]"),
            "button line must render when file list is empty"
        );
    }

    #[test]
    fn zero_width_area_does_not_panic() {
        // A zero-width render area leaves the 40×12 buffer completely untouched
        // (all cells remain the initial space).  Any non-space symbol would mean
        // content leaked outside the clipped area — a regression.
        let rendered = draw_confirm(Rect::new(0, 0, 0, H), &["a.txt", "b.txt"]);
        assert!(
            rendered.chars().all(|c| c == ' '),
            "zero-width area must leave the buffer blank — no content should be written"
        );
    }

    #[test]
    fn zero_height_area_does_not_panic() {
        // A zero-height render area must similarly leave the buffer completely blank.
        let rendered = draw_confirm(Rect::new(0, 0, W, 0), &["a.txt", "b.txt"]);
        assert!(
            rendered.chars().all(|c| c == ' '),
            "zero-height area must leave the buffer blank — no content should be written"
        );
    }

    #[test]
    fn huge_file_count_does_not_wrap_or_panic() {
        // Far beyond u16::MAX: the row count must not wrap to zero, so the list
        // region stays non-empty and renders its "+N more" summary row.
        let files = vec!["f.txt"; 70_000];
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal
            .draw(|f| {
                render_confirm_dialog(f, f.area(), "T", "M", 0, &files, &DEFAULT_COLORS);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..H {
            for x in 0..W {
                rendered.push_str(buffer[(x, y)].symbol());
            }
        }
        assert!(
            rendered.contains("more"),
            "file list should render a summary row, not vanish"
        );
    }

    #[test]
    fn overwrite_dialog_renders_without_panic() {
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal
            .draw(|f| {
                render_overwrite_dialog(f, f.area(), 0, &["a.txt", "b.txt"], &DEFAULT_COLORS);
            })
            .unwrap();
    }
}
