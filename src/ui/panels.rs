use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};
use std::borrow::Cow;
use std::fmt::Write;
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

use crate::app::types::{
    FileCategory, FileEntry, ListingMode, PanelState, format_permissions, format_size,
};

/// Get color/style for a file category
pub fn get_file_color(category: &FileCategory, bold: bool) -> Style {
    let color = Theme::category_color(*category);
    Theme::panel_item(color, bold)
}

/// Get icon for a file category
pub fn get_file_icon(category: &FileCategory) -> &'static str {
    match category {
        FileCategory::Dir => "📁 ",
        FileCategory::Symlink => "🔗 ",
        FileCategory::Executable => "⚡ ",
        FileCategory::Code => "💻 ",
        FileCategory::Config => "⚙ ",
        FileCategory::Archive => "📦 ",
        FileCategory::Image => "🖼 ",
        FileCategory::Video => "🎬 ",
        FileCategory::Audio => "🎵 ",
        FileCategory::Document => "📝 ",
        FileCategory::Font => "🔤 ",
        FileCategory::Other => "📄 ",
    }
}

/// Format modification time
pub fn format_time(modified: SystemTime) -> Cow<'static, str> {
    use chrono::{DateTime, Datelike, Timelike};

    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
        let timestamp = duration.as_secs();

        if let Some(dt) = DateTime::from_timestamp(timestamp as i64, 0) {
            let local = dt.with_timezone(&chrono::Local);
            return Cow::Owned(format!(
                "{:02}-{:02}-{:02} {:02}:{:02}",
                local.day(),
                local.month(),
                local.year() % 100,
                local.hour(),
                local.minute()
            ));
        }
    }
    Cow::Borrowed("??-??-?? ??:??")
}

/// Render a single file panel with border
pub fn render_panel(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let border_style = if is_active {
        Theme::border_active()
    } else {
        Theme::border_inactive()
    };

    let title = format!(" {} ", panel.path.display());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Theme::title());

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner_area);

    let start_idx = panel.scroll_offset.min(panel.entries.len());
    let end_idx = std::cmp::min(panel.entries.len(), start_idx + inner_area.height as usize);

    let mut list_items = Vec::with_capacity(end_idx.saturating_sub(start_idx));

    for entry in panel
        .entries
        .iter()
        .skip(start_idx)
        .take(end_idx.saturating_sub(start_idx))
    {
        let cat = entry.category();
        let bold = entry.is_dir() || entry.is_executable();

        let string_line = match panel.listing_mode {
            ListingMode::Long => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_entry_line(entry, width, panel.show_permissions, &cat)
            }
            ListingMode::Brief => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_brief_entry_line(entry, width, &cat)
            }
        };

        let line_style = if entry.selected {
            get_file_color(&cat, bold).fg(Theme::SELECTED_FILE_FG)
        } else {
            get_file_color(&cat, bold)
        };

        list_items.push(ListItem::new(Span::styled(string_line, line_style)));
    }

    let highlight_style = if is_active {
        Theme::highlight()
    } else {
        Theme::panel()
    };

    let list = List::new(list_items)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .highlight_style(highlight_style);

    let mut list_state = ListState::default();
    if panel.cursor >= start_idx && panel.cursor < end_idx {
        list_state.select(Some(panel.cursor - start_idx));
    }

    f.render_stateful_widget(list, chunks[0], &mut list_state);

    if panel.entries.is_empty()
        && let Some(ref err) = panel.last_error
    {
        let err_text = Paragraph::new(format!(" Error: {err}")).style(Theme::error());
        f.render_widget(err_text, chunks[0]);
    }

    if !panel.entries.is_empty() {
        render_scrollbar(f, chunks[1], panel, is_active);
    }
}

fn build_suffix(
    entry: &FileEntry,
    size_str: &str,
    date_str: &str,
    width: usize,
    show_permissions: bool,
) -> (String, usize) {
    let size_width = UnicodeWidthStr::width(size_str);
    let date_width = UnicodeWidthStr::width(date_str);
    let size_date_width = size_width + date_width + 2;

    if show_permissions {
        let perms_str = format_permissions(entry.mode_bits());
        let perms_width = UnicodeWidthStr::width(perms_str.as_str());
        let full_width = size_date_width + perms_width + 1;
        if 2 + full_width <= width {
            return (format!(" {size_str} {date_str} {perms_str}"), full_width);
        }
        if 2 + size_date_width <= width {
            return (format!(" {size_str} {date_str}"), size_date_width);
        }
        if 2 + size_width < width {
            return (format!(" {size_str}"), size_width + 1);
        }
        return (String::new(), 0);
    }

    if 2 + size_date_width <= width {
        (format!(" {size_str} {date_str}"), size_date_width)
    } else if 2 + size_width <= width {
        (format!(" {size_str}"), size_width + 1)
    } else {
        (String::new(), 0)
    }
}

fn truncate_name(name: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let name_width = UnicodeWidthStr::width(name);
    if name_width <= max_width {
        return name.to_string();
    }
    let truncate_to = max_width.saturating_sub(1);
    let mut result = String::new();
    let mut taken = 0;
    for ch in name.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if taken + cw > truncate_to {
            break;
        }
        result.push(ch);
        taken += cw;
    }
    result.push('…');
    result
}

fn format_entry_line(
    entry: &FileEntry,
    width: usize,
    show_permissions: bool,
    category: &FileCategory,
) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    if width <= 1 {
        return format!("{marker}");
    }

    let icon = get_file_icon(category);
    let icon_width = UnicodeWidthStr::width(icon);
    let size_str = format!("{:>10}", format_size(entry.len()));
    let date_str = format_time(entry.mtime());
    let (suffix, suffix_width) = build_suffix(entry, &size_str, &date_str, width, show_permissions);

    let available_name_width = width.saturating_sub(1 + suffix_width);
    if available_name_width == 0 {
        return format!("{marker}");
    }

    let name_with_icon = format!("{icon}{}", entry.name);
    let name_width = UnicodeWidthStr::width(name_with_icon.as_str());
    let name = if name_width <= available_name_width {
        name_with_icon
    } else if available_name_width <= icon_width {
        let mut result = String::new();
        let mut taken = 0;
        for ch in icon.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if taken + cw > available_name_width {
                break;
            }
            result.push(ch);
            taken += cw;
        }
        result
    } else {
        let truncated = truncate_name(&entry.name, available_name_width.saturating_sub(icon_width));
        format!("{icon}{truncated}")
    };

    let name_actual_width = UnicodeWidthStr::width(name.as_str());
    let padding = available_name_width.saturating_sub(name_actual_width);

    format!("{marker}{name}{}{suffix}", " ".repeat(padding))
}

fn status_metadata(size: &str, entry: &FileEntry, show_permissions: bool) -> String {
    if show_permissions {
        let perms = format_permissions(entry.mode_bits());
        format!("{size} | {perms} | {} | {}", entry.owner, entry.group)
    } else {
        format!("{size} | {} | {}", entry.owner, entry.group)
    }
}

fn format_brief_entry_line(entry: &FileEntry, width: usize, category: &FileCategory) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    let icon = get_file_icon(category);
    let icon_width = UnicodeWidthStr::width(icon);
    let available = width.saturating_sub(1);
    if available == 0 {
        return format!("{marker}");
    }
    if available < icon_width {
        return format!("{marker}");
    }
    let name_width = UnicodeWidthStr::width(entry.name.as_str());
    let name_available = available - icon_width;
    if name_available >= name_width {
        return format!("{marker}{icon}{}", entry.name);
    }
    if name_available == 0 {
        return format!("{marker}{icon}");
    }
    let truncated = truncate_name(&entry.name, name_available);
    format!("{marker}{icon}{truncated}")
}

/// Render scrollbar indicator
pub fn render_scrollbar(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    if panel.entries.is_empty() {
        return;
    }

    let total_entries = panel.entries.len();
    let height = area.height as usize;
    let max_scroll = total_entries.saturating_sub(height);
    let scroll_offset = panel.scroll_offset.min(max_scroll);

    let thumb_pos = if max_scroll > 0 && height > 1 {
        scroll_offset * (height - 1) / max_scroll
    } else {
        0
    };

    let mut scrollbar = String::with_capacity(height * 4);
    for i in 0..height {
        let is_last = i == height - 1;
        if i == thumb_pos && total_entries > height {
            scrollbar.push_str(if is_last { "█" } else { "█\n" });
        } else {
            scrollbar.push_str(if is_last { "│" } else { "│\n" });
        }
    }

    let style = if is_active {
        Style::default().fg(Theme::SCROLLBAR_ACTIVE)
    } else {
        Style::default().fg(Theme::SCROLLBAR_INACTIVE)
    };

    let paragraph = Paragraph::new(scrollbar)
        .style(style)
        .block(Block::default().padding(Padding::new(0, 0, 0, 0)));

    f.render_widget(paragraph, area);
}

/// Compute compact panel status summary string.
/// Format: "  5/100  5%  Sel: 3 [1.2 MB]"
/// Returns (summary_string, summary_display_width).
pub fn panel_status_summary(panel: &PanelState) -> (String, usize) {
    let total = panel.entries.len();
    if total == 0 {
        return (String::new(), 0);
    }

    let pos = (panel.cursor + 1).min(total);
    let pct = pos * 100 / total;

    let mut summary = String::new();
    let _ = write!(summary, " {}/{} {}%", pos, total, pct);

    if panel.selected_count > 0 {
        let _ = write!(
            summary,
            " Sel: {} [{}]",
            panel.selected_count,
            format_size(panel.selected_size)
        );
    }

    summary.push(' ');
    let width = UnicodeWidthStr::width(summary.as_str());
    (summary, width)
}

/// Render status bar showing current file info
pub fn render_status_bar(f: &mut Frame, area: Rect, panel: &PanelState) {
    let available = area.width as usize;

    let (right_info, right_width) = panel_status_summary(panel);
    let remaining = available.saturating_sub(right_width);

    let info_line = if !panel.entries.is_empty() && panel.cursor < panel.entries.len() {
        let entry = &panel.entries[panel.cursor];
        let size_str = format_size(entry.len());
        let metadata = status_metadata(&size_str, entry, panel.show_permissions);
        let full_info = format!("{} | {metadata}", entry.name);
        let full_width = UnicodeWidthStr::width(full_info.as_str());

        if full_width <= remaining {
            full_info
        } else {
            let meta = format!(" | {metadata}");
            let meta_width = UnicodeWidthStr::width(meta.as_str());
            let name_budget = remaining.saturating_sub(meta_width);

            if name_budget >= 3 {
                let mut truncated = String::new();
                let mut taken = 0;
                for ch in entry.name.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if taken + cw > name_budget.saturating_sub(1) {
                        break;
                    }
                    truncated.push(ch);
                    taken += cw;
                }
                format!("{truncated}…{meta}")
            } else if remaining > 0 {
                let mut truncated = String::new();
                let mut taken = 0;
                for ch in full_info.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if taken + cw > remaining {
                        break;
                    }
                    truncated.push(ch);
                    taken += cw;
                }
                truncated
            } else {
                String::new()
            }
        }
    } else {
        String::new()
    };

    let full_text = format!("{info_line}{right_info}");

    let paragraph = Paragraph::new(full_text)
        .style(Style::default().fg(Theme::STATUS_BAR_FG))
        .block(Block::default().borders(Borders::TOP));

    f.render_widget(paragraph, area);
}

/// Render function bar (F-keys)
pub fn render_function_bar(f: &mut Frame, area: Rect) {
    const CONSTRAINTS: [Constraint; 10] = [Constraint::Percentage(10); 10];

    let keys = [
        ("F1", "Help"),
        ("F2", "Menu"),
        ("F3", "View"),
        ("F4", "Edit"),
        ("F5", "Copy"),
        ("F6", "Move"),
        ("F7", "Mkdir"),
        ("F8", "Delete"),
        ("F9", "Menu"),
        ("F10", "Quit"),
    ];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(CONSTRAINTS)
        .split(area);

    let key_style = Style::default()
        .fg(Theme::FUNCTION_BAR_FG)
        .bg(Theme::FUNCTION_BAR_BG)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default()
        .fg(Theme::FUNCTION_BAR_FG)
        .bg(Theme::FUNCTION_BAR_BG);

    for (i, (key, label)) in keys.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(format!(" {key} "), key_style),
            Span::styled(format!("{label} "), label_style),
        ]);
        let paragraph =
            Paragraph::new(line).block(Block::default().padding(Padding::new(1, 1, 0, 0)));

        f.render_widget(paragraph, chunks[i]);
    }
}

/// Render menu bar at top
pub fn render_menu_bar(f: &mut Frame, area: Rect) {
    let menu_text = "   Left   File   Command   Options   Right   ";
    let text_width = UnicodeWidthStr::width(menu_text) as u16;
    let x = area.x + area.width.saturating_sub(text_width) / 2;
    let centered_area = Rect::new(x, area.y, text_width, area.height);

    let paragraph = Paragraph::new(menu_text)
        .style(
            Style::default()
                .fg(Theme::MENU_BAR_FG)
                .bg(Theme::MENU_BAR_BG),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, centered_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_type::*;
    use ratatui::style::Color;
    use std::path::PathBuf;

    fn create_test_entry(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool) -> FileEntry {
        FileEntry::builder()
            .name(name)
            .path(name)
            .is_dir(is_dir)
            .is_symlink(is_symlink)
            .is_executable(is_exec)
            .size(1024)
            .modified(SystemTime::now())
            .owner("user")
            .group("group")
            .is_hidden(name.starts_with('.'))
            .build()
    }

    #[test]
    fn test_get_file_color_directory() {
        let entry = create_test_entry("mydir", true, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::White));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_get_file_color_code_script() {
        let entry = create_test_entry("script.sh", false, true, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_get_file_color_extensionless_executable() {
        let entry = create_test_entry("mybinary", false, true, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Green));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_get_file_color_symlink() {
        let entry = create_test_entry("link", false, false, true);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_get_file_color_archive() {
        let entry = create_test_entry("archive.tar.gz", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_get_file_color_image() {
        let entry = create_test_entry("photo.png", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Magenta));
    }

    #[test]
    fn test_get_file_color_source_code() {
        let entry = create_test_entry("main.rs", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_get_file_color_hidden() {
        let entry = create_test_entry(".hidden", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_get_file_color_regular() {
        let entry = create_test_entry("unknown.xyz", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_get_file_color_document() {
        let entry = create_test_entry("document.txt", false, false, false);
        let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
        assert_eq!(style.fg, Some(Color::LightYellow));
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        let result = format_size(1536);
        assert!(result.contains("KB"));
    }

    #[test]
    fn test_format_size_megabytes() {
        let result = format_size(1024 * 1024 * 5);
        assert!(result.contains("MB"));
    }

    #[test]
    fn test_format_permissions_full() {
        let result = format_permissions(0o755);
        assert_eq!(result, "rwxr-xr-x");
    }

    #[test]
    fn test_format_permissions_readonly() {
        let result = format_permissions(0o444);
        assert_eq!(result, "r--r--r--");
    }

    #[test]
    fn test_format_permissions_no_permissions() {
        let result = format_permissions(0o000);
        assert_eq!(result, "---------");
    }

    #[test]
    fn test_is_archive_tar() {
        assert!(is_archive("file.tar"));
        assert!(is_archive("archive.TAR"));
        assert!(is_archive("backup.tar.gz"));
    }

    #[test]
    fn test_is_archive_zip() {
        assert!(is_archive("files.zip"));
        assert!(is_archive("data.7z"));
        assert!(is_archive("backup.rar"));
    }

    #[test]
    fn test_is_archive_negative() {
        assert!(!is_archive("document.txt"));
        assert!(!is_archive("image.png"));
    }

    #[test]
    fn test_is_image_jpg() {
        assert!(is_image("photo.jpg"));
        assert!(is_image("image.JPEG"));
    }

    #[test]
    fn test_is_image_png() {
        assert!(is_image("screenshot.png"));
        assert!(is_image("icon.PNG"));
    }

    #[test]
    fn test_is_image_negative() {
        assert!(!is_image("document.txt"));
        assert!(!is_image("code.rs"));
    }

    #[test]
    fn test_is_source_code_rust() {
        assert!(is_source_code("main.rs"));
        assert!(is_source_code("lib.RS"));
    }

    #[test]
    fn test_is_source_code_python() {
        assert!(is_source_code("script.py"));
        assert!(is_source_code("module.PY"));
    }

    #[test]
    fn test_is_source_code_js() {
        assert!(is_source_code("app.js"));
        assert!(is_source_code("component.ts"));
    }

    #[test]
    fn test_is_source_code_negative() {
        assert!(!is_source_code("image.png"));
        assert!(!is_source_code("data.ini"));
    }

    #[test]
    fn test_format_time_current() {
        let time = SystemTime::now();
        let result = format_time(time);
        assert!(result.len() >= 14);
        assert!(result.contains("-"));
        assert!(result.contains(":"));
    }

    #[test]
    fn test_format_time_returns_cow() {
        let time = SystemTime::UNIX_EPOCH;
        let result = format_time(time);
        assert!(matches!(result, Cow::Owned(_)));
    }

    #[test]
    fn test_format_entry_line_basic() {
        let entry = create_test_entry("file.txt", false, false, false);
        let result = format_entry_line(&entry, 60, false, &entry.category());
        assert!(result.contains("file.txt"));
    }

    #[test]
    fn test_format_entry_line_selected() {
        let mut entry = create_test_entry("file.txt", false, false, false);
        entry.selected = true;
        let result = format_entry_line(&entry, 60, false, &entry.category());
        assert!(result.starts_with('*'));
    }

    #[test]
    fn test_get_file_icon_directory() {
        let entry = create_test_entry("mydir", true, false, false);
        assert_eq!(get_file_icon(&entry.category()), "📁 ");
    }

    #[test]
    fn test_get_file_icon_document() {
        let entry = create_test_entry("report.pdf", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "📝 ");
    }

    #[test]
    fn test_get_file_icon_archive() {
        let entry = create_test_entry("backup.tar.gz", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "📦 ");
    }

    #[test]
    fn test_get_file_icon_image() {
        let entry = create_test_entry("photo.jpg", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "🖼 ");
    }

    #[test]
    fn test_get_file_icon_audio() {
        let entry = create_test_entry("song.mp3", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "🎵 ");
    }

    #[test]
    fn test_get_file_icon_video() {
        let entry = create_test_entry("movie.mp4", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "🎬 ");
    }

    #[test]
    fn test_get_file_icon_config() {
        let entry = create_test_entry("config.toml", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "⚙ ");
    }

    #[test]
    fn test_get_file_icon_code() {
        let entry = create_test_entry("main.rs", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "💻 ");
    }

    #[test]
    fn test_get_file_icon_default() {
        let entry = create_test_entry("unknown.xyz", false, false, false);
        assert_eq!(get_file_icon(&entry.category()), "📄 ");
    }

    #[test]
    fn test_format_entry_line_truncation() {
        let entry = create_test_entry(
            "very_long_filename_that_should_be_truncated.txt",
            false,
            false,
            false,
        );
        let result = format_entry_line(&entry, 47, false, &entry.category());
        assert!(result.contains('…'));
    }

    #[test]
    fn test_format_entry_line_truncation_handles_unicode() {
        let entry = create_test_entry("日本語テストファイル.txt", false, false, false);
        let result = format_entry_line(&entry, 47, false, &entry.category());
        assert!(result.contains('…'));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 47);
    }

    #[test]
    fn test_panel_state_is_not_send_sync() {
        let panel = PanelState::new(PathBuf::from("/test"));

        assert_eq!(panel.path, PathBuf::from("/test"));
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn test_panel_status_summary_empty_panel() {
        let panel = PanelState::new(PathBuf::from("/test"));
        let (summary, width) = panel_status_summary(&panel);
        assert_eq!(summary, "");
        assert_eq!(width, 0);
    }

    #[test]
    fn test_panel_status_summary_first_item() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("a.txt", false, false, false),
            create_test_entry("b.txt", false, false, false),
            create_test_entry("c.txt", false, false, false),
        ];
        panel.cursor = 0;
        let (summary, _) = panel_status_summary(&panel);
        assert!(summary.contains("1/3"));
        assert!(summary.contains("33%"));
    }

    #[test]
    fn test_panel_status_summary_last_item() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("a.txt", false, false, false),
            create_test_entry("b.txt", false, false, false),
            create_test_entry("c.txt", false, false, false),
        ];
        panel.cursor = 2;
        let (summary, _) = panel_status_summary(&panel);
        assert!(summary.contains("3/3"));
        assert!(summary.contains("100%"));
    }

    #[test]
    fn test_panel_status_summary_with_selection() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![
            create_test_entry("a.txt", false, false, false),
            create_test_entry("b.txt", false, false, false),
        ];
        panel.cursor = 0;
        panel.selected_count = 1;
        panel.selected_size = 1024;
        let (summary, _) = panel_status_summary(&panel);
        assert!(summary.contains("Sel: 1"));
        assert!(summary.contains("1.0 KB"));
    }

    #[test]
    fn test_panel_status_summary_no_selection_when_zero() {
        let mut panel = PanelState::new(PathBuf::from("/test"));
        panel.entries = vec![create_test_entry("a.txt", false, false, false)];
        panel.cursor = 0;
        panel.selected_count = 0;
        let (summary, _) = panel_status_summary(&panel);
        assert!(!summary.contains("Sel:"));
    }

    #[test]
    fn test_truncate_name_no_truncation() {
        assert_eq!(truncate_name("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_name_with_ellipsis() {
        let result = truncate_name("hello world", 8);
        assert!(result.ends_with('…'));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 8);
    }

    #[test]
    fn test_truncate_name_unicode() {
        let result = truncate_name("日本語テストファイル", 6);
        assert!(result.ends_with('…'));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 6);
    }
}
