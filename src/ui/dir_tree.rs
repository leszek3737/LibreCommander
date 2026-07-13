use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::app::dir_tree::TreeEntry;
use crate::ui::theme::{ColorPalette, Theme};

const HELP_TEXT: &str = " Enter: expand/collapse  c: cd  Esc: close  PgUp/PgDn: scroll";

/// 80 spaces — supports up to 40 indentation levels (2 spaces each).
/// Deeper nesting silently stops indenting; this is fine for practical use.
const INDENT_BUF: &str =
    "                                                                                ";

fn indent_for_depth(depth: usize) -> &'static str {
    let needed = depth * 2;
    &INDENT_BUF[..needed.min(INDENT_BUF.len())]
}

/// Maximum indentation depth `INDENT_BUF` can represent (2 spaces per level).
const MAX_INDENT_DEPTH: usize = INDENT_BUF.len() / 2;

/// Precomputed visual width of `indent_for_depth(depth)` for each depth.
/// All indent characters are spaces, so width == byte count == `depth * 2`
/// (clamped at `INDENT_BUF` length). Avoids a `UnicodeWidthStr::width` scan
/// per visible entry, per frame.
const INDENT_WIDTHS: [usize; MAX_INDENT_DEPTH + 1] = {
    let mut widths = [0usize; MAX_INDENT_DEPTH + 1];
    let mut depth = 0;
    while depth <= MAX_INDENT_DEPTH {
        widths[depth] = depth * 2;
        depth += 1;
    }
    widths
};

fn indent_width_for_depth(depth: usize) -> usize {
    INDENT_WIDTHS[depth.min(MAX_INDENT_DEPTH)]
}

/// Truncates `name` to `max_width` columns, appending `…` if it does not fit.
/// `name_width` is the caller-known display width of `name`; passing it avoids
/// recomputing it on the hot per-entry render path.
fn truncate_name_to_width(name: &str, name_width: usize, max_width: usize) -> Cow<'_, str> {
    if max_width == 0 {
        return Cow::Borrowed("");
    }
    if name_width <= max_width {
        return Cow::Borrowed(name);
    }
    let truncate_to = max_width.saturating_sub(1);
    let mut result = String::with_capacity(max_width + 1);
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
    Cow::Owned(result)
}

/// Convenience wrapper for callers that do not already know the display width
/// (e.g. static help text). Hot paths should call `truncate_name_to_width`
/// directly with a precomputed width.
fn truncate_name(name: &str, max_width: usize) -> Cow<'_, str> {
    truncate_name_to_width(name, UnicodeWidthStr::width(name), max_width)
}

fn render_tree_scrollbar(
    f: &mut Frame,
    area: Rect,
    total_entries: usize,
    mut scroll_offset: usize,
    visible_height: usize,
    colors: &ColorPalette,
) {
    if total_entries == 0 || area.height == 0 {
        return;
    }

    let height = area.height as usize;
    let max_scroll = total_entries.saturating_sub(visible_height);
    scroll_offset = scroll_offset.min(max_scroll);

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

    let style = Style::default().fg(colors.scrollbar_active);
    // Write glyphs straight into the frame buffer; avoids a per-frame `String`
    // allocation (the scrollbar is a single column, one glyph per row).
    let buf = f.buffer_mut();
    for i in 0..height {
        let in_thumb = i >= thumb_pos && i < thumb_pos + thumb_height && total_entries > height;
        let glyph = if in_thumb { "█" } else { "│" };
        buf.set_stringn(area.x, area.y + i as u16, glyph, 1, style);
    }
}

fn render_tree_entries(
    f: &mut Frame,
    entries: &[TreeEntry],
    row_range: Range<usize>,
    selected: usize,
    inner: Rect,
    content_width: u16,
    colors: &ColorPalette,
) {
    let row_start = row_range.start;
    let max_x = inner.x.saturating_add(content_width);
    let buf = f.buffer_mut();
    for (offset, entry) in entries[row_range].iter().enumerate() {
        let row = row_start + offset;
        let y = inner.y + offset as u16;

        let indent = indent_for_depth(entry.depth);
        let indent_width = indent_width_for_depth(entry.depth);
        let prefix = if entry.is_dir && entry.read_error {
            "! "
        } else if entry.is_dir {
            if entry.expanded { "- " } else { "+ " }
        } else {
            "  "
        };

        let prefix_width = UnicodeWidthStr::width(prefix);
        let available = (content_width as usize)
            .saturating_sub(indent_width)
            .saturating_sub(prefix_width);
        let display_name = truncate_name_to_width(entry.name.as_str(), entry.name_width, available);

        let line_style = if row == selected {
            Theme::highlight_with_colors(colors)
        } else if entry.is_dir {
            Theme::panel_file_with_colors(colors.directory, colors)
        } else {
            Theme::panel_file_with_colors(colors.regular_file, colors)
        };

        // Write the three segments (all sharing `line_style`) directly into the
        // buffer, clipped to `content_width`. Avoids a per-entry `Vec<Span>` +
        // `Paragraph` allocation. `set_stringn` returns the next x position.
        let mut x = inner.x;
        x = buf
            .set_stringn(x, y, indent, max_x.saturating_sub(x) as usize, line_style)
            .0;
        x = buf
            .set_stringn(x, y, prefix, max_x.saturating_sub(x) as usize, line_style)
            .0;
        buf.set_stringn(
            x,
            y,
            display_name.as_ref(),
            max_x.saturating_sub(x) as usize,
            line_style,
        );
    }
}

pub fn render_directory_tree_with_colors(
    f: &mut Frame,
    tree_root: &Path,
    entries: &[TreeEntry],
    selected: usize,
    scroll: usize,
    colors: &ColorPalette,
) {
    let area = f.area();

    let bg_block = Block::default().style(Theme::panel_bg_with_colors(colors));
    f.render_widget(bg_block, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Directory Tree: {} ", tree_root.display()))
        .title_style(Theme::title_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    if entries.is_empty() {
        let placeholder = Paragraph::new("(empty directory)")
            .style(Theme::warning_with_colors(colors))
            .centered();
        f.render_widget(placeholder, inner);
        return;
    }

    let selected = selected.min(entries.len().saturating_sub(1));

    // Last row of inner area is reserved for the help bar (see below).
    let visible_height = inner.height.saturating_sub(1) as usize;

    if visible_height == 0 {
        return;
    }

    let effective_scroll = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };

    let start = effective_scroll;
    let end = (start + visible_height).min(entries.len());

    let has_scrollbar = entries.len() > visible_height && inner.width > 1;

    if has_scrollbar {
        let sb_area = Rect::new(
            inner.x + inner.width.saturating_sub(1),
            inner.y,
            1,
            inner.height.saturating_sub(1),
        );
        render_tree_scrollbar(
            f,
            sb_area,
            entries.len(),
            effective_scroll,
            visible_height,
            colors,
        );
    }

    let content_width = if has_scrollbar {
        inner.width.saturating_sub(1)
    } else {
        inner.width
    };
    render_tree_entries(
        f,
        entries,
        start..end,
        selected,
        inner,
        content_width,
        colors,
    );

    let bottom_y = inner.y + inner.height.saturating_sub(1);
    let bottom_area = Rect::new(inner.x, bottom_y, inner.width, 1);

    let avail = inner.width as usize;
    let help_width = UnicodeWidthStr::width(HELP_TEXT);
    if avail >= help_width {
        let help_para = Paragraph::new(HELP_TEXT).style(Theme::warning_with_colors(colors));
        f.render_widget(help_para, bottom_area);
    } else if avail > 1 {
        let truncated = truncate_name(HELP_TEXT, avail);
        let help_para = Paragraph::new(truncated).style(Theme::warning_with_colors(colors));
        f.render_widget(help_para, bottom_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn make_entry(name: &str, depth: usize, is_dir: bool, expanded: bool) -> TreeEntry {
        TreeEntry {
            path: PathBuf::from(name),
            depth,
            is_dir,
            expanded,
            name: name.to_string(),
            name_width: UnicodeWidthStr::width(name),
            read_error: false,
        }
    }

    fn render(entries: &[TreeEntry], selected: usize, scroll: usize) -> Terminal<TestBackend> {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_directory_tree_with_colors(
                    f,
                    Path::new("/test"),
                    entries,
                    selected,
                    scroll,
                    &crate::ui::theme::ColorPalette::default(),
                );
            })
            .unwrap();
        terminal
    }

    #[test]
    fn truncate_name_short_name_unchanged() {
        assert_eq!(truncate_name("foo", 10), "foo");
    }

    #[test]
    fn truncate_name_exact_fit() {
        assert_eq!(truncate_name("foo", 3), "foo");
    }

    #[test]
    fn truncate_name_truncates_with_ellipsis() {
        assert_eq!(truncate_name("hello world", 8), "hello w…");
    }

    #[test]
    fn truncate_name_zero_width_returns_empty() {
        assert_eq!(truncate_name("foo", 0), "");
    }

    #[test]
    fn truncate_name_single_char_width() {
        assert_eq!(truncate_name("abc", 1), "…");
    }

    #[test]
    fn truncate_name_handles_unicode() {
        assert_eq!(truncate_name("zażółć", 5), "zażó…");
    }

    #[test]
    fn render_empty_entries_no_panic() {
        let terminal = render(&[], 0, 0);
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Directory Tree"));
    }

    #[test]
    fn render_single_entry_no_panic() {
        let entries = vec![make_entry("src", 0, true, false)];
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("+ src"));
    }

    #[test]
    fn render_many_entries_shows_scrollbar() {
        let entries: Vec<TreeEntry> = (0..50)
            .map(|i| make_entry(&format!("file{i:02}"), 0, false, false))
            .collect();
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let sb_x = 38u16;
        assert_eq!(buffer[(sb_x, 1)].symbol(), "█");
    }

    #[test]
    fn render_few_entries_no_scrollbar() {
        let entries = vec![
            make_entry("a", 0, false, false),
            make_entry("b", 0, false, false),
        ];
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let sb_x = 38u16;
        assert_eq!(buffer[(sb_x, 1)].symbol(), " ");
    }

    #[test]
    fn render_long_filename_is_truncated() {
        let long_name = "a_very_long_filename_that_should_be_truncated_by_the_renderer";
        let entries = vec![make_entry(long_name, 0, false, false)];
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(!content.contains(long_name));
    }

    #[test]
    fn render_nested_entries_with_indent() {
        let entries = vec![
            make_entry("src", 0, true, true),
            make_entry("main.rs", 1, false, false),
        ];
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("- src"));
        assert!(content.contains("main.rs"));
    }

    #[test]
    fn render_selected_highlighted() {
        let entries = vec![
            make_entry("a", 0, false, false),
            make_entry("b", 0, false, false),
        ];
        let terminal = render(&entries, 1, 0);
        let buffer = terminal.backend().buffer();
        let row_b_y = 2u16;
        let cell_style = buffer[(2, row_b_y)].style();
        assert_ne!(cell_style, Style::default());
    }

    #[test]
    fn render_help_bar_visible() {
        let entries = vec![make_entry("a", 0, false, false)];
        let terminal = render(&entries, 0, 0);
        let buffer = terminal.backend().buffer();
        let bottom_y = 8u16;
        let content: String = (0..40)
            .map(|x| buffer[(x as u16, bottom_y)].symbol())
            .collect();
        assert!(content.contains("Enter"));
    }

    #[test]
    fn indent_widths_table_matches_unicode_width() {
        for depth in [0usize, 1, 2, 5, 10, MAX_INDENT_DEPTH] {
            let expected = UnicodeWidthStr::width(indent_for_depth(depth));
            assert_eq!(indent_width_for_depth(depth), expected, "depth {depth}");
        }
    }

    #[test]
    fn indent_width_clamps_beyond_max_depth() {
        let beyond = MAX_INDENT_DEPTH + 25;
        assert_eq!(
            indent_width_for_depth(beyond),
            UnicodeWidthStr::width(indent_for_depth(beyond)),
        );
        assert_eq!(indent_width_for_depth(beyond), MAX_INDENT_DEPTH * 2);
    }

    #[test]
    fn truncate_provided_width_matches_recomputed() {
        let names = ["foo", "hello world", "zażółć", "日本語ファイル", "a🍕b"];
        for name in names {
            let w = UnicodeWidthStr::width(name);
            for max in 0..=(w + 2) {
                let provided = truncate_name_to_width(name, w, max);
                let recomputed = truncate_name(name, max);
                assert_eq!(provided, recomputed, "name={name:?} max={max}");
            }
        }
    }

    #[test]
    fn truncate_wide_cjk_respects_column_width() {
        // Each CJK glyph is 2 columns wide; "日本語" is 6 columns.
        assert_eq!(truncate_name("日本語", 6), "日本語");
        // max_width 5 -> keep within 4 columns then "…": only "日本" (4) + "…" fits.
        assert_eq!(truncate_name("日本語", 5), "日本…");
        // A 2-column glyph cannot fit in a single remaining column before "…".
        assert_eq!(truncate_name("日本語", 2), "…");
    }
}
