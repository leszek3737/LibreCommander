use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::app::job_runner::{RunningJob, start_confirmed_action};
use crate::app::shell;
use crate::app::types::{ActivePanel, AppMode, AppState, DialogKind};
use crate::menu::{MENU_ITEMS, MENU_TITLES, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::viewer;

use super::dialogs::{check_overwrite_conflict, dismiss_dialog, finish_confirmed_action};
use crate::app::panel_ops::{refresh_active, refresh_both, refresh_panel};

const SCROLL_LINES: usize = 3;
const DOUBLE_CLICK_THRESHOLD_MS: u64 = 300;

pub enum MouseOutcome {
    Consumed,
    NormalKey(KeyCode),
    MenuAction,
}

pub(crate) struct MousePosition {
    pub(crate) col: u16,
    pub(crate) row: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub fn handle_mouse_event(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    running_job: &mut Option<RunningJob>,
    mouse_event: crossterm::event::MouseEvent,
    terminal_size: ratatui::layout::Size,
) -> Option<MouseOutcome> {
    let pos = MousePosition {
        col: mouse_event.column,
        row: mouse_event.row,
        width: terminal_size.width,
        height: terminal_size.height,
    };

    if matches!(
        mouse_event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        handle_mouse_scroll(state, _viewer_state, mouse_event.kind, &pos);
        return None;
    }

    if matches!(mouse_event.kind, MouseEventKind::Drag(MouseButton::Left)) {
        handle_mouse_drag(state, &pos);
        return None;
    }

    if matches!(mouse_event.kind, MouseEventKind::Up(_)) {
        handle_mouse_up(state);
        return None;
    }

    let MouseEventKind::Down(button) = mouse_event.kind else {
        return None;
    };

    match button {
        MouseButton::Left => {
            handle_left_down(state, _viewer_state, viewer_loader, running_job, &pos)
        }
        MouseButton::Middle => handle_middle_down(state, &pos),
        MouseButton::Right => handle_right_down(state, &pos),
    }
}

fn handle_left_down(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    if let Some(outcome) = handle_mouse_dialog(state, running_job, pos) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_menu_bar(state, pos) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_menu_dropdown(state, pos) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_function_bar(state, pos) {
        return Some(outcome);
    }

    handle_mouse_panels(state, _viewer_state, viewer_loader, pos);
    None
}

fn handle_middle_down(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);
    if pos.row > panel_start_row
        && pos.row < panel_end_row
        && !matches!(state.mode, AppMode::Dialog(_))
    {
        if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
            return Some(MouseOutcome::Consumed);
        }
        let mid_col = pos.width / 2;
        if pos.col < mid_col {
            state.active_panel = ActivePanel::Left;
        } else {
            state.active_panel = ActivePanel::Right;
        }
        Some(MouseOutcome::NormalKey(KeyCode::F(5)))
    } else {
        Some(MouseOutcome::Consumed)
    }
}

fn handle_right_down(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    if let AppMode::Dialog(_) = state.mode {
        return Some(MouseOutcome::NormalKey(KeyCode::Esc));
    }
    if matches!(state.mode, AppMode::Menu) {
        return Some(MouseOutcome::NormalKey(KeyCode::Esc));
    }
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);
    if pos.row > panel_start_row && pos.row < panel_end_row {
        return Some(MouseOutcome::NormalKey(KeyCode::Esc));
    }
    Some(MouseOutcome::Consumed)
}

fn panel_bounds(height: u16) -> (u16, u16) {
    (1u16, height.saturating_sub(4))
}

fn handle_mouse_scroll(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    kind: crossterm::event::MouseEventKind,
    pos: &MousePosition,
) {
    use crossterm::event::MouseEventKind;

    let delta = match kind {
        MouseEventKind::ScrollUp => -(SCROLL_LINES as isize),
        MouseEventKind::ScrollDown => SCROLL_LINES as isize,
        _ => return,
    };

    match &mut state.mode {
        AppMode::Dialog(DialogKind::Help {
            message,
            scroll_offset: off,
        }) => {
            let term_rect = Rect::new(0, 0, pos.width, pos.height);
            let visible = crate::ui::dialogs::help_visible_height(term_rect);
            let total_lines = crate::ui::dialogs::wrapped_line_count(
                message,
                crate::ui::dialogs::help_message_width(term_rect),
            );
            *off = apply_scroll_delta(*off, delta, visible, total_lines);
            return;
        }
        AppMode::Dialog(DialogKind::Error(_)) => {
            return;
        }
        AppMode::Viewing => {
            if let Some(vs) = viewer_state {
                match kind {
                    MouseEventKind::ScrollUp => vs.scroll_up(SCROLL_LINES),
                    MouseEventKind::ScrollDown => vs.scroll_down(SCROLL_LINES),
                    _ => {}
                }
            }
            return;
        }
        _ => {}
    }

    if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
        return;
    }
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);
    if pos.row < panel_start_row || pos.row > panel_end_row {
        return;
    }
    let panel_height = panel_end_row.saturating_sub(panel_start_row) + 1;
    let visible_rows = panel_height.saturating_sub(2) as usize;
    let mid_col = pos.width / 2;
    if pos.col < mid_col {
        state.active_panel = ActivePanel::Left;
    } else {
        state.active_panel = ActivePanel::Right;
    }
    let panel = state.active_panel_mut();
    let len = panel.entries.len();
    match kind {
        MouseEventKind::ScrollUp => {
            panel.cursor = panel.cursor.saturating_sub(SCROLL_LINES);
            panel.ensure_cursor_visible(visible_rows);
        }
        MouseEventKind::ScrollDown => {
            if panel.cursor + SCROLL_LINES < len {
                panel.cursor += SCROLL_LINES;
            } else {
                panel.cursor = len.saturating_sub(1);
            }
            panel.ensure_cursor_visible(visible_rows);
        }
        _ => {}
    }
}

fn apply_scroll_delta(current: usize, delta: isize, visible: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        let max_scroll = total.saturating_sub(visible);
        (current + delta as usize).min(max_scroll)
    }
}

fn handle_mouse_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    if let AppMode::Dialog(DialogKind::Progress(_, _, _)) = state.mode {
        return handle_progress_click(state, running_job, pos);
    }

    if let AppMode::Dialog(DialogKind::Input { .. }) = state.mode {
        return Some(MouseOutcome::Consumed);
    }

    if let AppMode::Dialog(DialogKind::Confirm(_)) = state.mode {
        return handle_confirm_click(state, running_job, pos);
    }

    if let AppMode::Dialog(DialogKind::OverwriteConfirm { .. }) = state.mode {
        return handle_overwrite_click(state, running_job, pos);
    }

    if let AppMode::Dialog(_) = state.mode {
        return Some(MouseOutcome::Consumed);
    }

    None
}

fn dialog_button_row(height: u16, dialog_height: u16) -> u16 {
    let dialog_y = (height.saturating_sub(dialog_height)) / 2;
    dialog_y + dialog_height.saturating_sub(2)
}

fn dialog_left(width: u16, dialog_width: u16) -> u16 {
    (width.saturating_sub(dialog_width)) / 2
}

fn handle_confirm_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let dialog_height = pos.height * 40 / 100;
    let btn_row = dialog_button_row(pos.height, dialog_height);
    let dialog_width = pos.width / 2;
    let dialog_left = dialog_left(pos.width, dialog_width);

    if pos.row == btn_row && pos.col >= dialog_left && pos.col < dialog_left + dialog_width {
        let btn_center = dialog_left + dialog_width / 2;
        let new_sel = if pos.col < btn_center { 0 } else { 1 };
        if state.dialog_selection == new_sel {
            if new_sel == 0 {
                if state.pending_action.is_some() {
                    if let Some(conflicting) = check_overwrite_conflict(state) {
                        state.dialog_selection = 0;
                        state.mode = AppMode::Dialog(DialogKind::OverwriteConfirm { conflicting });
                        return Some(MouseOutcome::Consumed);
                    }
                    let status_message = state.status_message.take();
                    start_confirmed_action(state, running_job);
                    if state.status_message.is_none() {
                        state.status_message = status_message;
                    }
                    finish_confirmed_action(state);
                    return Some(MouseOutcome::Consumed);
                }
                if let Some(cmd) = state.pending_menu_command.take() {
                    state.mode = AppMode::Normal;
                    shell::run_shell_command(state, &cmd, true, refresh_active);
                    return Some(MouseOutcome::Consumed);
                }
                dismiss_dialog(state);
                refresh_both(state);
            } else {
                dismiss_dialog(state);
            }
        } else {
            state.dialog_selection = new_sel;
        }
    }
    Some(MouseOutcome::Consumed)
}

fn handle_overwrite_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let dialog_height = pos.height * 40 / 100;
    let btn_row = dialog_button_row(pos.height, dialog_height);
    let dialog_width = pos.width / 2;
    let dialog_left = dialog_left(pos.width, dialog_width);

    if pos.row == btn_row && pos.col >= dialog_left && pos.col < dialog_left + dialog_width {
        let btn_center = dialog_left + dialog_width / 2;
        let new_sel = if pos.col < btn_center { 0 } else { 1 };
        if state.dialog_selection == new_sel {
            match new_sel {
                0 => {
                    set_pending_overwrite(state);
                    start_confirmed_action(state, running_job);
                    finish_confirmed_action(state);
                }
                1 => {
                    dismiss_dialog(state);
                }
                _ => {}
            }
        } else {
            state.dialog_selection = new_sel;
        }
    }
    Some(MouseOutcome::Consumed)
}

fn set_pending_overwrite(state: &mut AppState) {
    if let Some(action) = state.pending_action.as_mut() {
        match action {
            crate::app::types::PendingAction::Copy { overwrite, .. }
            | crate::app::types::PendingAction::Move { overwrite, .. } => {
                *overwrite = true;
            }
            crate::app::types::PendingAction::Delete { .. } => {}
        }
    }
}

fn handle_progress_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let dialog_height = pos.height * 40 / 100;
    let dialog_y = (pos.height.saturating_sub(dialog_height)) / 2;
    let cancel_row = dialog_y + dialog_height.saturating_sub(2);
    let dialog_width = pos.width / 2;
    let dialog_left = dialog_left(pos.width, dialog_width);

    if pos.row == cancel_row
        && pos.col >= dialog_left
        && pos.col < dialog_left + dialog_width
        && let Some(job) = running_job.as_ref()
    {
        job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        state.status_message = Some("Cancel requested".to_string());
    }
    Some(MouseOutcome::Consumed)
}

fn handle_mouse_menu_bar(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    if pos.row != 0
        || !matches!(
            state.mode,
            AppMode::Normal | AppMode::Menu | AppMode::DirectoryTree | AppMode::Search
        )
    {
        return None;
    }
    for (i, title) in MENU_TITLES.iter().enumerate() {
        let x_offset = menu_title_x(pos.width, i);
        let title_width = menu_title_width(title);
        if pos.col >= x_offset && pos.col < x_offset + title_width {
            state.menu_selected = i;
            state.menu_item_selected = 0;
            if state.mode != AppMode::Menu {
                state.prev_mode = Some(state.mode.clone());
                state.mode = AppMode::Menu;
            }
            return Some(MouseOutcome::Consumed);
        }
    }
    Some(MouseOutcome::Consumed)
}

fn handle_mouse_menu_dropdown(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    if !matches!(state.mode, AppMode::Menu) || pos.row < 1 {
        return None;
    }
    let items = MENU_ITEMS[state.menu_selected];
    let dropdown_width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(*s))
        .max()
        .unwrap_or(10) as u16
        + 4;
    let menu_bar_area = Rect::new(0, 0, pos.width, 1);
    let dropdown_x = menu_dropdown_x(menu_bar_area, state.menu_selected, dropdown_width);

    let inner_x = dropdown_x + 1;
    let inner_y = 2u16;
    let inner_width = dropdown_width.saturating_sub(2);

    if pos.col >= inner_x && pos.col < inner_x + inner_width && pos.row >= inner_y {
        let item_idx = (pos.row - inner_y) as usize;
        if item_idx < items.len() {
            state.menu_item_selected = item_idx;
            return Some(MouseOutcome::MenuAction);
        }
    }
    state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
    Some(MouseOutcome::Consumed)
}

fn handle_mouse_function_bar(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    if pos.row != pos.height.saturating_sub(1)
        || !matches!(state.mode, AppMode::Normal | AppMode::Search)
    {
        return None;
    }
    if pos.width == 0 {
        return Some(MouseOutcome::Consumed);
    }
    let btn_idx = (pos.col * 10 / pos.width).min(9);
    let fkey = match btn_idx {
        0 => KeyCode::F(1),
        1 => KeyCode::F(2),
        2 => KeyCode::F(3),
        3 => KeyCode::F(4),
        4 => KeyCode::F(5),
        5 => KeyCode::F(6),
        6 => KeyCode::F(7),
        7 => KeyCode::F(8),
        8 => KeyCode::F(9),
        _ => KeyCode::F(10),
    };
    Some(MouseOutcome::NormalKey(fkey))
}

fn handle_mouse_panels(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    pos: &MousePosition,
) {
    use std::time::Duration;

    if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
        return;
    }

    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);

    if pos.row <= panel_start_row || pos.row >= panel_end_row {
        return;
    }

    let panel_height = panel_end_row.saturating_sub(panel_start_row) + 1;
    let mid_col = pos.width / 2;
    let clicked_left = pos.col < mid_col;

    if clicked_left {
        state.active_panel = ActivePanel::Left;
    } else {
        state.active_panel = ActivePanel::Right;
    }

    let panel = if clicked_left {
        &state.left_panel
    } else {
        &state.right_panel
    };

    let list_start_row = panel_start_row + 1;
    let relative_row = pos.row.saturating_sub(list_start_row);
    let clicked_index = panel.scroll_offset + relative_row as usize;

    if clicked_index >= panel.entries.len() {
        return;
    }

    let now = std::time::Instant::now();
    let is_double_click = if let Some(last_time) = state.last_click_time {
        if let Some(last_pos) = state.last_click_position {
            last_pos.0 == pos.col
                && last_pos.1 == pos.row
                && now.duration_since(last_time) < Duration::from_millis(DOUBLE_CLICK_THRESHOLD_MS)
        } else {
            false
        }
    } else {
        false
    };

    if is_double_click {
        state.last_click_time = None;
        state.last_click_position = None;
        state.drag_anchor_index = None;

        let entry = &panel.entries[clicked_index];
        let is_dir = entry.is_dir();
        let path = entry.path.clone();
        if is_dir {
            if matches!(state.mode, AppMode::Search) {
                let visible = crate::app::panel_ops::panel_visible_height(pos.height);
                super::mode_dispatch::clear_search_state(state, visible);
            }
            let panel_mut = state.active_panel_mut();
            panel_mut.history.push(panel_mut.path.clone());
            panel_mut.set_path(path);
            panel_mut.cursor = 0;
            panel_mut.scroll_offset = 0;
            refresh_panel(panel_mut, panel_height as usize);
        } else {
            *viewer_loader = Some(viewer::ViewerState::open_background(path));
            state.prev_mode = Some(state.mode.clone());
            state.mode = AppMode::Viewing;
        }
    } else {
        state.last_click_time = Some(now);
        state.last_click_position = Some((pos.col, pos.row));
        state.drag_anchor_index = Some(clicked_index);

        let panel_mut = state.active_panel_mut();
        panel_mut.cursor = clicked_index;
        panel_mut.ensure_cursor_visible(panel_height.saturating_sub(2) as usize);
    }
}

fn handle_mouse_drag(state: &mut AppState, pos: &MousePosition) {
    if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
        return;
    }

    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);
    if pos.row <= panel_start_row || pos.row >= panel_end_row {
        return;
    }

    let anchor = match state.drag_anchor_index {
        Some(idx) => idx,
        None => return,
    };

    let mid_col = pos.width / 2;
    let clicked_left = pos.col < mid_col;
    let same_panel = clicked_left == matches!(state.active_panel, ActivePanel::Left);
    if !same_panel {
        return;
    }

    let panel = if clicked_left {
        &state.left_panel
    } else {
        &state.right_panel
    };

    let list_start_row = panel_start_row + 1;
    let relative_row = pos.row.saturating_sub(list_start_row);
    let current_index = panel.scroll_offset + relative_row as usize;

    if current_index >= panel.entries.len() {
        return;
    }

    let panel_mut = state.active_panel_mut();
    let start = anchor.min(current_index);
    let end = anchor.max(current_index);
    for entry in panel_mut.entries.iter_mut() {
        entry.selected = false;
    }
    for entry in panel_mut
        .entries
        .iter_mut()
        .skip(start)
        .take(end - start + 1)
    {
        entry.selected = true;
    }
    panel_mut.cursor = current_index;
    let visible_rows = (panel_end_row - panel_start_row).saturating_sub(1) as usize;
    panel_mut.ensure_cursor_visible(visible_rows);
}

fn handle_mouse_up(state: &mut AppState) {
    state.drag_anchor_index = None;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::app::types::{ConfirmDetails, InputAction, PendingAction, TextInput};

    #[test]
    fn mouse_input_dialog_outside_preserves_text() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Input {
                prompt: "Name:".to_string(),
                default_text: "".to_string(),
                action: InputAction::CreateDirectory,
            }),
            dialog_input: TextInput {
                text: "draft".to_string(),
                cursor: 5,
            },
            ..Default::default()
        };

        let mut running_job = None;
        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 0,
                row: 0,
                width: 100,
                height: 40,
            },
        );

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Input { .. })
        ));
        assert_eq!(state.dialog_input.text, "draft");
        assert_eq!(state.dialog_input.cursor, 5);
    }

    #[test]
    fn mouse_input_dialog_inside_consumes_click() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Input {
                prompt: "Name:".to_string(),
                default_text: "".to_string(),
                action: InputAction::CreateDirectory,
            }),
            dialog_input: TextInput {
                text: "draft".to_string(),
                cursor: 0,
            },
            ..Default::default()
        };

        let mut running_job = None;
        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 50,
                row: 20,
                width: 100,
                height: 40,
            },
        );

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Input { .. })
        ));
        assert_eq!(state.dialog_input.text, "draft");
    }

    #[test]
    fn mouse_function_bar_zero_width_does_not_panic() {
        let mut state = AppState::default();

        let outcomes = handle_mouse_function_bar(
            &mut state,
            &MousePosition {
                col: 0,
                row: 0,
                width: 0,
                height: 1,
            },
        );

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    }

    #[test]
    fn mouse_error_dialog_click_does_not_dismiss() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Error("error".to_string())),
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 1,
                row: 1,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(state.mode, AppMode::Dialog(DialogKind::Error(_))));
    }

    #[test]
    fn mouse_properties_dialog_click_does_not_dismiss() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Properties {
                name: "file.txt".to_string(),
                size: 0,
                mtime: std::time::SystemTime::UNIX_EPOCH,
                permissions: 0o644,
                owner: String::new(),
                group: String::new(),
                is_dir: false,
                is_symlink: false,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 1,
                row: 1,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Properties { .. })
        ));
    }

    #[test]
    fn mouse_help_dialog_click_does_not_dismiss() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Help {
                message: "help".to_string(),
                scroll_offset: 0,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 1,
                row: 1,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Help { .. })
        ));
    }

    #[test]
    fn mouse_confirm_dialog_keeps_existing_behavior() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                "Confirm", "Run?",
            ))),
            dialog_selection: 1,
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 79,
                row: 23,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Confirm(_))
        ));
    }

    #[test]
    fn mouse_overwrite_confirm_dialog_handled() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::OverwriteConfirm {
                conflicting: vec![],
            }),
            dialog_selection: 0,
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 1,
                row: 1,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::OverwriteConfirm { .. })
        ));
    }

    #[test]
    fn mouse_progress_click_is_consumed() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Progress("Copying".to_string(), 0.5, true)),
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 40,
                row: 21,
                width: 80,
                height: 24,
            },
        );

        assert!(outcomes.is_some());
        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    }

    #[test]
    fn mouse_scroll_handles_help_dialog() {
        let long_text = (0..200)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Help {
                message: long_text,
                scroll_offset: 0,
            }),
            ..Default::default()
        };

        handle_mouse_scroll(
            &mut state,
            &mut None,
            MouseEventKind::ScrollDown,
            &MousePosition {
                col: 0,
                row: 0,
                width: 80,
                height: 40,
            },
        );

        assert!(
            matches!(&state.mode, AppMode::Dialog(DialogKind::Help { scroll_offset, .. }) if *scroll_offset > 0),
            "expected Help dialog with scroll_offset > 0"
        );
    }

    #[test]
    fn mouse_up_clears_drag_anchor() {
        let mut state = AppState {
            drag_anchor_index: Some(5),
            ..Default::default()
        };

        handle_mouse_up(&mut state);

        assert!(state.drag_anchor_index.is_none());
    }

    #[test]
    fn drag_select_range() {
        use crate::app::types::FileEntry;
        let mk = |name: &str| FileEntry {
            name: name.to_string(),
            path: std::path::PathBuf::from(format!("/{}", name)),
            cha: crate::fs::cha::Cha {
                kind: crate::fs::cha::ChaKind::empty(),
                mode: crate::fs::cha::ChaMode::new(0o100644),
                len: 0,
                mtime: None,
                btime: None,
                ctime: None,
                atime: None,
                uid: 0,
                gid: 0,
                dev: 0,
                nlink: 0,
            },
            owner: String::new(),
            group: String::new(),
            selected: false,
            mime_type: None,
            time_str: String::new(),
            size_str: String::new(),
            name_width: unicode_width::UnicodeWidthStr::width(name),
            size_width: 0,
            time_width: 0,
        };
        let entries = vec![mk("a"), mk("b"), mk("c"), mk("d"), mk("e")];
        let mut left_panel = crate::app::types::PanelState::new(std::path::PathBuf::from("/"));
        left_panel.entries = entries.clone();
        let mut right_panel = crate::app::types::PanelState::new(std::path::PathBuf::from("/"));
        right_panel.entries = entries;
        let mut state = AppState {
            left_panel,
            right_panel,
            drag_anchor_index: Some(0),
            ..Default::default()
        };

        handle_mouse_drag(
            &mut state,
            &MousePosition {
                col: 1,
                row: 5,
                width: 80,
                height: 24,
            },
        );

        let selected: Vec<_> = state
            .left_panel
            .entries
            .iter()
            .filter(|e| e.selected)
            .collect();
        assert_eq!(selected.len(), 4);
    }

    #[test]
    fn handle_right_click_in_dialog_emits_esc() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple("Title", "Body"))),
            ..Default::default()
        };

        let outcome = handle_right_down(
            &mut state,
            &MousePosition {
                col: 40,
                row: 10,
                width: 80,
                height: 24,
            },
        );
        assert!(matches!(
            outcome,
            Some(MouseOutcome::NormalKey(KeyCode::Esc))
        ));
    }

    #[test]
    fn handle_right_click_in_menu_emits_esc() {
        let mut state = AppState {
            mode: AppMode::Menu,
            ..Default::default()
        };

        let outcome = handle_right_down(
            &mut state,
            &MousePosition {
                col: 40,
                row: 10,
                width: 80,
                height: 24,
            },
        );
        assert!(matches!(
            outcome,
            Some(MouseOutcome::NormalKey(KeyCode::Esc))
        ));
    }

    #[test]
    fn mouse_menu_dropdown_outside_restores_previous_mode() {
        let mut state = AppState {
            mode: AppMode::Menu,
            prev_mode: Some(AppMode::Search),
            ..Default::default()
        };

        let outcome = handle_mouse_menu_dropdown(
            &mut state,
            &MousePosition {
                col: 79,
                row: 23,
                width: 80,
                height: 24,
            },
        );

        assert!(matches!(outcome, Some(MouseOutcome::Consumed)));
        assert!(matches!(state.mode, AppMode::Search));
        assert!(state.prev_mode.is_none());
    }

    #[test]
    fn handle_right_click_in_panel_emits_esc() {
        let mut state = AppState::default();

        let outcome = handle_right_down(
            &mut state,
            &MousePosition {
                col: 10,
                row: 10,
                width: 80,
                height: 24,
            },
        );
        assert!(matches!(
            outcome,
            Some(MouseOutcome::NormalKey(KeyCode::Esc))
        ));
    }

    #[test]
    fn handle_middle_click_in_panel_emits_f5() {
        let mut state = AppState::default();

        let outcome = handle_middle_down(
            &mut state,
            &MousePosition {
                col: 10,
                row: 10,
                width: 80,
                height: 24,
            },
        );
        assert!(matches!(
            outcome,
            Some(MouseOutcome::NormalKey(KeyCode::F(5)))
        ));
    }

    #[test]
    fn handle_middle_click_in_dialog_consumed() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Error("err".to_string())),
            ..Default::default()
        };

        let outcome = handle_middle_down(
            &mut state,
            &MousePosition {
                col: 40,
                row: 10,
                width: 80,
                height: 24,
            },
        );
        assert!(matches!(outcome, Some(MouseOutcome::Consumed)));
    }

    #[test]
    fn mouse_confirm_click_with_overwrite_conflict_shows_overwrite_dialog() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(src_dir.join("clash.txt"), b"src").unwrap();
        std::fs::write(dest_dir.join("clash.txt"), b"dest").unwrap();

        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                "Copy", "Proceed?",
            ))),
            dialog_selection: 0,
            pending_action: Some(PendingAction::Copy {
                sources: vec![src_dir.join("clash.txt")],
                dest: dest_dir,
                overwrite: false,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let height: u16 = 24;
        let width: u16 = 80;
        let dialog_height = height * 40 / 100;
        let btn_row = {
            let dialog_y = (height.saturating_sub(dialog_height)) / 2;
            dialog_y + dialog_height.saturating_sub(2)
        };

        let _outcome = handle_confirm_click(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 30,
                row: btn_row,
                width,
                height,
            },
        );

        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::OverwriteConfirm { .. })
        ));
        assert!(state.pending_action.is_some());
    }

    #[test]
    fn mouse_confirm_click_without_conflict_starts_action() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(src_dir.join("unique.txt"), b"data").unwrap();

        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                "Copy", "Proceed?",
            ))),
            dialog_selection: 0,
            pending_action: Some(PendingAction::Copy {
                sources: vec![src_dir.join("unique.txt")],
                dest: dest_dir,
                overwrite: false,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let height: u16 = 24;
        let width: u16 = 80;
        let dialog_height = height * 40 / 100;
        let btn_row = {
            let dialog_y = (height.saturating_sub(dialog_height)) / 2;
            dialog_y + dialog_height.saturating_sub(2)
        };

        let _outcome = handle_confirm_click(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 30,
                row: btn_row,
                width,
                height,
            },
        );

        assert!(!matches!(
            state.mode,
            AppMode::Dialog(DialogKind::OverwriteConfirm { .. })
        ));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Progress(_, _, _))
        ));
    }

    #[test]
    fn mouse_confirm_click_preserves_status_message() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(src_dir.join("unique.txt"), b"data").unwrap();

        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                "Copy", "Proceed?",
            ))),
            dialog_selection: 0,
            status_message: Some("Queued".to_string()),
            pending_action: Some(PendingAction::Copy {
                sources: vec![src_dir.join("unique.txt")],
                dest: dest_dir,
                overwrite: false,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let height: u16 = 24;
        let width: u16 = 80;
        let dialog_height = height * 40 / 100;
        let btn_row = {
            let dialog_y = (height.saturating_sub(dialog_height)) / 2;
            dialog_y + dialog_height.saturating_sub(2)
        };

        let _outcome = handle_confirm_click(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 30,
                row: btn_row,
                width,
                height,
            },
        );

        assert_eq!(state.status_message.as_deref(), Some("Queued"));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Progress(_, _, _))
        ));
    }

    #[test]
    fn mouse_confirm_click_keeps_new_status_message() {
        let tmp = tempfile::tempdir().unwrap();
        let first_src_dir = tmp.path().join("first-src");
        let second_src_dir = tmp.path().join("second-src");
        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&first_src_dir).unwrap();
        std::fs::create_dir_all(&second_src_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(first_src_dir.join("first.txt"), b"data").unwrap();
        std::fs::write(second_src_dir.join("second.txt"), b"data").unwrap();

        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
                "Copy", "Proceed?",
            ))),
            dialog_selection: 0,
            pending_action: Some(PendingAction::Copy {
                sources: vec![first_src_dir.join("first.txt")],
                dest: dest_dir.clone(),
                overwrite: false,
            }),
            ..Default::default()
        };
        let mut running_job = None;

        let height: u16 = 24;
        let width: u16 = 80;
        let dialog_height = height * 40 / 100;
        let btn_row = {
            let dialog_y = (height.saturating_sub(dialog_height)) / 2;
            dialog_y + dialog_height.saturating_sub(2)
        };

        let _outcome = handle_confirm_click(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 30,
                row: btn_row,
                width,
                height,
            },
        );

        state.mode = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple(
            "Copy", "Proceed?",
        )));
        state.status_message = Some("Queued".to_string());
        state.pending_action = Some(PendingAction::Copy {
            sources: vec![second_src_dir.join("second.txt")],
            dest: dest_dir,
            overwrite: false,
        });

        let _outcome = handle_confirm_click(
            &mut state,
            &mut running_job,
            &MousePosition {
                col: 30,
                row: btn_row,
                width,
                height,
            },
        );

        assert_eq!(
            state.status_message.as_deref(),
            Some("Another job is already running")
        );
    }
}
