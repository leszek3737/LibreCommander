use super::SearchLineMatch;
use super::hex::{HEX_BYTES_PER_LINE, HEX_OFFSET_PREFIX_WIDTH};
use super::open::ViewerState;

impl ViewerState {
    pub(crate) fn clear_search_results(&mut self) {
        if self.search_matches.capacity() > 1024 {
            self.search_matches = Vec::new();
        } else {
            self.search_matches.clear();
        }
        if self.search_matches_by_line.capacity() > 1024 {
            self.search_matches_by_line = Vec::new();
        } else {
            self.search_matches_by_line.clear();
        }
    }

    pub fn search(&mut self, query: &str, page_height: usize) {
        self.search_query = Some(query.to_string());
        self.clear_search_results();
        self.current_match = None;

        if query.is_empty() {
            return;
        }

        if self.is_hex_mode() {
            self.search_hex(query);
            return;
        }

        let lower_query: String = if query.is_ascii() {
            query.to_ascii_lowercase()
        } else {
            query.chars().flat_map(|c| c.to_lowercase()).collect()
        };

        let mut lower_buf = String::new();
        let mut byte_map_buf = Vec::new();
        let mut local_matches: Vec<(usize, usize, usize)> = Vec::with_capacity(64);
        let mut local_by_line: Vec<SearchLineMatch> = Vec::with_capacity(64);

        for line_idx in 0..self.line_count {
            let line = self.get_line(line_idx);
            build_lowercase_mapping(&line, &mut lower_buf, &mut byte_map_buf);
            let line_is_ascii = line.is_ascii();
            let mut search_start = 0;
            while let Some(pos) = lower_buf[search_start..].find(&lower_query) {
                let match_byte_start = search_start + pos;
                let match_byte_end = match_byte_start + lower_query.len();
                let orig_byte_start = byte_map_buf[match_byte_start];
                let mapped_end = byte_map_buf
                    .get(match_byte_end)
                    .copied()
                    .unwrap_or(line.len());
                let orig_byte_end = if mapped_end <= orig_byte_start && orig_byte_start < line.len()
                {
                    line[orig_byte_start..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| orig_byte_start + i)
                        .unwrap_or(line.len())
                } else {
                    mapped_end
                };
                let char_pos = if line_is_ascii {
                    orig_byte_start
                } else {
                    line[..orig_byte_start].chars().count()
                };
                let match_char_len = if line_is_ascii {
                    orig_byte_end.saturating_sub(orig_byte_start).max(1)
                } else {
                    line[orig_byte_start..orig_byte_end].chars().count().max(1)
                };
                let global_idx = local_matches.len();
                local_matches.push((line_idx, char_pos, match_char_len));
                local_by_line.push(SearchLineMatch {
                    line: line_idx,
                    global_idx,
                    start_byte: orig_byte_start,
                    end_byte: orig_byte_end,
                });
                search_start = match_byte_end;
            }
        }

        self.search_matches = local_matches;
        self.search_matches_by_line = local_by_line;

        let current_logical = if self.is_visual_scroll() {
            self.visual_row_to_logical(self.scroll_offset).0
        } else {
            self.scroll_offset
        };
        for (i, &(line_idx, _, _)) in self.search_matches.iter().enumerate() {
            if line_idx >= current_logical {
                self.current_match = Some(i);
                self.scroll_to_current_match(page_height);
                return;
            }
        }
        if !self.search_matches.is_empty() {
            self.current_match = Some(0);
            self.scroll_to_current_match(page_height);
        }
    }

    fn push_match_segments(
        search_matches: &mut Vec<(usize, usize, usize)>,
        search_matches_by_line: &mut Vec<SearchLineMatch>,
        start_byte: usize,
        match_byte_len: usize,
        global_idx: usize,
    ) {
        let bpl = HEX_BYTES_PER_LINE;
        let mut remaining = match_byte_len;
        let mut byte_offset = start_byte;
        let mut first_segment = true;

        while remaining > 0 {
            let line_idx = byte_offset / bpl;
            let byte_in_line = byte_offset % bpl;
            let hex_col =
                HEX_OFFSET_PREFIX_WIDTH + byte_in_line * 3 + if byte_in_line >= 8 { 1 } else { 0 };
            let segment_len = remaining.min(bpl - byte_in_line);
            let match_hex_len = segment_len * 3 - 1;

            if first_segment {
                search_matches.push((line_idx, hex_col, match_hex_len));
                first_segment = false;
            }
            search_matches_by_line.push(SearchLineMatch {
                line: line_idx,
                global_idx,
                start_byte: hex_col,
                end_byte: hex_col + match_hex_len,
            });

            byte_offset += segment_len;
            remaining -= segment_len;
        }
    }

    pub(crate) fn search_hex(&mut self, query: &str) {
        let lower_query: String = if query.is_ascii() {
            query.to_ascii_lowercase()
        } else {
            query.chars().flat_map(|c| c.to_lowercase()).collect()
        };
        let query_bytes = Self::parse_hex_query(&lower_query);

        if let Some(ref needle) = query_bytes {
            let mut pos = 0;
            while let Some(idx) = super::hex::find_bytes(&self.raw_bytes[pos..], needle) {
                let abs_offset = pos + idx;
                let global_idx = self.search_matches.len();
                Self::push_match_segments(
                    &mut self.search_matches,
                    &mut self.search_matches_by_line,
                    abs_offset,
                    needle.len(),
                    global_idx,
                );
                pos = abs_offset + 1;
            }
        } else {
            const CHUNK_SIZE: usize = 1024 * 1024;
            let query_lower_len = lower_query.len();
            let overlap_bytes = query_lower_len * 3 + 16;
            let total_len = self.raw_bytes.len();
            let mut chunk_start: usize = 0;
            let mut lower_buf = String::with_capacity(CHUNK_SIZE);
            let mut byte_map: Vec<usize> = Vec::with_capacity(CHUNK_SIZE);

            while chunk_start < total_len {
                let primary_end = (chunk_start + CHUNK_SIZE).min(total_len);
                let buf_end = (primary_end + overlap_bytes).min(total_len);
                let slice = &self.raw_bytes[chunk_start..buf_end];

                let lossy = String::from_utf8_lossy(slice);
                build_lowercase_mapping(&lossy, &mut lower_buf, &mut byte_map);

                let mut search_start = 0;
                while let Some(pos) = lower_buf[search_start..].find(&lower_query) {
                    let abs_pos = search_start + pos;
                    search_start = abs_pos + query_lower_len;

                    let orig_byte_in_slice = byte_map.get(abs_pos).copied().unwrap_or(abs_pos);
                    let orig_byte = chunk_start + orig_byte_in_slice;

                    if orig_byte >= primary_end {
                        continue;
                    }

                    let match_end_in_lower = abs_pos + lower_query.len();
                    let orig_byte_end_in_slice = byte_map
                        .get(match_end_in_lower)
                        .copied()
                        .unwrap_or(lossy.len());
                    let orig_byte_end = chunk_start + orig_byte_end_in_slice;

                    let match_byte_len = if orig_byte_end > orig_byte {
                        (orig_byte_end - orig_byte)
                            .min(self.raw_bytes.len().saturating_sub(orig_byte))
                    } else {
                        let max_len = self.raw_bytes.len().saturating_sub(orig_byte);
                        if max_len == 0 {
                            0
                        } else {
                            let first = self.raw_bytes[orig_byte];
                            let char_len = if first & 0x80 == 0 {
                                1
                            } else if first & 0xE0 == 0xC0 {
                                2
                            } else if first & 0xF0 == 0xE0 {
                                3
                            } else if first & 0xF8 == 0xF0 {
                                4
                            } else {
                                1
                            };
                            char_len.min(max_len)
                        }
                    };
                    if match_byte_len == 0 {
                        continue;
                    }

                    let global_idx = self.search_matches.len();
                    Self::push_match_segments(
                        &mut self.search_matches,
                        &mut self.search_matches_by_line,
                        orig_byte,
                        match_byte_len,
                        global_idx,
                    );
                }

                chunk_start = primary_end;
            }
        }

        if !self.search_matches.is_empty() {
            self.current_match = Some(0);
            self.scroll_offset = self.search_matches[0].0.min(self.max_scroll());
        }
    }

    pub(crate) fn parse_hex_query(query: &str) -> Option<Vec<u8>> {
        let cleaned: String = query.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.len() < 2 || !cleaned.len().is_multiple_of(2) {
            return None;
        }
        (0..cleaned.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).ok())
            .collect()
    }

    pub(crate) fn scroll_to_current_match(&mut self, page_height: usize) {
        let Some(current) = self.current_match else {
            return;
        };
        if let Some(&(line_idx, _, _)) = self.search_matches.get(current) {
            let context = 5usize.min(page_height.saturating_sub(1));
            if self.is_visual_scroll() {
                let visual_row = self.logical_to_visual_row(line_idx);
                self.scroll_offset = visual_row.saturating_sub(context).min(self.max_scroll());
            } else {
                self.scroll_offset = line_idx.saturating_sub(context).min(self.max_scroll());
            }
        }
    }

    pub fn next_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.current_match.unwrap_or(0);
        self.current_match = Some((current + 1) % self.search_matches.len());
        self.scroll_to_current_match(page_height);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
    }

    pub fn prev_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.current_match.unwrap_or(0);
        self.current_match = Some(if current == 0 {
            self.search_matches.len() - 1
        } else {
            current - 1
        });
        self.scroll_to_current_match(page_height);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
    }
}

pub(crate) fn build_lowercase_mapping(
    original: &str,
    lower: &mut String,
    byte_map: &mut Vec<usize>,
) {
    lower.clear();
    byte_map.clear();
    for (orig_byte_idx, ch) in original.char_indices() {
        let len_before = lower.len();
        lower.extend(ch.to_lowercase());
        for _ in len_before..lower.len() {
            byte_map.push(orig_byte_idx);
        }
    }
}
