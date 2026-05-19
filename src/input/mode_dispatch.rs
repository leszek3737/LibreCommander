use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::prelude::*;

use lc::app::panel_ops;
use lc::app::types::{AppMode, AppState, DialogKind, InputAction};
use lc::menu::{menu_item_count, menu_total_count};
use lc::ui::viewer;

use crate::{
    handle_alt_keys, handle_ctrl_keys, handle_enter_key, handle_function_keys,
    handle_navigation_keys,
};

const VIEWER_CHROME_HEIGHT: u16 = 3;
const HORIZONTAL_SCROLL_STEP: usize = 4;

pub(crate) fn clear_search_state(state: &mut AppState, visible_height: usize) {
    state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
    state.search_query.clear();
    state.search_cursor = 0;
    let panel = state.active_panel_mut();
    panel.filter = None;
    if panel.listing.unfiltered_dirty || panel.listing.unfiltered_entries.is_empty() {
        panel_ops::refresh_active(state);
    } else {
        panel_ops::rebuild_visible_entries(panel, visible_height);
    }
}

pub(crate) fn initiate_search(
    state: &mut AppState,
    prev_mode: AppMode,
    c: char,
    visible_height: usize,
) {
    state.prev_mode = Some(prev_mode);
    state.search_query.push(c);
    state.search_cursor = state.search_query.len();
    let filter_query = state.search_query.clone();
    state.mode = AppMode::Search;
    let panel = state.active_panel_mut();
    panel.filter = Some(filter_query);
    if panel.listing.unfiltered_dirty || panel.listing.unfiltered_entries.is_empty() {
        panel_ops::refresh_active(state);
    } else {
        panel_ops::rebuild_visible_entries(panel, visible_height);
    }
}

pub(crate) fn handle_normal_mode<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    modifiers: KeyModifiers,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<B>,
) {
    let visible = panel_ops::panel_visible_height(terminal_height);
    match key {
        KeyCode::F(_) => {
            handle_function_keys(state, viewer_state, viewer_loader, key, terminal);
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
            handle_enter_key(state, visible);
        }
        KeyCode::Char('u' | 's' | 'h' | 'r' | 'o') if modifiers == KeyModifiers::CONTROL => {
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

pub(crate) fn handle_viewer_mode(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    terminal_size: Size,
) {
    if viewer_loader.is_some() {
        if matches!(key, KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char('q')) {
            viewer_loader.take();
            state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
            *viewer_state = None;
        }
        return;
    }
    if let Some(vs) = viewer_state.as_mut() {
        let page_height = terminal_size.height.saturating_sub(VIEWER_CHROME_HEIGHT) as usize;
        let content_width = terminal_size.width as usize;
        vs.update_wrap_layout(content_width);
        vs.clamp_scroll();
        match key {
            KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char('q') => {
                state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
                *viewer_state = None;
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
                state.dialog_input.text = vs.search_query.clone().unwrap_or_default();
                state.dialog_input.cursor_end();
                state.mode = AppMode::Dialog(DialogKind::Input {
                    prompt: "Find in viewer:".to_string(),
                    default_text: state.dialog_input.text.clone(),
                    action: InputAction::ViewerSearch,
                });
            }
            _ => {}
        }
    } else {
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
            state.search_query.pop();
            state.search_cursor = state.search_query.len();
            if state.search_query.is_empty() {
                clear_search_state(state, visible);
            } else {
                let filter_query = Some(state.search_query.clone());
                let panel = state.active_panel_mut();
                panel.filter = filter_query;
                panel_ops::rebuild_visible_entries(panel, visible);
            }
        }
        KeyCode::Char(c) => {
            state.search_query.push(c);
            state.search_cursor = state.search_query.len();
            let filter_query = state.search_query.clone();
            let panel = state.active_panel_mut();
            panel.filter = Some(filter_query);
            panel_ops::rebuild_visible_entries(panel, visible);
        }
        _ => {}
    }
}

pub(crate) fn run_selected_menu_action<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<B>,
) {
    let previous_discriminant = std::mem::discriminant(&state.mode);
    if let Some((key, modifiers, for_menu_panel)) = super::menu_actions::execute_menu_action(state)
    {
        state.mode = AppMode::Normal;
        if for_menu_panel {
            panel_ops::with_menu_panel(state, |state| {
                handle_normal_mode(
                    state,
                    viewer_state,
                    viewer_loader,
                    key,
                    modifiers,
                    terminal_height,
                    terminal,
                );
            });
        } else {
            handle_normal_mode(
                state,
                viewer_state,
                viewer_loader,
                key,
                modifiers,
                terminal_height,
                terminal,
            );
        }
    } else if std::mem::discriminant(&state.mode) == previous_discriminant {
        state.mode = AppMode::Normal;
    }
}

pub(crate) fn handle_menu_mode<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<B>,
) {
    let max_items = menu_item_count(state.menu_selected);
    if max_items == 0 {
        state.mode = AppMode::Normal;
        return;
    }

    match key {
        KeyCode::Esc | KeyCode::F(9 | 10) => {
            state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
        }
        KeyCode::Left => {
            state.menu_selected = if state.menu_selected == 0 {
                menu_total_count() - 1
            } else {
                state.menu_selected - 1
            };
            state.menu_item_selected = 0;
        }
        KeyCode::Right => {
            state.menu_selected = (state.menu_selected + 1) % menu_total_count();
            state.menu_item_selected = 0;
        }
        KeyCode::Up => {
            state.menu_item_selected = if state.menu_item_selected == 0 {
                max_items - 1
            } else {
                state.menu_item_selected - 1
            };
        }
        KeyCode::Down => {
            state.menu_item_selected = (state.menu_item_selected + 1) % max_items;
        }
        KeyCode::Enter => {
            run_selected_menu_action(
                state,
                viewer_state,
                viewer_loader,
                terminal_height,
                terminal,
            );
        }
        _ => {}
    }
}
