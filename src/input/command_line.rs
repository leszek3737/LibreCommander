use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use lc::app::{shell, types::*};

use crate::app::panel_ops::refresh_active;

fn reset_history(state: &mut AppState) {
    state.history_index = None;
}

fn cancel_command_input(state: &mut AppState) {
    state.mode = AppMode::Normal;
    state.command_line.clear();
    state.command_draft.clear();
    reset_history(state);
}

fn command_execute(state: &mut AppState) {
    let cmd = std::mem::take(&mut state.command_line.text);
    state.command_line.cursor = 0;
    state.mode = AppMode::Normal;
    state.command_draft.clear();
    reset_history(state);
    if !cmd.is_empty() {
        shell::run_shell_command(state, &cmd, false, refresh_active);
    }
}

pub(crate) fn handle_command_line(state: &mut AppState, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => {
                state.command_line.cursor_start();
                return;
            }
            KeyCode::Char('e') => {
                state.command_line.cursor_end();
                return;
            }
            KeyCode::Char('u') => {
                state.command_line.drain_to_start();
                return;
            }
            KeyCode::Char('w') => {
                if state.command_line.delete_word_backward() {
                    reset_history(state);
                }
                return;
            }
            KeyCode::Char('c') => {
                cancel_command_input(state);
                return;
            }
            _ => {}
        }
        return;
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        if key.code == KeyCode::Backspace && state.command_line.delete_word_backward() {
            reset_history(state);
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            cancel_command_input(state);
        }
        KeyCode::Enter => {
            command_execute(state);
        }
        KeyCode::Backspace if state.command_line.backspace() => {
            reset_history(state);
        }
        KeyCode::Left => {
            state.command_line.cursor_left();
        }
        KeyCode::Right => {
            state.command_line.cursor_right();
        }
        KeyCode::Up if !state.command_history.is_empty() => {
            if state.history_index.is_none() {
                state.command_draft = std::mem::take(&mut state.command_line.text);
            }
            let idx = match state.history_index {
                Some(i) if i > 0 => i - 1,
                // idx == 0: already at oldest entry, clamp here
                Some(i) => i,
                None => state.command_history.len() - 1,
            };
            state.history_index = Some(idx);
            state.command_line.text = state.command_history[idx].clone();
            state.command_line.cursor_end();
        }
        KeyCode::Down if !state.command_history.is_empty() => {
            if let Some(idx) = state.history_index {
                if idx + 1 < state.command_history.len() {
                    state.history_index = Some(idx + 1);
                    state.command_line.text = state.command_history[idx + 1].clone();
                } else {
                    state.history_index = None;
                    state.command_line.text = state.command_draft.clone();
                }
                state.command_line.cursor_end();
            }
        }
        KeyCode::Char(c) => {
            state.command_line.insert_char(c);
            reset_history(state);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cmd_state(line: &str, cursor: usize) -> AppState {
        let mut command_line = TextInput::new();
        command_line.text = line.to_string();
        command_line.recompute_grapheme_count();
        command_line.cursor = cursor;
        AppState {
            command_line,
            ..Default::default()
        }
    }

    #[test]
    fn cmd_backspace_deletes_char() {
        let mut state = make_cmd_state("hello", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.text, "hell");
        assert_eq!(state.command_line.cursor, 4);
    }

    #[test]
    fn cmd_backspace_at_start_does_nothing() {
        let mut state = make_cmd_state("hello", 0);
        state.history_index = Some(0);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.text, "hello");
        assert_eq!(state.command_line.cursor, 0);
        assert_eq!(state.history_index, Some(0));
    }

    #[test]
    fn cmd_left_moves_cursor() {
        let mut state = make_cmd_state("hello", 3);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.command_line.cursor, 2);
    }

    #[test]
    fn cmd_left_at_start_does_nothing() {
        let mut state = make_cmd_state("hello", 0);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.command_line.cursor, 0);
    }

    #[test]
    fn cmd_right_moves_cursor() {
        let mut state = make_cmd_state("hello", 2);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.cursor, 3);
    }

    #[test]
    fn cmd_right_at_end_does_nothing() {
        let mut state = make_cmd_state("hello", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.cursor, 5);
    }

    #[test]
    fn cmd_ctrl_a_moves_to_start() {
        let mut state = make_cmd_state("hello", 3);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.cursor, 0);
    }

    #[test]
    fn cmd_ctrl_e_moves_to_end() {
        let mut state = make_cmd_state("hello", 2);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.cursor, 5);
    }

    #[test]
    fn cmd_ctrl_u_kills_to_beginning() {
        let mut state = make_cmd_state("hello world", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.text, " world");
        assert_eq!(state.command_line.cursor, 0);
    }

    #[test]
    fn cmd_ctrl_w_deletes_word() {
        let mut state = make_cmd_state("hello world", 11);
        state.history_index = Some(0);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.text, "hello ");
        assert_eq!(state.command_line.cursor, 6);
        assert!(state.history_index.is_none());
    }

    #[test]
    fn cmd_ctrl_w_at_start_keeps_history_index() {
        let mut state = make_cmd_state("hello", 0);
        state.history_index = Some(0);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.text, "hello");
        assert_eq!(state.command_line.cursor, 0);
        assert_eq!(state.history_index, Some(0));
    }

    #[test]
    fn cmd_insert_char() {
        let mut state = make_cmd_state("hllo", 1);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.text, "hello");
        assert_eq!(state.command_line.cursor, 2);
    }

    #[test]
    fn cmd_multibyte_char_cursor() {
        let mut state = make_cmd_state("test", 4);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('ą'), KeyModifiers::NONE),
        );
        assert_eq!(state.command_line.text, "testą");
        assert_eq!(state.command_line.cursor, 5);
    }

    #[test]
    fn cmd_esc_clears() {
        let mut state = make_cmd_state("hello", 5);
        state.history_index = Some(0);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.command_line.text, "");
        assert_eq!(state.command_line.cursor, 0);
        assert!(state.history_index.is_none());
    }

    #[test]
    fn cmd_up_loads_history() {
        let mut state = make_cmd_state("", 0);
        state.command_history.push_back("first".to_string());
        state.command_history.push_back("second".to_string());
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.command_line.text, "second");
        assert_eq!(state.command_line.cursor, 6);
        assert_eq!(state.history_index, Some(1));
    }

    #[test]
    fn cmd_down_restores_draft() {
        let mut state = make_cmd_state("draft", 5);
        state.command_history.push_back("first".to_string());
        state.history_index = Some(0);
        state.command_draft = "draft".to_string();
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.command_line.text, "draft");
        assert!(state.history_index.is_none());
    }

    #[test]
    fn cmd_delete_word_backward_first_word() {
        let mut state = make_cmd_state("hello", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line.text, "");
        assert_eq!(state.command_line.cursor, 0);
    }

    #[test]
    fn cmd_cursor_respects_char_boundaries() {
        let mut state = make_cmd_state("testą", 5);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.command_line.cursor, 4);
    }
}
