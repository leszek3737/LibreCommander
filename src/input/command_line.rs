use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use lc::app::{shell, types::*};

use crate::app::panel_ops::refresh_active;

fn command_delete_word_backward(state: &mut AppState) {
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

pub(crate) fn handle_command_line(state: &mut AppState, key: KeyEvent) {
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
                state.command_line.clear();
                state.command_cursor = 0;
                state.history_index = None;
                return;
            }
            KeyCode::Char('w') => {
                command_delete_word_backward(state);
                return;
            }
            _ => {}
        }
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
