use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::dir_tree::TreeEntry;
use crate::ui::theme::Theme;

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

    let visible_height = inner.height.saturating_sub(1) as usize;

    let effective_scroll = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };

    let start = effective_scroll;
    let end = (start + visible_height).min(entries.len());

    for (offset, entry) in entries[start..end].iter().enumerate() {
        let row = start + offset;
        let y = inner.y + offset as u16;
        if y >= inner.y + inner.height {
            break;
        }

        let indent = "  ".repeat(entry.depth);
        let prefix = if entry.is_dir {
            if entry.expanded { "- " } else { "+ " }
        } else {
            "  "
        };

        let line_style = if row == selected {
            Theme::highlight()
        } else if entry.is_dir {
            Theme::panel_file(Theme::DIRECTORY)
        } else {
            Theme::panel_file(Theme::REGULAR_FILE)
        };

        let text = format!("{}{}{}", indent, prefix, entry.name);
        let para = Paragraph::new(text).style(line_style);
        let row_area = Rect::new(inner.x, y, inner.width, 1);
        f.render_widget(para, row_area);
    }

    let bottom_y = inner.y + inner.height.saturating_sub(1);
    let bottom_area = Rect::new(inner.x, bottom_y, inner.width, 1);
    let help_text = " Enter: expand/collapse  c: cd  Esc: close  PgUp/PgDn: scroll";
    let help_para = Paragraph::new(help_text).style(Theme::warning());
    f.render_widget(help_para, bottom_area);
}
