use crate::app::types::ViewMode;

use super::mime::is_image_mime;
use super::open::ViewerState;
use super::scroll::line_number_column_width;

impl ViewerState {
    fn invalidate_visual_cache(&self) {
        self.render_cache.visual_heights.borrow_mut().clear();
        self.render_cache.visual_offsets.borrow_mut().clear();
        *self.render_cache.cached_content_width.borrow_mut() = 0;
    }

    fn next_view_mode(&self) -> ViewMode {
        if self.is_hex_mode() {
            if is_image_mime(self.detected_mime.as_deref()) {
                ViewMode::Image
            } else {
                ViewMode::Text
            }
        } else {
            ViewMode::Hex
        }
    }

    pub fn toggle_line_numbers(&mut self) {
        if self.is_image_mode() {
            return;
        }
        self.show_line_numbers = !self.show_line_numbers;
        self.invalidate_visual_cache();
    }

    pub fn toggle_wrap(&mut self) {
        if self.is_image_mode() {
            return;
        }
        self.wrap_lines = !self.wrap_lines;
        if self.wrap_lines {
            self.horizontal_offset = 0;
        }
        self.invalidate_visual_cache();
    }

    pub fn toggle_hex_mode(&mut self) {
        self.view_mode = self.next_view_mode();
        self.scroll_offset = 0;
        self.horizontal_offset = 0;
        self.clear_search_results();

        if self.view_mode == ViewMode::Text && self.originally_binary {
            self.line_offsets = Self::compute_line_offsets(&self.raw_bytes);
            self.line_count = if self.raw_bytes.is_empty() {
                1
            } else {
                self.line_offsets.len()
            };
            self.render_cache
                .cached_line_num_col_width
                .set(line_number_column_width(self.line_count));
            self.max_line_width = if self.raw_bytes.is_empty() {
                0
            } else {
                Self::compute_max_line_width(&self.line_offsets, &self.raw_bytes)
            };
        }
    }

    /// Interior mutability is used here so that `render` (which borrows `&self`)
    /// can update the wrap layout cache when the content width changes.
    pub fn update_wrap_layout(&self, content_width: usize) {
        if !self.wrap_lines || self.is_hex_mode() || self.line_count == 0 {
            if !self.render_cache.visual_heights.borrow().is_empty() {
                self.invalidate_visual_cache();
            }
            return;
        }
        if *self.render_cache.cached_content_width.borrow() == content_width
            && !self.render_cache.visual_heights.borrow().is_empty()
        {
            return;
        }
        let line_num_width = if self.show_line_numbers {
            self.render_cache.cached_line_num_col_width.get()
        } else {
            0
        };
        let width = content_width.max(1);
        /// Upper bound to prevent OOM on pathological input (e.g. a 4 GB
        /// file with millions of short lines).  Lines beyond this limit are
        /// truncated visually rather than fully laid out.
        const MAX_VISUAL_LINES: usize = 1_000_000;
        let cap = self.line_count.min(MAX_VISUAL_LINES);
        let mut new_heights = Vec::with_capacity(cap);
        for i in (0..self.line_count).take(MAX_VISUAL_LINES) {
            let line = self.get_line(i);
            let text_width = unicode_width::UnicodeWidthStr::width(line.as_ref());
            let total_width = line_num_width.saturating_add(text_width);
            new_heights.push(total_width.div_ceil(width).max(1));
        }
        if new_heights.len() < self.line_count {
            self.invalidate_visual_cache();
            return;
        }
        let mut new_offsets = Vec::with_capacity(new_heights.len());
        let mut acc = 0usize;
        for &h in &new_heights {
            acc += h;
            new_offsets.push(acc);
        }
        *self.render_cache.visual_heights.borrow_mut() = new_heights;
        *self.render_cache.visual_offsets.borrow_mut() = new_offsets;
        *self.render_cache.cached_content_width.borrow_mut() = content_width;
    }
}
