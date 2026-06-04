use unicode_segmentation::UnicodeSegmentation;

/// Single-line editable text field with a grapheme-cluster cursor.
///
/// # Invariants
///
/// * `cursor <= grapheme_count` at all times (enforced by every mutating method).
/// * `grapheme_count` is kept in sync with `text` and is O(1) to read, except
///   after direct `.text =` assignments which must be followed by
///   [`recompute_grapheme_count`] or [`cursor_end`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextInput {
    pub text: String,
    pub cursor: usize,
    grapheme_count: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            grapheme_count: 0,
        }
    }

    pub fn recompute_grapheme_count(&mut self) {
        self.grapheme_count = self.text.graphemes(true).count();
    }

    pub fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.grapheme_count);
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
    }

    pub fn backspace(&mut self) -> bool {
        self.clamp_cursor();
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.grapheme_count -= 1;
        let pos = self.byte_pos();
        let end = self.text[pos..]
            .graphemes(true)
            .next()
            .map(|g| pos + g.len())
            .unwrap_or_else(|| self.text.len());
        self.text.drain(pos..end);
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        self.clamp_cursor();
        let pos = self.byte_pos();
        if pos >= self.text.len() {
            return false;
        }
        let end = self.text[pos..]
            .graphemes(true)
            .next()
            .map(|g| pos + g.len())
            .unwrap_or_else(|| self.text.len());
        self.text.drain(pos..end);
        self.grapheme_count -= 1;
        true
    }

    pub fn cursor_left(&mut self) {
        self.clamp_cursor();
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn cursor_right(&mut self) {
        self.clamp_cursor();
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
        self.clamp_cursor();
        let pos = self.byte_pos();
        if pos == 0 {
            return false;
        }
        let text = &self.text[..pos];
        let word_start = text
            .grapheme_indices(true)
            .rev()
            .skip_while(|&(_, g)| g.chars().all(|c| c.is_whitespace()))
            .find(|&(_, g)| g.chars().all(|c| c.is_whitespace()))
            .map(|(i, g)| i + g.len())
            .unwrap_or(0);
        let removed_graphemes = text[word_start..].graphemes(true).count();
        self.text.drain(word_start..pos);
        self.cursor = self.cursor.saturating_sub(removed_graphemes);
        self.grapheme_count -= removed_graphemes;
        removed_graphemes > 0
    }

    pub fn drain_to_start(&mut self) {
        self.clamp_cursor();
        let pos = self.byte_pos();
        let removed = self.cursor;
        self.text.drain(..pos);
        self.cursor = 0;
        self.grapheme_count -= removed;
    }
}
