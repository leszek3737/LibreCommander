use std::cell::RefCell;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;

pub fn render_input_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    prompt: &str,
    value: &str,
    cursor_pos: usize,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    let prompt_paragraph = Paragraph::new(prompt);
    f.render_widget(prompt_paragraph, chunks[0]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain);
    let input_block = if value.is_empty() {
        input_block.border_style(Theme::warning_with_colors(colors))
    } else {
        input_block
    };
    let input_inner = input_block.inner(chunks[1]);

    let visible_width = input_inner.width as usize;
    // Narrow-terminal guard. When the input's inner viewport collapses to zero
    // width or height (terminal only a few columns/rows wide), there is no room
    // to show the value or a meaningful cursor. Degrade gracefully: render the
    // (clipped) input block and clamp the cursor into that block's area so it
    // never lands outside the dialog. This branch must never panic.
    if visible_width == 0 || input_inner.height == 0 {
        f.render_widget(input_block, chunks[1]);
        let cursor_x = input_inner.x.min(chunks[1].right().saturating_sub(1));
        let cursor_y = input_inner.y.min(chunks[1].bottom().saturating_sub(1));
        f.set_cursor_position((cursor_x, cursor_y));
        return;
    }

    let window = compute_visible_window(value, cursor_pos, visible_width);

    let cursor_x = input_inner.x + window.cursor_col.min(window.width) as u16;
    let cursor_y = input_inner.y;

    let input_paragraph = Paragraph::new(window.text).block(input_block);
    f.render_widget(input_paragraph, chunks[1]);
    f.set_cursor_position((cursor_x, cursor_y));
}

/// Result of horizontally scrolling an input value into a fixed-width viewport.
///
/// Shared by input and archive dialogs for grapheme-aware horizontal scroll.
pub(crate) struct VisibleWindow {
    /// The substring that fits within the viewport, ready to render.
    pub text: String,
    /// Cursor column (0-based, terminal cells) relative to the visible slice.
    pub cursor_col: usize,
    /// Total display width (terminal cells) occupied by `text`.
    pub width: usize,
}

/// A single grapheme cluster of an input value: its byte range within the
/// source string plus its display width in terminal cells.
///
/// Storing byte offsets (rather than owned `String`s) lets the cache keep one
/// backing allocation for the whole value while staying trivially `Copy`.
#[derive(Clone, Copy)]
struct GraphemeSegment {
    start: usize,
    end: usize,
    width: usize,
}

/// Per-thread memoization of the grapheme segmentation of the last rendered
/// input value.
///
/// Rendering stays a pure function of its arguments: this cache only avoids
/// re-segmenting an unchanged value across consecutive frames/keystrokes. It is
/// NOT application state and never changes the output for a given `value`.
struct SegmentCache {
    value: String,
    segments: Vec<GraphemeSegment>,
}

impl SegmentCache {
    /// Ensure `segments` describes `value`, recomputing (single pass) only when
    /// the value changed since the previous call. On a cache hit no allocation
    /// happens; on a miss both the value buffer and the segment vec are reused.
    fn refresh(&mut self, value: &str) {
        if self.value == value {
            return;
        }
        self.value.clear();
        self.value.push_str(value);
        self.segments.clear();
        // Single pass: byte offsets and widths are derived together. The
        // previous implementation collected a `Vec<&str>` of graphemes *and* a
        // separate `Vec<usize>` of widths (two allocations) per frame.
        for (start, g) in value.grapheme_indices(true) {
            self.segments.push(GraphemeSegment {
                start,
                end: start + g.len(),
                width: UnicodeWidthStr::width(g),
            });
        }
    }
}

thread_local! {
    static SEGMENT_CACHE: RefCell<SegmentCache> = const {
        RefCell::new(SegmentCache {
            value: String::new(),
            segments: Vec::new(),
        })
    };
}

pub(crate) fn compute_visible_window(
    value: &str,
    cursor_pos: usize,
    visible_width: usize,
) -> VisibleWindow {
    SEGMENT_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        cache.refresh(value);
        // `value` is byte-identical to `cache.value` after `refresh`, so the
        // cached byte offsets index it on valid grapheme boundaries.
        compute_window_from_segments(value, &cache.segments, cursor_pos, visible_width)
    })
}

fn compute_window_from_segments(
    source: &str,
    segments: &[GraphemeSegment],
    cursor_pos: usize,
    visible_width: usize,
) -> VisibleWindow {
    // Display-only clamp: the event layer owns the authoritative cursor bound
    // (0..=grapheme_count). We clamp here solely so rendering a transiently
    // out-of-range cursor cannot index out of bounds; we never mutate state.
    let clamped_cursor = cursor_pos.min(segments.len());
    let cursor_display: usize = segments[..clamped_cursor].iter().map(|s| s.width).sum();

    let scroll_display = cursor_display.saturating_sub(visible_width.saturating_sub(1));

    // Left-edge anchor (grapheme index). With no horizontal scroll we anchor at
    // the start; otherwise we pick the first grapheme whose starting column
    // reaches `scroll_display`. If none does (e.g. only trailing zero-width
    // marks remain) we fall back to the start. Folding both the no-scroll and
    // the fallback cases into one path lets `build_visible` be called once.
    let start_idx = if scroll_display == 0 {
        0
    } else {
        segments
            .iter()
            .scan(0usize, |cum, seg| {
                let col = *cum;
                *cum += seg.width;
                Some(col)
            })
            .position(|col| col >= scroll_display)
            .unwrap_or(0)
    };

    let start_cum: usize = segments[..start_idx].iter().map(|s| s.width).sum();
    let (text, width) = build_visible(source, segments, start_idx, visible_width);
    VisibleWindow {
        text,
        // `start_cum <= cursor_display`: `start_idx` is the first grapheme whose
        // start column reaches `scroll_display <= cursor_display`, so it never
        // sits past the cursor — the subtraction cannot underflow.
        cursor_col: cursor_display - start_cum,
        width,
    }
}

/// Concatenate graphemes starting at `start` until adding the next one would
/// exceed `max_width`, returning the rendered slice and its display width.
fn build_visible(
    source: &str,
    segments: &[GraphemeSegment],
    start: usize,
    max_width: usize,
) -> (String, usize) {
    let mut text = String::with_capacity(max_width.saturating_mul(4));
    let mut width = 0usize;
    for seg in &segments[start..] {
        if width + seg.width > max_width {
            break;
        }
        text.push_str(&source[seg.start..seg.end]);
        width += seg.width;
    }
    (text, width)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn ascii_window_fits_whole_value() {
        let w = compute_visible_window("hello", 5, 10);
        assert_eq!(w.text, "hello");
        assert_eq!(w.width, 5);
        assert_eq!(w.cursor_col, 5);
    }

    #[test]
    fn wide_graphemes_use_display_width_not_count() {
        // Each CJK ideograph is one grapheme of display width 2.
        let w = compute_visible_window("你好世界", 4, 10);
        assert_eq!(w.text, "你好世界");
        assert_eq!(w.width, 8, "four wide graphemes occupy eight cells");
        assert_eq!(w.cursor_col, 8);
    }

    #[test]
    fn wide_graphemes_scroll_keeps_cursor_in_view() {
        // Viewport of 4 cells, cursor at the end: window scrolls right.
        let w = compute_visible_window("你好世界", 4, 4);
        assert_eq!(w.text, "界");
        assert_eq!(w.width, 2);
        assert_eq!(w.cursor_col, 2);
        assert!(w.cursor_col <= w.width.max(4));
    }

    #[test]
    fn combining_marks_count_as_single_zero_extra_width() {
        // "e" + combining acute is one grapheme of width 1.
        let value = "e\u{0301}llo";
        let w = compute_visible_window(value, 4, 10);
        assert_eq!(w.text, value);
        assert_eq!(w.width, 4, "combining mark adds no extra display width");
        assert_eq!(w.cursor_col, 4);
    }

    #[test]
    fn emoji_grapheme_has_width_two() {
        let w = compute_visible_window("😀x", 2, 10);
        assert_eq!(w.text, "😀x");
        assert_eq!(w.width, 3);
        assert_eq!(w.cursor_col, 3);
    }

    #[test]
    fn zwj_emoji_is_one_grapheme() {
        // Family emoji joined with ZWJ collapses to a single grapheme cluster.
        let family = "👨\u{200d}👩\u{200d}👧";
        let w = compute_visible_window(family, 1, 10);
        assert_eq!(w.text, family);
        assert_eq!(w.cursor_col, w.width, "cursor sits past the single cluster");
    }

    #[test]
    fn zero_width_viewport_does_not_panic() {
        // Defensive: render guards against width 0, but the computation itself
        // must also stay panic-free if ever reached with a zero viewport.
        let w = compute_visible_window("hello", 3, 0);
        assert_eq!(w.text, "");
        assert_eq!(w.width, 0);
        assert_eq!(w.cursor_col, 0);
    }

    #[test]
    fn cursor_beyond_length_is_clamped() {
        let w = compute_visible_window("ab", 99, 10);
        assert_eq!(w.text, "ab");
        assert_eq!(
            w.cursor_col, 2,
            "cursor clamped to grapheme count for display"
        );
    }

    #[test]
    fn cache_returns_consistent_result_across_calls() {
        // Two consecutive calls with the same value hit the cache; a changed
        // value invalidates it. All must produce correct, equal results.
        let a = compute_visible_window("café", 4, 10);
        let b = compute_visible_window("café", 4, 10);
        assert_eq!(a.text, b.text);
        assert_eq!(a.width, b.width);
        assert_eq!(a.cursor_col, b.cursor_col);

        let c = compute_visible_window("你好", 2, 10);
        assert_eq!(c.text, "你好");
        assert_eq!(c.width, 4);

        let d = compute_visible_window("café", 4, 10);
        assert_eq!(d.text, a.text, "cache miss/refresh restores correct value");
    }

    #[test]
    fn narrow_terminal_render_does_not_panic() {
        // Tiny backend forces the inner input viewport to zero width, exercising
        // the narrow-terminal guard in `render_input_dialog`.
        let mut terminal = Terminal::new(TestBackend::new(3, 8)).expect("backend");
        let res = terminal.draw(|f| {
            render_input_dialog(f, f.area(), "T", "P:", "value", 2, &DEFAULT_COLORS);
        });
        assert!(res.is_ok(), "narrow-terminal render must not fail or panic");
    }

    #[test]
    fn wide_grapheme_value_renders_without_panic() {
        // Normal-sized backend with a wide-grapheme value renders cleanly.
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("backend");
        let res = terminal.draw(|f| {
            render_input_dialog(f, f.area(), "标题", "输入:", "你好世界", 4, &DEFAULT_COLORS);
        });
        assert!(res.is_ok(), "wide-grapheme render must not fail or panic");
    }
}
