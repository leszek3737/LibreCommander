use super::helpers::*;
use crate::input::dialogs;
use crossterm::event::KeyCode;
use lc::app::job_runner::RunningJob;
use lc::app::types::{
    ActivePanel, AppMode, AppState, DialogKind, InputAction, TextInput, ViewMode,
};
use lc::ui;
use lc::ui::viewer;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

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
        .set_entries(vec![TestEntry::new("view.txt").path(&file).size(4).build()]);
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
        dialog_input: TextInput {
            text: "needle".to_string(),
            cursor: 6,
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
        ratatui::layout::Size::new(80, 24),
    );

    assert!(matches!(state.mode, AppMode::Viewing));
    assert_eq!(state.prev_mode, Some(AppMode::Normal));
    assert!(state.dialog_input.text.is_empty());
    assert_eq!(state.dialog_input.cursor, 0);
}

#[test]
fn viewer_scroll_up_down() {
    let file = create_test_file("line1\nline2\nline3\nline4\nline5\n");
    let mut vs = ui::viewer::ViewerState::open(file.path()).unwrap();
    assert_eq!(vs.scroll_offset, 0);
    vs.scroll_down(2);
    assert_eq!(vs.scroll_offset, 2);
    vs.scroll_up(1);
    assert_eq!(vs.scroll_offset, 1);
    vs.scroll_up(10);
    assert_eq!(vs.scroll_offset, 0);
    vs.scroll_down(100);
    assert_eq!(vs.scroll_offset, 4);
}

#[test]
fn viewer_page_up_page_down() {
    let mut content = String::new();
    for i in 0..50 {
        content.push_str(&format!("line {}\n", i));
    }
    let file = create_test_file(&content);
    let mut vs = ui::viewer::ViewerState::open(file.path()).unwrap();
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
    let mut vs = ui::viewer::ViewerState::open(file.path()).unwrap();
    vs.search("apple", 24);
    assert_eq!(vs.search_query.as_deref(), Some("apple"));
    assert!(!vs.search_matches.is_empty());
    assert_eq!(vs.current_match, Some(0));
    vs.next_match(24);
    assert_eq!(vs.current_match, Some(1));
    vs.prev_match(24);
    assert_eq!(vs.current_match, Some(0));
    vs.search("", 24);
    assert!(vs.search_matches.is_empty());
}

#[test]
fn viewer_hex_mode_toggle() {
    let file = create_test_file("hello\nworld\n");
    let mut vs = ui::viewer::ViewerState::open(file.path()).unwrap();
    assert!(!vs.is_hex_mode());
    assert!(matches!(vs.view_mode, ViewMode::Text));
    vs.toggle_hex_mode();
    assert!(vs.is_hex_mode());
    assert!(matches!(vs.view_mode, ViewMode::Hex));
    vs.toggle_hex_mode();
    assert!(!vs.is_hex_mode());
    assert!(matches!(vs.view_mode, ViewMode::Text));
}

#[test]
fn viewer_close_via_escape() {
    let file = create_test_file("content\n");
    let mut viewer_state = Some(ui::viewer::ViewerState::open(file.path()).unwrap());
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
        ratatui::layout::Size::new(80, 24),
    );

    assert!(matches!(state.mode, AppMode::Normal));
    assert!(viewer_state.is_none());
    assert!(image_preview_loader.is_none());
}
