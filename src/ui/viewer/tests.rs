use super::SearchLineMatch;
use super::hex::format_hex_line;
use super::mime::should_open_as_text;
use super::open::ViewerState;
use super::render::{format_line_with_highlight, render_viewer_with_colors};
use super::scroll::{line_number_column_width, paragraph_horizontal_scroll};
use crate::app::types::ViewMode;
use crate::app::types::format_size;
use crate::ui::theme::ColorPalette;
use crate::ui::theme::DEFAULT_COLORS;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use tempfile::NamedTempFile;

const DEFAULT_PAGE_HEIGHT: usize = 20;
const TEST_CHANNEL_TIMEOUT_SECS: u64 = 1;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

fn init_state(content: &str) -> ViewerState {
    let file = create_test_file(content);
    ViewerState::open(file.path()).unwrap()
}

fn join_lines(lines: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    lines
        .into_iter()
        .map(|l| l.as_ref().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_viewer_buffer(state: &ViewerState, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_viewer_with_colors(frame, frame.area(), state, &ColorPalette::default())
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_line(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
}

#[test]
fn test_viewer_loader_drop_cancels_worker() {
    let (_tx, rx) = mpsc::channel(); // _tx dropped immediately — closed sender signals worker exit
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_worker = Arc::clone(&cancel);
    let (done_tx, done_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        // Busy-wait; acceptable for a short-lived test helper.
        while !cancel_for_worker.load(Ordering::Relaxed) {
            thread::yield_now();
        }
        let _ = done_tx.send(());
    });

    let loader = super::loader::ViewerLoader {
        receiver: rx,
        cancel: Arc::clone(&cancel),
        path: PathBuf::new(),
        handle: Some(handle),
    };

    drop(loader);

    assert!(cancel.load(Ordering::Relaxed));
    done_rx
        .recv_timeout(std::time::Duration::from_secs(TEST_CHANNEL_TIMEOUT_SECS))
        .unwrap();
}

#[test]
fn test_open_file() {
    let state = init_state("Line 1\nLine 2\nLine 3");

    assert_eq!(state.line_count, 3);
    assert_eq!(state.get_line(0), "Line 1");
    assert_eq!(state.get_line(1), "Line 2");
    assert_eq!(state.get_line(2), "Line 3");
}

#[test]
fn test_open_file_with_trailing_newline_omits_empty_tail() {
    let file = create_test_file("Line 1\nLine 2\n");
    let state = ViewerState::open(file.path()).unwrap();

    assert_eq!(state.line_count, 2);
    assert_eq!(state.get_line(0), "Line 1");
    assert_eq!(state.get_line(1), "Line 2");
}

#[test]
fn test_scroll_up() {
    let mut state = init_state("Line 1\nLine 2\nLine 3");

    state.scroll_offset = 5;
    state.scroll_up(2);
    assert_eq!(state.scroll_offset, 3);

    state.scroll_up(10);
    assert_eq!(state.scroll_offset, 0);
}

#[test]
fn test_scroll_down() {
    let mut state = init_state("Line 1\nLine 2\nLine 3");

    state.scroll_down(1);
    assert_eq!(state.scroll_offset, 1);

    state.scroll_down(5);
    assert_eq!(state.scroll_offset, 2);
}

#[test]
fn test_page_up_down() {
    let mut state = init_state("Line 1\nLine 2\nLine 3\nLine 4\nLine 5");
    state.wrap_lines = false;

    state.scroll_offset = 10;
    let page_height = 5;
    state.page_up(page_height);
    assert_eq!(state.scroll_offset, 5);

    state.page_down(page_height);
    assert_eq!(state.scroll_offset, 4);
}

#[test]
fn test_go_to_top_bottom() {
    let mut state = init_state("Line 1\nLine 2\nLine 3");

    state.go_to_bottom(1);
    assert_eq!(state.scroll_offset, 2);

    state.go_to_top();
    assert_eq!(state.scroll_offset, 0);
}

#[test]
fn test_search() {
    let mut state = init_state("apple\nbanana\ncherry\napple pie");

    state.search("apple", DEFAULT_PAGE_HEIGHT);

    assert_eq!(state.search_matches.len(), 2);
    assert_eq!(state.search_matches[0], (0, 0, 5));
    assert_eq!(state.search_matches[1], (3, 0, 5));
    assert_eq!(state.current_match, Some(0));
}

#[test]
fn test_next_prev_match() {
    let mut state = init_state("apple\nbanana\napple pie");

    state.search("apple", DEFAULT_PAGE_HEIGHT);
    assert_eq!(state.current_match, Some(0));

    state.next_match(DEFAULT_PAGE_HEIGHT);
    assert_eq!(state.current_match, Some(1));
    assert_eq!(state.scroll_offset, 0);

    state.next_match(DEFAULT_PAGE_HEIGHT);
    assert_eq!(state.current_match, Some(0));

    state.prev_match(DEFAULT_PAGE_HEIGHT);
    assert_eq!(state.current_match, Some(1));
}

#[test]
fn test_search_case_insensitive() {
    let mut state = init_state("Hello World\nfoo BAR\nhello world");

    state.search("hello", DEFAULT_PAGE_HEIGHT);

    assert_eq!(state.search_matches.len(), 2);
    assert_eq!(state.search_matches[0], (0, 0, 5));
    assert_eq!(state.search_matches[1], (2, 0, 5));
}

#[test]
fn test_open_empty_file_has_placeholder() {
    let state = init_state("");

    assert_eq!(state.line_count, 1);
    assert_eq!(state.get_line(0), "[Empty file]");
    assert_eq!(state.file_size, 0);
    assert_eq!(state.scroll_offset, 0);
}

#[test]
fn test_should_open_as_text_allows_text_mime() {
    assert!(should_open_as_text(
        Path::new("README"),
        Some("text/plain"),
        b"hello"
    ));
}

#[test]
fn test_should_open_as_text_allows_source_and_config_extensions() {
    assert!(should_open_as_text(
        Path::new("main.rs"),
        Some("application/octet-stream"),
        b"fn main() {}"
    ));
    assert!(should_open_as_text(
        Path::new("config.toml"),
        Some("application/octet-stream"),
        b"key = \"value\""
    ));
}

#[test]
fn test_should_open_as_text_rejects_known_binary_mime() {
    assert!(!should_open_as_text(
        Path::new("archive.zip"),
        Some("application/zip"),
        b"PK\0\0"
    ));
    assert!(!should_open_as_text(
        Path::new("image.png"),
        Some("image/png"),
        b"\x89PNG\r\n"
    ));
}

#[test]
fn test_should_open_as_text_rejects_unknown_nul_bytes() {
    assert!(!should_open_as_text(
        Path::new("unknown.bin"),
        None,
        b"abc\0def"
    ));
}

#[test]
fn test_open_binary_file_defaults_to_hex_mode() {
    let mut file = NamedTempFile::with_suffix(".bin").unwrap();
    file.write_all(b"abc\0def").unwrap();

    let state = ViewerState::open(file.path()).unwrap();

    assert!(state.is_hex_mode());
    assert_eq!(state.raw_bytes, b"abc\0def");
}

#[test]
fn test_source_code_ext_opens_as_text_even_with_nul_bytes() {
    let mut file = NamedTempFile::with_suffix(".rs").unwrap();
    file.write_all(b"fn main() {}\0\0\0\0binary").unwrap();
    let state = ViewerState::open(file.path()).unwrap();
    assert!(!state.is_hex_mode());
}

#[test]
fn test_search_unicode_match_uses_char_columns() {
    let mut state = init_state("zażółć gęślą jaźń");

    state.search("gęśl", DEFAULT_PAGE_HEIGHT);

    assert_eq!(state.search_matches, vec![(0, 7, 4)]);
}

#[test]
fn test_search_unicode_repeated_matches_keep_char_columns() {
    let mut state = init_state("żółw żółw");

    state.search("żółw", DEFAULT_PAGE_HEIGHT);

    assert_eq!(state.search_matches, vec![(0, 0, 4), (0, 5, 4)]);
}

// NOTE: search_matches stores (line_idx, char_start, char_len) — column positions.
// search_matches_by_line stores SearchLineMatch with (start_byte, end_byte) — byte offsets.
// In ASCII these coincide; for multi-byte Unicode they diverge (see tests below).

#[test]
fn test_format_line_with_highlight_handles_unicode() {
    let spans = format_line_with_highlight(
        "zażółć gęślą jaźń",
        &[SearchLineMatch {
            line: 0,
            global_idx: 0,
            start_byte: 11,
            end_byte: 17,
        }],
        Some(0),
        &DEFAULT_COLORS,
    );

    let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(rendered, "zażółć gęślą jaźń");
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0], Span::raw("zażółć "));
    assert_eq!(spans[1].content.as_ref(), "gęśl");
    assert_eq!(spans[2], Span::raw("ą jaźń"));
}

#[test]
fn test_format_line_with_highlight_overlapping_matches_no_duplicates() {
    let line = "0123456789abcdef";
    let regular_style = Style::default()
        .fg(crate::ui::theme::DEFAULT_COLORS.search_match_fg)
        .bg(crate::ui::theme::DEFAULT_COLORS.search_match_bg);
    let current_style = Style::default()
        .fg(crate::ui::theme::DEFAULT_COLORS.search_match_current_fg)
        .bg(crate::ui::theme::DEFAULT_COLORS.search_match_current_bg)
        .add_modifier(Modifier::BOLD);

    let spans = format_line_with_highlight(
        line,
        &[
            SearchLineMatch {
                line: 0,
                global_idx: 0,
                start_byte: 3,
                end_byte: 9,
            },
            SearchLineMatch {
                line: 0,
                global_idx: 1,
                start_byte: 4,
                end_byte: 10,
            },
        ],
        Some(1),
        &DEFAULT_COLORS,
    );

    let mut rendered = String::with_capacity(line.len());
    for s in &spans {
        rendered.push_str(s.content.as_ref());
    }
    assert_eq!(
        rendered, line,
        "overlapping matches must not duplicate text"
    );

    assert_eq!(spans[0], Span::raw("012"));
    assert_eq!(spans[1], Span::styled("3", regular_style));
    assert_eq!(spans[2], Span::styled("456789", current_style));
    assert_eq!(spans[3], Span::raw("abcdef"));
}

#[test]
fn test_format_line_with_highlight_fully_overlapping_skipped() {
    let line = "abcdefghij";
    let spans = format_line_with_highlight(
        line,
        &[
            SearchLineMatch {
                line: 0,
                global_idx: 0,
                start_byte: 2,
                end_byte: 8,
            },
            SearchLineMatch {
                line: 0,
                global_idx: 1,
                start_byte: 3,
                end_byte: 6,
            },
        ],
        None,
        &DEFAULT_COLORS,
    );

    let mut rendered = String::with_capacity(line.len());
    for s in &spans {
        rendered.push_str(s.content.as_ref());
    }
    assert_eq!(rendered, line, "fully overlapped match must be skipped");
}

#[test]
fn test_search_line_match_cache_stores_unicode_byte_ranges() {
    let mut state = init_state("zażółć gęślą jaźń");

    state.search("gęśl", DEFAULT_PAGE_HEIGHT);

    assert_eq!(
        state.search_matches_by_line,
        vec![SearchLineMatch {
            line: 0,
            global_idx: 0,
            start_byte: 11,
            end_byte: 17,
        }]
    );
}

#[test]
fn test_search_replace_clears_line_match_cache() {
    let mut state = init_state("alpha\nbeta");

    state.search("alpha", DEFAULT_PAGE_HEIGHT);
    state.search("missing", DEFAULT_PAGE_HEIGHT);

    assert!(state.search_matches.is_empty());
    assert!(state.search_matches_by_line.is_empty());
}

#[test]
fn test_horizontal_scroll_uses_cached_max_line_width() {
    let mut state = init_state("short\nabcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ");

    assert_eq!(state.max_line_width, 46);
    state.scroll_right(100, 10);

    assert_eq!(state.horizontal_offset, 36);
}

#[test]
fn test_format_hex_line() {
    let bytes = &[
        0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x00,
    ];
    let line = format_hex_line(0x1000, bytes);

    assert!(line.starts_with("0000000000001000:"));
    assert!(line.contains("48 65 6c 6c 6f 20 57 6f  72 6c 64 00"));
    assert!(line.contains("|Hello World.|"));
}

#[test]
fn test_open_valid_replacement_character_is_not_invalid_utf8() {
    let state = init_state("valid replacement: \u{FFFD}");

    assert!(!state.has_invalid_utf8);
    assert!(state.get_line(0).contains('\u{FFFD}'));
}

#[test]
fn test_open_invalid_utf8_sets_warning() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"valid\xffinvalid").unwrap();

    let state = ViewerState::open(file.path()).unwrap();

    assert!(state.has_invalid_utf8);
    assert!(state.get_line(0).contains('\u{FFFD}'));
}

#[test]
fn test_format_hex_line_accepts_more_than_sixteen_bytes() {
    let bytes = [b'A'; 17];

    let line = format_hex_line(0, &bytes);

    assert!(line.starts_with("0000000000000000:"));
    assert!(line.ends_with("|AAAAAAAAAAAAAAAAA|"));
}

#[test]
fn test_toggle_states() {
    let mut state = init_state("test");

    assert!(!state.show_line_numbers);
    state.toggle_line_numbers();
    assert!(state.show_line_numbers);

    assert!(state.wrap_lines);
    state.toggle_wrap();
    assert!(!state.wrap_lines);

    assert!(!state.is_hex_mode());
    state.toggle_hex_mode();
    assert!(state.is_hex_mode());
    assert_eq!(state.view_mode, ViewMode::Hex);
    state.toggle_hex_mode();
    assert_eq!(state.view_mode, ViewMode::Text);
}

#[test]
fn test_horizontal_scroll() {
    let mut state = init_state("abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ");

    state.scroll_right(5, 10);
    assert_eq!(state.horizontal_offset, 5);

    state.scroll_right(100, 10);
    assert_eq!(state.horizontal_offset, 36);

    state.scroll_left(2);
    assert_eq!(state.horizontal_offset, 34);

    state.scroll_left(100);
    assert_eq!(state.horizontal_offset, 0);
}

#[test]
fn test_line_number_width_expands_for_large_files() {
    assert_eq!(line_number_column_width(9_999), 6);
    assert_eq!(line_number_column_width(10_000), 7);

    let content = join_lines((1..=10_000).map(|i| format!("line {i}")));
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();
    state.show_line_numbers = true;
    state.wrap_lines = false;
    state.scroll_offset = 9_999;

    let buffer = render_viewer_buffer(&state, 24, 4);

    assert!(buffer_line(&buffer, 1).contains("10000  line 10000"));
}

#[test]
fn test_horizontal_scroll_uses_dynamic_line_number_width() {
    let content = join_lines((1..=10_000).map(|_| "abcdefghijkl"));
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();
    state.show_line_numbers = true;

    state.scroll_right(100, 10);

    assert_eq!(state.horizontal_offset, 9);
}

#[test]
fn test_format_size() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1024), "1.0 KB");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_size(1048576), "1.0 MB");
    assert_eq!(format_size(1073741824), "1.0 GB");
}

#[test]
fn test_paragraph_horizontal_scroll_clamps_to_u16() {
    assert_eq!(paragraph_horizontal_scroll(usize::MAX), u16::MAX);
}

#[test]
fn test_render_viewer_reserves_last_row_for_status_bar() {
    let mut state = init_state("line 1\nline 2\nline 3");
    state.wrap_lines = false;

    let buffer = render_viewer_buffer(&state, 60, 5);

    assert!(buffer_line(&buffer, 1).contains("line 1"));
    assert!(buffer_line(&buffer, 2).contains("line 2"));
    let status = buffer_line(&buffer, 3);
    assert!(status.contains("Line: 1/3"));
    assert!(status.contains("Text"));
    assert!(!status.contains("line 3"));
}

#[test]
fn test_wrap_scroll_advances_by_visual_row() {
    let long_line = "a".repeat(200);
    let content = format!("short\n{long_line}\nend");
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();
    assert!(state.wrap_lines);

    state.update_wrap_layout(80);
    let total_visual: usize;
    let long_height;
    {
        let heights = state.render_cache.visual_heights.borrow();
        assert!(!heights.is_empty());
        assert_eq!(heights[0], 1);
        long_height = heights[1];
        total_visual = heights.iter().sum();
    }
    assert!(
        long_height > 1,
        "long line should wrap to multiple visual rows"
    );
    assert!(total_visual > state.line_count);

    state.scroll_down(1);
    assert_eq!(state.scroll_offset, 1);

    state.scroll_down(1);
    assert_eq!(state.scroll_offset, 2);

    state.scroll_up(1);
    assert_eq!(state.scroll_offset, 1);

    let max = state.max_scroll();
    assert_eq!(max, total_visual.saturating_sub(1));
    assert!(max > state.line_count);
}

#[test]
fn test_wrap_scroll_with_narrow_width() {
    let mut state = init_state("abcdefghij");
    state.update_wrap_layout(5);

    assert_eq!(state.render_cache.visual_heights.borrow().len(), 1);
    assert_eq!(state.render_cache.visual_heights.borrow()[0], 2);

    state.scroll_down(1);
    assert_eq!(state.scroll_offset, 1);

    let max = state.max_scroll();
    assert_eq!(max, 1);
}

#[test]
fn test_wrap_go_to_bottom_uses_visual_rows() {
    let long_line = "x".repeat(160);
    let content = format!("a\nb\n{long_line}\nc");
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();
    state.update_wrap_layout(80);

    let total_visual: usize = state.render_cache.visual_heights.borrow().iter().sum();
    state.go_to_bottom(3);
    assert_eq!(
        state.scroll_offset,
        total_visual.saturating_sub(3).min(state.max_scroll())
    );
}

#[test]
fn test_toggle_wrap_clears_visual_heights() {
    let mut state = init_state("some text");
    state.update_wrap_layout(80);
    assert!(!state.render_cache.visual_heights.borrow().is_empty());

    state.toggle_wrap();
    assert!(state.render_cache.visual_heights.borrow().is_empty());
    assert_eq!(*state.render_cache.cached_content_width.borrow(), 0);
}

#[test]
fn test_no_wrap_uses_logical_lines() {
    let mut state = init_state("Line 1\nLine 2\nLine 3");
    state.wrap_lines = false;

    assert!(!state.is_visual_scroll());
    assert_eq!(state.max_scroll(), 2);

    state.scroll_down(1);
    assert_eq!(state.scroll_offset, 1);
}

#[test]
fn test_visual_row_to_logical_roundtrip() {
    let state = init_state("short\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nend");
    state.update_wrap_layout(10);

    let total_visual: usize = state.render_cache.visual_heights.borrow().iter().sum();
    for row in 0..total_visual {
        let (logical, sub) = state.visual_row_to_logical(row);
        let back = state.logical_to_visual_row(logical);
        assert_eq!(back + sub, row, "roundtrip failed for visual row {row}");
    }
}

fn assert_no_duplicate_matches(matches: &[(usize, usize, usize)]) {
    let mut seen = std::collections::HashSet::new();
    for m in matches {
        assert!(seen.insert(*m), "duplicate match tuple: {:?}", m);
    }
}

#[test]
fn test_search_deduplicates_matches_after_multi_char_lowercase() {
    let mut state = init_state("Straße\nmessage\n");

    state.search("ss", DEFAULT_PAGE_HEIGHT);

    assert!(
        !state.search_matches.is_empty(),
        "expected at least one match"
    );
    assert_no_duplicate_matches(&state.search_matches);
    assert!(
        state.search_matches.len() <= 4,
        "expected at most 4 matches, got {}",
        state.search_matches.len()
    );
}

#[test]
fn test_hex_mode_search() {
    let mut file = NamedTempFile::with_suffix(".bin").unwrap();
    file.write_all(b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f")
        .unwrap();
    let mut state = ViewerState::open(file.path()).unwrap();
    assert!(state.is_hex_mode());

    state.search("01 02", DEFAULT_PAGE_HEIGHT);

    assert!(
        !state.search_matches.is_empty(),
        "hex search for '01 02' should find matches in hex data section"
    );
    assert_eq!(state.current_match, Some(0));
    assert!(state.search_matches[0].0 == 0);
}

#[test]
fn test_search_scroll_clamp() {
    let content = join_lines((0..100).map(|i| format!("Line {i:03}")));
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();
    state.wrap_lines = false;
    let page_height = 5usize;

    state.search("Line 000", page_height);
    assert!(!state.search_matches.is_empty());

    state.scroll_offset = state.line_count;
    state.search("Line 000", page_height);

    assert!(
        state.scroll_offset <= state.max_scroll(),
        "scroll_offset {} > max_scroll {}",
        state.scroll_offset,
        state.max_scroll()
    );
    assert!(!state.search_matches.is_empty());
    assert_eq!(state.current_match, Some(0));
}

fn create_wrapped_state_30_lines(width: usize) -> ViewerState {
    let content = join_lines((0..30).map(|i| format!("L{i:03}")));
    let file = create_test_file(&content);
    let state = ViewerState::open(file.path()).unwrap();
    state.update_wrap_layout(width);
    state
}

fn assert_visual_roundtrip(state: &ViewerState, row: usize) {
    let (logical, sub) = state.visual_row_to_logical(row);
    let back = state.logical_to_visual_row(logical);
    assert_eq!(
        back + sub,
        row,
        "roundtrip failed for visual row {row}: logical={logical}, sub={sub}, back={back}"
    );
}

#[test]
fn test_visual_row_to_logical_binary_search() {
    let state = create_wrapped_state_30_lines(10);

    assert!(
        state.render_cache.visual_heights.borrow().len() > 24,
        "need > 24 visual heights to exercise binary search path"
    );

    let total_visual: usize = state.render_cache.visual_heights.borrow().iter().sum();

    assert_eq!(state.visual_row_to_logical(0), (0, 0));

    let (last_logical, last_sub) = state.visual_row_to_logical(total_visual.saturating_sub(1));
    assert_eq!(
        last_logical,
        state.render_cache.visual_heights.borrow().len() - 1
    );
    assert_eq!(last_logical, state.line_count - 1);
    assert!(
        last_sub < state.render_cache.visual_heights.borrow()[last_logical],
        "sub-row should be within line height"
    );

    for &row in &[0usize, 1, 5, 10, 15, 20, 25] {
        if row < total_visual {
            assert_visual_roundtrip(&state, row);
        }
    }

    let result = state.visual_row_to_logical(total_visual);
    assert_eq!(result.0, state.line_count.saturating_sub(1));
    assert_eq!(result.1, 0);
}

#[test]
fn test_search_empty_query_noop() {
    let mut state = init_state("alpha\nbeta\ngamma");

    state.search("alpha", DEFAULT_PAGE_HEIGHT);
    assert_eq!(state.search_matches.len(), 1);
    assert_eq!(state.current_match, Some(0));

    state.search("", DEFAULT_PAGE_HEIGHT);

    assert!(state.search_matches.is_empty());
    assert!(state.current_match.is_none());
    assert_eq!(state.scroll_offset, 0);
}

#[test]
fn test_search_no_match_returns_none() {
    let mut state = init_state("apple\nbanana\ncherry");
    state.search("durian", DEFAULT_PAGE_HEIGHT);
    assert!(state.search_matches.is_empty());
    assert_eq!(state.current_match, None);
}

#[test]
fn test_hex_search_skips_offset_prefix() {
    let mut file = NamedTempFile::with_suffix(".bin").unwrap();
    file.write_all(b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00")
        .unwrap();
    let mut state = ViewerState::open(file.path()).unwrap();
    assert!(state.is_hex_mode());

    state.search("0000000", DEFAULT_PAGE_HEIGHT);
    assert!(
        state.search_matches.is_empty(),
        "hex search should not match offset prefix"
    );
}

fn create_png_file() -> NamedTempFile {
    const PNG_HEADER: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE,
    ];
    let mut file = NamedTempFile::with_suffix(".png").unwrap();
    file.write_all(PNG_HEADER).unwrap();
    file
}

#[test]
fn test_open_image_file_sets_view_mode_to_image() {
    let file = create_png_file();
    let state = ViewerState::open(file.path()).unwrap();

    assert_eq!(state.view_mode, ViewMode::Image);
    assert!(
        state
            .detected_mime
            .as_deref()
            .is_some_and(|m| m.starts_with("image/"))
    );
}

#[test]
fn test_toggle_hex_mode_with_image_file() {
    let file = create_png_file();
    let mut state = ViewerState::open(file.path()).unwrap();

    assert_eq!(state.view_mode, ViewMode::Image);

    state.toggle_hex_mode();
    assert_eq!(state.view_mode, ViewMode::Hex);

    state.toggle_hex_mode();
    assert_eq!(state.view_mode, ViewMode::Image);
}

fn assert_toggle_noop_in_image_mode(toggle: impl FnOnce(&mut ViewerState)) {
    let file = create_png_file();
    let mut state = ViewerState::open(file.path()).unwrap();
    assert_eq!(state.view_mode, ViewMode::Image);
    let original_wrap = state.wrap_lines;
    let original_line_numbers = state.show_line_numbers;

    toggle(&mut state);

    assert_eq!(state.view_mode, ViewMode::Image);
    assert_eq!(state.wrap_lines, original_wrap);
    assert_eq!(state.show_line_numbers, original_line_numbers);
}

#[test]
fn test_toggle_wrap_noop_in_image_mode() {
    assert_toggle_noop_in_image_mode(|s| s.toggle_wrap());
}

#[test]
fn test_toggle_line_numbers_noop_in_image_mode() {
    assert_toggle_noop_in_image_mode(|s| s.toggle_line_numbers());
}

#[test]
fn test_image_file_has_correct_mode_after_toggle() {
    let file = create_png_file();
    let mut state = ViewerState::open(file.path()).unwrap();

    assert_eq!(state.view_mode, ViewMode::Image);

    state.toggle_hex_mode();
    assert_eq!(state.view_mode, ViewMode::Hex);

    state.toggle_hex_mode();
    assert_eq!(
        state.view_mode,
        ViewMode::Image,
        "should return to Image mode, not Text"
    );
}

#[test]
fn test_render_image_view_does_not_panic() {
    let file = create_png_file();
    let state = ViewerState::open(file.path()).unwrap();

    assert!(state.render_cache.cached_image_size.is_none());
    assert!(state.render_cache.cached_image_text.is_none());

    let buffer = render_viewer_buffer(&state, 80, 24);

    assert!(!buffer.area.is_empty());
}

#[test]
// Smoke test: verifies the struct has the expected shape. Real image loading
// logic lives in the loader worker thread; there is no mockable I/O here.
fn test_image_preview_loader_stores_file_path() {
    let path_a = PathBuf::from("/tmp/image_a.png");
    let path_b = PathBuf::from("/tmp/image_b.png");

    let loader_a = super::loader::ImagePreviewLoader {
        file_path: path_a.clone(),
        receiver: mpsc::channel().1,
        cancel: Arc::new(AtomicBool::new(false)),
        handle: None,
    };
    let loader_b = super::loader::ImagePreviewLoader {
        file_path: path_b.clone(),
        receiver: mpsc::channel().1,
        cancel: Arc::new(AtomicBool::new(false)),
        handle: None,
    };

    assert_eq!(loader_a.file_path, path_a);
    assert_eq!(loader_b.file_path, path_b);
    assert_ne!(loader_a.file_path, loader_b.file_path);
}

#[test]
// Verifies the path comparison guard (used in the real race-condition
// handler) behaves correctly for PathBuf. The actual race condition
// scenario requires concurrent threads; this isolates the comparison.
fn test_image_preview_race_condition_guard_discards_mismatched_path() {
    let loader_path = PathBuf::from("/tmp/old_image.png");
    let viewer_path = PathBuf::from("/tmp/new_image.png");

    let loader = super::loader::ImagePreviewLoader {
        file_path: loader_path,
        receiver: mpsc::channel().1,
        cancel: Arc::new(AtomicBool::new(false)),
        handle: None,
    };

    let matched = viewer_path == loader.file_path;
    assert!(
        !matched,
        "loader for old file must not match new viewer path"
    );
}

#[test]
fn test_scroll_boundaries() {
    let mut state = init_state("a\nb\nc\nd\ne\nf\ng\nh\ni\nj");
    assert_eq!(state.line_count, 10);

    state.scroll_up(100);
    assert_eq!(state.scroll_offset, 0);

    state.scroll_down(100);
    assert_eq!(state.scroll_offset, state.max_scroll());

    state.go_to_top();
    assert_eq!(state.scroll_offset, 0);

    state.go_to_bottom(1);
    assert_eq!(state.scroll_offset, state.max_scroll());
}

#[test]
fn test_search_scroll_interaction() {
    let content = join_lines((0..50).map(|i| format!("Line {i:03}")));
    let mut state = init_state(&content);
    state.wrap_lines = false;
    let page_height = 5;

    state.search("Line 040", page_height);
    assert_eq!(state.search_matches.len(), 1);
    assert_eq!(state.current_match, Some(0));
    assert!(state.scroll_offset <= state.max_scroll());

    state.search("Line 000", page_height);
    assert!(!state.search_matches.is_empty());
    assert_eq!(state.current_match, Some(0));

    state.next_match(page_height);
    assert!(state.scroll_offset <= state.max_scroll());
}

#[test]
fn test_large_unicode_file() {
    let cjk_line = "日本語テスト".repeat(100);
    let emoji_line = "🎉🎊🎈".repeat(100);
    let content = join_lines((0..100).map(|i| {
        if i % 2 == 0 {
            cjk_line.clone()
        } else {
            emoji_line.clone()
        }
    }));
    let file = create_test_file(&content);
    let mut state = ViewerState::open(file.path()).unwrap();

    assert_eq!(state.line_count, 100);
    assert!(!state.has_invalid_utf8);
    assert!(state.max_line_width > 0);

    state.scroll_down(50);
    assert!(state.scroll_offset <= state.max_scroll());

    state.go_to_bottom(DEFAULT_PAGE_HEIGHT);
    assert!(state.scroll_offset <= state.max_scroll());
}
