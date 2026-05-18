use crate::input::pickers;
use crate::*;
use app::types::PickerKind;

#[test]
fn history_dedup_consecutive() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.path = tmp.path().to_path_buf();
    state.command_history.push_back("echo hi".to_string());
    let cmd = "echo hi";
    if state.command_history.back().is_none_or(|l| l != cmd) {
        state.command_history.push_back(cmd.to_string());
    }
    assert_eq!(state.command_history.len(), 1);
    assert_eq!(state.command_history[0], "echo hi");
}

#[test]
fn history_dedup_different_commands() {
    let mut state = AppState::default();
    state.command_history.push_back("echo hi".to_string());
    let cmd = "ls -la";
    if state.command_history.back().is_none_or(|l| l != cmd) {
        state.command_history.push_back(cmd.to_string());
    }
    assert_eq!(state.command_history.len(), 2);
}

#[test]
fn history_cap_at_100() {
    let mut state = AppState::default();
    for i in 0..101 {
        let cmd = format!("cmd_{}", i);
        if state
            .command_history
            .back()
            .is_none_or(|l| l.as_str() != cmd.as_str())
        {
            state.command_history.push_back(cmd);
            if state.command_history.len() > shell::MAX_HISTORY {
                state.command_history.pop_front();
            }
        }
    }
    assert_eq!(state.command_history.len(), 100);
    assert_eq!(state.command_history[0], "cmd_1");
    assert_eq!(state.command_history[99], "cmd_100");
}

#[test]
fn history_picker_enter_loads_command_line() {
    let mut state = AppState::default();
    state.command_history.push_back("git status".to_string());
    state.command_history.push_back("git log".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(state.mode, AppMode::CommandLine);
    assert_eq!(state.command_line.text, "git log");
}

#[test]
fn history_picker_esc_cancels() {
    let mut state = AppState::default();
    state.command_history.push_back("ls".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn history_picker_navigate_up_down() {
    let mut state = AppState::default();
    state.command_history.push_back("cmd1".to_string());
    state.command_history.push_back("cmd2".to_string());
    state.command_history.push_back("cmd3".to_string());
    state.mode = AppMode::ListPicker(PickerKind::History);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Up);
    assert_eq!(state.picker_selected, 0);
}

#[test]
fn empty_history_does_not_open_picker() {
    let mut state = AppState {
        mode: AppMode::ListPicker(PickerKind::History),
        picker_selected: 0,
        ..Default::default()
    };
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
}
