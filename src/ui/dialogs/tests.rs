use std::borrow::Cow;

use super::*;
use crate::ui::theme::DEFAULT_COLORS;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::widgets::{Block, Borders};

#[test]
fn test_centered_rect_basic() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 50);
    let rect = centered_rect(50, 50, area);
    assert_eq!(rect.width, 50);
    assert_eq!(rect.height, 25);
    assert_eq!(rect.x, 25);
    assert_eq!(rect.y, 12);
}

#[test]
fn test_centered_rect_full() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 100);
    let rect = centered_rect(100, 100, area);
    assert_eq!(rect.width, 100);
    assert_eq!(rect.height, 100);
    assert_eq!(rect.x, 0);
    assert_eq!(rect.y, 0);
}

#[test]
fn test_centered_rect_with_offset() {
    let area = ratatui::layout::Rect::new(10, 5, 80, 40);
    let rect = centered_rect(50, 50, area);
    assert_eq!(rect.width, 40);
    assert_eq!(rect.height, 20);
    assert_eq!(rect.x, 30);
    assert_eq!(rect.y, 15);
}

#[test]
fn help_visible_height_matches_render_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let visible = help_visible_height(area);
    let dialog_area = centered_rect(50, 40, area);
    let inner = Block::default().borders(Borders::ALL).inner(dialog_area);
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(3),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);

    assert_eq!(visible, chunks[0].height as usize);
}

#[test]
fn help_scrollbar_does_not_overwrite_text_area() {
    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    let message = (0..20)
        .map(|i| format!("line-{i:02}-abcdef"))
        .collect::<Vec<_>>()
        .join("\n");

    terminal
        .draw(|f| {
            let area = centered_rect(50, 40, f.area());
            render_help_dialog(f, area, "Help", &message, 0, &DEFAULT_COLORS);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let area = centered_rect(50, 40, ratatui::layout::Rect::new(0, 0, 40, 12));
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(3),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);
    let text_last_x = chunks[0].x + chunks[0].width - 2;
    let scrollbar_x = chunks[0].x + chunks[0].width - 1;

    assert_ne!(buffer[(text_last_x, chunks[0].y)].symbol(), "█");
    assert_ne!(buffer[(text_last_x, chunks[0].y)].symbol(), "░");
    assert!(matches!(
        buffer[(scrollbar_x, chunks[0].y)].symbol(),
        "█" | "░"
    ));
}

#[test]
fn help_scrollbar_reserved_for_wrapped_single_line() {
    let message = "abcdefghijklmnopqrstuvwxyz".repeat(5);
    let area = centered_rect(50, 40, ratatui::layout::Rect::new(0, 0, 40, 12));
    let content = layout::help_dialog_content_rect(area);
    let text_last_x = content.x + content.width - 2;
    let scrollbar_x = content.x + content.width - 1;

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|f| render_help_dialog(f, area, "Help", &message, 0, &DEFAULT_COLORS))
        .unwrap();
    let unscrolled_start = terminal.backend().buffer()[(content.x, content.y)]
        .symbol()
        .to_owned();

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|f| render_help_dialog(f, area, "Help", &message, 1, &DEFAULT_COLORS))
        .unwrap();
    let buffer = terminal.backend().buffer();

    assert_ne!(buffer[(content.x, content.y)].symbol(), unscrolled_start);
    assert_ne!(buffer[(text_last_x, content.y)].symbol(), "█");
    assert_ne!(buffer[(text_last_x, content.y)].symbol(), "░");
    assert!(matches!(
        buffer[(scrollbar_x, content.y)].symbol(),
        "█" | "░"
    ));
}

#[test]
fn wrapped_line_count_long_line_narrow_area() {
    let text = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(wrapped_line_count(text, 10), 3);
    // 26 width-1 chars at width 1 wrap to exactly 26 lines.
    assert_eq!(wrapped_line_count(text, 1), 26);
}

#[test]
fn wrapped_line_count_multi_word() {
    assert_eq!(wrapped_line_count("hello world foo bar", 10), 3);
}

#[test]
fn wrapped_line_count_short_line_wide_area() {
    assert_eq!(wrapped_line_count("abc", 80), 1);
}

#[test]
fn wrapped_line_count_empty_text() {
    assert_eq!(wrapped_line_count("", 10), 0);
}

#[test]
fn wrapped_line_count_multiline() {
    let text = "short\nthis is a much longer line that should wrap\nend";
    assert!(wrapped_line_count(text, 20) > text.lines().count());
}

#[test]
fn help_scroll_uses_wrapped_lines() {
    let long_line: String = "x".repeat(200);
    let message = format!("header\n{long_line}\nfooter");
    let width: u16 = 20;
    let total = wrapped_line_count(&message, width);
    assert!(total > 3, "wrapped count {total} should exceed 3 raw lines");
}

#[test]
fn truncate_path_keeps_short_utf8_path() {
    assert_eq!(text::truncate_path("zażółć", 6), "zażółć");
}

#[test]
fn truncate_path_truncates_utf8_suffix_safely() {
    // The dir/file parts are re-joined with the platform separator.
    let expected = format!("...ć{}plik", std::path::MAIN_SEPARATOR);
    assert_eq!(text::truncate_path("/tmp/zażółć/plik", 9), expected);
}

#[test]
fn truncate_path_truncates_tiny_utf8_width_safely() {
    assert_eq!(text::truncate_path("żółć", 3), "żół");
}

#[test]
fn truncate_path_preserves_filename() {
    // The dir/file parts are re-joined with the platform separator.
    let expected = format!("...ath{}file.txt", std::path::MAIN_SEPARATOR);
    assert_eq!(
        text::truncate_path("/very/long/directory/path/file.txt", 15),
        expected
    );
}

#[test]
fn overwrite_dialog_empty_files_returns_early() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|f| {
            render_dialog_with_colors(
                f,
                &DialogKind::OverwriteConfirm {
                    selection: 0,
                    files: Cow::Borrowed(&[]),
                },
                &DEFAULT_COLORS,
            );
        })
        .unwrap();

    // Early return draws nothing, so the buffer stays at its default blank state.
    let all_blank = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .all(|cell| cell.symbol() == " ");
    assert!(
        all_blank,
        "empty-files overwrite dialog must render nothing"
    );

    // Sanity: a non-empty overwrite DOES draw, proving "all blank" has teeth.
    let files = vec!["conflict.txt".to_string()];
    let mut terminal2 = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal2
        .draw(|f| {
            render_dialog_with_colors(
                f,
                &DialogKind::OverwriteConfirm {
                    selection: 0,
                    files: Cow::Borrowed(files.as_slice()),
                },
                &DEFAULT_COLORS,
            );
        })
        .unwrap();
    let drew_something = terminal2
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|cell| cell.symbol() != " ");
    assert!(drew_something, "non-empty overwrite dialog must render");
}

#[test]
fn list_picker_keeps_selected_visible() {
    let area = ratatui::layout::Rect::new(0, 0, 40, 10);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();
    let selected = items.len() - 1;

    terminal
        .draw(|f| {
            render_list_picker_with_colors(
                f,
                "Pick",
                &items,
                selected,
                "hint",
                &crate::ui::theme::ColorPalette::default(),
            )
        })
        .unwrap();

    // Mirror the picker geometry (centered_rect + bordered block + vertical
    // split) to locate the list viewport rect, then assert WHERE the selection
    // lands rather than scanning the whole buffer.
    let picker_area = centered_rect(60, 70, area);
    let inner = Block::default().borders(Borders::ALL).inner(picker_area);
    let list_area = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner)[0];

    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| -> String {
        (list_area.x..list_area.x + list_area.width)
            .filter_map(|x| buffer.cell((x, y)).map(|c| c.symbol()))
            .collect()
    };

    // The selection is the last item, so it scrolls to the bottom viewport row
    // and carries the "> " highlight symbol.
    let selected_row = row_text(list_area.y + list_area.height - 1);
    assert!(
        selected_row.contains("Item 19"),
        "selected item should occupy the last viewport row, got: {selected_row:?}"
    );
    assert!(
        selected_row.trim_start().starts_with("> "),
        "selected viewport row should carry the highlight symbol, got: {selected_row:?}"
    );

    // Items scrolled above the viewport must not render anywhere.
    let whole: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        !whole.contains("Item 0"),
        "Item 0 should be scrolled out of view"
    );
}

#[test]
fn test_render_confirmation_dialog_inner() {
    let backend = TestBackend::new(60, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = centered_rect(50, 40, f.area());
            confirm::render_confirmation_dialog_inner(
                f,
                area,
                "Confirm",
                "Are you sure?",
                &[
                    (
                        Theme::highlight_bold_with_colors(&DEFAULT_COLORS),
                        "[ Yes ]",
                    ),
                    (Theme::dialog_with_colors(&DEFAULT_COLORS), "[ No ]"),
                ],
                &["file1.txt", "file2.txt"] as &[&str],
                &DEFAULT_COLORS,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(rendered.contains("Confirm"), "title should be rendered");
    assert!(
        rendered.contains("Are you sure?"),
        "message should be rendered"
    );
    assert!(
        rendered.contains("[ Yes ]"),
        "yes button should be rendered"
    );
    assert!(rendered.contains("[ No ]"), "no button should be rendered");
    assert!(
        rendered.contains("file1.txt"),
        "file list should show first file"
    );
    assert!(
        rendered.contains("file2.txt"),
        "file list should show second file"
    );
}

#[test]
fn test_render_confirmation_dialog_inner_empty_title_no_files() {
    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = centered_rect(50, 40, f.area());
            confirm::render_confirmation_dialog_inner(
                f,
                area,
                "",
                "msg",
                &[(Theme::dialog_with_colors(&DEFAULT_COLORS), "[ OK ]")],
                &[] as &[&str],
                &DEFAULT_COLORS,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(
        rendered.contains("msg"),
        "message should render with empty title"
    );
    assert!(rendered.contains("[ OK ]"), "single button should render");
}

#[test]
fn test_help_dialog_content_rect() {
    let rect = layout::help_dialog_content_rect(ratatui::layout::Rect::new(0, 0, 40, 20));
    assert_eq!(rect.x, 1, "x should be 1 after border");
    assert_eq!(rect.y, 1, "y should be 1 after border");
    assert_eq!(rect.width, 38, "width should be inner width of dialog");
    assert_eq!(rect.height, 17, "height accounts for border and bottom bar");

    let rect2 = layout::help_dialog_content_rect(ratatui::layout::Rect::new(0, 0, 80, 40));
    assert_eq!(rect2.x, 1);
    assert_eq!(rect2.y, 1);
    assert_eq!(rect2.width, 78);
    assert_eq!(rect2.height, 37);
}

#[test]
fn test_help_dialog_content_rect_small_terminal() {
    let rect = layout::help_dialog_content_rect(ratatui::layout::Rect::new(0, 0, 8, 6));
    assert_eq!(rect.x, 1);
    assert_eq!(rect.y, 1);
    assert_eq!(rect.width, 6);
    assert_eq!(rect.height, 3);

    let rect2 = layout::help_dialog_content_rect(ratatui::layout::Rect::new(0, 0, 3, 3));
    assert_eq!(rect2.x, 1);
    assert_eq!(rect2.y, 1);
    assert_eq!(rect2.width, 1);
    assert_eq!(rect2.height, 1);
}

#[test]
fn test_input_dialog_empty_value_has_warning_border() {
    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = centered_rect(50, 40, f.area());
            render_input_dialog(f, area, "Input", "Enter value:", "", 0, &DEFAULT_COLORS);
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let area = centered_rect(50, 40, ratatui::layout::Rect::new(0, 0, 60, 12));
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(0),
        ])
        .split(inner);

    let input_area = chunks[1];
    let top_left = buf[(input_area.x, input_area.y)].clone();
    let warning_color = Theme::warning_with_colors(&DEFAULT_COLORS)
        .fg
        .unwrap_or(Color::Yellow);
    assert_eq!(
        top_left.fg, warning_color,
        "empty value input border should have warning color"
    );
}

#[test]
fn test_properties_dialog_renders_content() {
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let info = PropertiesInfo {
        name: Cow::Borrowed("/very/long/path/to/some/file.txt"),
        size: Cow::Borrowed("1.2 MB"),
        mtime: Cow::Borrowed("2024-01-15 10:30"),
        permissions: Cow::Borrowed("rw-r--r--"),
        owner: Cow::Borrowed("user"),
        group: Cow::Borrowed("staff"),
        file_type: Cow::Borrowed("Regular File"),
    };

    terminal
        .draw(|f| {
            let area = centered_rect(50, 40, f.area());
            render_properties_dialog(f, area, &info, &DEFAULT_COLORS);
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(
        rendered.contains("Name:"),
        "properties dialog should show file name label"
    );
    assert!(
        rendered.contains("Press Enter or Esc to close"),
        "properties dialog should show close hint"
    );
}
