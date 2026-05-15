use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::app::dir_tree::TreeEntry;
use crate::ui::theme::Theme;

const HELP_TEXT: &str = " Enter: expand/collapse  c: cd  Esc: close  PgUp/PgDn: scroll";

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

fn render_tree_scrollbar(
    f: &mut Frame,
    area: Rect,
    total_entries: usize,
    mut scroll_offset: usize,
    visible_height: usize,
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

    let mut scrollbar = String::with_capacity(height * 4);
    for i in 0..height {
        let is_last = i == height - 1;
        let in_thumb = i >= thumb_pos && i < thumb_pos + thumb_height && total_entries > height;
        if in_thumb {
            scrollbar.push_str(if is_last { "█" } else { "█\n" });
        } else {
            scrollbar.push_str(if is_last { "│" } else { "│\n" });
        }
    }

    let style = Style::default().fg(Theme::scrollbar_active());
    let paragraph = Paragraph::new(scrollbar).style(style);
    f.render_widget(paragraph, area);
}

pub fn render_directory_tree(
    f: &mut Frame,
    tree_root: &Path,
    entries: &[TreeEntry],
    selected: usize,
    scroll: usize,
) {
    let area = f.area();

    let bg_block = Block::default().style(Theme::panel_bg());
    f.render_widget(bg_block, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Directory Tree: {} ", tree_root.display()))
        .title_style(Theme::title());
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || entries.is_empty() {
        return;
    }

    let selected = selected.min(entries.len().saturating_sub(1));

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
    let content_width = if has_scrollbar {
        inner.width.saturating_sub(1)
    } else {
        inner.width
    };

    if has_scrollbar {
        let sb_area = Rect::new(
            inner.x + inner.width.saturating_sub(1),
            inner.y,
            1,
            inner.height.saturating_sub(1),
        );
        render_tree_scrollbar(f, sb_area, entries.len(), effective_scroll, visible_height);
    }

    for (offset, entry) in entries[start..end].iter().enumerate() {
        let row = start + offset;
        let y = inner.y + offset as u16;
        if y >= inner.y + inner.height.saturating_sub(1) {
            break;
        }

        let indent = "  ".repeat(entry.depth);
        let indent_width = UnicodeWidthStr::width(indent.as_str());
        let prefix = if entry.is_dir {
            if entry.expanded { "- " } else { "+ " }
        } else {
            "  "
        };

        let prefix_width = UnicodeWidthStr::width(prefix);
        let available = (content_width as usize)
            .saturating_sub(indent_width)
            .saturating_sub(prefix_width);
        let display_name = truncate_name(entry.name.as_str(), available);

        let line_style = if row == selected {
            Theme::highlight()
        } else if entry.is_dir {
            Theme::panel_file(Theme::directory())
        } else {
            Theme::panel_file(Theme::regular_file())
        };

        let line = Line::from(vec![
            Span::styled(indent, line_style),
            Span::styled(prefix, line_style),
            Span::styled(display_name, line_style),
        ]);
        let para = Paragraph::new(line);
        let row_area = Rect::new(inner.x, y, content_width, 1);
        f.render_widget(para, row_area);
    }

    let bottom_y = inner.y + inner.height.saturating_sub(1);
    let bottom_area = Rect::new(inner.x, bottom_y, inner.width, 1);

    let avail = inner.width as usize;
    let help_width = UnicodeWidthStr::width(HELP_TEXT);
    if avail >= help_width {
        let help_para = Paragraph::new(HELP_TEXT).style(Theme::warning());
        f.render_widget(help_para, bottom_area);
    } else if avail > 1 {
        let truncated = truncate_name(HELP_TEXT, avail);
        let help_para = Paragraph::new(truncated).style(Theme::warning());
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
        }
    }

    fn render(entries: &[TreeEntry], selected: usize, scroll: usize) -> Terminal<TestBackend> {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_directory_tree(f, Path::new("/test"), entries, selected, scroll);
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
}
