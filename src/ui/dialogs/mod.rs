use std::borrow::Cow;

use ratatui::{Frame, layout::Rect, widgets::Clear};

use super::theme::{ColorPalette, Theme};

mod archive;
mod confirm;
mod help;
mod input;
mod layout;
mod list_picker;
mod simple;
mod text;

pub use confirm::{render_confirm_dialog, render_overwrite_dialog};
pub use help::render_help_dialog;
pub use input::render_input_dialog;
pub use layout::{centered_rect, help_message_width, help_visible_height, input_dialog_rect};
pub use list_picker::{render_list_picker, render_list_picker_with_colors};
pub use simple::{render_error_dialog, render_progress_dialog, render_properties_dialog};
pub use text::wrapped_line_count;

use layout::{DIALOG_HEIGHT_PERCENT, DIALOG_WIDTH_PERCENT};

#[derive(Debug, Clone)]
pub struct PropertiesInfo {
    pub name: String,
    pub size: String,
    pub mtime: String,
    pub permissions: String,
    pub owner: String,
    pub group: String,
    pub file_type: String,
}

#[derive(Debug, Clone)]
pub enum DialogKind<'a> {
    Confirm {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        selection: usize,
        files: Cow<'a, [String]>,
    },
    Input {
        title: Cow<'a, str>,
        prompt: Cow<'a, str>,
        value: Cow<'a, str>,
        cursor_pos: usize,
    },
    Error {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
    },
    Help {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        scroll_offset: usize,
    },
    Progress {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        percent: f32,
        cancellable: bool,
    },
    Properties {
        info: PropertiesInfo,
    },
    OverwriteConfirm {
        selection: usize,
        files: Cow<'a, [String]>,
    },
    ArchiveExtract {
        info: Cow<'a, str>,
        dest_value: Cow<'a, str>,
        dest_cursor: usize,
        selection: usize,
    },
    ArchiveCreate {
        source_count: usize,
        dest_value: Cow<'a, str>,
        dest_cursor: usize,
        selection: usize,
    },
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind<'_>) {
    render_dialog_with_colors(f, dialog, &ColorPalette::default());
}

pub fn render_dialog_with_colors(f: &mut Frame, dialog: &DialogKind<'_>, colors: &ColorPalette) {
    if matches!(dialog, DialogKind::OverwriteConfirm { files, .. } if files.is_empty()) {
        return;
    }

    let rect = f.area();
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, rect);

    f.render_widget(Clear, dialog_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, dialog_area);

    dispatch_dialog_render(f, dialog, dialog_area, colors);
}

fn dispatch_dialog_render(
    f: &mut Frame,
    dialog: &DialogKind<'_>,
    area: Rect,
    colors: &ColorPalette,
) {
    match dialog {
        DialogKind::Confirm {
            title,
            message,
            selection,
            files,
        } => {
            render_confirm_dialog(
                f,
                area,
                title.as_ref(),
                message.as_ref(),
                *selection,
                files,
                colors,
            );
        }
        DialogKind::Input {
            title,
            prompt,
            value,
            cursor_pos,
        } => {
            render_input_dialog(
                f,
                area,
                title.as_ref(),
                prompt.as_ref(),
                value.as_ref(),
                *cursor_pos,
                colors,
            );
        }
        DialogKind::Error { title, message } => {
            render_error_dialog(f, area, title.as_ref(), message.as_ref(), colors);
        }
        DialogKind::Help {
            title,
            message,
            scroll_offset,
        } => {
            render_help_dialog(
                f,
                area,
                title.as_ref(),
                message.as_ref(),
                *scroll_offset,
                colors,
            );
        }
        DialogKind::Progress {
            title,
            message,
            percent,
            cancellable,
        } => {
            render_progress_dialog(
                f,
                area,
                title.as_ref(),
                message.as_ref(),
                *percent,
                *cancellable,
                colors,
            );
        }
        other => dispatch_special_dialog_render(f, other, area, colors),
    }
}

fn dispatch_special_dialog_render(
    f: &mut Frame,
    dialog: &DialogKind<'_>,
    area: Rect,
    colors: &ColorPalette,
) {
    match dialog {
        DialogKind::Properties { info } => {
            render_properties_dialog(f, area, info, colors);
        }
        DialogKind::OverwriteConfirm { selection, files } => {
            render_overwrite_dialog(f, area, *selection, files.as_ref(), colors);
        }
        DialogKind::ArchiveExtract {
            info,
            dest_value,
            dest_cursor,
            selection,
        } => {
            archive::render_archive_extract_dialog(
                f,
                area,
                info.as_ref(),
                dest_value.as_ref(),
                *dest_cursor,
                *selection,
                colors,
            );
        }
        DialogKind::ArchiveCreate {
            source_count,
            dest_value,
            dest_cursor,
            selection,
        } => {
            archive::render_archive_create_dialog(
                f,
                area,
                *source_count,
                dest_value.as_ref(),
                *dest_cursor,
                *selection,
                colors,
            );
        }
        _ => unreachable!("all dialog kinds handled"),
    }
}

#[cfg(test)]
mod tests {
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
        assert!(wrapped_line_count(text, 1) > 1);
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
        assert_eq!(text::truncate_path("/tmp/zażółć/plik", 9), "...ć/plik");
    }

    #[test]
    fn truncate_path_truncates_tiny_utf8_width_safely() {
        assert_eq!(text::truncate_path("żółć", 3), "żół");
    }

    #[test]
    fn truncate_path_preserves_filename() {
        assert_eq!(
            text::truncate_path("/very/long/directory/path/file.txt", 15),
            "...ath/file.txt"
        );
    }

    #[test]
    fn overwrite_dialog_empty_files_returns_early() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_dialog(
                    f,
                    &DialogKind::OverwriteConfirm {
                        selection: 0,
                        files: Cow::Borrowed(&[]),
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn list_picker_keeps_selected_visible() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();

        terminal
            .draw(|f| render_list_picker(f, "Pick", &items, 19, "hint"))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Item 19"));
        assert!(!rendered.contains("Item 0"));
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
                        (Theme::highlight_bold(), "[ Yes ]"),
                        (Theme::dialog(), "[ No ]"),
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
                    &[(Theme::dialog(), "[ OK ]")],
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
        let warning_color = Theme::warning().fg.unwrap_or(Color::Yellow);
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
            name: "/very/long/path/to/some/file.txt".into(),
            size: "1.2 MB".into(),
            mtime: "2024-01-15 10:30".into(),
            permissions: "rw-r--r--".into(),
            owner: "user".into(),
            group: "staff".into(),
            file_type: "Regular File".into(),
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
}
