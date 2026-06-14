use crossterm::event::{KeyCode, KeyModifiers};

use lc::app::panel_ops;
use lc::app::types::{AppMode, AppState, DialogKind, InputAction};
use lc::menu::{MENUS, menu_item_count};

use super::EventContext;
use crate::{
    handle_alt_keys, handle_ctrl_keys, handle_enter_key, handle_function_keys,
    handle_navigation_keys,
};

const VIEWER_CHROME_HEIGHT: u16 = 3;
const HORIZONTAL_SCROLL_STEP: usize = 4;

/// Keys that close the file viewer (both the loading and loaded states).
/// Single source of truth so the loader and viewer-state branches can't drift.
fn is_viewer_exit_key(key: KeyCode) -> bool {
    matches!(key, KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char('q'))
}

fn refresh_or_rebuild(state: &mut AppState, visible_height: usize) {
    let needs_refresh = {
        let panel = state.active_panel();
        panel.listing.needs_full_read() || panel.listing.unfiltered().is_empty()
    };
    if needs_refresh {
        panel_ops::refresh_active(state);
    } else {
        panel_ops::rebuild_visible_entries(state.active_panel_mut(), visible_height);
    }
}

pub(crate) fn clear_search_state(state: &mut AppState, visible_height: usize) {
    state.restore_prev_mode();
    state.input.search_query.clear();
    state.input.search_cursor = 0;
    state.active_panel_mut().set_filter(None);
    refresh_or_rebuild(state, visible_height);
}

/// Applies the current `search_query` as the active panel's filter and
/// refreshes/rebuilds the visible entries. Shared by `initiate_search` and
/// `handle_search_mode` so the clone-query → set-filter → refresh sequence
/// lives in one place.
fn apply_search_filter(state: &mut AppState, visible: usize) {
    let filter_query = state.input.search_query.clone();
    state.active_panel_mut().set_filter(Some(filter_query));
    refresh_or_rebuild(state, visible);
}

pub(crate) fn initiate_search(
    state: &mut AppState,
    prev_mode: AppMode,
    c: char,
    visible_height: usize,
) {
    state.prev_mode = Some(prev_mode);
    state.input.search_query.push(c);
    state.input.search_cursor = state.input.search_query.len();
    state.mode = AppMode::Search;
    apply_search_filter(state, visible_height);
}

pub(crate) fn handle_normal_mode<B: ratatui::backend::Backend>(
    ctx: &mut EventContext,
    key: KeyCode,
    modifiers: KeyModifiers,
    terminal: &mut ratatui::Terminal<B>,
) {
    let terminal_height = ctx.term_size.height;
    let visible = panel_ops::panel_visible_height(terminal_height);
    let state = &mut *ctx.state;
    match key {
        KeyCode::F(_) => {
            handle_function_keys(state, ctx.viewer_loader, key, terminal);
        }
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Char('k')
        | KeyCode::Char('j')
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::Insert => {
            handle_navigation_keys(state, key, modifiers, visible);
        }
        KeyCode::Enter if !modifiers.contains(KeyModifiers::ALT) => {
            handle_enter_key(state, ctx.viewer_loader, visible, terminal);
        }
        KeyCode::Char('u' | 's' | 'h' | 'r' | 'o') if modifiers.contains(KeyModifiers::CONTROL) => {
            handle_ctrl_keys(state, key, terminal_height);
        }
        KeyCode::Enter | KeyCode::Backspace | KeyCode::Char(_)
            if modifiers.contains(KeyModifiers::ALT) =>
        {
            handle_alt_keys(state, key, visible);
        }
        _ => {
            if let KeyCode::Char(c) = key
                && (modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT)
            {
                initiate_search(state, AppMode::Normal, c, visible);
            }
        }
    }
}

pub(crate) fn handle_viewer_mode(ctx: &mut EventContext, key: KeyCode) {
    let terminal_size = ctx.term_size;
    let state = &mut *ctx.state;
    if ctx.viewer_loader.is_some() {
        if is_viewer_exit_key(key) {
            ctx.viewer_loader.take();
            *ctx.image_preview_loader = None;
            state.restore_prev_mode();
            *ctx.viewer_state = None;
        }
        return;
    }
    if let Some(vs) = ctx.viewer_state.as_mut() {
        let page_height = terminal_size.height.saturating_sub(VIEWER_CHROME_HEIGHT) as usize;
        let content_width = terminal_size.width as usize;
        vs.update_wrap_layout(content_width);
        vs.clamp_scroll();
        match key {
            _ if is_viewer_exit_key(key) => {
                *ctx.image_preview_loader = None;
                state.restore_prev_mode();
                *ctx.viewer_state = None;
            }
            KeyCode::Up | KeyCode::Char('k') => vs.scroll_up(1),
            KeyCode::Down | KeyCode::Char('j') => vs.scroll_down(1),
            KeyCode::PageUp => vs.page_up(page_height),
            KeyCode::PageDown => vs.page_down(page_height),
            KeyCode::Home => vs.go_to_top(),
            KeyCode::End => vs.go_to_bottom(page_height),
            KeyCode::Left => vs.scroll_left(HORIZONTAL_SCROLL_STEP),
            KeyCode::Right => vs.scroll_right(HORIZONTAL_SCROLL_STEP, content_width),
            KeyCode::Char('l') => vs.toggle_line_numbers(),
            KeyCode::Char('w') => vs.toggle_wrap(),
            KeyCode::Char('h') => vs.toggle_hex_mode(),
            KeyCode::Char('n') => vs.next_match(page_height),
            KeyCode::Char('N') => vs.prev_match(page_height),
            KeyCode::Char('/') => {
                state
                    .input
                    .dialog_input
                    .set_text_at_end(vs.search_query.clone().unwrap_or_default());
                state.mode = AppMode::Dialog(DialogKind::Input {
                    prompt: "Find in viewer:".to_string(),
                    action: InputAction::ViewerSearch,
                });
            }
            _ => {}
        }
    } else {
        *ctx.image_preview_loader = None;
        state.mode = AppMode::Normal;
    }
}

pub(crate) fn handle_search_mode(state: &mut AppState, key: KeyCode, terminal_height: u16) {
    let visible = panel_ops::panel_visible_height(terminal_height);
    match key {
        KeyCode::Esc => {
            clear_search_state(state, visible);
        }
        KeyCode::Enter => {
            clear_search_state(state, visible);
        }
        KeyCode::Backspace => {
            state.input.search_query.pop();
            state.input.search_cursor = state.input.search_query.len();
            if state.input.search_query.is_empty() {
                clear_search_state(state, visible);
            } else {
                apply_search_filter(state, visible);
            }
        }
        KeyCode::Char(c) => {
            state.input.search_query.push(c);
            state.input.search_cursor = state.input.search_query.len();
            apply_search_filter(state, visible);
        }
        _ => {}
    }
}

pub(crate) fn run_selected_menu_action<B: ratatui::backend::Backend>(
    ctx: &mut EventContext,
    terminal: &mut ratatui::Terminal<B>,
) {
    let prev = ctx.state.mode.clone();
    let Some((key, modifiers, for_menu_panel)) =
        super::menu_actions::execute_menu_action(ctx.state)
    else {
        if ctx.state.mode == prev {
            ctx.state.restore_prev_mode();
        }
        return;
    };
    ctx.state.mode = AppMode::Normal;
    if for_menu_panel {
        // `with_menu_panel` must own `&mut state` for the duration of the
        // closure (it flips the active panel before/after). The other context
        // fields are disjoint, so we reborrow them into a fresh `EventContext`
        // bound to the closure's `state` rather than passing `ctx` (whose
        // `state` field is moved into `with_menu_panel`).
        let viewer_state = &mut *ctx.viewer_state;
        let viewer_loader = &mut *ctx.viewer_loader;
        let image_preview_loader = &mut *ctx.image_preview_loader;
        let running_job = &mut *ctx.running_job;
        let term_size = ctx.term_size;
        panel_ops::with_menu_panel(ctx.state, |state| {
            let mut inner = EventContext {
                state,
                viewer_state,
                viewer_loader,
                image_preview_loader,
                running_job,
                term_size,
            };
            handle_normal_mode(&mut inner, key, modifiers, terminal);
        });
    } else {
        handle_normal_mode(ctx, key, modifiers, terminal);
    }
}

pub(crate) fn handle_menu_mode<B: ratatui::backend::Backend>(
    ctx: &mut EventContext,
    key: KeyCode,
    terminal: &mut ratatui::Terminal<B>,
) {
    let total = MENUS.len();
    let max_items = menu_item_count(ctx.state.ui.menu_selected);
    if max_items == 0 {
        ctx.state.mode = AppMode::Normal;
        return;
    }

    match key {
        KeyCode::Esc | KeyCode::F(9 | 10) => {
            ctx.state.restore_prev_mode();
        }
        KeyCode::Left => {
            ctx.state.ui.menu_selected = if ctx.state.ui.menu_selected == 0 {
                total - 1
            } else {
                ctx.state.ui.menu_selected - 1
            };
            ctx.state.ui.menu_item_selected = 0;
        }
        KeyCode::Right => {
            ctx.state.ui.menu_selected = (ctx.state.ui.menu_selected + 1) % total;
            ctx.state.ui.menu_item_selected = 0;
        }
        KeyCode::Up => {
            ctx.state.ui.menu_item_selected = if ctx.state.ui.menu_item_selected == 0 {
                max_items - 1
            } else {
                ctx.state.ui.menu_item_selected - 1
            };
        }
        KeyCode::Down => {
            ctx.state.ui.menu_item_selected = (ctx.state.ui.menu_item_selected + 1) % max_items;
        }
        KeyCode::Enter => {
            run_selected_menu_action(ctx, terminal);
        }
        _ => {}
    }
}
