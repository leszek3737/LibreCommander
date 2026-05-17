use std::sync::atomic::Ordering;

use crossterm::event::KeyCode;
use ratatui::layout::Rect;

use lc::app::job_runner::{RunningJob, start_confirmed_action};
use lc::app::shell;
use lc::app::types::*;
use lc::fs;
use lc::ops;
use lc::ui::{dialogs, viewer};

use crate::app::panel_ops::{panel_visible_height, refresh_active, refresh_both, set_active_panel};

const MAX_DIALOG_INPUT_BYTES: usize = 4096;

pub(crate) fn parse_octal_mode(input: &str) -> Option<u32> {
    let mode = u32::from_str_radix(input.trim(), 8).ok()?;
    if mode <= 0o7777 { Some(mode) } else { None }
}

enum ValidationResult {
    Valid,
    EmptyInput,
    InvalidPath(String),
    InvalidOctal(String),
}

fn validate_non_empty(input: &str) -> ValidationResult {
    if input.trim().is_empty() {
        ValidationResult::EmptyInput
    } else {
        ValidationResult::Valid
    }
}

fn contains_parent_dir(input: &str) -> bool {
    std::path::Path::new(input)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn validate_path_name(input: &str) -> ValidationResult {
    match validate_non_empty(input) {
        ValidationResult::Valid => {}
        other => return other,
    }
    if input.contains('/') || input.contains('\\') {
        return ValidationResult::InvalidPath(format!("Name contains path separator: {input}"));
    }
    if contains_parent_dir(input) {
        ValidationResult::InvalidPath(input.to_string())
    } else {
        ValidationResult::Valid
    }
}

fn validate_octal(input: &str) -> ValidationResult {
    match validate_non_empty(input) {
        ValidationResult::Valid => {}
        other => return other,
    }
    if parse_octal_mode(input).is_some() {
        ValidationResult::Valid
    } else {
        ValidationResult::InvalidOctal(input.to_string())
    }
}

fn dismiss_dialog_and_restore(state: &mut AppState) {
    state.mode = AppMode::Normal;
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

fn finish_confirmed_action(state: &mut AppState) {
    state.dialog_selection = 0;
    if state.status_message.is_some() {
        state.mode = AppMode::Normal;
        refresh_both(state);
        if let Some(panel) = state.menu_restore_panel.take() {
            set_active_panel(state, panel);
        }
    }
}

pub(crate) fn dismiss_dialog(state: &mut AppState) {
    state.mode = AppMode::Normal;
    state.pending_action = None;
    state.pending_menu_command = None;
    state.status_message = None;
    state.dialog_selection = 0;
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

#[cfg(unix)]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(src_meta) = std::fs::symlink_metadata(src) else {
        return false;
    };
    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        return false;
    };
    src_meta.dev() == dest_meta.dev() && src_meta.ino() == dest_meta.ino()
}

#[cfg(not(unix))]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    match (src.canonicalize(), dest.canonicalize()) {
        (Ok(s), Ok(d)) => s == d,
        _ => src == dest,
    }
}

pub(crate) fn check_overwrite_conflict(state: &AppState) -> Option<Vec<String>> {
    let action = state.pending_action.as_ref()?;
    let (sources, dest_dir, overwrite) = match action {
        PendingAction::Copy {
            sources,
            dest,
            overwrite,
        } => (sources, dest, *overwrite),
        PendingAction::Move {
            sources,
            dest,
            overwrite,
        } => (sources, dest, *overwrite),
        PendingAction::Delete { .. } => return None,
    };
    if overwrite {
        return None;
    }
    let conflicting: Vec<String> = sources
        .iter()
        .filter_map(|s| {
            let name = s.file_name()?;
            let target = dest_dir.join(name);
            if is_same_file(s, &target) {
                return None;
            }
            if std::fs::symlink_metadata(&target).is_ok() {
                Some(name.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    if conflicting.is_empty() {
        None
    } else {
        Some(conflicting)
    }
}

fn confirm_dialog_key(state: &mut AppState, key: KeyCode) -> Option<bool> {
    match key {
        KeyCode::Char('y' | 'Y') => Some(true),
        KeyCode::Char('n' | 'N') => Some(false),
        KeyCode::Enter => Some(state.dialog_selection == 0),
        KeyCode::Esc => {
            dismiss_dialog(state);
            None
        }
        KeyCode::Left | KeyCode::Right => {
            state.dialog_selection = if state.dialog_selection == 0 { 1 } else { 0 };
            None
        }
        _ => None,
    }
}

fn handle_confirm_dialog(state: &mut AppState, running_job: &mut Option<RunningJob>, key: KeyCode) {
    let Some(confirmed) = confirm_dialog_key(state, key) else {
        return;
    };

    if confirmed {
        if state.pending_action.is_some() {
            if let Some(conflicting) = check_overwrite_conflict(state) {
                state.dialog_selection = 0;
                state.mode = AppMode::Dialog(DialogKind::OverwriteConfirm { conflicting });
                return;
            }
            start_confirmed_action(state, running_job);
            finish_confirmed_action(state);
        } else if let Some(cmd) = state.pending_menu_command.take() {
            state.mode = AppMode::Normal;
            shell::run_shell_command(state, &cmd, true, refresh_active);
        } else {
            dismiss_dialog(state);
            refresh_both(state);
        }
    } else {
        dismiss_dialog(state);
    }
}

fn handle_overwrite_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    match key {
        KeyCode::Esc => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Left => {
            state.dialog_selection = state.dialog_selection.saturating_sub(1);
            return;
        }
        KeyCode::Right => {
            state.dialog_selection = (state.dialog_selection + 1).min(1);
            return;
        }
        KeyCode::Char('o' | 'O') => {
            set_pending_overwrite(state);
        }
        KeyCode::Char('c' | 'C') => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Enter => match state.dialog_selection {
            0 => set_pending_overwrite(state),
            1 => {
                dismiss_dialog(state);
                return;
            }
            _ => return,
        },
        _ => return,
    }
    start_confirmed_action(state, running_job);
    finish_confirmed_action(state);
}

fn set_pending_overwrite(state: &mut AppState) {
    if let Some(action) = state.pending_action.as_mut() {
        match action {
            PendingAction::Copy { overwrite, .. } | PendingAction::Move { overwrite, .. } => {
                *overwrite = true;
            }
            PendingAction::Delete { .. } => {}
        }
    }
}

fn handle_find_file(state: &mut AppState, input: &str, terminal_height: u16) {
    let dir = state.active_panel().path.clone();
    let outcome = ops::FileSearch::search_files_with_diagnostics(&dir, input, true, false);
    let result_count = outcome.matches.len();
    let error_count = outcome.errors.len();
    let truncated = outcome.truncated;
    if let Some(first) = outcome.matches.first()
        && let Some(parent) = first.path.parent()
    {
        state.active_panel_mut().path = parent.to_path_buf();
        refresh_active(state);
        if let Some(pos) = state
            .active_panel()
            .entries
            .iter()
            .position(|e| e.path == first.path)
        {
            state.active_panel_mut().cursor = pos;
            state
                .active_panel_mut()
                .ensure_cursor_visible(panel_visible_height(terminal_height));
        }
    }
    let mut message = if result_count > 0 {
        format!("Found {result_count} match(es) for '{input}'")
    } else {
        format!("No matches for '{input}'")
    };
    if error_count > 0 {
        message.push_str(&format!(", {error_count} error(s)"));
    }
    if let Some(reason) = truncated {
        let label = match reason {
            ops::TruncationReason::DepthLimit => "depth limit",
            ops::TruncationReason::ItemLimit => "item limit",
            ops::TruncationReason::ContentResultLimit => "result limit",
            ops::TruncationReason::FileTooLarge => "file too large",
            ops::TruncationReason::LineTooLong => "line too long",
            ops::TruncationReason::BinaryFile => "binary file",
        };
        message.push_str(&format!(", truncated ({label})"));
    }
    state.status_message = Some(message);
}

fn handle_quick_cd(state: &mut AppState, input: &str) {
    let expanded = fs::path::resolve_user_path(&state.active_panel().path, input);

    if expanded.is_dir() {
        let panel = state.active_panel_mut();
        panel.history.push(panel.path.clone());
        panel.path = expanded.clone();
        panel.cursor = 0;
        panel.scroll_offset = 0;
        refresh_active(state);
        if !state.directory_hotlist.iter().any(|p| p == &expanded) {
            state.directory_hotlist.push(expanded);
        }
    } else if expanded.exists() {
        state.status_message = Some(format!("Not a directory: {input}"));
    } else {
        state.status_message = Some(format!("Directory not found: {input}"));
    }
}

fn handle_input_action(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    action: &InputAction,
    terminal_height: u16,
) -> bool {
    let input = state.dialog_input.clone();
    match action {
        InputAction::ViewerSearch => {
            if let Some(vs) = viewer_state.as_mut() {
                vs.search(&input, terminal_height.saturating_sub(3) as usize);
            }
            state.mode = AppMode::Viewing;
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            return true;
        }
        InputAction::CreateDirectory => {
            match validate_path_name(&input) {
                ValidationResult::Valid => {}
                ValidationResult::EmptyInput => {
                    state.status_message = Some("Directory name cannot be empty".to_string());
                    return false;
                }
                ValidationResult::InvalidPath(p) => {
                    state.status_message = Some(format!("Invalid path: '..' not allowed in '{p}'"));
                    return false;
                }
                _ => return false,
            }
            let target = fs::path::resolve_user_path(&state.active_panel().path, &input);
            if let Err(err) = ops::create_directory(&target) {
                state.status_message = Some(format!("Create directory failed: {err}"));
            }
        }
        InputAction::Rename => {
            match validate_path_name(&input) {
                ValidationResult::Valid => {}
                ValidationResult::EmptyInput => {
                    state.status_message = Some("New name cannot be empty".to_string());
                    return false;
                }
                ValidationResult::InvalidPath(p) => {
                    state.status_message = Some(format!("Invalid name: '..' not allowed in '{p}'"));
                    return false;
                }
                _ => return false,
            }
            if let Some(entry) = state.active_panel().current_entry()
                && let Err(err) = ops::rename_entry(&entry.path, &input)
            {
                state.status_message = Some(format!("Rename failed: {err}"));
            }
        }
        InputAction::Chmod => {
            match validate_octal(&input) {
                ValidationResult::Valid => {}
                ValidationResult::EmptyInput => {
                    state.status_message = Some("Octal mode cannot be empty".to_string());
                    return false;
                }
                ValidationResult::InvalidOctal(o) => {
                    state.status_message = Some(format!("Invalid octal mode '{o}'"));
                    return false;
                }
                _ => return false,
            }
            let mode = parse_octal_mode(&input).unwrap_or(0);
            if let Some(entry) = state.active_panel().current_entry()
                && let Err(err) = ops::chmod(&entry.path, mode)
            {
                state.status_message = Some(format!("Chmod failed: {err}"));
            }
        }
        InputAction::Filter => {
            let panel = state.active_panel_mut();
            panel.filter = if input.trim().is_empty() {
                None
            } else {
                Some(input)
            };
        }
        InputAction::QuickCd => handle_quick_cd(state, &input),
        InputAction::FindFile => handle_find_file(state, &input, terminal_height),
    }
    state.mode = AppMode::Normal;
    state.dialog_input.clear();
    state.dialog_cursor_pos = 0;
    refresh_active(state);
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
    false
}

fn apply_text_edit(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Backspace if state.dialog_cursor_pos > 0 => {
            state.dialog_cursor_pos -= 1;
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(state.dialog_input.len());
            let next_byte = state.dialog_input[byte_pos..]
                .chars()
                .next()
                .map(|c| byte_pos + c.len_utf8())
                .unwrap_or(state.dialog_input.len());
            state.dialog_input.drain(byte_pos..next_byte);
        }
        KeyCode::Delete => {
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i);
            if let Some(pos) = byte_pos {
                let next_char_end = state.dialog_input[pos..]
                    .chars()
                    .next()
                    .map(|c| pos + c.len_utf8())
                    .unwrap_or(state.dialog_input.len());
                state.dialog_input.drain(pos..next_char_end);
            }
        }
        KeyCode::Char(c) => {
            if state.dialog_input.len() >= MAX_DIALOG_INPUT_BYTES {
                return;
            }
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(state.dialog_input.len());
            state.dialog_input.insert(byte_pos, c);
            state.dialog_cursor_pos += 1;
        }
        KeyCode::Left if state.dialog_cursor_pos > 0 => {
            state.dialog_cursor_pos -= 1;
        }
        KeyCode::Right if state.dialog_cursor_pos < state.dialog_input.chars().count() => {
            state.dialog_cursor_pos += 1;
        }
        KeyCode::Home => {
            state.dialog_cursor_pos = 0;
        }
        KeyCode::End => {
            state.dialog_cursor_pos = state.dialog_input.chars().count();
        }
        _ => {}
    }
}

fn handle_input_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    action: &InputAction,
    key: KeyCode,
    terminal_height: u16,
) -> bool {
    match key {
        KeyCode::Enter => handle_input_action(state, viewer_state, action, terminal_height),
        KeyCode::Esc => {
            if *action == InputAction::ViewerSearch {
                state.mode = AppMode::Viewing;
            } else {
                state.mode = AppMode::Normal;
            }
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
            false
        }
        _ => {
            apply_text_edit(state, key);
            false
        }
    }
}

fn handle_error_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

fn handle_progress_dialog(state: &mut AppState, running_job: &Option<RunningJob>, key: KeyCode) {
    if key == KeyCode::Esc
        && let Some(job) = running_job.as_ref()
    {
        job.cancel.store(true, Ordering::Relaxed);
        state.status_message = Some("Cancel requested".to_string());
    }
}

fn handle_properties_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

fn handle_copymove_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    let Some(confirmed) = confirm_dialog_key(state, key) else {
        return;
    };

    if confirmed {
        let action = if let AppMode::Dialog(DialogKind::CopyMove {
            source,
            dest,
            is_move,
        }) = &state.mode
        {
            if *is_move {
                PendingAction::Move {
                    sources: source.clone(),
                    dest: dest.clone(),
                    overwrite: false,
                }
            } else {
                PendingAction::Copy {
                    sources: source.clone(),
                    dest: dest.clone(),
                    overwrite: false,
                }
            }
        } else {
            return;
        };
        state.pending_action = Some(action);
        if let Some(conflicting) = check_overwrite_conflict(state) {
            state.dialog_selection = 0;
            state.mode = AppMode::Dialog(DialogKind::OverwriteConfirm { conflicting });
            return;
        }
        start_confirmed_action(state, running_job);
        finish_confirmed_action(state);
    } else {
        dismiss_dialog(state);
    }
}

pub(crate) fn handle_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
    terminal_size: ratatui::layout::Size,
) {
    if let AppMode::Dialog(DialogKind::Help {
        message,
        scroll_offset,
    }) = &mut state.mode
    {
        let term_rect = Rect::new(0, 0, terminal_size.width, terminal_size.height);
        let total_lines =
            dialogs::wrapped_line_count(message, dialogs::help_message_width(term_rect));
        let max_lines = dialogs::help_visible_height(term_rect);
        let should_exit = match key {
            KeyCode::Up | KeyCode::Char('k') => {
                *scroll_offset = scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + 1).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::PageUp => {
                *scroll_offset = scroll_offset.saturating_sub(max_lines);
                false
            }
            KeyCode::PageDown => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + max_lines).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::Home => {
                *scroll_offset = 0;
                false
            }
            KeyCode::End => {
                if total_lines > max_lines {
                    *scroll_offset = total_lines - max_lines;
                }
                false
            }
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => true,
            _ => false,
        };
        if should_exit {
            dismiss_dialog_and_restore(state);
        }
        return;
    }

    let input_action = if let AppMode::Dialog(DialogKind::Input { ref action, .. }) = state.mode {
        Some(*action)
    } else {
        None
    };

    let dk = if let AppMode::Dialog(ref dk) = state.mode {
        dk
    } else {
        return;
    };

    match dk {
        DialogKind::Confirm(_) => {
            handle_confirm_dialog(state, running_job, key);
        }
        DialogKind::Input { .. } => {
            if let Some(action) = input_action {
                let _ =
                    handle_input_dialog(state, viewer_state, &action, key, terminal_size.height);
            }
        }
        DialogKind::Error(_) => {
            handle_error_dialog(state, key);
        }
        DialogKind::Progress(_, _, _) => {
            handle_progress_dialog(state, running_job, key);
        }
        DialogKind::Properties { .. } => {
            handle_properties_dialog(state, key);
        }
        DialogKind::CopyMove { .. } => {
            handle_copymove_dialog(state, running_job, key);
        }
        DialogKind::OverwriteConfirm { .. } => {
            handle_overwrite_dialog(state, running_job, key);
        }
        // unreachable: Help handled above; arm kept for match exhaustiveness
        DialogKind::Help { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input_state(text: &str, cursor: usize) -> AppState {
        AppState {
            dialog_input: text.to_string(),
            dialog_cursor_pos: cursor,
            ..Default::default()
        }
    }

    #[test]
    fn text_edit_insert_char() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Char('!'));
        assert_eq!(state.dialog_input, "hello!");
        assert_eq!(state.dialog_cursor_pos, 6);
    }

    #[test]
    fn text_edit_insert_middle() {
        let mut state = make_input_state("helo", 2);
        apply_text_edit(&mut state, KeyCode::Char('l'));
        assert_eq!(state.dialog_input, "hello");
        assert_eq!(state.dialog_cursor_pos, 3);
    }

    #[test]
    fn text_edit_backspace() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.dialog_input, "hell");
        assert_eq!(state.dialog_cursor_pos, 4);
    }

    #[test]
    fn text_edit_backspace_at_start() {
        let mut state = make_input_state("hello", 0);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.dialog_input, "hello");
        assert_eq!(state.dialog_cursor_pos, 0);
    }

    #[test]
    fn text_edit_delete() {
        let mut state = make_input_state("hello", 0);
        apply_text_edit(&mut state, KeyCode::Delete);
        assert_eq!(state.dialog_input, "ello");
        assert_eq!(state.dialog_cursor_pos, 0);
    }

    #[test]
    fn text_edit_delete_at_end() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Delete);
        assert_eq!(state.dialog_input, "hello");
        assert_eq!(state.dialog_cursor_pos, 5);
    }

    #[test]
    fn text_edit_left_right() {
        let mut state = make_input_state("hello", 3);
        apply_text_edit(&mut state, KeyCode::Left);
        assert_eq!(state.dialog_cursor_pos, 2);
        apply_text_edit(&mut state, KeyCode::Right);
        assert_eq!(state.dialog_cursor_pos, 3);
    }

    #[test]
    fn text_edit_home_end() {
        let mut state = make_input_state("hello", 3);
        apply_text_edit(&mut state, KeyCode::Home);
        assert_eq!(state.dialog_cursor_pos, 0);
        apply_text_edit(&mut state, KeyCode::End);
        assert_eq!(state.dialog_cursor_pos, 5);
    }

    #[test]
    fn text_edit_multibyte_insert() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Char('ą'));
        assert_eq!(state.dialog_input, "helloą");
        assert_eq!(state.dialog_cursor_pos, 6);
    }

    #[test]
    fn text_edit_multibyte_backspace() {
        let mut state = make_input_state("helloą", 6);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.dialog_input, "hello");
        assert_eq!(state.dialog_cursor_pos, 5);
    }

    #[test]
    fn text_edit_emoji_insert() {
        let mut state = make_input_state("test", 4);
        apply_text_edit(&mut state, KeyCode::Char('🎉'));
        assert_eq!(state.dialog_input, "test🎉");
        assert_eq!(state.dialog_cursor_pos, 5);
    }

    #[test]
    fn text_edit_emoji_backspace() {
        let mut state = make_input_state("test🎉", 5);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.dialog_input, "test");
        assert_eq!(state.dialog_cursor_pos, 4);
    }
}
