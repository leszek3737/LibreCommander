use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};
use std::borrow::Cow;
use std::fmt::Write;
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;

use super::theme::{ColorCtx, ColorPalette, DEFAULT_COLORS, IconTheme, Theme};

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

pub fn render_panel(f: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let ctx = ColorCtx::defaults();
    render_panel_with_colors(f, area, panel, is_active, ctx.colors, ctx.icon_theme);
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

    let entry_count = panel.listing.entries.len();
    let start_idx = panel.scroll_offset.min(entry_count);
    let end_idx = std::cmp::min(entry_count, start_idx + inner_area.height as usize);

    let ctx = ColorCtx::new(colors, icon_theme);
    let mode = panel.listing_mode();
    let show_permissions = panel.show_permissions();
    let content_width = chunks[0].width.saturating_sub(2) as usize;

    let mut list_items = Vec::with_capacity(end_idx.saturating_sub(start_idx));
    // Reused across lines for the size/date/permissions suffix only. Each panel
    // line still needs its own `String` because the `List` widget owns them all
    // simultaneously, but the per-line buffer is pre-sized to avoid reallocs.
    let mut suffix_buf = String::with_capacity(64);

    for entry in panel
        .listing
        .entries
        .iter()
        .skip(start_idx)
        .take(end_idx.saturating_sub(start_idx))
    {
        let cat = entry.category();
        let bold = entry.is_dir() || entry.is_executable();

        let mut line = String::with_capacity(content_width + 8);
        match mode {
            ListingMode::Long => format_entry_line(
                entry,
                content_width,
                show_permissions,
                &cat,
                ctx,
                &mut suffix_buf,
                &mut line,
            ),
            ListingMode::Brief => {
                format_brief_entry_line(entry, content_width, &cat, ctx, &mut line)
            }
        }

        let line_style = if entry.selected {
            get_file_color_with_palette(&cat, bold, colors).fg(colors.selected_file_fg)
        } else {
            get_file_color_with_palette(&cat, bold, colors)
        };

        list_items.push(ListItem::new(Span::styled(line, line_style)));
    }

    let highlight_style = if is_active {
        Theme::highlight_with_colors(colors)
    } else {
        Theme::panel_with_colors(colors)
    };

    let list = List::new(list_items)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .highlight_style(highlight_style);

    // Event-layer contract: input handlers keep `panel.cursor` inside
    // `0..entries.len()`. We clamp here purely for DISPLAY so a transiently
    // stale cursor (e.g. a directory that shrank before the next input tick)
    // still highlights a valid row instead of selecting nothing. This does NOT
    // mutate AppState.
    let mut list_state = ListState::default();
    if entry_count > 0 {
        let cursor = panel.cursor.min(entry_count - 1);
        if cursor >= start_idx && cursor < end_idx {
            list_state.select(Some(cursor - start_idx));
        }
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
        render_scrollbar_with_colors(f, chunks[1], panel, is_active, colors, &mut suffix_buf);
    }
}

/// Appends the trailing metadata column (size, date and optionally permissions)
/// for `entry` to `buf` and returns its display width.
///
/// This has BOTH a side effect (writing into `buf`) and a return value: callers
/// size the preceding filename column from the returned width, so the two must
/// stay in sync. The suffix degrades gracefully to fit `width`: permissions are
/// dropped first, then the date, then the size, returning width 0 (writing
/// nothing) when not even the size fits.
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

/// Writes `icon`, a separating space and the (possibly truncated) `display_name`
/// into `out`, never exceeding `available_width` display columns. Returns the
/// display width actually consumed so the caller can pad the rest of the column.
fn write_icon_and_name(
    out: &mut String,
    icon: &str,
    icon_width: usize,
    display_name: &str,
    display_name_width: usize,
    available_width: usize,
) -> usize {
    // Guard the width arithmetic below against an overflow on pathological names.
    if display_name_width >= usize::MAX - icon_width {
        return 0;
    }

    let name_with_icon_total = icon_width + 1 + display_name_width;
    if name_with_icon_total <= available_width {
        out.push_str(icon);
        out.push(' ');
        out.push_str(display_name);
        return name_with_icon_total;
    }

    let name_budget = available_width.saturating_sub(icon_width + 1);
    if name_budget > 0 {
        let truncated = truncate_to_width(display_name, name_budget);
        out.push_str(icon);
        out.push(' ');
        out.push_str(&truncated);
        icon_width + 1 + UnicodeWidthStr::width(&*truncated)
    } else {
        // Not even the icon fits cleanly; truncate the icon into what we have.
        let truncated = truncate_to_width(icon, available_width);
        let w = UnicodeWidthStr::width(&*truncated);
        out.push_str(&truncated);
        w
    }
}

/// Renders one long-mode panel row directly into `out` (no per-line return
/// allocation). `suffix_buf` is a caller-owned scratch buffer reused across rows
/// for the trailing metadata column. `out` is expected to start empty.
fn format_entry_line(
    entry: &FileEntry,
    width: usize,
    show_permissions: bool,
    category: &FileCategory,
    ctx: ColorCtx,
    suffix_buf: &mut String,
    out: &mut String,
) {
    let marker = if entry.selected { '*' } else { ' ' };
    if width <= 1 {
        out.push(marker);
        return;
    }

    let display_name = entry.display_name();
    let display_name_width = UnicodeWidthStr::width(display_name);

    let icon = get_file_icon_with_theme(category, ctx.icon_theme);
    let icon_width = icon_display_width(ctx.icon_theme);

    suffix_buf.clear();
    let suffix_width = build_suffix_into(entry, width, show_permissions, suffix_buf);

    let available_name_width = width.saturating_sub(1 + suffix_width);
    if available_name_width == 0 {
        out.push(marker);
        return;
    }

    out.push(marker);
    let name_actual_width = write_icon_and_name(
        out,
        icon,
        icon_width,
        display_name,
        display_name_width,
        available_name_width,
    );

    let padding = available_name_width.saturating_sub(name_actual_width);
    out.extend(std::iter::repeat_n(' ', padding));
    out.push_str(suffix_buf.as_str());
}

fn write_status_metadata(buf: &mut String, size: &str, entry: &FileEntry, show_permissions: bool) {
    if show_permissions {
        let perms = format_permissions(entry.mode_bits());
        write!(buf, "{size} | {perms} | {} | {}", entry.owner, entry.group).ok();
    } else {
        write!(buf, "{size} | {} | {}", entry.owner, entry.group).ok();
    }
}

/// Renders one brief-mode panel row directly into `out` (expected to start
/// empty), avoiding the extra owned `String` the previous `scratch.clone()`
/// allocated for every visible row each frame.
fn format_brief_entry_line(
    entry: &FileEntry,
    width: usize,
    category: &FileCategory,
    ctx: ColorCtx,
    out: &mut String,
) {
    let marker = if entry.selected { '*' } else { ' ' };
    let display_name = entry.display_name();
    let display_name_width = UnicodeWidthStr::width(display_name);

    let icon = get_file_icon_with_theme(category, ctx.icon_theme);
    let icon_width = icon_display_width(ctx.icon_theme) + 1;
    let available = width.saturating_sub(1);
    // `icon_width` is always >= 2, so this also covers `available == 0`
    // (widths 0 and 1 leave room only for the selection marker).
    if available < icon_width {
        out.push(marker);
        return;
    }

    write!(out, "{marker}{icon} ").ok();

    let name_available = available - icon_width;
    if name_available >= display_name_width {
        out.push_str(display_name);
    } else if name_available == 0 {
        // Drop the trailing space after the icon when no room is left for a name.
        out.pop();
    } else {
        let truncated = truncate_name(display_name, name_available);
        out.push_str(&truncated);
    }
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

pub fn render_status_bar(f: &mut Frame, area: Rect, panel: &PanelState) {
    render_status_bar_with_colors(f, area, panel, &ColorPalette::default());
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

pub fn render_function_bar(f: &mut Frame, area: Rect) {
    render_function_bar_with_colors(f, area, &ColorPalette::default());
}

fn function_bar_styles(colors: &ColorPalette) -> (Style, Style) {
    let key_style = Style::default()
        .fg(colors.function_bar_fg)
        .bg(colors.function_bar_bg)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default()
        .fg(colors.function_bar_fg)
        .bg(colors.function_bar_bg);
    (key_style, label_style)
}

fn make_function_bar_cell(i: usize, key_style: Style, label_style: Style) -> Paragraph<'static> {
    let line = Line::from(vec![
        Span::styled(FN_KEY_TEXTS[i], key_style),
        Span::styled(FN_LABEL_TEXTS[i], label_style),
    ]);
    Paragraph::new(line).block(Block::default().padding(Padding::new(1, 1, 0, 0)))
}

/// The 10 function-bar cells for the built-in palette, built once. The bar is
/// static text and (for the default theme) static styling, so it is rendered by
/// reference instead of rebuilt every frame.
fn default_function_bar_cells() -> &'static [Paragraph<'static>; 10] {
    static CELLS: OnceLock<[Paragraph<'static>; 10]> = OnceLock::new();
    CELLS.get_or_init(|| {
        let (key_style, label_style) = function_bar_styles(&DEFAULT_COLORS);
        std::array::from_fn(|i| make_function_bar_cell(i, key_style, label_style))
    })
}

pub fn render_function_bar_with_colors(f: &mut Frame, area: Rect, colors: &ColorPalette) {
    const CONSTRAINTS: [Constraint; 10] = [Constraint::Percentage(10); 10];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(CONSTRAINTS)
        .split(area);

    // Common path: default function-bar colors -> reuse the precomputed cells.
    if colors.function_bar_fg == DEFAULT_COLORS.function_bar_fg
        && colors.function_bar_bg == DEFAULT_COLORS.function_bar_bg
    {
        let cells = default_function_bar_cells();
        for (cell, chunk) in cells.iter().zip(chunks.iter()) {
            f.render_widget(cell, *chunk);
        }
        return;
    }

    // Custom function-bar colors: build the cells for this frame.
    let (key_style, label_style) = function_bar_styles(colors);
    for (i, chunk) in chunks.iter().enumerate() {
        f.render_widget(make_function_bar_cell(i, key_style, label_style), *chunk);
    }
}

pub fn render_menu_bar(f: &mut Frame, area: Rect) {
    render_menu_bar_with_colors(f, area, &ColorPalette::default());
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
