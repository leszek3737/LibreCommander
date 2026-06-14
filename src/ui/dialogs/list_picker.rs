use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::Style,
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::{centered_rect, dialog_block};

/// Placeholder shown when the picker has no items.
const EMPTY_PLACEHOLDER: &str = "(empty)";
/// Prefix drawn in front of the currently selected row.
const HIGHLIGHT_SYMBOL: &str = "> ";

/// Default-palette convenience wrapper over [`render_list_picker_with_colors`].
///
/// Kept as a separate public entry point because callers (e.g. tests) use the
/// default palette while `render.rs` always passes an explicit `ColorPalette`
/// via [`render_list_picker_with_colors`]. Both are part of the public API, so
/// they are not collapsed into one.
pub fn render_list_picker<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
) {
    render_list_picker_with_colors(f, title, items, selected, hint, &ColorPalette::default());
}

pub fn render_list_picker_with_colors<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
    colors: &ColorPalette,
) {
    let area = f.area();
    let picker_area = centered_rect(60, 70, area);

    f.render_widget(Clear, picker_area);
    let bg_block = ratatui::widgets::Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, picker_area);

    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(picker_area);
    f.render_widget(block, picker_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if items.is_empty() {
        let empty = Paragraph::new(EMPTY_PLACEHOLDER)
            .style(Style::default().fg(colors.regular_file))
            .alignment(Alignment::Center);
        f.render_widget(empty, chunks[0]);
    } else {
        let visible_height = chunks[0].height as usize;
        // Defensive: with zero rows there is nothing to draw and the windowing
        // below would build an empty `start_idx..end_idx` slice (and the
        // scroll position would vanish). The `Min(1)` constraint should keep
        // this >= 1, but never rely on layout for slice safety. The hint row
        // (`chunks[1]`) is still rendered below regardless.
        if visible_height > 0 {
            let clamped_selected = selected.min(items.len().saturating_sub(1));
            let half = visible_height / 2;
            let start_idx = if clamped_selected < half {
                0
            } else if clamped_selected + half >= items.len() {
                items.len().saturating_sub(visible_height)
            } else {
                clamped_selected - half
            };
            let end_idx = (start_idx + visible_height).min(items.len());
            let list = List::new(
                items[start_idx..end_idx]
                    .iter()
                    .map(|s| ListItem::new(s.as_ref())),
            )
            .highlight_style(Theme::highlight_bold_with_colors(colors))
            .highlight_symbol(HIGHLIGHT_SYMBOL);
            let mut list_state = ListState::default();
            list_state.select(Some(clamped_selected - start_idx));
            f.render_stateful_widget(list, chunks[0], &mut list_state);
        }
    }

    let hint_para = Paragraph::new(hint)
        .style(Theme::warning_with_colors(colors))
        .alignment(Alignment::Center);
    f.render_widget(hint_para, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Render into a [`TestBackend`] and return the full buffer as a flat string
    /// (row-major, no newlines). Used by tests that verify rendered content.
    fn render(items: &[&str], selected: usize, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| render_list_picker(f, "Pick", items, selected, "hint"))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
        }
        out
    }

    /// Convenience wrapper for tests that only need a no-panic guarantee.
    fn draw(items: &[&str], selected: usize, width: u16, height: u16) {
        render(items, selected, width, height);
    }

    #[test]
    fn empty_list_renders_without_panic() {
        let rendered = render(&[], 0, 60, 20);
        // The empty-state placeholder must appear in the buffer, not just survive.
        assert!(
            rendered.contains(EMPTY_PLACEHOLDER),
            "empty placeholder must appear in buffer; got: {rendered:?}"
        );
    }

    #[test]
    fn single_item_renders_without_panic() {
        let rendered = render(&["only"], 0, 60, 20);
        // Item text must be present and the highlight symbol must mark the row.
        assert!(
            rendered.contains("only"),
            "item text must appear in buffer; got: {rendered:?}"
        );
        assert!(
            rendered.contains(HIGHLIGHT_SYMBOL),
            "highlight symbol must appear on the selected row; got: {rendered:?}"
        );
        // Selecting the lone item in a tight layout must also be fine.
        draw(&["only"], 0, 40, 8);
    }

    #[test]
    fn out_of_range_selected_is_clamped_no_panic() {
        let items = ["a", "b", "c", "d", "e"];
        // `selected` far past the end must clamp instead of panicking on the
        // slice / `select` index.
        let rendered = render(&items, 999, 60, 20);
        // After clamping, the highlight symbol must appear (not a blank/garbage row).
        assert!(
            rendered.contains(HIGHLIGHT_SYMBOL),
            "highlight must appear even when selected is clamped; got: {rendered:?}"
        );
        // The last item "e" (clamped-to index 4) must be visible in the viewport.
        assert!(
            rendered.contains('e'),
            "last item must be visible after out-of-range selected is clamped; got: {rendered:?}"
        );
    }

    #[test]
    fn minimal_area_renders_without_panic() {
        // Very small terminal exercises the tight-layout / small-viewport path.
        draw(&["a", "b", "c"], 2, 30, 5);
    }
}
