use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Single-line editable text field with a grapheme-cluster cursor.
///
/// # Invariants
///
/// * `cursor <= grapheme_count` at all times.
/// * `grapheme_count` is kept in sync with `text` and is O(1) to read.
/// * `scroll_offset` reflects the display-column offset of the left edge of the
///   visible window, snapped to grapheme boundaries, whenever `visible_width > 0`.
///
// INVARIANT: `cursor` MUST be ≤ `grapheme_count`. `grapheme_count` MUST match
// `text.graphemes(true).count()`. Direct mutation of `.text`/`.cursor` BREAKS these
// without immediate call to `recompute_grapheme_count()`/`cursor_end()` (for text)
// or `clamp_cursor()` (for cursor). Prefer `set_text()` / `set_cursor()`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextInput {
    text: String,
    cursor: usize,
    grapheme_count: usize,
    scroll_offset: usize,
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
            scroll_offset: 0,
            visible_width: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn set_visible_width(&mut self, width: usize) {
        self.visible_width = width;
        self.recompute_scroll_offset();
    }

    fn cursor_display(&self) -> usize {
        self.text
            .graphemes(true)
            .take(self.cursor)
            .map(UnicodeWidthStr::width)
            .sum()
    }

    fn recompute_scroll_offset(&mut self) {
        if self.visible_width == 0 {
            return;
        }
        let cursor_display = self.cursor_display();
        let raw_scroll = cursor_display.saturating_sub(self.visible_width.saturating_sub(1));
        if raw_scroll == 0 {
            self.scroll_offset = 0;
            return;
        }
        let widths: Vec<usize> = self
            .text
            .graphemes(true)
            .map(UnicodeWidthStr::width)
            .collect();
        let start_idx = widths
            .iter()
            .scan(0usize, |cum, &w| {
                let c = *cum;
                *cum += w;
                Some(c)
            })
            .position(|cum| cum >= raw_scroll)
            .unwrap_or(0);
        self.scroll_offset = widths[..start_idx].iter().sum();
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.recompute_grapheme_count();
        self.clamp_cursor();
        self.recompute_scroll_offset();
    }

    pub fn set_text_at_end(&mut self, text: String) {
        self.text = text;
        self.recompute_grapheme_count();
        self.cursor = self.grapheme_count;
        self.recompute_scroll_offset();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.clamp_cursor();
        self.recompute_scroll_offset();
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
        self.scroll_offset = 0;
        std::mem::take(&mut self.text)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.grapheme_count = 0;
        self.scroll_offset = 0;
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
        self.clamp_cursor();
        let pos = self.byte_pos();
        self.text.insert(pos, c);
        if c.is_ascii() {
            self.cursor += 1;
            self.grapheme_count += 1;
        } else {
            self.cursor = self.text[..pos + c.len_utf8()].graphemes(true).count();
            self.recompute_grapheme_count();
        }
        self.recompute_scroll_offset();
    }

    pub fn backspace(&mut self) -> bool {
        self.clamp_cursor();
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.grapheme_count -= 1;
        let pos = self.byte_pos();
        self.delete_grapheme_at(pos);
        self.recompute_scroll_offset();
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        self.clamp_cursor();
        let pos = self.byte_pos();
        if pos >= self.text.len() {
            return false;
        }
        self.delete_grapheme_at(pos);
        self.grapheme_count -= 1;
        self.recompute_scroll_offset();
        true
    }

    pub fn cursor_left(&mut self) {
        self.clamp_cursor();
        self.cursor = self.cursor.saturating_sub(1);
        self.recompute_scroll_offset();
    }

    pub fn cursor_right(&mut self) {
        self.clamp_cursor();
        if self.cursor < self.grapheme_count {
            self.cursor += 1;
        }
        self.recompute_scroll_offset();
    }

    pub fn cursor_start(&mut self) {
        self.cursor = 0;
        self.recompute_scroll_offset();
    }

    pub fn cursor_end(&mut self) {
        self.recompute_grapheme_count();
        self.cursor = self.grapheme_count;
        self.recompute_scroll_offset();
    }

    pub fn delete_word_backward(&mut self) -> bool {
        self.clamp_cursor();
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
        self.recompute_scroll_offset();
        removed_graphemes > 0
    }

    pub fn drain_to_start(&mut self) {
        self.clamp_cursor();
        let pos = self.byte_pos();
        let removed = self.cursor;
        self.text.drain(..pos);
        self.cursor = 0;
        self.grapheme_count -= removed;
        self.recompute_scroll_offset();
    }
}
