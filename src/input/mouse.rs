use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::job_runner::{RunningJob, start_confirmed_action};
use crate::app::shell;
use crate::app::types::{
    ActivePanel, AppMode, AppState, DialogKind, OverwriteConfirmDetails, TextInput,
};
use crate::menu::{MENUS, menu_dropdown_x, menu_title_width, menu_title_x};
use crate::ui::dialogs;
use crate::ui::viewer;

use super::EventContext;
use super::dialogs::{check_overwrite_conflict, dismiss_dialog, finish_confirmed_action};
use crate::app::panel_ops::{refresh_active, refresh_both, refresh_panel};

const SCROLL_LINES: usize = 3;
const DOUBLE_CLICK_THRESHOLD_MS: u64 = 300;
const ARCHIVE_EXTRACT_INPUT_ROW_OFFSET: u16 = 3;
const ARCHIVE_CREATE_INPUT_ROW_OFFSET: u16 = 4;

/// Fallback dropdown width (in cells) when a menu has no items to measure.
const DEFAULT_DROPDOWN_WIDTH: usize = 10;
/// Chrome rows reserved by a panel's border + header before the file rows
/// (top border, header row, bottom border, function bar). Used by
/// [`panel_bounds`] to derive the file-row range from the terminal height.
const PANEL_CHROME_ROWS: u16 = 4;
/// Smallest terminal height for which `height - PANEL_CHROME_ROWS` stays `>= 1`,
/// i.e. the threshold below which [`panel_bounds`] would underflow into a
/// malformed `(1, 0)` range. It is NOT the height needed to *display* a file
/// row: the body has `height - LAYOUT_OVERHEAD_ROWS` (= 6) rows, so the first
/// real file row only appears at `height >= 7`. For heights 5–6 the bounds are
/// well-formed but the body is empty, which correctly matches the renderer
/// (`panel_visible_height`), so clicks/scrolls in the body are no-ops.
const MIN_PANEL_HEIGHT: u16 = PANEL_CHROME_ROWS + 1;
/// The function bar is split into 10 equal-width F-key buttons (F1..=F10).
const FUNCTION_BAR_BUTTONS: u32 = 10;
/// Highest selectable function-bar button index (`FUNCTION_BAR_BUTTONS - 1`).
const FUNCTION_BAR_MAX_INDEX: u32 = FUNCTION_BAR_BUTTONS - 1;

#[derive(Debug)]
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

pub(crate) fn handle_mouse_event(
    ctx: &mut EventContext,
    mouse_event: crossterm::event::MouseEvent,
) -> Option<MouseOutcome> {
    let terminal_size = ctx.term_size;
    let state = &mut *ctx.state;
    let viewer_state = &mut *ctx.viewer_state;
    let viewer_loader = &mut *ctx.viewer_loader;
    let running_job = &mut *ctx.running_job;
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
        MouseButton::Left => handle_left_down(state, viewer_loader, running_job, &pos),
        MouseButton::Middle => handle_middle_down(state, &pos),
        MouseButton::Right => handle_right_down(state, &pos),
    }
}

fn handle_left_down(
    state: &mut AppState,
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

    handle_mouse_panels(state, viewer_loader, pos);
    None
}

fn handle_middle_down(state: &mut AppState, pos: &MousePosition) -> Option<MouseOutcome> {
    if in_panel_file_rows(pos) && !matches!(state.mode, AppMode::Dialog(_)) {
        if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
            return Some(MouseOutcome::Consumed);
        }
        if pos.col < mid_col(pos.width) {
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
    if in_panel_file_rows(pos) {
        return Some(MouseOutcome::NormalKey(KeyCode::Esc));
    }
    Some(MouseOutcome::Consumed)
}

/// Column splitting the two panels. The left panel owns `col < mid_col`.
fn mid_col(width: u16) -> u16 {
    width / 2
}

/// File-row range (`start`, `end`) for a terminal of `height`. The interior
/// file rows are those strictly between `start` and `end` (see
/// [`in_panel_file_rows`]).
///
/// On a terminal too short to host any file row (`height < MIN_PANEL_HEIGHT`)
/// this returns an empty `(1, 1)` range so callers treat the panel body as
/// having no rows, rather than relying on `saturating_sub` underflowing to 0
/// and producing a malformed `(1, 0)` range.
fn panel_bounds(height: u16) -> (u16, u16) {
    if height < MIN_PANEL_HEIGHT {
        return (1, 1);
    }
    (1u16, height - PANEL_CHROME_ROWS)
}

/// Inclusive panel row span (`end - start + 1`) for the file-list area.
fn panel_height(start: u16, end: u16) -> u16 {
    end.saturating_sub(start) + 1
}

/// Number of visible file rows in the list area: the panel height minus the
/// two rows of chrome (header + bottom border) that frame the scrollable body.
/// Centralized here so the scroll and click paths cannot drift apart.
fn visible_rows(start: u16, end: u16) -> usize {
    panel_height(start, end).saturating_sub(2) as usize
}

fn in_panel_file_rows(pos: &MousePosition) -> bool {
    let (start, end) = panel_bounds(pos.height);
    pos.row > start && pos.row < end
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
            let geo = crate::ui::dialogs::help_dialog_geometry(term_rect);
            let visible = geo.height;
            let msg_width = geo.width;
            let total_lines = crate::ui::dialogs::wrapped_line_count(message, msg_width);
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
    if !in_panel_file_rows(pos) {
        return;
    }
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);
    let visible = visible_rows(panel_start_row, panel_end_row);
    if pos.col < mid_col(pos.width) {
        state.active_panel = ActivePanel::Left;
    } else {
        state.active_panel = ActivePanel::Right;
    }
    let panel = state.active_panel_mut();
    let len = panel.listing.filtered_len();
    match kind {
        MouseEventKind::ScrollUp => {
            panel.cursor = panel.cursor.saturating_sub(SCROLL_LINES);
            panel.ensure_cursor_visible(visible);
        }
        MouseEventKind::ScrollDown => {
            if panel.cursor + SCROLL_LINES < len {
                panel.cursor += SCROLL_LINES;
            } else {
                panel.cursor = len.saturating_sub(1);
            }
            panel.ensure_cursor_visible(visible);
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
        (current + delta.unsigned_abs()).min(max_scroll)
    }
}

fn handle_mouse_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    // Dispatch on the active dialog kind. The two archive variants mutate their
    // boxed `details` in place, so they are handled inside this borrow of
    // `state.mode`; the click-handler variants (Progress/Confirm/Overwrite) need
    // a fresh `&mut AppState`, so they delegate *after* the match releases the
    // borrow. Any other dialog swallows the click without acting on it.
    let AppMode::Dialog(kind) = &mut state.mode else {
        return None;
    };

    enum Delegate {
        Progress,
        Confirm,
        Overwrite,
    }

    let delegate = match kind {
        DialogKind::ArchiveExtract(details) => {
            position_text_input_cursor(
                &mut details.dest_input,
                pos,
                archive_input_rect(pos, ARCHIVE_EXTRACT_INPUT_ROW_OFFSET),
            );
            return Some(MouseOutcome::Consumed);
        }
        DialogKind::ArchiveCreate(details) => {
            position_text_input_cursor(
                &mut details.dest_input,
                pos,
                archive_input_rect(pos, ARCHIVE_CREATE_INPUT_ROW_OFFSET),
            );
            return Some(MouseOutcome::Consumed);
        }
        DialogKind::Progress { .. } => Delegate::Progress,
        DialogKind::Confirm(_) => Delegate::Confirm,
        DialogKind::OverwriteConfirm(..) => Delegate::Overwrite,
        // Input and every other dialog: consume the click, change nothing.
        _ => return Some(MouseOutcome::Consumed),
    };

    match delegate {
        Delegate::Progress => handle_progress_click(state, running_job, pos),
        Delegate::Confirm => handle_confirm_click(state, running_job, pos),
        Delegate::Overwrite => handle_overwrite_click(state, running_job, pos),
    }
}

fn archive_input_rect(pos: &MousePosition, row_offset: u16) -> Rect {
    let area = Rect::new(0, 0, pos.width, pos.height);
    let dialog = dialogs::centered_rect(50, 40, area);
    Rect::new(
        dialog.x.saturating_add(2),
        dialog.y.saturating_add(row_offset),
        dialog.width.saturating_sub(4),
        1,
    )
}

fn position_text_input_cursor(input: &mut TextInput, pos: &MousePosition, rect: Rect) {
    if !hit_rect(rect, pos) || rect.width == 0 {
        return;
    }

    let visible_width = usize::from(rect.width);
    input.set_visible_width(visible_width);
    let scroll_display = input.scroll_offset();
    let click_display = usize::from(pos.col.saturating_sub(rect.x));
    let target_display = scroll_display.saturating_add(click_display);

    input.set_cursor(text_cursor_for_display(input, target_display));
}

fn text_cursor_for_display(input: &TextInput, target_display: usize) -> usize {
    let mut display = 0usize;
    for (index, grapheme) in input.text().graphemes(true).enumerate() {
        if display >= target_display {
            return index;
        }
        let width = UnicodeWidthStr::width(grapheme);
        if display.saturating_add(width) > target_display {
            return index;
        }
        display = display.saturating_add(width);
    }
    input.grapheme_count()
}

fn hit_rect(rect: Rect, pos: &MousePosition) -> bool {
    pos.row >= rect.y
        && pos.row < rect.y.saturating_add(rect.height)
        && pos.col >= rect.x
        && pos.col < rect.x.saturating_add(rect.width)
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

impl DialogGeometry {
    fn hit_button_row(&self, pos: &MousePosition) -> bool {
        pos.row == self.btn_row
            && pos.col >= self.dialog_left
            && pos.col < self.dialog_left + self.dialog_width
    }
}

fn handle_confirm_click(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    pos: &MousePosition,
) -> Option<MouseOutcome> {
    let geo = dialog_geometry(pos);

    if geo.hit_button_row(pos) {
        let new_sel = if pos.col < geo.btn_center { 0 } else { 1 };
        if state.input.dialog_selection == new_sel {
            if new_sel == 0 {
                if state.ui.pending_action.is_some() {
                    if let Some(conflicting) = check_overwrite_conflict(state) {
                        state.input.dialog_selection = 0;
                        state.mode = AppMode::Dialog(DialogKind::OverwriteConfirm(Box::new(
                            OverwriteConfirmDetails { conflicting },
                        )));
                        return Some(MouseOutcome::Consumed);
                    }
                    let status_message = state.ui.status_message.take();
                    start_confirmed_action(state, running_job);
                    if state.ui.status_message.is_none() {
                        state.ui.status_message = status_message;
                    }
                    finish_confirmed_action(state);
                    return Some(MouseOutcome::Consumed);
                }
                if let Some(cmd) = state.ui.pending_menu_command.take() {
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
            state.input.dialog_selection = new_sel;
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

    if geo.hit_button_row(pos) {
        let new_sel = if pos.col < geo.btn_center { 0 } else { 1 };
        if state.input.dialog_selection == new_sel {
            match new_sel {
                0 => {
                    if let Some(a) = &mut state.ui.pending_action {
                        a.set_overwrite();
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
            state.input.dialog_selection = new_sel;
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

    if geo.hit_button_row(pos)
        && let Some(job) = running_job.as_ref()
    {
        job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        state.ui.status_message = Some("Cancel requested".to_string());
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
    for (i, entry) in MENUS.iter().enumerate() {
        let title = entry.title;
        let x_offset = menu_title_x(pos.width, i);
        let title_width = menu_title_width(title);
        if pos.col >= x_offset && pos.col < x_offset + title_width {
            state.ui.menu_selected = i;
            state.ui.menu_item_selected = 0;
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
    let items = MENUS[state.ui.menu_selected].items;
    let dropdown_width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(*s))
        .max()
        .unwrap_or(DEFAULT_DROPDOWN_WIDTH) as u16
        + 4;
    let menu_bar_area = Rect::new(0, 0, pos.width, 1);
    let dropdown_x = menu_dropdown_x(menu_bar_area, state.ui.menu_selected, dropdown_width);

    let inner_x = dropdown_x + 1;
    let inner_y = 2u16;
    let inner_width = dropdown_width.saturating_sub(2);

    let max_visible = pos.height.saturating_sub(1);
    // Clamp the item count so `+ 2` (the dropdown's top/bottom border) cannot
    // overflow `u16` before the `.min(max_visible)` clamp.
    const MAX_DROPDOWN_ITEMS: usize = u16::MAX as usize - 2;
    let dropdown_height = ((items.len().min(MAX_DROPDOWN_ITEMS)) as u16 + 2).min(max_visible);
    let visible_items = dropdown_height.saturating_sub(2) as usize;
    let clamped_selected = state
        .ui
        .menu_item_selected
        .min(items.len().saturating_sub(1));
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
            state.ui.menu_item_selected = item_idx;
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
    let btn_idx = (u32::from(pos.col) * FUNCTION_BAR_BUTTONS / u32::from(pos.width))
        .min(FUNCTION_BAR_MAX_INDEX) as u16;
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
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    pos: &MousePosition,
) {
    use std::time::Duration;

    if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
        return;
    }

    if !in_panel_file_rows(pos) {
        return;
    }
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);

    let panel_rows = panel_height(panel_start_row, panel_end_row);
    let clicked_left = pos.col < mid_col(pos.width);

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

    if clicked_index >= panel.listing.filtered_len() {
        return;
    }

    let now = std::time::Instant::now();
    let is_double_click = if let Some((last_time, last_pos)) = state.interaction.last_click {
        last_pos.0 == pos.col
            && last_pos.1 == pos.row
            && now.duration_since(last_time) < Duration::from_millis(DOUBLE_CLICK_THRESHOLD_MS)
    } else {
        false
    };

    if is_double_click {
        state.interaction.last_click = None;
        state.interaction.drag_anchor_index = None;

        // Bail out gracefully if the entry vanished between the bounds check
        // and here (e.g. a concurrent refresh) instead of panicking.
        let Some(entry) = panel.listing.filtered_get(clicked_index) else {
            return;
        };
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
            // Contract: `refresh_panel` takes the *full* inclusive panel height
            // (header + body + bottom border) because it recomputes the whole
            // listing layout, whereas `ensure_cursor_visible` below takes only
            // the count of scrollable body rows (`visible_rows`). The two are
            // intentionally different units; keep them in sync via the helpers.
            refresh_panel(panel_mut, panel_rows as usize);
        } else {
            *viewer_loader = Some(viewer::ViewerState::open_background(path));
            state.prev_mode = Some(state.mode.clone());
            state.mode = AppMode::Viewing;
        }
    } else {
        state.interaction.last_click = Some((now, (pos.col, pos.row)));
        state.interaction.drag_anchor_index = Some(clicked_index);

        let panel_mut = state.active_panel_mut();
        panel_mut.cursor = clicked_index;
        panel_mut.ensure_cursor_visible(visible_rows(panel_start_row, panel_end_row));
    }
}

fn handle_mouse_drag(state: &mut AppState, pos: &MousePosition) {
    if !matches!(state.mode, AppMode::Normal | AppMode::Search) {
        return;
    }

    if !in_panel_file_rows(pos) {
        return;
    }
    let (panel_start_row, panel_end_row) = panel_bounds(pos.height);

    let anchor = match state.interaction.drag_anchor_index {
        Some(idx) => idx,
        None => return,
    };

    let clicked_left = pos.col < mid_col(pos.width);
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

    if current_index >= panel.listing.filtered_len() {
        return;
    }

    let panel_mut = state.active_panel_mut();
    let start = anchor.min(current_index);
    let end = anchor.max(current_index);
    set_selection_range(panel_mut, start..=end);
    panel_mut.cursor = current_index;
    panel_mut.ensure_cursor_visible(visible_rows(panel_start_row, panel_end_row));
}

/// Replace the panel's selection with exactly the filtered indices in `range`.
///
/// Clears any prior selection, then selects `range` in a single pass. Kept as a
/// dedicated helper so the drag path has one well-named entry point and the
/// clear-then-select sequence cannot be reordered by accident.
///
/// Note: it still routes each index through [`PanelState::set_selection_at`],
/// which maps the filtered index to the backing store per call. A true
/// single-scan batch would need a `PanelState`-side method with access to the
/// private filtered-index table; that lives outside this module.
fn set_selection_range(
    panel: &mut crate::app::types::PanelState,
    range: std::ops::RangeInclusive<usize>,
) {
    panel.clear_selection();
    for i in range {
        panel.set_selection_at(i, true);
    }
}

fn handle_mouse_up(state: &mut AppState) {
    // Do NOT clear `last_click` here. crossterm reports a physical double-click as
    // Down, Up, Down, Up (no native double-click event), so clearing it on the
    // first Up would erase the timestamp the second Down needs, making
    // double-click detection impossible. Stale entries are already invalidated by
    // the `DOUBLE_CLICK_THRESHOLD_MS` timestamp check and reset on a successful
    // double-click.
    state.interaction.drag_anchor_index = None;
}

#[cfg(test)]
mod tests;
