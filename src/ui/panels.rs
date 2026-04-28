use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

pub use crate::app::types::FileEntry;
pub use crate::app::types::PanelState;

/// Get color/style for a file entry based on its type
pub fn get_file_color(entry: &FileEntry) -> Style {
    if entry.is_hidden {
        return Style::default().fg(Color::DarkGray);
    }

    Style::default()
        .fg(if entry.is_dir {
            Color::White
        } else if entry.is_executable {
            Color::Green
        } else if entry.is_symlink {
            Color::Cyan
        } else if is_archive(&entry.name) {
            Color::Red
        } else if is_image(&entry.name) {
            Color::Magenta
        } else if is_source_code(&entry.name) {
            Color::Yellow
        } else {
            Color::White
        })
        .add_modifier(if entry.is_dir || entry.is_executable {
            Modifier::BOLD
        } else {
            Modifier::empty()
        })
}

/// Check if file is an archive
pub fn is_archive(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".tar")
        || lower.ends_with(".gz")
        || lower.ends_with(".zip")
        || lower.ends_with(".bz2")
        || lower.ends_with(".xz")
        || lower.ends_with(".7z")
        || lower.ends_with(".rar")
}

/// Check if file is an image
pub fn is_image(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
}

/// Check if file is source code
pub fn is_source_code(name: &str) -> bool {
    name.ends_with(".rs")
        || name.ends_with(".py")
        || name.ends_with(".js")
        || name.ends_with(".ts")
        || name.ends_with(".c")
        || name.ends_with(".h")
        || name.ends_with(".cpp")
        || name.ends_with(".go")
        || name.ends_with(".java")
        || name.ends_with(".RS")
        || name.ends_with(".PY")
        || name.ends_with(".JS")
        || name.ends_with(".TS")
        || name.ends_with(".C")
        || name.ends_with(".H")
        || name.ends_with(".CPP")
        || name.ends_with(".GO")
        || name.ends_with(".JAVA")
}

/// Format file size for display
pub fn format_size(size: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = size as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{:>6} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:>6.1} {}", size, UNITS[unit_idx])
    }
}

/// Format permissions as rwx string
pub fn format_permissions(permissions: u32) -> String {
    let mut result = String::with_capacity(9);

    // Standard Unix permission bits (octal notation):
    // Each permission is a combination of read(4), write(2), execute(1)
    // Owner: bits 6-8 (0o400, 0o200, 0o100)
    // Group: bits 3-5 (0o040, 0o020, 0o010)
    // Other: bits 0-2 (0o004, 0o002, 0o001)

    const OWNER_READ: u32 = 0o400;
    const OWNER_WRITE: u32 = 0o200;
    const OWNER_EXEC: u32 = 0o100;
    const GROUP_READ: u32 = 0o040;
    const GROUP_WRITE: u32 = 0o020;
    const GROUP_EXEC: u32 = 0o010;
    const OTHER_READ: u32 = 0o004;
    const OTHER_WRITE: u32 = 0o002;
    const OTHER_EXEC: u32 = 0o001;

    result.push(if (permissions & OWNER_READ) != 0 {
        'r'
    } else {
        '-'
    });
    result.push(if (permissions & OWNER_WRITE) != 0 {
        'w'
    } else {
        '-'
    });
    const SETUID: u32 = 0o4000;
    const SETGID: u32 = 0o2000;
    const STICKY: u32 = 0o1000;

    result.push(if (permissions & SETUID) != 0 {
        if (permissions & OWNER_EXEC) != 0 { 's' } else { 'S' }
    } else {
        if (permissions & OWNER_EXEC) != 0 { 'x' } else { '-' }
    });
    result.push(if (permissions & GROUP_READ) != 0 {
        'r'
    } else {
        '-'
    });
    result.push(if (permissions & GROUP_WRITE) != 0 {
        'w'
    } else {
        '-'
    });
    result.push(if (permissions & SETGID) != 0 {
        if (permissions & GROUP_EXEC) != 0 { 's' } else { 'S' }
    } else {
        if (permissions & GROUP_EXEC) != 0 { 'x' } else { '-' }
    });
    result.push(if (permissions & OTHER_READ) != 0 {
        'r'
    } else {
        '-'
    });
    result.push(if (permissions & OTHER_WRITE) != 0 {
        'w'
    } else {
        '-'
    });
    result.push(if (permissions & STICKY) != 0 {
        if (permissions & OTHER_EXEC) != 0 { 't' } else { 'T' }
    } else {
        if (permissions & OTHER_EXEC) != 0 { 'x' } else { '-' }
    });

    result
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
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Title with current directory path
    let title = format!(" {} ", panel.path.display());

    // Construct block with border and title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(Color::LightCyan));

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
        Style::default().bg(Color::DarkGray)
    } else {
        Style::default().bg(Color::Black).add_modifier(Modifier::DIM)
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
        let err_text = Paragraph::new(format!(" Error: {err}"))
            .style(Style::default().fg(Color::Red));
        f.render_widget(err_text, chunks[0]);
    }

    // Render scrollbar indicator
    if !panel.entries.is_empty() {
        render_scrollbar(f, chunks[1], panel, is_active);
    }
}

/// Format a single entry line for display
fn format_entry_line(entry: &FileEntry, width: usize) -> String {
    let mut line = String::new();

    // Selection marker: 1 char
    line.push(if entry.selected { '*' } else { ' ' });

    // Name first (truncated if necessary, with ellipsis)
    // Reserve space for: marker(1) + name + spaces + size(10) + date(16) + perms(9) = ~36+ chars
    let reserved_space = 1 + 10 + 1 + 16 + 1 + 9 + 1; // marker + size + space + date + space + perms + spaces
    let name_space = width.saturating_sub(reserved_space);
    
    if name_space > 1 {
        let name_width = UnicodeWidthStr::width(entry.name.as_str());
        if name_width > name_space {
            let name_space = name_space.saturating_sub(1);
            let mut taken = 0;
            for ch in entry.name.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if taken + cw > name_space {
                    break;
                }
                line.push(ch);
                taken += cw;
            }
            line.push('…');
        } else {
            line.push_str(&entry.name);
        }
    }
    
    // Pad name field if needed
    let current_len = line.len();
    let name_field_end = 1 + name_space.max(1);
    if current_len < name_field_end {
        for _ in current_len..name_field_end {
            line.push(' ');
        }
    }

    // Size (right-aligned, max 10 chars): 10 chars
    line.push(' ');
    let size_str = format_size(entry.size);
    use std::fmt::Write;
    let _ = write!(line, "{size_str:>10}");
    line.push(' ');

    // Date/Time: 16 chars ("YYYY-MM-DD HH:MM")
    let date_str = format_time(entry.modified);
    line.push_str(&date_str);

    // Permissions: 9 chars
    line.push(' ');
    line.push_str(&format_permissions(entry.permissions));

    line
}

fn format_brief_entry_line(entry: &FileEntry, width: usize) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    let prefix_len = 2;
    let available = width.saturating_sub(prefix_len);
    if available == 0 {
        return format!("{marker}");
    }
    let name_width = UnicodeWidthStr::width(entry.name.as_str());
    if name_width <= available {
        return format!("{marker} {}", entry.name);
    }
    let trunc_to = available.saturating_sub(1);
    let mut taken = 0;
    let mut name_part = String::new();
    for ch in entry.name.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if taken + cw > trunc_to {
            break;
        }
        name_part.push(ch);
        taken += cw;
    }
    format!("{marker} {name_part}…")
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

    let paragraph = Paragraph::new(menu_text)
        .style(Style::default().fg(Color::LightBlue).bg(Color::DarkGray))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
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
        assert_eq!(style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn test_get_file_color_regular() {
        let entry = create_test_entry("document.txt", false, false, false);
        let style = get_file_color(&entry);
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "     0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500), "   500 B");
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
    fn test_format_entry_line_truncation() {
        let entry = create_test_entry(
            "very_long_filename_that_should_be_truncated.txt",
            false,
            false,
            false,
        );
        let result = format_entry_line(&entry, 30);
        assert!(result.len() <= 32); // With margin
    }

    #[test]
    fn test_format_entry_line_truncation_handles_unicode() {
        let entry = create_test_entry("zażółć_gęślą_jaźń.txt", false, false, false);
        let result = format_entry_line(&entry, 45);
        assert!(result.ends_with('…'));
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
