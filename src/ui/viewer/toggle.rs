use crate::app::mime::is_image_mime;
use crate::app::types::ViewMode;

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
        // Reset horizontal scroll on every toggle, both directions. Wrap-mode
        // rendering ignores `horizontal_offset`, but `scroll_right` keeps
        // growing it while wrapped; without this reset the view would jump by
        // that stale offset the moment wrap is turned back off. (Pairs with the
        // `mode_dispatch` `scroll_right` wrap-guard tracked in PR8.)
        self.horizontal_offset = 0;
        self.invalidate_visual_cache();
    }

    pub fn toggle_hex_mode(&mut self) {
        self.view_mode = self.next_view_mode();
        self.scroll_offset = 0;
        self.horizontal_offset = 0;
        self.clear_search_results();
        // The wrap layout is mode- and line-count-specific; drop it so the new
        // mode rebuilds it instead of reusing a stale (binary↔text) layout.
        self.invalidate_visual_cache();

        if self.view_mode == ViewMode::Text && self.originally_binary {
            let (line_offsets, line_count, max_line_width) =
                Self::compute_text_metrics(&self.raw_bytes);
            self.line_offsets = line_offsets;
            self.line_count = line_count;
            self.max_line_width = max_line_width;
            self.render_cache
                .cached_line_num_col_width
                .set(line_number_column_width(self.line_count));
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
        // Above this many logical lines, skip visual (wrapped) layout entirely
        // and fall back to logical-line scrolling. Laying out every line builds
        // two usize-per-line vectors; for a multi-million-line file that is both
        // an OOM risk and -- because the result would otherwise be discarded and
        // the cache left empty -- a full re-scan every frame. Short-circuit here
        // so the per-frame cost stays O(1).
        const MAX_VISUAL_LINES: usize = 1_000_000;
        if self.line_count > MAX_VISUAL_LINES {
            // Defensively drop any stale layout so logical-line scrolling can't
            // pick up heights from a smaller prior state.
            if !self.render_cache.visual_heights.borrow().is_empty() {
                self.invalidate_visual_cache();
            }
            return;
        }
        let mut new_heights = Vec::with_capacity(self.line_count);
        for i in 0..self.line_count {
            let total_width = line_num_width.saturating_add(self.line_width(i));
            new_heights.push(total_width.div_ceil(width).max(1));
        }
        let mut new_offsets = Vec::with_capacity(new_heights.len());
        let mut acc = 0usize;
        for &h in &new_heights {
            // Guard the cumulative offset against `usize` overflow (only reachable
            // for absurd inputs). Storing a wrapped/partial offset would corrupt
            // the binary search in `visual_row_to_logical`; instead drop the cache
            // and fall back to logical-line scrolling, mirroring the `checked_add`
            // guard on the linear-search path in `scroll.rs`.
            match acc.checked_add(h) {
                Some(v) => {
                    acc = v;
                    new_offsets.push(acc);
                }
                None => {
                    self.invalidate_visual_cache();
                    return;
                }
            }
        }
        *self.render_cache.visual_heights.borrow_mut() = new_heights;
        *self.render_cache.visual_offsets.borrow_mut() = new_offsets;
        *self.render_cache.cached_content_width.borrow_mut() = content_width;
    }
}
