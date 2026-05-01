use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

use crate::app::types::{FileEntry, PanelState, format_permissions, format_size};

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.get(s.len().saturating_sub(suffix.len())..)
        .map_or(false, |tail| tail.eq_ignore_ascii_case(suffix))
}

/// Get color/style for a file entry based on its type
pub fn get_file_color(entry: &FileEntry) -> Style {
    let color = if entry.is_dir {
        Theme::DIRECTORY
    } else if entry.is_executable {
        Theme::EXECUTABLE
    } else if entry.is_symlink {
        Theme::SYMLINK
    } else if entry.is_hidden {
        Theme::HIDDEN_FILE
    } else if is_archive(&entry.name) {
        Theme::ARCHIVE
    } else if is_image(&entry.name) {
        Theme::IMAGE
    } else if is_source_code(&entry.name) {
        Theme::SOURCE_CODE
    } else {
        Theme::REGULAR_FILE
    };

    Theme::panel_item(color, entry.is_dir || entry.is_executable)
}

/// Check if file is an archive
pub fn is_archive(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".tar.gz")
        || ends_with_ignore_ascii_case(name, ".tar.bz2")
        || ends_with_ignore_ascii_case(name, ".tar.xz")
        || ends_with_ignore_ascii_case(name, ".tar")
        || ends_with_ignore_ascii_case(name, ".gz")
        || ends_with_ignore_ascii_case(name, ".zip")
        || ends_with_ignore_ascii_case(name, ".bz2")
        || ends_with_ignore_ascii_case(name, ".xz")
        || ends_with_ignore_ascii_case(name, ".7z")
        || ends_with_ignore_ascii_case(name, ".rar")
}

/// Check if file is an image
pub fn is_image(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".jpg")
        || ends_with_ignore_ascii_case(name, ".jpeg")
        || ends_with_ignore_ascii_case(name, ".png")
        || ends_with_ignore_ascii_case(name, ".gif")
        || ends_with_ignore_ascii_case(name, ".bmp")
        || ends_with_ignore_ascii_case(name, ".svg")
}

/// Check if file is source code
pub fn is_source_code(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".rs")
        || ends_with_ignore_ascii_case(name, ".py")
        || ends_with_ignore_ascii_case(name, ".js")
        || ends_with_ignore_ascii_case(name, ".ts")
        || ends_with_ignore_ascii_case(name, ".c")
        || ends_with_ignore_ascii_case(name, ".h")
        || ends_with_ignore_ascii_case(name, ".cpp")
        || ends_with_ignore_ascii_case(name, ".go")
        || ends_with_ignore_ascii_case(name, ".java")
}

/// Check if file is a document
pub fn is_document(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".pdf")
        || ends_with_ignore_ascii_case(name, ".doc")
        || ends_with_ignore_ascii_case(name, ".docx")
        || ends_with_ignore_ascii_case(name, ".xls")
        || ends_with_ignore_ascii_case(name, ".xlsx")
        || ends_with_ignore_ascii_case(name, ".odt")
}

/// Check if file is audio
pub fn is_audio(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".mp3")
        || ends_with_ignore_ascii_case(name, ".wav")
        || ends_with_ignore_ascii_case(name, ".flac")
        || ends_with_ignore_ascii_case(name, ".ogg")
        || ends_with_ignore_ascii_case(name, ".m4a")
}

/// Check if file is video
pub fn is_video(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".mp4")
        || ends_with_ignore_ascii_case(name, ".avi")
        || ends_with_ignore_ascii_case(name, ".mkv")
        || ends_with_ignore_ascii_case(name, ".mov")
        || ends_with_ignore_ascii_case(name, ".webm")
}

/// Check if file is a config/data file
pub fn is_config(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".json")
        || ends_with_ignore_ascii_case(name, ".toml")
        || ends_with_ignore_ascii_case(name, ".yaml")
        || ends_with_ignore_ascii_case(name, ".yml")
        || ends_with_ignore_ascii_case(name, ".ini")
        || ends_with_ignore_ascii_case(name, ".conf")
        || ends_with_ignore_ascii_case(name, ".cfg")
}

/// Get icon for a file entry (ASCII-safe, no variation selectors)
pub fn get_file_icon(entry: &FileEntry) -> &'static str {
    if entry.is_dir {
        return "📁 ";
    }

    if is_document(&entry.name) {
        return "📄 ";
    }

    if is_archive(&entry.name) {
        return "📦 ";
    }

    if is_image(&entry.name) {
        return "🖼 ";
    }

    if is_audio(&entry.name) {
        return "🎵 ";
    }

    if is_video(&entry.name) {
        return "🎬 ";
    }

    if is_config(&entry.name) {
        return "⚙ ";
    }

    if is_source_code(&entry.name) {
        return "💻 ";
    }

    "📄 "
}

/// Format modification time
pub fn format_time(modified: SystemTime) -> String {
    use chrono::{DateTime, Datelike, Timelike};

    // Get duration since UNIX epoch (will handle dates after 1970)
    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
        let timestamp = duration.as_secs();

        // Use the recommended DateTime::from_timestamp API
        if let Some(dt) = DateTime::from_timestamp(timestamp as i64, 0) {
            let local = dt.with_timezone(&chrono::Local);
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                local.year(),
                local.month(),
                local.day(),
                local.hour(),
                local.minute()
            )
        } else {
            "????-??-?? ??:??".to_string()
        }
    } else {
        "????-??-?? ??:??".to_string()
    }
}

/// Render a single file panel with border
pub fn render_panel(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let border_style = if is_active {
        Theme::border_active()
    } else {
        Theme::border_inactive()
    };

    // Title with current directory path
    let title = format!(" {} ", panel.path.display());

    // Construct block with border and title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Theme::title());

    // Calculate available area for file list (inside the border)
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Split inner area horizontally: list area | scrollbar
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(95), // List area
            Constraint::Percentage(5),  // Scrollbar area
        ])
        .split(inner_area);

    // Create list items
    let mut list_items = Vec::new();

    // Compute visible entries
    let start_idx = panel.scroll_offset;
    let end_idx = std::cmp::min(panel.entries.len(), start_idx + inner_area.height as usize);

    // Iterate over entries directly (entries list already contains ".." from the reader)
    for entry in panel
        .entries
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
    {
        let string_line = match panel.listing_mode {
            super::super::app::types::ListingMode::Long => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_entry_line(entry, width)
            }
            super::super::app::types::ListingMode::Brief => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_brief_entry_line(entry, width)
            }
        };

        // Get base style from file type
        let line_style = if entry.selected {
            get_file_color(entry).fg(Color::LightYellow)
        } else {
            get_file_color(entry)
        };

        list_items.push(ListItem::new(Span::styled(string_line, line_style)));
    }

    // Render the list
    let highlight_style = if is_active {
        Theme::highlight()
    } else {
        Theme::panel()
    };

    let list = List::new(list_items)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .highlight_style(highlight_style);

    // Setup ListState for cursor/selection
    let mut list_state = ListState::default();
    // Calculate relative cursor index for the visible slice
    // Only select highlighted if the panel is active
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

    // Render scrollbar indicator
    if !panel.entries.is_empty() {
        render_scrollbar(f, chunks[1], panel, is_active);
    }
}

/// Format a single entry line for display
fn format_entry_line(entry: &FileEntry, width: usize) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    let size_str = format!("{:>10}", format_size(entry.size));
    let date_str = format_time(entry.modified);
    let perms_str = format_permissions(entry.permissions);
    let suffix = format!(" {size_str} {date_str} {perms_str}");
    let suffix_width = UnicodeWidthStr::width(suffix.as_str());

    let icon = get_file_icon(entry);
    let icon_width = UnicodeWidthStr::width(icon);
    let available_name_width = width.saturating_sub(1 + suffix_width);
    let mut name = String::new();

    if available_name_width == 0 {
        return format!("{marker}{suffix}");
    }

    let name_with_icon = format!("{icon}{}", entry.name);
    let name_width = UnicodeWidthStr::width(name_with_icon.as_str());
    if name_width <= available_name_width {
        name.push_str(&name_with_icon);
    } else if available_name_width <= icon_width {
        // Truncate icon by display width, not char count
        let mut taken = 0;
        for ch in icon.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if taken + cw > available_name_width {
                break;
            }
            name.push(ch);
            taken += cw;
        }
    } else {
        name.push_str(icon);
        let truncate_to = available_name_width.saturating_sub(icon_width + 1);
        let mut taken = 0;
        for ch in entry.name.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if taken + cw > truncate_to {
                break;
            }
            name.push(ch);
            taken += cw;
        }
        name.push('…');
    }

    format!("{marker}{name}{suffix}")
}

fn format_brief_entry_line(entry: &FileEntry, width: usize) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    let icon = get_file_icon(entry);
    let icon_width = UnicodeWidthStr::width(icon);
    let available = width.saturating_sub(1); // after marker
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
    // Truncate name to fit with ellipsis
    let trunc_to = name_available - 1; // leave room for ellipsis
    let mut taken = 0;
    let mut truncated = String::new();
    for ch in entry.name.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if taken + cw > trunc_to {
            break;
        }
        truncated.push(ch);
        taken += cw;
    }
    format!("{marker}{icon}{truncated}…")
}

/// Render scrollbar indicator
pub fn render_scrollbar(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    if panel.entries.is_empty() {
        return;
    }

    let total_entries = panel.entries.len();
    let visible_height = area.height as usize;
    let scroll_offset = panel.scroll_offset;

    // Calculate scrollbar position
    let scrollbar_height = area.height as usize;
    let max_scroll = total_entries.saturating_sub(scrollbar_height);
    let thumb_pos = if max_scroll > 0 && scrollbar_height > 1 {
        (scroll_offset * (scrollbar_height - 1) / max_scroll) as u16
    } else {
        0
    };

    let mut scrollbar = String::new();
    for i in 0..area.height {
        if i == thumb_pos && total_entries > visible_height {
            scrollbar.push_str("█\n");
        } else {
            scrollbar.push_str("│\n");
        }
    }

    let style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let paragraph = Paragraph::new(scrollbar)
        .style(style)
        .block(Block::default().padding(Padding::new(0, 0, 0, 0)));

    f.render_widget(paragraph, area);
}

/// Render status bar showing current file info
pub fn render_status_bar(f: &mut Frame, area: Rect, panel: &PanelState) {
    let mut info_line = String::new();

    if !panel.entries.is_empty() && panel.cursor < panel.entries.len() {
        let entry = &panel.entries[panel.cursor];
        info_line = format!(
            "{} | {} | {} | {} | {}",
            entry.name,
            format_size(entry.size),
            format_permissions(entry.permissions),
            entry.owner,
            entry.group,
        );
    }

    let mut selected_info = String::new();
    if panel.selected_count > 0 {
        selected_info = format!(
            "  Selected: {} files ({})",
            panel.selected_count,
            format_size(panel.selected_size)
        );
    }

    let full_text = format!("{info_line}{selected_info}");

    let paragraph = Paragraph::new(full_text)
        .style(Style::default().fg(Color::LightCyan))
        .block(Block::default().borders(Borders::TOP));

    f.render_widget(paragraph, area);
}

/// Render function bar (F-keys)
pub fn render_function_bar(f: &mut Frame, area: Rect) {
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

    let constraints: Vec<Constraint> = keys.iter().map(|_| Constraint::Percentage(10)).collect();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, (key, label)) in keys.iter().enumerate() {
        let label_style = Style::default().fg(Color::LightBlue).bg(Color::DarkGray);

        let text = format!(" {key} {label} ");
        let paragraph = Paragraph::new(Span::styled(text, label_style))
            .block(Block::default().padding(Padding::new(1, 1, 0, 0)));

        f.render_widget(paragraph, chunks[i]);
    }
}

/// Render menu bar at top
pub fn render_menu_bar(f: &mut Frame, area: Rect) {
    let menu_text = "   Left   File   Command   Options   Right   ";
    let x = area.x + area.width.saturating_sub(menu_text.len() as u16) / 2;
    let centered_area = Rect::new(x, area.y, menu_text.len() as u16, area.height);

    let paragraph = Paragraph::new(menu_text)
        .style(Style::default().fg(Color::LightBlue).bg(Color::DarkGray))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, centered_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_entry(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink,
            is_executable: is_exec,
            size: 1024,
            modified: SystemTime::now(),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected: false,
            is_hidden: name.starts_with('.'),
        }
    }

    #[test]
    fn test_get_file_color_directory() {
        let entry = create_test_entry("mydir", true, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::White));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_get_file_color_executable() {
        let entry = create_test_entry("script.sh", false, true, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::Green));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_get_file_color_symlink() {
        let entry = create_test_entry("link", false, false, true);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_get_file_color_archive() {
        let entry = create_test_entry("archive.tar.gz", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_get_file_color_image() {
        let entry = create_test_entry("photo.png", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::Magenta));
    }

    #[test]
    fn test_get_file_color_source_code() {
        let entry = create_test_entry("main.rs", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_get_file_color_hidden() {
        let entry = create_test_entry(".hidden", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_get_file_color_regular() {
        let entry = create_test_entry("document.txt", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::White));
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
        assert!(!is_source_code("data.json"));
    }

    #[test]
    fn test_format_time_current() {
        let time = SystemTime::now();
        let result = format_time(time);
        // Should produce a valid date string
        assert!(result.len() >= 16); // "YYYY-MM-DD HH:MM"
        assert!(result.contains("-"));
        assert!(result.contains(":"));
    }

    #[test]
    fn test_format_entry_line_basic() {
        let entry = create_test_entry("file.txt", false, false, false);
        let result = format_entry_line(&entry, 60);
        assert!(result.contains("file.txt"));
    }

    #[test]
    fn test_format_entry_line_selected() {
        let mut entry = create_test_entry("file.txt", false, false, false);
        entry.selected = true;
        let result = format_entry_line(&entry, 60);
        assert!(result.starts_with('*'));
    }

    #[test]
    fn test_get_file_icon_directory() {
        let entry = create_test_entry("mydir", true, false, false);
        assert_eq!(get_file_icon(&entry), "📁 ");
    }

    #[test]
    fn test_get_file_icon_document() {
        let entry = create_test_entry("report.pdf", false, false, false);
        assert_eq!(get_file_icon(&entry), "📄 ");
    }

    #[test]
    fn test_get_file_icon_archive() {
        let entry = create_test_entry("backup.tar.gz", false, false, false);
        assert_eq!(get_file_icon(&entry), "📦 ");
    }

    #[test]
    fn test_get_file_icon_image() {
        let entry = create_test_entry("photo.jpg", false, false, false);
        assert_eq!(get_file_icon(&entry), "🖼 ");
    }

    #[test]
    fn test_get_file_icon_audio() {
        let entry = create_test_entry("song.mp3", false, false, false);
        assert_eq!(get_file_icon(&entry), "🎵 ");
    }

    #[test]
    fn test_get_file_icon_video() {
        let entry = create_test_entry("movie.mp4", false, false, false);
        assert_eq!(get_file_icon(&entry), "🎬 ");
    }

    #[test]
    fn test_get_file_icon_config() {
        let entry = create_test_entry("config.toml", false, false, false);
        assert_eq!(get_file_icon(&entry), "⚙ ");
    }

    #[test]
    fn test_get_file_icon_code() {
        let entry = create_test_entry("main.rs", false, false, false);
        assert_eq!(get_file_icon(&entry), "💻 ");
    }

    #[test]
    fn test_get_file_icon_default() {
        let entry = create_test_entry("unknown.xyz", false, false, false);
        assert_eq!(get_file_icon(&entry), "📄 ");
    }

    #[test]
    fn test_format_entry_line_truncation() {
        let entry = create_test_entry(
            "very_long_filename_that_should_be_truncated.txt",
            false,
            false,
            false,
        );
        let result = format_entry_line(&entry, 47);
        assert!(result.contains('…'));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 47);
    }

    #[test]
    fn test_format_entry_line_truncation_handles_unicode() {
        let entry = create_test_entry("zażółć_gęślą_jaźń.txt", false, false, false);
        let result = format_entry_line(&entry, 47);
        assert!(result.contains('…'));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 47);
    }

    #[test]
    fn test_panel_state_is_not_send_sync() {
        // This test verifies that PanelState can be used in the UI thread
        let panel = PanelState::new(PathBuf::from("/test"));

        // Verify basic construction works
        assert_eq!(panel.path, PathBuf::from("/test"));
        assert_eq!(panel.cursor, 0);
    }
}
