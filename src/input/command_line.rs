use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use lc::app::{shell, types::*};

use crate::app::panel_ops::refresh_active;

fn clamp_to_char_boundary(s: &str, mut byte_idx: usize) -> usize {
    if byte_idx > s.len() {
        byte_idx = s.len();
    }
    while byte_idx > 0 && !s.is_char_boundary(byte_idx) {
        byte_idx -= 1;
    }
    byte_idx
}

fn command_delete_word_backward(state: &mut AppState) {
    state.command_cursor = clamp_to_char_boundary(&state.command_line, state.command_cursor);
    let cursor = state.command_cursor;
    if cursor > 0 {
        let text = &state.command_line[..cursor];
        let word_start = text
            .char_indices()
            .rev()
            .skip_while(|&(_, c)| c.is_whitespace())
            .find(|&(_, c)| c.is_whitespace())
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        state.command_line.drain(word_start..cursor);
        state.command_cursor = word_start;
        state.history_index = None;
    }
}

fn command_execute(state: &mut AppState) {
    let cmd = state.command_line.clone();
    state.mode = AppMode::Normal;
    state.command_line.clear();
    state.command_cursor = 0;
    state.history_index = None;
    if !cmd.is_empty() {
        shell::run_shell_command(state, &cmd, false, refresh_active);
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn handle_command_line(state: &mut AppState, key: KeyEvent) {
    state.command_cursor = clamp_to_char_boundary(&state.command_line, state.command_cursor);
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => {
                state.command_cursor = 0;
                return;
            }
            KeyCode::Char('e') => {
                state.command_cursor = state.command_line.len();
                return;
            }
            KeyCode::Char('u') => {
                state.command_line.drain(..state.command_cursor);
                state.command_cursor = 0;
                return;
            }
            KeyCode::Char('w') => {
                command_delete_word_backward(state);
                return;
            }
            KeyCode::Char('c') => {
                state.mode = AppMode::Normal;
                state.command_line.clear();
                state.command_cursor = 0;
                state.history_index = None;
                return;
            }
            _ => return,
        }
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        return;
    }

    match key.code {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.command_cursor = 0;
            state.history_index = None;
        }
        KeyCode::Enter => {
            command_execute(state);
        }
        KeyCode::Backspace => {
            let cursor = state.command_cursor;
            if cursor > 0 {
                let prev = state.command_line[..cursor]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.command_line.drain(prev..cursor);
                state.command_cursor = prev;
                state.history_index = None;
            }
        }
        KeyCode::Left if state.command_cursor > 0 => {
            state.command_cursor = state.command_line[..state.command_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
        KeyCode::Left => {}
        KeyCode::Right if state.command_cursor < state.command_line.len() => {
            state.command_cursor = state.command_line[state.command_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| state.command_cursor + i)
                .unwrap_or(state.command_line.len());
        }
        KeyCode::Right => {}
        KeyCode::Up if !state.command_history.is_empty() => {
            if state.history_index.is_none() {
                state.command_draft = state.command_line.clone();
            }
            let idx = match state.history_index {
                Some(i) if i > 0 => i - 1,
                Some(i) => i,
                None => state.command_history.len() - 1,
            };
            state.history_index = Some(idx);
            state.command_line = state.command_history[idx].clone();
            state.command_cursor = state.command_line.len();
        }
        KeyCode::Down => {
            if let Some(idx) = state.history_index {
                if idx + 1 < state.command_history.len() {
                    state.history_index = Some(idx + 1);
                    state.command_line = state.command_history[idx + 1].clone();
                } else {
                    state.history_index = None;
                    state.command_line = state.command_draft.clone();
                }
                state.command_cursor = state.command_line.len();
            }
        }
        KeyCode::Char(c) => {
            state.command_line.insert(state.command_cursor, c);
            state.command_cursor += c.len_utf8();
            state.history_index = None;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cmd_state(line: &str, cursor: usize) -> AppState {
        AppState {
            command_line: line.to_string(),
            command_cursor: cursor,
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
        assert_eq!(state.command_line, "hell");
        assert_eq!(state.command_cursor, 4);
    }

    #[test]
    fn cmd_backspace_at_start_does_nothing() {
        let mut state = make_cmd_state("hello", 0);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(state.command_line, "hello");
        assert_eq!(state.command_cursor, 0);
    }

    #[test]
    fn cmd_left_moves_cursor() {
        let mut state = make_cmd_state("hello", 3);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.command_cursor, 2);
    }

    #[test]
    fn cmd_left_at_start_does_nothing() {
        let mut state = make_cmd_state("hello", 0);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(state.command_cursor, 0);
    }

    #[test]
    fn cmd_right_moves_cursor() {
        let mut state = make_cmd_state("hello", 2);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        assert_eq!(state.command_cursor, 3);
    }

    #[test]
    fn cmd_right_at_end_does_nothing() {
        let mut state = make_cmd_state("hello", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        assert_eq!(state.command_cursor, 5);
    }

    #[test]
    fn cmd_ctrl_a_moves_to_start() {
        let mut state = make_cmd_state("hello", 3);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_cursor, 0);
    }

    #[test]
    fn cmd_ctrl_e_moves_to_end() {
        let mut state = make_cmd_state("hello", 2);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_cursor, 5);
    }

    #[test]
    fn cmd_ctrl_u_kills_to_beginning() {
        let mut state = make_cmd_state("hello world", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line, " world");
        assert_eq!(state.command_cursor, 0);
    }

    #[test]
    fn cmd_ctrl_w_deletes_word() {
        let mut state = make_cmd_state("hello world", 11);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line, "hello ");
        assert_eq!(state.command_cursor, 6);
    }

    #[test]
    fn cmd_insert_char() {
        let mut state = make_cmd_state("hllo", 1);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        );
        assert_eq!(state.command_line, "hello");
        assert_eq!(state.command_cursor, 2);
    }

    #[test]
    fn cmd_multibyte_char_cursor() {
        let mut state = make_cmd_state("test", 4);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('ą'), KeyModifiers::NONE),
        );
        assert_eq!(state.command_line, "testą");
        assert_eq!(state.command_cursor, 6);
    }

    #[test]
    fn cmd_esc_clears() {
        let mut state = make_cmd_state("hello", 5);
        state.history_index = Some(0);
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.command_line, "");
        assert_eq!(state.command_cursor, 0);
        assert!(state.history_index.is_none());
    }

    #[test]
    fn cmd_up_loads_history() {
        let mut state = make_cmd_state("", 0);
        state.command_history.push_back("first".to_string());
        state.command_history.push_back("second".to_string());
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.command_line, "second");
        assert_eq!(state.command_cursor, 6);
        assert_eq!(state.history_index, Some(1));
    }

    #[test]
    fn cmd_down_restores_draft() {
        let mut state = make_cmd_state("draft", 5);
        state.command_history.push_back("first".to_string());
        state.history_index = Some(0);
        state.command_draft = "draft".to_string();
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.command_line, "draft");
        assert!(state.history_index.is_none());
    }

    #[test]
    fn cmd_delete_word_backward_first_word() {
        let mut state = make_cmd_state("hello", 5);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.command_line, "");
        assert_eq!(state.command_cursor, 0);
    }

    #[test]
    fn cmd_clamp_cursor_mid_multibyte() {
        let mut state = make_cmd_state("teąst", 4);
        assert_eq!(state.command_cursor, 4);
        state.command_cursor = 3;
        handle_command_line(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(state.command_line.is_char_boundary(state.command_cursor));
    }

    #[test]
    fn cmd_clamp_cursor_overshoot() {
        let mut state = make_cmd_state("hello", 100);
        handle_command_line(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        assert_eq!(state.command_cursor, 5);
    }
}
