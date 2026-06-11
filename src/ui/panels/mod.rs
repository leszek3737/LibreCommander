use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};
use std::borrow::Cow;
use std::fmt::Write;
use unicode_width::UnicodeWidthStr;

use super::theme::IconTheme;
use super::theme::{ColorPalette, Theme};

use crate::app::types::{
    FileCategory, FileEntry, ListingMode, PanelState, format_permissions, format_size,
};

const FN_KEY_TEXTS: [&str; 10] = [
    " F1 ", " F2 ", " F3 ", " F4 ", " F5 ", " F6 ", " F7 ", " F8 ", " F9 ", " F10 ",
];

const FN_LABEL_TEXTS: [&str; 10] = [
    "Help ", "Menu ", "View ", "Edit ", "Copy ", "Move ", "Mkdir ", "Delete ", "Menu ", "Quit ",
];

const fn icon_display_width(theme: IconTheme) -> usize {
    match theme {
        IconTheme::Ascii | IconTheme::NerdFont => 1,
        IconTheme::Emoji => 2,
    }
}

pub fn get_file_color(category: &FileCategory, bold: bool) -> Style {
    get_file_color_with_palette(category, bold, &ColorPalette::default())
}

pub fn get_file_color_with_palette(
    category: &FileCategory,
    bold: bool,
    colors: &ColorPalette,
) -> Style {
    let color = Theme::category_color_with_colors(*category, colors);
    Theme::panel_item_with_colors(color, bold, colors)
}

pub fn get_file_icon(category: &FileCategory) -> &'static str {
    get_file_icon_with_theme(category, IconTheme::default())
}

macro_rules! impl_default_colors {
    ($vis:vis fn $name:ident(f: &mut Frame, area: Rect $(, $($arg:ident : $ty:ty),+ $(,)?)?) =>
     $with:ident, $($default:expr),* $(,)?) => {
        $vis fn $name(f: &mut Frame, area: Rect $(, $($arg: $ty),+)?) {
            $with(f, area $(, $($arg),+)?, $($default),*);
        }
    };
}

pub fn get_file_icon_with_theme(category: &FileCategory, theme: IconTheme) -> &'static str {
    match theme {
        IconTheme::Ascii => match category {
            FileCategory::Dir => "D",
            FileCategory::Symlink => "@",
            FileCategory::Executable => "*",
            FileCategory::Code => "{",
            FileCategory::Config => "#",
            FileCategory::Archive => "A",
            FileCategory::Image => "I",
            FileCategory::Video => "V",
            FileCategory::Audio => "~",
            FileCategory::Document => "=",
            FileCategory::Font => "F",
            FileCategory::Other => ".",
        },
        IconTheme::NerdFont => match category {
            FileCategory::Dir => "",
            FileCategory::Symlink => "",
            FileCategory::Executable => "",
            FileCategory::Code => "",
            FileCategory::Config => "",
            FileCategory::Archive => "",
            FileCategory::Image => "",
            FileCategory::Video => "",
            FileCategory::Audio => "",
            FileCategory::Document => "",
            FileCategory::Font => "",
            FileCategory::Other => "",
        },
        IconTheme::Emoji => match category {
            FileCategory::Dir => "📁",
            FileCategory::Symlink => "🔗",
            FileCategory::Executable => "⚡",
            FileCategory::Code => "💻",
            FileCategory::Config => "⚙",
            FileCategory::Archive => "📦",
            FileCategory::Image => "🖼",
            FileCategory::Video => "🎬",
            FileCategory::Audio => "🎵",
            FileCategory::Document => "📝",
            FileCategory::Font => "🔤",
            FileCategory::Other => "📄",
        },
    }
}

fn truncate_to_width<'a>(s: &'a str, max_width: usize) -> Cow<'a, str> {
    if max_width == 0 {
        return Cow::Borrowed("");
    }
    let truncate_to = max_width.saturating_sub(1);
    let mut taken = 0;
    let mut truncate_byte = None;
    let mut truncate_width = 0;
    for (byte_idx, ch) in s.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if truncate_byte.is_none() && taken + cw > truncate_to {
            truncate_byte = Some(byte_idx);
            truncate_width = taken;
        }
        if taken + cw > max_width {
            if taken == max_width && truncate_width + 1 < max_width {
                return Cow::Borrowed(&s[..byte_idx]);
            }
            let tb = truncate_byte.unwrap_or(byte_idx);
            let mut result = String::with_capacity(tb + 3);
            result.push_str(&s[..tb]);
            result.push('\u{2026}');
            return Cow::Owned(result);
        }
        taken += cw;
    }
    Cow::Borrowed(s)
}

fn truncate_name<'a>(name: &'a str, max_width: usize) -> Cow<'a, str> {
    truncate_to_width(name, max_width)
}

impl_default_colors! {
    pub fn render_panel(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) =>
    render_panel_with_colors, &ColorPalette::default(), IconTheme::default()
}

pub fn render_panel_with_colors(
    f: &mut Frame,
    area: Rect,
    panel: &PanelState,
    is_active: bool,
    colors: &ColorPalette,
    icon_theme: IconTheme,
) {
    let border_style = if is_active {
        Theme::border_active_with_colors(colors)
    } else {
        Theme::border_inactive_with_colors(colors)
    };

    let path_lossy = panel.path().to_string_lossy();
    let mut title = String::with_capacity(path_lossy.len() + 2);
    title.push(' ');
    title.push_str(&path_lossy);
    title.push(' ');

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Theme::title_with_colors(colors));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner_area);

    let start_idx = panel.scroll_offset.min(panel.listing.entries.len());
    let end_idx = std::cmp::min(
        panel.listing.entries.len(),
        start_idx + inner_area.height as usize,
    );

    let mut list_items = Vec::with_capacity(end_idx.saturating_sub(start_idx));
    let mut scratch = String::with_capacity(256);

    for entry in panel
        .listing
        .entries
        .iter()
        .skip(start_idx)
        .take(end_idx.saturating_sub(start_idx))
    {
        let cat = entry.category();
        let bold = entry.is_dir() || entry.is_executable();

        scratch.clear();
        let string_line = match panel.listing_mode() {
            ListingMode::Long => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_entry_line(
                    entry,
                    width,
                    panel.show_permissions(),
                    &cat,
                    icon_theme,
                    &mut scratch,
                )
            }
            ListingMode::Brief => {
                let width = chunks[0].width.saturating_sub(2) as usize;
                format_brief_entry_line(entry, width, &cat, icon_theme, &mut scratch)
            }
        };

        let line_style = if entry.selected {
            get_file_color_with_palette(&cat, bold, colors).fg(colors.selected_file_fg)
        } else {
            get_file_color_with_palette(&cat, bold, colors)
        };

        list_items.push(ListItem::new(Span::styled(string_line, line_style)));
    }

    let highlight_style = if is_active {
        Theme::highlight_with_colors(colors)
    } else {
        Theme::panel_with_colors(colors)
    };

    let list = List::new(list_items)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .highlight_style(highlight_style);

    let mut list_state = ListState::default();
    if panel.cursor >= start_idx && panel.cursor < end_idx {
        list_state.select(Some(panel.cursor - start_idx));
    }

    f.render_stateful_widget(list, chunks[0], &mut list_state);

    if panel.listing.entries.is_empty()
        && let Some(err) = panel.last_error()
    {
        let err_text =
            Paragraph::new(format!(" Error: {err}")).style(Theme::error_with_colors(colors));
        f.render_widget(err_text, chunks[0]);
    }

    if !panel.listing.entries.is_empty() {
        scratch.clear();
        render_scrollbar_with_colors(f, chunks[1], panel, is_active, colors, &mut scratch);
    }
}

fn build_suffix_into(
    entry: &FileEntry,
    width: usize,
    show_permissions: bool,
    buf: &mut String,
) -> usize {
    let size_width = entry.size_width;
    let date_width = entry.time_width;
    let size_str = &entry.size_str;
    let date_str = &entry.time_str;
    let size_date_width = size_width + date_width + 2;

    if show_permissions {
        let perms_str = format_permissions(entry.mode_bits());
        let perms_width = UnicodeWidthStr::width(perms_str.as_str());
        let full_width = size_date_width + perms_width + 1;
        if 2 + full_width <= width {
            write!(buf, " {size_str} {date_str} {perms_str}").ok();
            return full_width;
        }
    }

    if 2 + size_date_width <= width {
        write!(buf, " {size_str} {date_str}").ok();
        size_date_width
    } else if 2 + size_width <= width {
        write!(buf, " {size_str}").ok();
        size_width + 1
    } else {
        0
    }
}

fn format_entry_line(
    entry: &FileEntry,
    width: usize,
    show_permissions: bool,
    category: &FileCategory,
    icon_theme: IconTheme,
    scratch: &mut String,
) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    if width <= 1 {
        return format!("{marker}");
    }

    let display_name = entry.display_name();
    let display_name_width = UnicodeWidthStr::width(display_name);

    let icon = get_file_icon_with_theme(category, icon_theme);
    let icon_width = icon_display_width(icon_theme);

    scratch.clear();
    let suffix_width = build_suffix_into(entry, width, show_permissions, scratch);

    let available_name_width = width.saturating_sub(1 + suffix_width);
    if available_name_width == 0 {
        return format!("{marker}");
    }

    let mut out = String::with_capacity(width + 32);
    out.push(marker);

    let name_actual_width = if display_name_width < usize::MAX - icon_width {
        let name_with_icon_total = icon_width + 1 + display_name_width;
        if name_with_icon_total <= available_name_width {
            out.push_str(icon);
            out.push(' ');
            out.push_str(display_name);
            name_with_icon_total
        } else {
            let name_budget = available_name_width.saturating_sub(icon_width + 1);
            if name_budget > 0 {
                let truncated = truncate_to_width(display_name, name_budget);
                out.push_str(icon);
                out.push(' ');
                out.push_str(&truncated);
                icon_width + 1 + UnicodeWidthStr::width(&*truncated)
            } else {
                let truncated = truncate_to_width(icon, available_name_width);
                let w = UnicodeWidthStr::width(&*truncated);
                out.push_str(&truncated);
                w
            }
        }
    } else {
        0
    };

    let padding = available_name_width.saturating_sub(name_actual_width);
    out.extend(std::iter::repeat_n(' ', padding));
    out.push_str(scratch.as_str());
    out
}

fn write_status_metadata(buf: &mut String, size: &str, entry: &FileEntry, show_permissions: bool) {
    if show_permissions {
        let perms = format_permissions(entry.mode_bits());
        write!(buf, "{size} | {perms} | {} | {}", entry.owner, entry.group).ok();
    } else {
        write!(buf, "{size} | {} | {}", entry.owner, entry.group).ok();
    }
}

fn format_brief_entry_line(
    entry: &FileEntry,
    width: usize,
    category: &FileCategory,
    icon_theme: IconTheme,
    scratch: &mut String,
) -> String {
    let marker = if entry.selected { '*' } else { ' ' };
    let display_name = entry.display_name();
    let display_name_width = UnicodeWidthStr::width(display_name);

    let icon = get_file_icon_with_theme(category, icon_theme);
    let icon_width = icon_display_width(icon_theme) + 1;
    let available = width.saturating_sub(1);
    if available == 0 {
        return format!("{marker}");
    }
    if available < icon_width {
        return format!("{marker}");
    }

    scratch.clear();
    write!(scratch, "{marker}{icon} ").ok();

    let name_available = available - icon_width;
    if name_available >= display_name_width {
        scratch.push_str(display_name);
    } else if name_available == 0 {
        scratch.pop();
    } else {
        let truncated = truncate_name(display_name, name_available);
        scratch.push_str(&truncated);
    }
    scratch.clone()
}

pub fn render_scrollbar(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let mut buf = String::new();
    render_scrollbar_with_colors(
        f,
        area,
        panel,
        is_active,
        &ColorPalette::default(),
        &mut buf,
    );
}

pub fn render_scrollbar_with_colors(
    f: &mut Frame,
    area: Rect,
    panel: &PanelState,
    is_active: bool,
    colors: &ColorPalette,
    buf: &mut String,
) {
    if panel.listing.entries.is_empty() {
        return;
    }

    let total_entries = panel.listing.entries.len();
    let height = area.height as usize;
    let max_scroll = total_entries.saturating_sub(height);
    let scroll_offset = panel.scroll_offset.min(max_scroll);

    let thumb_height = if total_entries <= height {
        1
    } else {
        ((height * height) / total_entries).max(1).min(height)
    };

    let thumb_pos = if max_scroll > 0 && height > 1 {
        let track = height.saturating_sub(thumb_height);
        (scroll_offset * track) / max_scroll
    } else {
        0
    };

    let style = if is_active {
        Style::default().fg(colors.scrollbar_active)
    } else {
        Style::default().fg(colors.scrollbar_inactive)
    };

    buf.clear();
    buf.reserve(height * 4);
    for i in 0..height {
        let in_thumb = i >= thumb_pos && i < thumb_pos + thumb_height && total_entries > height;
        buf.push_str(if in_thumb { "█" } else { "│" });
        if i < height - 1 {
            buf.push('\n');
        }
    }

    let paragraph = Paragraph::new(buf.as_str())
        .style(style)
        .block(Block::default().padding(Padding::new(0, 0, 0, 0)));

    f.render_widget(paragraph, area);
}

pub fn panel_status_summary(panel: &PanelState, buf: &mut String) -> usize {
    buf.clear();
    let total = panel.listing.entries.len();
    if total == 0 {
        return 0;
    }

    let pos = (panel.cursor + 1).min(total);
    let pct = pos * 100 / total;

    write!(buf, " {}/{} {}%", pos, total, pct).ok();

    if panel.selected_count() > 0 {
        write!(
            buf,
            " ({} {})",
            panel.selected_count(),
            format_size(panel.selected_size())
        )
        .ok();
    }

    buf.push(' ');
    UnicodeWidthStr::width(buf.as_str())
}

impl_default_colors! {
    pub fn render_status_bar(f: &mut Frame, area: Rect, panel: &PanelState) =>
    render_status_bar_with_colors, &ColorPalette::default()
}

pub fn render_status_bar_with_colors(
    f: &mut Frame,
    area: Rect,
    panel: &PanelState,
    colors: &ColorPalette,
) {
    let available = area.width as usize;

    let mut scratch = String::with_capacity(128);
    let right_width = panel_status_summary(panel, &mut scratch);
    let right_summary = scratch.clone();
    let remaining = available.saturating_sub(right_width);

    let mut out = String::with_capacity(remaining + right_summary.len() + 8);

    if !panel.listing.entries.is_empty() && panel.cursor < panel.listing.entries.len() {
        let entry = &panel.listing.entries[panel.cursor];
        let display_name = entry.display_name();
        let size_str = format_size(entry.size());

        scratch.clear();
        write_status_metadata(&mut scratch, &size_str, entry, panel.show_permissions());
        let meta_width = UnicodeWidthStr::width(scratch.as_str());

        let full_width = UnicodeWidthStr::width(display_name) + 3 + meta_width;

        if full_width <= remaining {
            out.push_str(display_name);
            out.push_str(" | ");
            out.push_str(&scratch);
        } else {
            let meta_with_sep_width = meta_width + 3;
            let name_budget = remaining.saturating_sub(meta_with_sep_width);

            if name_budget >= 3 {
                let truncated = truncate_to_width(display_name, name_budget);
                out.push_str(&truncated);
                out.push_str(" | ");
                out.push_str(&scratch);
            } else {
                scratch.clear();
                write!(scratch, "{display_name} | ").ok();
                write_status_metadata(&mut scratch, &size_str, entry, panel.show_permissions());
                let truncated = truncate_to_width(&scratch, remaining);
                out.push_str(&truncated);
            }
        }
    }

    let info_line_width = UnicodeWidthStr::width(out.as_str());
    let padding = remaining.saturating_sub(info_line_width);
    out.extend(std::iter::repeat_n(' ', padding));
    out.push_str(&right_summary);

    let paragraph = Paragraph::new(out)
        .style(Theme::status_bar_with_colors(colors))
        .block(Block::default());

    f.render_widget(paragraph, area);
}

impl_default_colors! {
    pub fn render_function_bar(f: &mut Frame, area: Rect) =>
    render_function_bar_with_colors, &ColorPalette::default()
}

pub fn render_function_bar_with_colors(f: &mut Frame, area: Rect, colors: &ColorPalette) {
    const CONSTRAINTS: [Constraint; 10] = [Constraint::Percentage(10); 10];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(CONSTRAINTS)
        .split(area);

    let key_style = Style::default()
        .fg(colors.function_bar_fg)
        .bg(colors.function_bar_bg)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default()
        .fg(colors.function_bar_fg)
        .bg(colors.function_bar_bg);

    for i in 0..10 {
        let line = Line::from(vec![
            Span::styled(FN_KEY_TEXTS[i], key_style),
            Span::styled(FN_LABEL_TEXTS[i], label_style),
        ]);
        let paragraph =
            Paragraph::new(line).block(Block::default().padding(Padding::new(1, 1, 0, 0)));

        f.render_widget(paragraph, chunks[i]);
    }
}

impl_default_colors! {
    pub fn render_menu_bar(f: &mut Frame, area: Rect) =>
    render_menu_bar_with_colors, &ColorPalette::default()
}

pub fn render_menu_bar_with_colors(f: &mut Frame, area: Rect, colors: &ColorPalette) {
    f.render_widget(
        Paragraph::new("").style(Theme::menu_bar_with_colors(colors)),
        area,
    );

    let menu_text = "   Left   File   Command   Options   Right   ";
    let text_width = UnicodeWidthStr::width(menu_text) as u16;
    let clipped_width = text_width.min(area.width);
    let x = area.x + area.width.saturating_sub(text_width) / 2;
    let centered_area = Rect::new(x, area.y, clipped_width, area.height);

    let paragraph = Paragraph::new(menu_text)
        .style(Theme::menu_bar_with_colors(colors))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, centered_area);
}

#[cfg(test)]
mod tests;
