use std::borrow::Cow;

use super::hex::{HEX_BYTES_PER_LINE, HEX_OFFSET_PREFIX_WIDTH};
use super::open::ViewerState;
use super::{SearchLineMatch, SearchMatch};

fn lowercase_query(query: &str) -> String {
    if query.is_ascii() {
        query.to_ascii_lowercase()
    } else {
        query.chars().flat_map(|c| c.to_lowercase()).collect()
    }
}

/// How the search query, once it fails to parse as a hex byte sequence, is
/// classified. Both errors fall back to a case-insensitive text search over the
/// raw bytes, but keeping them distinct lets callers (and tests) tell an
/// incomplete query apart from a malformed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HexQueryError {
    /// Fewer than two hex digits — not enough to form a single byte.
    TooShort,
    /// An odd number of digits, or a non-hex-digit character.
    Invalid,
}

/// Releases an over-grown match buffer, or clears it in place.
///
/// After searching a match-dense file the backing allocation can be far larger
/// than a subsequent search needs; free it when capacity dwarfs the data it
/// held. Below the floor we keep the allocation to avoid reallocation churn on
/// the common small-result path.
fn shrink_or_clear<T>(vec: &mut Vec<T>) {
    const SHRINK_RATIO: usize = 4;
    const MIN_RETAINED: usize = 64;
    if vec.capacity() > vec.len().saturating_mul(SHRINK_RATIO).max(MIN_RETAINED) {
        *vec = Vec::new();
    } else {
        vec.clear();
    }
}

/// Lossily decodes `slice` to a string while tracking, for every byte of the
/// decoded string, the offset within `slice` of the raw byte it came from.
///
/// Returns `(decoded, slice_offset_of)`:
/// - For valid UTF-8 the decoded string borrows `slice` and `slice_offset_of`
///   is `None` (the mapping is the identity `j -> j`).
/// - For invalid UTF-8, `String::from_utf8_lossy` substitutes one `U+FFFD`
///   (3 bytes) per maximal ill-formed subsequence, so decoded-string byte `j`
///   no longer lines up with `slice` byte `j`. The returned map has length
///   `decoded.len() + 1` (a trailing sentinel of `slice.len()`); all three
///   bytes of a substituted `U+FFFD` map to the *start* of the invalid run, and
///   the entry after it maps past the run — so a match's end offset always
///   resolves to a correct raw boundary.
///
/// This keeps search and highlight in a single (raw-byte) coordinate space,
/// fixing the prior bug where lossy-string indices were used directly as raw
/// offsets.
fn decode_lossy_with_map(slice: &[u8]) -> (Cow<'_, str>, Option<Vec<usize>>) {
    match std::str::from_utf8(slice) {
        Ok(s) => (Cow::Borrowed(s), None),
        Err(_) => {
            let mut decoded = String::with_capacity(slice.len());
            let mut map = Vec::with_capacity(slice.len() + 1);
            let mut raw = 0usize;
            for chunk in slice.utf8_chunks() {
                let valid = chunk.valid();
                decoded.push_str(valid);
                for k in 0..valid.len() {
                    map.push(raw + k);
                }
                raw += valid.len();

                let invalid = chunk.invalid();
                if !invalid.is_empty() {
                    let before = decoded.len();
                    decoded.push('\u{FFFD}');
                    for _ in before..decoded.len() {
                        map.push(raw);
                    }
                    raw += invalid.len();
                }
            }
            map.push(raw); // sentinel for the one-past-the-end index
            (Cow::Owned(decoded), Some(map))
        }
    }
}

impl ViewerState {
    pub(crate) fn clear_search_results(&mut self) {
        shrink_or_clear(&mut self.search_matches);
        shrink_or_clear(&mut self.search_matches_by_line);
        self.current_match = None;
        self.search_query = None;
    }

    pub fn search(&mut self, query: &str, page_height: usize) {
        self.clear_search_results();
        self.search_query = Some(query.to_string());

        if query.is_empty() {
            return;
        }

        if self.is_hex_mode() {
            self.collect_hex_matches(query);
        } else {
            self.collect_text_matches(query);
        }

        self.select_initial_match(page_height);
    }

    /// Selects the first match at or after the current viewport line (wrapping
    /// to the first match otherwise) and scrolls to it. Shared by text and hex
    /// search so both behave identically.
    fn select_initial_match(&mut self, page_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let current_line = if self.is_visual_scroll() {
            self.visual_row_to_logical(self.scroll_offset).0
        } else {
            self.scroll_offset
        };
        let idx = self
            .search_matches
            .iter()
            .position(|m| m.line >= current_line)
            .unwrap_or(0);
        self.current_match = Some(idx);
        self.scroll_to_current_match(page_height);
    }

    fn collect_text_matches(&mut self, query: &str) {
        let lower_query = lowercase_query(query);

        let mut lower_buf = String::new();
        let mut byte_map_buf = Vec::with_capacity(256);
        let mut local_matches: Vec<SearchMatch> = Vec::with_capacity(64);
        let mut local_by_line: Vec<SearchLineMatch> = Vec::with_capacity(64);

        for line_idx in 0..self.line_count {
            let line = self.get_line(line_idx);
            build_lowercase_mapping(&line, &mut lower_buf, &mut byte_map_buf);
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
                    debug_assert_eq!(
                        mapped_end, orig_byte_start,
                        "lowercase mapping produced non-monotonic byte span: \
                         orig_start={orig_byte_start}, mapped_end={mapped_end}"
                    );
                    line[orig_byte_start..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| orig_byte_start + i)
                        .unwrap_or(line.len())
                } else {
                    mapped_end
                };

                let global_idx = local_matches.len();
                local_matches.push(SearchMatch {
                    line: line_idx,
                    start_byte: orig_byte_start,
                    end_byte: orig_byte_end,
                });
                local_by_line.push(SearchLineMatch {
                    line: line_idx,
                    global_idx,
                    start_byte: orig_byte_start,
                    end_byte: orig_byte_end,
                });

                let char_width = lower_buf[match_byte_start..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                search_start = match_byte_start + char_width;
            }
        }

        self.search_matches = local_matches;
        self.search_matches_by_line = local_by_line;
    }

    fn push_hex_match_segments(
        &mut self,
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
                self.search_matches.push(SearchMatch {
                    line: line_idx,
                    start_byte: hex_col,
                    end_byte: hex_col + match_hex_len,
                });
                first_segment = false;
            }
            self.search_matches_by_line.push(SearchLineMatch {
                line: line_idx,
                global_idx,
                start_byte: hex_col,
                end_byte: hex_col + match_hex_len,
            });

            byte_offset += segment_len;
            remaining -= segment_len;
        }
    }

    fn collect_hex_matches(&mut self, query: &str) {
        let lower_query = lowercase_query(query);

        match Self::parse_hex_query(&lower_query) {
            Ok(needle) => {
                let mut pos = 0;
                while let Some(idx) = super::hex::find_bytes(&self.raw_bytes[pos..], &needle) {
                    let abs_offset = pos + idx;
                    let global_idx = self.search_matches.len();
                    self.push_hex_match_segments(abs_offset, needle.len(), global_idx);
                    pos = abs_offset + 1;
                }
            }
            Err(_) => self.collect_hex_text_matches(&lower_query),
        }
    }

    /// Case-insensitive text search over the raw bytes, used in hex view when
    /// the query is not a hex byte sequence. Decodes the file in overlapping
    /// chunks and maps every match back to raw-byte offsets so highlights line
    /// up with the bytes shown, even across invalid UTF-8.
    fn collect_hex_text_matches(&mut self, lower_query: &str) {
        const CHUNK_SIZE: usize = 1024 * 1024;
        // A match found just before a chunk boundary may extend past it. Each
        // lowercased byte maps back to at most a 4-byte UTF-8 scalar (or a 1-3
        // byte invalid run), so the original span is at most 4x the lowercased
        // query length; +16 is boundary slack.
        const MAX_RAW_BYTES_PER_LOWER_BYTE: usize = 4;
        let overlap_bytes = lower_query.len() * MAX_RAW_BYTES_PER_LOWER_BYTE + 16;

        let total_len = self.raw_bytes.len();
        let mut chunk_start: usize = 0;
        let mut lower_buf = String::with_capacity(CHUNK_SIZE);
        let mut byte_map: Vec<usize> = Vec::with_capacity(CHUNK_SIZE);
        let mut chunk_hits: Vec<(usize, usize)> = Vec::new();

        while chunk_start < total_len {
            let primary_end = (chunk_start + CHUNK_SIZE).min(total_len);
            let buf_end = (primary_end + overlap_bytes).min(total_len);
            let slice = &self.raw_bytes[chunk_start..buf_end];

            let (decoded, slice_offset_of) = decode_lossy_with_map(slice);
            build_lowercase_mapping(&decoded, &mut lower_buf, &mut byte_map);

            // Composes the two coordinate maps: lowercased index -> decoded
            // byte index -> offset within `slice` -> absolute raw offset.
            let to_raw = |decoded_idx: usize| -> usize {
                let rel = match &slice_offset_of {
                    Some(m) => m.get(decoded_idx).copied().unwrap_or(slice.len()),
                    None => decoded_idx.min(slice.len()),
                };
                chunk_start + rel
            };

            // Gather matches while only `&self.raw_bytes` is borrowed (via
            // `slice`/`decoded`), then record them once that borrow has ended —
            // `push_hex_match_segments` needs `&mut self`.
            chunk_hits.clear();
            let mut search_start = 0;
            while let Some(pos) = lower_buf[search_start..].find(lower_query) {
                let abs_pos = search_start + pos;
                let char_width = lower_buf[abs_pos..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                search_start = abs_pos + char_width;

                let decoded_start = byte_map.get(abs_pos).copied().unwrap_or(decoded.len());
                let orig_byte = to_raw(decoded_start);
                if orig_byte >= primary_end {
                    continue;
                }

                let match_end_in_lower = abs_pos + lower_query.len();
                let decoded_end = byte_map
                    .get(match_end_in_lower)
                    .copied()
                    .unwrap_or(decoded.len());
                let orig_byte_end = to_raw(decoded_end);

                let match_byte_len = orig_byte_end
                    .saturating_sub(orig_byte)
                    .min(total_len.saturating_sub(orig_byte));
                if match_byte_len == 0 {
                    continue;
                }

                chunk_hits.push((orig_byte, match_byte_len));
            }

            for &(orig_byte, match_byte_len) in &chunk_hits {
                let global_idx = self.search_matches.len();
                self.push_hex_match_segments(orig_byte, match_byte_len, global_idx);
            }

            chunk_start = primary_end;
        }
    }

    pub(crate) fn parse_hex_query(query: &str) -> Result<Vec<u8>, HexQueryError> {
        let cleaned: String = query.chars().filter(|c| !c.is_whitespace()).collect();
        // A hex query is only ASCII hex digits. Reject non-ASCII up front: the
        // 2-byte slicing below indexes by byte and would panic on a multi-byte
        // codepoint whose boundary falls mid-pair (e.g. a 4-byte emoji).
        if !cleaned.is_ascii() {
            return Err(HexQueryError::Invalid);
        }
        if cleaned.len() < 2 {
            return Err(HexQueryError::TooShort);
        }
        if !cleaned.len().is_multiple_of(2) {
            return Err(HexQueryError::Invalid);
        }
        (0..cleaned.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).map_err(|_| HexQueryError::Invalid))
            .collect()
    }

    pub(crate) fn scroll_to_current_match(&mut self, page_height: usize) {
        let Some(current) = self.current_match else {
            return;
        };
        if let Some(line_idx) = self.search_matches.get(current).map(|m| m.line) {
            let context = 5usize.min(page_height.saturating_sub(1));
            if self.is_visual_scroll() {
                let visual_row = self.logical_line_visual_start(line_idx);
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

/// Builds the lowercased form of `original` together with a back-map from each
/// lowercased byte to the byte index in `original` of the character it came
/// from.
///
/// Both buffers are cleared and reused across calls. `byte_map` has one entry
/// per byte of `lower`; because a single source character can lowercase to
/// several characters (e.g. `İ` → `i̇`) the map is non-decreasing but not
/// injective. Used to translate match positions found in the lowercased text
/// back to offsets in the original.
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
