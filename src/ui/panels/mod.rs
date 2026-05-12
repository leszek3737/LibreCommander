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

const ICON_DISPLAY_WIDTH: usize = 2;

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
            get_file_color(&cat, bold).fg(Theme::selected_file_fg())
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
    let icon_width = ICON_DISPLAY_WIDTH;
    let size_str = format!("{:>10}", format_size(entry.len()));
    let date_str = format_time(entry.mtime());
    let (suffix, suffix_width) = build_suffix(entry, &size_str, &date_str, width, show_permissions);

    let available_name_width = width.saturating_sub(1 + suffix_width);
    if available_name_width == 0 {
        return format!("{marker}");
    }

    let name_with_icon = format!("{icon}{}", entry.name);
    let name_width = icon_width + UnicodeWidthStr::width(entry.name.as_str());
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

    let name_actual_width = if name_width <= available_name_width {
        name_width
    } else if available_name_width <= icon_width {
        UnicodeWidthStr::width(name.as_str())
    } else {
        icon_width + UnicodeWidthStr::width(name.get(icon.len()..).unwrap_or(""))
    };
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
    let icon_width = ICON_DISPLAY_WIDTH;
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
        Style::default().fg(Theme::scrollbar_active())
    } else {
        Style::default().fg(Theme::scrollbar_inactive())
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
        .style(Style::default().fg(Theme::status_bar_fg()))
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
        .fg(Theme::function_bar_fg())
        .bg(Theme::function_bar_bg())
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default()
        .fg(Theme::function_bar_fg())
        .bg(Theme::function_bar_bg());

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
                .fg(Theme::menu_bar_fg())
                .bg(Theme::menu_bar_bg()),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, centered_area);
}

#[cfg(test)]
mod tests;
