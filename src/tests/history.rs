use crate::input::pickers;
use crossterm::event::KeyCode;
use lc::app::shell;
use lc::app::types::{AppMode, AppState, PickerKind};

fn make_history_picker(commands: &[&str], selected: usize) -> AppState {
    let mut state = AppState::default();
    for cmd in commands {
        shell::push_history(&mut state, cmd);
    }
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.ui.picker_selected = selected;
    state
}

#[test]
fn history_dedup_consecutive() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "echo hi");
    shell::push_history(&mut state, "echo hi");
    assert_eq!(state.input.command_history.len(), 1);
    assert_eq!(state.input.command_history[0], "echo hi");
}

#[test]
fn history_dedup_different_commands() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "echo hi");
    shell::push_history(&mut state, "ls -la");
    assert_eq!(state.input.command_history.len(), 2);
}

#[test]
fn history_cap_at_100() {
    let mut state = AppState::default();
    for i in 0..101 {
        shell::push_history(&mut state, &format!("cmd_{i}"));
    }
    assert_eq!(state.input.command_history.len(), 100);
    assert_eq!(state.input.command_history[0], "cmd_1");
    assert_eq!(state.input.command_history[99], "cmd_100");
}

#[test]
fn history_picker_enter_loads_command_line() {
    let mut state = make_history_picker(&["git status", "git log"], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.input.command_line.text(), "git log");
}

#[test]
fn history_picker_esc_cancels() {
    let mut state = make_history_picker(&["ls"], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Esc);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn history_picker_navigate_up_down() {
    let mut state = make_history_picker(&["cmd1", "cmd2", "cmd3"], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn empty_history_does_not_open_picker() {
    let mut state = make_history_picker(&[], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn history_skips_empty_command() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "");
    assert!(state.input.command_history.is_empty());
}

#[test]
fn history_skips_whitespace_command() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "   ");
    assert!(state.input.command_history.is_empty());
}

#[test]
fn history_whitespace_after_valid_command() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "ls -la");
    shell::push_history(&mut state, "   ");
    assert_eq!(state.input.command_history.len(), 1);
    assert_eq!(state.input.command_history[0], "ls -la");
}

#[test]
fn history_picker_home_end() {
    let mut state = make_history_picker(&["cmd1", "cmd2", "cmd3"], 1);

    pickers::handle_list_picker(&mut state, KeyCode::Home);
    assert_eq!(state.ui.picker_selected, 0);

    pickers::handle_list_picker(&mut state, KeyCode::End);
    assert_eq!(state.ui.picker_selected, 2);
}

#[test]
fn history_dedup_non_consecutive_moves_to_end() {
    let mut state = AppState::default();
    shell::push_history(&mut state, "echo A");
    shell::push_history(&mut state, "echo B");
    shell::push_history(&mut state, "echo A");
    assert_eq!(state.input.command_history.len(), 2);
    assert_eq!(state.input.command_history[0], "echo B");
    assert_eq!(state.input.command_history[1], "echo A");
}

#[test]
fn history_picker_enter_selected_beyond_len() {
    let mut state = make_history_picker(&["cmd1"], 5);
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn history_picker_up_at_zero_clamps() {
    let mut state = make_history_picker(&["cmd1", "cmd2"], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.ui.picker_selected, 0);
}

#[test]
fn history_picker_down_at_last_clamps() {
    let mut state = make_history_picker(&["cmd1", "cmd2"], 1);
    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.ui.picker_selected, 1);
}

#[test]
fn history_picker_empty_list_all_directions_clamped() {
    let mut state = make_history_picker(&[], 0);
    for key in [KeyCode::Up, KeyCode::Down, KeyCode::Home, KeyCode::End] {
        pickers::handle_list_picker(&mut state, key);
        assert_eq!(state.ui.picker_selected, 0, "failed on {key:?}");
    }
}
