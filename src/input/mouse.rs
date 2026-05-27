use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::app::job_runner::{RunningJob, start_confirmed_action};
use crate::app::shell;
use crate::app::types::{ActivePanel, AppMode, AppState, DialogKind};
use crate::menu::{MENU_ITEMS, MENU_TITLES, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::dialogs;
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
    viewer_state: &mut Option<viewer::ViewerState>,
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
        handle_mouse_scroll(state, viewer_state, mouse_event.kind, &pos);
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
            handle_left_down(state, viewer_state, viewer_loader, running_job, &pos)
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
    let len = panel.listing.entries.len();
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
    if let AppMode::Dialog(DialogKind::Progress { .. }) = state.mode {
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

struct DialogGeometry {
    btn_row: u16,
    dialog_left: u16,
    dialog_width: u16,
    btn_center: u16,
}

fn dialog_geometry(pos: &MousePosition) -> DialogGeometry {
    let area = Rect::new(0, 0, pos.width, pos.height);
    let r = dialogs::centered_rect(50, 40, area);
    DialogGeometry {
        btn_row: r.y + r.height.saturating_sub(2),
        dialog_left: r.x,
        dialog_width: r.width,
        btn_center: r.x + r.width / 2,
    }
}

fn hit_button_row(geo: &DialogGeometry, pos: &MousePosition) -> bool {
    pos.row == geo.btn_row
        && pos.col >= geo.dialog_left
        && pos.col < geo.dialog_left + geo.dialog_width
}

fn handle_confirm_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let geo = dialog_geometry(pos);

    if hit_button_row(&geo, pos) {
        let new_sel = if pos.col < geo.btn_center { 0 } else { 1 };
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
    let geo = dialog_geometry(pos);

    if hit_button_row(&geo, pos) {
        let new_sel = if pos.col < geo.btn_center { 0 } else { 1 };
        if state.dialog_selection == new_sel {
            match new_sel {
                0 => {
                    if let Some(action) = state.pending_action.as_mut() {
                        action.set_overwrite();
                    }
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

fn handle_progress_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let geo = dialog_geometry(pos);

    if hit_button_row(&geo, pos)
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

    let max_visible = pos.height.saturating_sub(1);
    let dropdown_height = ((items.len().min(u16::MAX as usize - 2)) as u16 + 2).min(max_visible);
    let visible_items = dropdown_height.saturating_sub(2) as usize;
    let clamped_selected = state.menu_item_selected.min(items.len().saturating_sub(1));
    let scroll_offset = if items.len() <= visible_items {
        0
    } else {
        clamped_selected.saturating_sub(visible_items.saturating_sub(1))
    };

    if pos.col >= inner_x
        && pos.col < inner_x + inner_width
        && pos.row >= inner_y
        && pos.row < inner_y + visible_items as u16
    {
        let item_idx = scroll_offset + (pos.row - inner_y) as usize;
        if item_idx < items.len() {
            state.menu_item_selected = item_idx;
            return Some(MouseOutcome::MenuAction);
        }
    }
    state.restore_prev_mode();
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
    let btn_idx = (u32::from(pos.col) * 10 / u32::from(pos.width)).min(9) as u16;
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

    if clicked_index >= panel.listing.entries.len() {
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

        let entry = &panel.listing.entries[clicked_index];
        let is_dir = entry.is_dir();
        let path = entry.path.clone();
        if is_dir {
            if matches!(state.mode, AppMode::Search) {
                let visible = crate::app::panel_ops::panel_visible_height(pos.height);
                super::mode_dispatch::clear_search_state(state, visible);
            }
            let panel_mut = state.active_panel_mut();
            panel_mut.push_history(panel_mut.path().to_path_buf());
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

    if current_index >= panel.listing.entries.len() {
        return;
    }

    let panel_mut = state.active_panel_mut();
    let start = anchor.min(current_index);
    let end = anchor.max(current_index);
    panel_mut.clear_selection();
    for i in start..=end {
        panel_mut.set_selection_at(i, true);
    }
    panel_mut.cursor = current_index;
    let visible_rows = (panel_end_row - panel_start_row).saturating_sub(1) as usize;
    panel_mut.ensure_cursor_visible(visible_rows);
}

fn handle_mouse_up(state: &mut AppState) {
    state.drag_anchor_index = None;
    state.last_click_time = None;
    state.last_click_position = None;
}

#[cfg(test)]
mod tests;
