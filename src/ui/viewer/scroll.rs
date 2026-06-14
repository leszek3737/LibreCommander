use super::hex::{HEX_BYTES_PER_LINE, HEX_LINE_WIDTH};
use super::open::ViewerState;

impl ViewerState {
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    #[must_use]
    pub(crate) fn is_visual_scroll(&self) -> bool {
        self.wrap_lines
            && !self.is_hex_mode()
            && !self.render_cache.visual_heights.borrow().is_empty()
    }

    #[must_use]
    pub(crate) fn total_visual_rows(&self) -> usize {
        self.render_cache
            .visual_offsets
            .borrow()
            .last()
            .copied()
            .unwrap_or(0)
    }

    #[must_use]
    pub(crate) fn visual_row_to_logical(&self, visual_row: usize) -> (usize, usize) {
        const LINEAR_SEARCH_THRESHOLD: usize = 24;
        let heights = self.render_cache.visual_heights.borrow();
        if heights.len() <= LINEAR_SEARCH_THRESHOLD {
            let mut acc = 0usize;
            for (i, &h) in heights.iter().enumerate() {
                let next = match acc.checked_add(h) {
                    Some(v) => v,
                    None => return Self::past_end_logical(self.line_count),
                };
                if next > visual_row {
                    return (i, visual_row - acc);
                }
                acc = next;
            }
            return Self::past_end_logical(self.line_count);
        }
        drop(heights);
        let offsets = self.render_cache.visual_offsets.borrow();
        let idx = offsets.partition_point(|&offset| offset <= visual_row);
        if idx >= offsets.len() {
            return Self::past_end_logical(self.line_count);
        }
        let acc_before = if idx == 0 { 0 } else { offsets[idx - 1] };
        (idx, visual_row - acc_before)
    }

    /// Visual row at which logical line `logical_line` *starts* (its first
    /// wrapped row). The inverse of [`Self::visual_row_to_logical`] for the
    /// sub-row-0 case.
    #[must_use]
    pub(crate) fn logical_line_visual_start(&self, logical_line: usize) -> usize {
        if logical_line == 0 {
            0
        } else {
            self.render_cache
                .visual_offsets
                .borrow()
                .get(logical_line - 1)
                .copied()
                .unwrap_or_else(|| self.total_visual_rows())
        }
    }

    pub(crate) fn total_rows(&self) -> usize {
        if self.is_hex_mode() {
            self.raw_bytes.len().div_ceil(HEX_BYTES_PER_LINE)
        } else if self.is_visual_scroll() {
            self.total_visual_rows()
        } else {
            self.line_count
        }
    }

    #[must_use]
    pub(crate) fn max_scroll(&self) -> usize {
        self.total_rows().saturating_sub(1)
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = clamp_scroll_offset(self.scroll_offset, lines, self.max_scroll());
    }

    pub fn page_up(&mut self, page_height: usize) {
        let step = page_height.max(1);
        self.scroll_up(step);
    }

    pub fn page_down(&mut self, page_height: usize) {
        let step = page_height.max(1);
        self.scroll_down(step);
    }

    pub fn go_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn go_to_bottom(&mut self, page_height: usize) {
        self.scroll_offset = self
            .total_rows()
            .saturating_sub(page_height)
            .min(self.max_scroll());
    }

    pub fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }

    pub fn scroll_left(&mut self, cols: usize) {
        self.horizontal_offset = self.horizontal_offset.saturating_sub(cols);
    }

    pub fn scroll_right(&mut self, cols: usize, visible_width: usize) {
        let line_num_width = if self.show_line_numbers {
            self.render_cache.cached_line_num_col_width.get()
        } else {
            0
        };
        let effective_width = visible_width.saturating_sub(line_num_width);
        let max_line = if self.is_hex_mode() {
            HEX_LINE_WIDTH
        } else {
            self.max_line_width
        };
        let max_offset = max_line.saturating_sub(effective_width);
        self.horizontal_offset = clamp_scroll_offset(self.horizontal_offset, cols, max_offset);
    }

    pub fn needs_image_preview(&self, area_width: u16, area_height: u16) -> bool {
        let content_height = area_height.saturating_sub(3);
        area_width > 0
            && content_height > 0
            && self.render_cache.cached_image_size != Some((area_width, content_height))
    }

    pub fn set_image_preview(
        &mut self,
        width: u16,
        height: u16,
        text: ratatui::text::Text<'static>,
    ) {
        self.render_cache.cached_image_size = Some((width, height));
        self.render_cache.cached_image_text = Some(text);
    }

    fn past_end_logical(line_count: usize) -> (usize, usize) {
        (line_count.saturating_sub(1), 0)
    }
}

/// Advances a scroll offset by `delta`, saturating against overflow and
/// clamping the result to `max`. Shared by the vertical (`scroll_down`) and
/// horizontal (`scroll_right`) helpers so the saturate-then-clamp arithmetic
/// lives in one place.
fn clamp_scroll_offset(offset: usize, delta: usize, max: usize) -> usize {
    offset.saturating_add(delta).min(max)
}

pub(crate) fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).ilog10() as usize + 1
}

pub(crate) fn line_number_column_width(line_count: usize) -> usize {
    line_number_digits(line_count) + 2
}

pub(crate) fn paragraph_horizontal_scroll(horizontal_offset: usize) -> u16 {
    horizontal_offset.min(u16::MAX as usize) as u16
}
