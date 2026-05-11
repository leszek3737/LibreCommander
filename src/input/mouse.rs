use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::job_runner::{RunningJob, start_confirmed_action};
use crate::app::types::{ActivePanel, AppMode, AppState, DialogKind};
use crate::menu::{MENU_ITEMS, MENU_TITLES, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::viewer;

use super::super::{dismiss_dialog, refresh_both, refresh_panel};

const SCROLL_LINES: usize = 3;
const DOUBLE_CLICK_THRESHOLD_MS: u64 = 300;

pub enum MouseOutcome {
    Consumed,
    NormalKey(KeyCode),
    MenuAction,
}

pub fn handle_mouse_event(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    mouse_event: crossterm::event::MouseEvent,
    terminal_size: ratatui::layout::Size,
) -> Option<MouseOutcome> {
    let col = mouse_event.column;
    let row = mouse_event.row;
    let height = terminal_size.height;
    let width = terminal_size.width;

    if matches!(
        mouse_event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        handle_mouse_scroll(state, mouse_event.kind, col, row, width, height);
        return None;
    }

    let MouseEventKind::Down(button) = mouse_event.kind else {
        return None;
    };
    if button != MouseButton::Left {
        return None;
    }

    if let Some(outcome) = handle_mouse_dialog(state, running_job, col, row, width, height) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_menu_bar(state, col, row, width) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_menu_dropdown(state, col, row, width) {
        return Some(outcome);
    }

    if let Some(outcome) = handle_mouse_function_bar(state, col, row, width, height) {
        return Some(outcome);
    }

    handle_mouse_panels(state, viewer_state, col, row, width, height);
    None
}

fn handle_mouse_scroll(
    state: &mut AppState,
    kind: crossterm::event::MouseEventKind,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) {
    use crossterm::event::MouseEventKind;

    if !matches!(state.mode, AppMode::Normal) {
        return;
    }
    let panel_start_row = 1u16;
    let panel_end_row = height.saturating_sub(4);
    if row < panel_start_row || row > panel_end_row {
        return;
    }
    let panel_height = panel_end_row.saturating_sub(panel_start_row) + 1;
    let visible_rows = panel_height.saturating_sub(2) as usize;
    let mid_col = width / 2;
    if col < mid_col {
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

fn handle_mouse_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) -> Option<MouseOutcome> {
    if matches!(state.mode, AppMode::Dialog(DialogKind::Progress(_, _))) {
        return Some(MouseOutcome::Consumed);
    }

    if let AppMode::Dialog(DialogKind::Input { .. }) = state.mode {
        return Some(MouseOutcome::Consumed);
    }

    if let AppMode::Dialog(DialogKind::Confirm(_)) = state.mode {
        let dialog_height = height * 40 / 100;
        let dialog_y = (height.saturating_sub(dialog_height)) / 2;
        let btn_row = dialog_y + dialog_height.saturating_sub(2);
        let dialog_width = width / 2;
        let dialog_left = (width.saturating_sub(dialog_width)) / 2;

        if row == btn_row && col >= dialog_left && col < dialog_left + dialog_width {
            let btn_center = dialog_left + dialog_width / 2;
            let new_sel = if col < btn_center { 0 } else { 1 };
            if state.dialog_selection == new_sel {
                if new_sel == 0 {
                    if state.pending_action.is_some() {
                        start_confirmed_action(state, running_job);
                        state.dialog_selection = 0;
                        if state.status_message.is_some() {
                            dismiss_dialog(state);
                            refresh_both(state);
                            return Some(MouseOutcome::Consumed);
                        }
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
        return Some(MouseOutcome::Consumed);
    }

    if let AppMode::Dialog(_) = state.mode {
        return Some(MouseOutcome::Consumed);
    }

    None
}

fn handle_mouse_menu_bar(
    state: &mut AppState,
    col: u16,
    row: u16,
    width: u16,
) -> Option<MouseOutcome> {
    if row != 0 || !matches!(state.mode, AppMode::Normal | AppMode::Menu) {
        return None;
    }
    for (i, title) in MENU_TITLES.iter().enumerate() {
        let x_offset = menu_title_x(width, i);
        let title_width = menu_title_width(title);
        if col >= x_offset && col < x_offset + title_width {
            state.menu_selected = i;
            state.menu_item_selected = 0;
            state.mode = AppMode::Menu;
            return Some(MouseOutcome::Consumed);
        }
    }
    // Consume click on menu bar even outside title bounds — prevents click-through to panels.
    Some(MouseOutcome::Consumed)
}

fn handle_mouse_menu_dropdown(
    state: &mut AppState,
    col: u16,
    row: u16,
    width: u16,
) -> Option<MouseOutcome> {
    if !matches!(state.mode, AppMode::Menu) || row < 1 {
        return None;
    }
    let items = MENU_ITEMS[state.menu_selected];
    let dropdown_width = items.iter().map(|s| s.len()).max().unwrap_or(10) as u16 + 4;
    let menu_bar_area = Rect::new(0, 0, width, 1);
    let dropdown_x = menu_dropdown_x(menu_bar_area, state.menu_selected, dropdown_width);

    let inner_x = dropdown_x + 1;
    let inner_y = 2u16;
    let inner_width = dropdown_width.saturating_sub(2);

    if col >= inner_x && col < inner_x + inner_width && row >= inner_y {
        let item_idx = (row - inner_y) as usize;
        if item_idx < items.len() {
            state.menu_item_selected = item_idx;
            return Some(MouseOutcome::MenuAction);
        }
    }
    Some(MouseOutcome::Consumed)
}

fn handle_mouse_function_bar(
    state: &mut AppState,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) -> Option<MouseOutcome> {
    if row != height.saturating_sub(1) || !matches!(state.mode, AppMode::Normal) {
        return None;
    }
    if width == 0 {
        return Some(MouseOutcome::Consumed);
    }
    let btn_idx = (col * 10 / width).min(9);
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
    viewer_state: &mut Option<viewer::ViewerState>,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) {
    use std::time::Duration;

    if !matches!(state.mode, AppMode::Normal) {
        return;
    }

    let panel_start_row = 1u16;
    let panel_end_row = height.saturating_sub(4);

    if row <= panel_start_row || row >= panel_end_row {
        return;
    }

    let panel_height = panel_end_row.saturating_sub(panel_start_row) + 1;
    let mid_col = width / 2;
    let clicked_left = col < mid_col;

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
    let relative_row = row.saturating_sub(list_start_row);
    let clicked_index = panel.scroll_offset + relative_row as usize;

    if clicked_index >= panel.entries.len() {
        return;
    }

    let now = std::time::Instant::now();
    let is_double_click = if let Some(last_time) = state.last_click_time {
        if let Some(last_pos) = state.last_click_position {
            last_pos.0 == col
                && last_pos.1 == row
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

        let entry = &panel.entries[clicked_index];
        let is_dir = entry.is_dir();
        let path = entry.path.clone();
        if is_dir {
            let panel_mut = state.active_panel_mut();
            panel_mut.history.push(panel_mut.path.clone());
            panel_mut.path = path;
            panel_mut.cursor = 0;
            panel_mut.scroll_offset = 0;
            refresh_panel(panel_mut, panel_height as usize);
        } else {
            if let Ok(vs) = viewer::ViewerState::open(&path) {
                *viewer_state = Some(vs);
                state.prev_mode = Some(state.mode.clone());
                state.mode = AppMode::Viewing;
            }
        }
    } else {
        state.last_click_time = Some(now);
        state.last_click_position = Some((col, row));

        let panel_mut = state.active_panel_mut();
        panel_mut.cursor = clicked_index;
        panel_mut.ensure_cursor_visible(panel_height.saturating_sub(2) as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{AppState, ConfirmDetails, DialogKind, InputAction};

    #[test]
    fn mouse_input_dialog_outside_preserves_text() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Input {
                prompt: "Name:".to_string(),
                default_text: "".to_string(),
                action: InputAction::CreateDirectory,
            }),
            dialog_input: "draft".to_string(),
            dialog_cursor_pos: 5,
            ..Default::default()
        };

        let mut running_job = None;
        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 0, 0, 100, 40);

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Input { .. })
        ));
        assert_eq!(state.dialog_input, "draft");
        assert_eq!(state.dialog_cursor_pos, 5);
    }

    #[test]
    fn mouse_input_dialog_inside_consumes_click() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Input {
                prompt: "Name:".to_string(),
                default_text: "".to_string(),
                action: InputAction::CreateDirectory,
            }),
            dialog_input: "draft".to_string(),
            ..Default::default()
        };

        let mut running_job = None;
        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 50, 20, 100, 40);

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Input { .. })
        ));
        assert_eq!(state.dialog_input, "draft");
    }

    #[test]
    fn mouse_function_bar_zero_width_does_not_panic() {
        let mut state = AppState::default();

        let outcomes = handle_mouse_function_bar(&mut state, 0, 0, 0, 1);

        assert!(matches!(outcomes, Some(MouseOutcome::Consumed)));
    }

    #[test]
    fn mouse_error_dialog_click_does_not_dismiss() {
        let mut state = AppState {
            mode: AppMode::Dialog(DialogKind::Error("error".to_string())),
            ..Default::default()
        };
        let mut running_job = None;

        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 1, 1, 80, 24);

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

        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 1, 1, 80, 24);

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

        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 1, 1, 80, 24);

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

        let outcomes = handle_mouse_dialog(&mut state, &mut running_job, 79, 23, 80, 24);

        assert!(outcomes.is_some());
        assert!(matches!(
            state.mode,
            AppMode::Dialog(DialogKind::Confirm(_))
        ));
    }
}
