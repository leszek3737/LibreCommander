use super::helpers::*;
use crate::input::dialogs;
use crossterm::event::KeyCode;
use lc::app::job_runner::RunningJob;
use lc::app::types::{
    ActivePanel, AppMode, AppState, DialogKind, InputAction, TextInput, ViewMode,
};
use lc::ui;
use lc::ui::viewer;
use ratatui::layout::Size;
use std::io::Write;
use tempfile::NamedTempFile;

const TEST_VIEWPORT: Size = Size::new(80, 24);

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

fn generate_lines(count: usize) -> String {
    (0..count).map(|i| format!("line {}\n", i)).collect()
}

fn open_viewer(path: &std::path::Path) -> viewer::ViewerState {
    viewer::ViewerState::open(path).unwrap()
}

fn assert_viewer_mode(vs: &viewer::ViewerState, expected_hex: bool) {
    let expected_mode = if expected_hex {
        ViewMode::Hex
    } else {
        ViewMode::Text
    };
    assert_eq!(vs.view_mode, expected_mode);
}

// TODO: Add tests for:
// - Empty file (zero bytes)
// - Viewport taller than file (scroll should be no-op)
// - Search wrapping (last match → first match)
// - Binary / invalid UTF-8 edge cases
// - Long lines exceeding viewport width
// - CRLF line endings

#[test]
fn f3_viewer_clears_stale_prev_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("view.txt");
    std::fs::write(&file, b"view").unwrap();
    let mut state = AppState {
        active_panel: ActivePanel::Left,
        prev_mode: Some(AppMode::Search),
        ..Default::default()
    };
    state.left_panel.set_path(tmp.path().to_path_buf());
    state
        .left_panel
        .set_entries(vec![TestEntry::new("view.txt").path(&file).file(4).build()]);
    let mut loader = None;
    let mut terminal = test_terminal();

    super::super::handle_function_keys(&mut state, &mut loader, KeyCode::F(3), &mut terminal);

    assert!(matches!(state.mode, AppMode::Viewing));
    assert!(state.prev_mode.is_none());
}

#[test]
fn viewer_search_esc_keeps_viewer_previous_mode() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::Input {
            prompt: "Viewer search:".to_string(),
            action: InputAction::ViewerSearch,
        }),
        dialog_input: {
            let mut ti = TextInput::new();
            ti.text = "needle".to_string();
            ti.recompute_grapheme_count();
            ti.cursor = 6;
            ti
        },
        prev_mode: Some(AppMode::Normal),
        ..Default::default()
    };
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;

    dialogs::handle_dialog(
        &mut state,
        &mut viewer,
        &mut job,
        KeyCode::Esc,
        TEST_VIEWPORT,
    );

    assert!(matches!(state.mode, AppMode::Viewing));
    assert_eq!(state.prev_mode, Some(AppMode::Normal));
    assert!(state.dialog_input.text.is_empty());
    assert_eq!(state.dialog_input.cursor, 0);
}

#[test]
fn viewer_scroll_up_down() {
    let file = create_test_file("line1\nline2\nline3\nline4\nline5\n");
    let mut vs = open_viewer(file.path());
    assert_eq!(vs.scroll_offset, 0);
    vs.scroll_down(2);
    assert_eq!(vs.scroll_offset, 2);
    vs.scroll_up(1);
    assert_eq!(vs.scroll_offset, 1);
    vs.scroll_up(10);
    assert_eq!(vs.scroll_offset, 0);
    // Depends on TEST_HEIGHT viewport; will need adjustment if viewport changes.
    vs.scroll_down(100);
    assert_eq!(vs.scroll_offset, 4);
}

#[test]
fn viewer_page_up_page_down() {
    let content = generate_lines(50);
    let file = create_test_file(&content);
    let mut vs = open_viewer(file.path());
    assert_eq!(vs.scroll_offset, 0);
    vs.page_down(10);
    assert_eq!(vs.scroll_offset, 10);
    vs.page_up(10);
    assert_eq!(vs.scroll_offset, 0);
    vs.page_down(10);
    vs.page_down(10);
    assert_eq!(vs.scroll_offset, 20);
}

#[test]
fn viewer_search() {
    let file = create_test_file("apple\nbanana\ncherry\napple pie\ndate\n");
    let mut vs = open_viewer(file.path());
    let h = TEST_VIEWPORT.height.into();
    vs.search("apple", h);
    assert_eq!(vs.search_query.as_deref(), Some("apple"));
    assert!(!vs.search_matches.is_empty());
    assert_eq!(vs.current_match, Some(0));
    vs.next_match(h);
    assert_eq!(vs.current_match, Some(1));
    vs.prev_match(h);
    assert_eq!(vs.current_match, Some(0));
    vs.search("", h);
    assert!(vs.search_matches.is_empty());
}

#[test]
fn viewer_hex_mode_toggle() {
    let file = create_test_file("hello\nworld\n");
    let mut vs = open_viewer(file.path());
    assert_viewer_mode(&vs, false);
    vs.toggle_hex_mode();
    assert_viewer_mode(&vs, true);
    vs.toggle_hex_mode();
    assert_viewer_mode(&vs, false);
}

#[test]
fn viewer_close_via_escape() {
    let file = create_test_file("content\n");
    let mut viewer_state = Some(open_viewer(file.path()));
    let mut state = AppState {
        mode: AppMode::Viewing,
        prev_mode: Some(AppMode::Normal),
        ..Default::default()
    };
    let mut viewer_loader: Option<ui::viewer::ViewerLoader> = None;
    let mut image_preview_loader: Option<ui::viewer::ImagePreviewLoader> = None;

    crate::input::mode_dispatch::handle_viewer_mode(
        &mut state,
        &mut viewer_state,
        &mut viewer_loader,
        &mut image_preview_loader,
        KeyCode::Esc,
        TEST_VIEWPORT,
    );

    assert!(matches!(state.mode, AppMode::Normal));
    assert!(viewer_state.is_none());
    assert!(image_preview_loader.is_none());
}
