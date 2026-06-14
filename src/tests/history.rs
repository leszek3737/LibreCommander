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

// The picker IS open (mode is ListPicker); the old name "does_not_open" was
// misleading. What actually happens: pressing Enter with an empty history has
// nothing to select, so the handler closes the picker back to Normal mode and
// loads no command line.
#[test]
fn history_picker_enter_on_empty_history_closes_to_normal() {
    let mut state = make_history_picker(&[], 0);
    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(state.mode, AppMode::Normal);
    assert!(state.input.command_line.text().is_empty());
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

// PageUp/PageDown are not wired into the picker's navigation (only
// Up/Down/Home/End are), so they fall through as no-ops and leave the cursor
// where it was — they do not page through history entries.
#[test]
fn history_picker_page_up_down_are_noops() {
    let mut state = make_history_picker(&["cmd1", "cmd2", "cmd3"], 1);

    pickers::handle_list_picker(&mut state, KeyCode::PageDown);
    assert_eq!(state.ui.picker_selected, 1, "PageDown does not move cursor");

    pickers::handle_list_picker(&mut state, KeyCode::PageUp);
    assert_eq!(state.ui.picker_selected, 1, "PageUp does not move cursor");
}

// A typed character inside the history picker is inert: the history picker does
// not filter, so a char neither moves the cursor nor closes the picker nor
// loads a command line.
#[test]
fn history_picker_typed_char_is_noop() {
    let mut state = make_history_picker(&["git status", "git log"], 0);

    pickers::handle_list_picker(&mut state, KeyCode::Char('g'));

    assert_eq!(state.ui.picker_selected, 0, "char does not move cursor");
    assert_eq!(
        state.mode,
        AppMode::ListPicker(PickerKind::History),
        "char does not close the picker"
    );
    assert!(
        state.input.command_line.text().is_empty(),
        "char does not load a command line"
    );
}

// Dedup interaction with the cap: with the buffer full at MAX_HISTORY (100)
// unique entries, re-pushing one that already exists (here the oldest,
// "cmd_0", currently at the front) must NOT drop an unrelated entry. `retain`
// removes the existing copy first (len 99), then push_back re-appends it
// (len 100), so no pop_front fires. Net effect: still 100 entries, the
// duplicate moved to the most-recent slot, and "cmd_1" is now the oldest. No
// unique command is lost to the cap.
#[test]
fn history_dedup_of_existing_entry_at_cap_preserves_all_uniques() {
    let mut state = AppState::default();
    for i in 0..100 {
        shell::push_history(&mut state, &format!("cmd_{i}"));
    }
    assert_eq!(state.input.command_history.len(), 100);
    assert_eq!(state.input.command_history[0], "cmd_0");

    // Re-push the entry currently at the front (oldest) while the buffer is full.
    shell::push_history(&mut state, "cmd_0");

    assert_eq!(
        state.input.command_history.len(),
        100,
        "dedup keeps the buffer at cap without growing"
    );
    assert_eq!(
        state.input.command_history[0], "cmd_1",
        "cmd_1 becomes the oldest after cmd_0 was deduped to the front"
    );
    assert_eq!(
        state.input.command_history[99], "cmd_0",
        "the re-pushed duplicate is now the most recent"
    );
    // No unique command was evicted by the cap: cmd_1..=cmd_99 plus cmd_0 all
    // still present exactly once.
    for i in 0..100 {
        let cmd = format!("cmd_{i}");
        assert_eq!(
            state
                .input
                .command_history
                .iter()
                .filter(|e| **e == cmd)
                .count(),
            1,
            "{cmd} present exactly once"
        );
    }
}
