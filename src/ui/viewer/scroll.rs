use super::hex::{HEX_BYTES_PER_LINE, HEX_LINE_WIDTH};
use super::open::ViewerState;

impl ViewerState {
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    #[must_use]
    pub(crate) fn is_visual_scroll(&self) -> bool {
        self.wrap_lines && !self.is_hex_mode() && !self.visual_heights.borrow().is_empty()
    }

    #[must_use]
    pub(crate) fn total_visual_rows(&self) -> usize {
        self.visual_offsets.borrow().last().copied().unwrap_or(0)
    }

    #[must_use]
    pub(crate) fn visual_row_to_logical(&self, visual_row: usize) -> (usize, usize) {
        const LINEAR_SEARCH_THRESHOLD: usize = 24;
        let heights = self.visual_heights.borrow();
        if heights.len() <= LINEAR_SEARCH_THRESHOLD {
            let mut acc = 0usize;
            for (i, &h) in heights.iter().enumerate() {
                if acc + h > visual_row {
                    return (i, visual_row - acc);
                }
                acc += h;
            }
            return (self.line_count.saturating_sub(1), 0);
        }
        drop(heights);
        let offsets = self.visual_offsets.borrow();
        let idx = offsets.partition_point(|&offset| offset <= visual_row);
        if idx >= offsets.len() {
            return (self.line_count.saturating_sub(1), 0);
        }
        let acc_before = if idx == 0 { 0 } else { offsets[idx - 1] };
        (idx, visual_row - acc_before)
    }

    #[must_use]
    pub(crate) fn logical_to_visual_row(&self, logical_line: usize) -> usize {
        if logical_line == 0 {
            0
        } else {
            self.visual_offsets
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
        let max_scroll = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
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
            line_number_column_width(self.line_count)
        } else {
            0
        };
        let effective_width = visible_width.saturating_sub(line_num_width);
        let max_line = if self.is_hex_mode() {
            HEX_LINE_WIDTH
        } else {
            self.max_line_width
        };
        let max_offset = if effective_width > 0 {
            max_line.saturating_sub(effective_width)
        } else {
            max_line
        };
        self.horizontal_offset = (self.horizontal_offset + cols).min(max_offset);
    }

    pub fn needs_image_preview(&self, area_width: u16, area_height: u16) -> bool {
        let content_height = area_height.saturating_sub(3);
        area_width > 0
            && content_height > 0
            && self.cached_image_size != Some((area_width, content_height))
    }

    pub fn set_image_preview(
        &mut self,
        width: u16,
        height: u16,
        text: ratatui::text::Text<'static>,
    ) {
        self.cached_image_size = Some((width, height));
        self.cached_image_text = Some(text);
    }
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
