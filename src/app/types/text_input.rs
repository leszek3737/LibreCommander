use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Single-line editable text field with a grapheme-cluster cursor.
///
/// # Invariants
///
/// * `cursor <= grapheme_count` at all times.
/// * `grapheme_count` is kept in sync with `text` and is O(1) to read.
///
/// The horizontal scroll offset is **derived** state: it is computed on demand
/// from `text`, `cursor` and `visible_width` by [`TextInput::scroll_offset`],
/// so no mutator needs to refresh it.
///
// INVARIANT: `cursor` MUST be ≤ `grapheme_count`, and `grapheme_count` MUST
// match `text.graphemes(true).count()`. These are upheld at a SINGLE set of
// entry points — `set_text`, `set_text_at_end`, `set_cursor`, `cursor_end`,
// `cursor_start`, `take_text`, `clear` — each of which (re)computes the count
// and/or clamps the cursor. Every other mutator only moves the cursor inside the
// already-valid range, so it relies on the invariant instead of re-clamping
// defensively. Direct mutation of the private `text`/`cursor` fields bypasses
// this and MUST be followed by `recompute_grapheme_count()` + `clamp_cursor()`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextInput {
    text: String,
    cursor: usize,
    grapheme_count: usize,
    visible_width: usize,
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new()
    }
}

fn is_whitespace_grapheme(g: &str) -> bool {
    g.chars().all(|c| c.is_whitespace())
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            grapheme_count: 0,
            visible_width: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Display-column offset of the left edge of the visible window, snapped to
    /// a grapheme boundary. Derived on demand from the current text, cursor and
    /// `visible_width`; returns 0 when `visible_width == 0`.
    pub fn scroll_offset(&self) -> usize {
        self.current_scroll_offset()
    }

    pub fn set_visible_width(&mut self, width: usize) {
        self.visible_width = width;
    }

    // O(n) in the cursor index, but only invoked from `current_scroll_offset`,
    // which itself runs only when `scroll_offset()` is read (cursor positioning
    // on a mouse click) — not on the per-keystroke edit path — so caching it
    // would add invalidation complexity for no measurable win.
    fn cursor_display(&self) -> usize {
        self.text
            .graphemes(true)
            .take(self.cursor)
            .map(UnicodeWidthStr::width)
            .sum()
    }

    /// Centralized derivation of the horizontal scroll offset. This is the only
    /// place the scroll window is computed; mutators never refresh it eagerly.
    fn current_scroll_offset(&self) -> usize {
        if self.visible_width == 0 {
            return 0;
        }
        let cursor_display = self.cursor_display();
        let raw_scroll = cursor_display.saturating_sub(self.visible_width.saturating_sub(1));
        if raw_scroll == 0 {
            return 0;
        }
        let widths: Vec<usize> = self
            .text
            .graphemes(true)
            .map(UnicodeWidthStr::width)
            .collect();
        // When the cursor's (last) grapheme is wider than `visible_width`, no
        // boundary's cumulative offset reaches `raw_scroll`, so `position`
        // yields `None`. Fall back to the last grapheme boundary — i.e. scroll
        // as far right as possible — so the cursor stays visible on the right
        // edge instead of snapping back to the start.
        let start_idx = widths
            .iter()
            .scan(0usize, |cum, &w| {
                let c = *cum;
                *cum += w;
                Some(c)
            })
            .position(|cum| cum >= raw_scroll)
            .unwrap_or(widths.len().saturating_sub(1));
        widths[..start_idx].iter().sum()
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.recompute_grapheme_count();
        self.clamp_cursor();
    }

    pub fn set_text_at_end(&mut self, text: String) {
        self.text = text;
        self.recompute_grapheme_count();
        self.cursor = self.grapheme_count;
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.clamp_cursor();
    }

    pub fn recompute_grapheme_count(&mut self) {
        self.grapheme_count = self.text.graphemes(true).count();
    }

    pub fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.grapheme_count);
    }

    pub fn take_text(&mut self) -> String {
        self.cursor = 0;
        self.grapheme_count = 0;
        std::mem::take(&mut self.text)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.grapheme_count = 0;
    }

    pub fn grapheme_count(&self) -> usize {
        self.grapheme_count
    }

    pub fn byte_pos(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn next_grapheme_end(&self, byte_offset: usize) -> usize {
        self.text[byte_offset..]
            .graphemes(true)
            .next()
            .map(|g| byte_offset + g.len())
            .unwrap_or(self.text.len())
    }

    fn delete_grapheme_at(&mut self, pos: usize) {
        let end = self.next_grapheme_end(pos);
        self.text.drain(pos..end);
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.byte_pos();
        self.text.insert(pos, c);
        if c.is_ascii() {
            self.cursor += 1;
            self.grapheme_count += 1;
        } else {
            // A non-ASCII char may extend an existing grapheme cluster (e.g. a
            // combining mark merges with the previous grapheme), so the new
            // cursor index cannot be derived by a simple `+1`. Recompute both
            // counts via segmentation. Kept O(n) deliberately: an incremental
            // O(1) update would be incorrect under grapheme-cluster merging.
            self.cursor = self.text[..pos + c.len_utf8()].graphemes(true).count();
            self.recompute_grapheme_count();
        }
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.grapheme_count -= 1;
        let pos = self.byte_pos();
        self.delete_grapheme_at(pos);
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        let pos = self.byte_pos();
        if pos >= self.text.len() {
            return false;
        }
        self.delete_grapheme_at(pos);
        self.grapheme_count -= 1;
        true
    }

    pub fn cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn cursor_right(&mut self) {
        if self.cursor < self.grapheme_count {
            self.cursor += 1;
        }
    }

    pub fn cursor_start(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.recompute_grapheme_count();
        self.cursor = self.grapheme_count;
    }

    pub fn delete_word_backward(&mut self) -> bool {
        let pos = self.byte_pos();
        if pos == 0 {
            return false;
        }
        let text = &self.text[..pos];
        let word_start = text
            .grapheme_indices(true)
            .rev()
            .skip_while(|&(_, g)| is_whitespace_grapheme(g))
            .find(|&(_, g)| is_whitespace_grapheme(g))
            .map(|(i, g)| i + g.len())
            .unwrap_or(0);
        let removed_graphemes = text[word_start..].graphemes(true).count();
        self.text.drain(word_start..pos);
        self.cursor = self.cursor.saturating_sub(removed_graphemes);
        self.grapheme_count -= removed_graphemes;
        removed_graphemes > 0
    }

    pub fn drain_to_start(&mut self) {
        let pos = self.byte_pos();
        let removed = self.cursor;
        self.text.drain(..pos);
        self.cursor = 0;
        self.grapheme_count -= removed;
    }
}
