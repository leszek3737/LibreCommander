use crate::render_dialog_map;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
};

fn safe_split_at(s: &str, mut byte_idx: usize) -> (&str, &str) {
    byte_idx = byte_idx.min(s.len());
    while byte_idx > 0 && !s.is_char_boundary(byte_idx) {
        byte_idx -= 1;
    }
    s.split_at(byte_idx)
}

use lc::{app, ui};

use app::types::{ActivePanel, AppMode, AppState, PickerKind, ViewMode};
use std::borrow::Cow;
use ui::theme::{ColorPalette, Theme};
use ui::{dialogs, panels, viewer};

pub(crate) fn render_ui(
    f: &mut Frame,
    state: &AppState,
    viewer_state: Option<&viewer::ViewerState>,
    viewer_loader: Option<&viewer::ViewerLoader>,
) {
    let colors = &state.theme_colors;
    let icon_theme = colors.icon_theme();

    if state.mode == AppMode::Viewing {
        if let Some(vs) = viewer_state {
            match vs.view_mode {
                ViewMode::Hex => {
                    viewer::render_hex_view_with_colors(f, f.area(), vs, colors);
                }
                ViewMode::Image => {
                    viewer::render_image_view_with_colors(f, f.area(), vs, colors);
                }
                ViewMode::Text => {
                    viewer::render_viewer_with_colors(f, f.area(), vs, colors);
                }
            }
            return;
        }
        if let Some(loader) = viewer_loader {
            viewer::render_loading_with_colors(
                f,
                f.area(),
                &loader.path,
                colors,
                state.viewer_spinner_frame,
            );
            return;
        }
        // Viewing mode entered but no state or loader ready yet — fall through
        // to the normal panel layout so the screen isn't blank.
    }

    if state.mode == AppMode::DirectoryTree {
        ui::dir_tree::render_directory_tree_with_colors(
            f,
            &state.tree_root,
            &state.tree_entries,
            state.tree_selected,
            state.tree_scroll,
            colors,
        );
        return;
    }

    let size = f.area();

    let bg_block = ratatui::widgets::Block::default().style(Theme::panel_bg_with_colors(colors));
    f.render_widget(bg_block, size);

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(size);

    panels::render_menu_bar_with_colors(f, main_layout[0], colors);

    let panel_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    panels::render_panel_with_colors(
        f,
        panel_area[0],
        &state.left_panel,
        state.active_panel == ActivePanel::Left,
        colors,
        icon_theme,
    );
    panels::render_panel_with_colors(
        f,
        panel_area[1],
        &state.right_panel,
        state.active_panel == ActivePanel::Right,
        colors,
        icon_theme,
    );

    let active = if state.active_panel == ActivePanel::Left {
        &state.left_panel
    } else {
        &state.right_panel
    };
    panels::render_status_bar_with_colors(f, main_layout[2], active, colors);

    let cmd_text: Cow<'_, str> = if state.mode == AppMode::CommandLine {
        let (before, after) =
            safe_split_at(&state.command_line.text, state.command_line.byte_pos());
        format!("$ {before}_{after}").into()
    } else if state.mode == AppMode::Search {
        let (before, after) = safe_split_at(&state.search_query, state.search_cursor);
        format!("Search: {before}_{after}").into()
    } else if let Some(ref msg) = state.status_message {
        Cow::Borrowed(msg.as_str())
    } else {
        let ap = state.active_panel();
        ap.path().to_string_lossy()
    };
    let cmd_paragraph =
        ratatui::widgets::Paragraph::new(cmd_text).style(Theme::status_bar_with_colors(colors));
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar_with_colors(f, main_layout[4], colors);

    render_overlays(f, state, main_layout[0], colors);
}

fn render_overlays(f: &mut Frame, state: &AppState, menu_bar_area: Rect, colors: &ColorPalette) {
    if let AppMode::Dialog(ref dialog_kind) = state.mode {
        let ui_dialog = render_dialog_map::to_ui_dialog(dialog_kind, state);
        dialogs::render_dialog_with_colors(f, &ui_dialog, colors);
    }

    if state.mode == AppMode::Menu {
        ui::menu::render_menu_bar_with_colors(
            f,
            menu_bar_area,
            state.menu_selected,
            state.menu_item_selected,
            colors,
        );
    }

    if let AppMode::ListPicker(ref kind) = state.mode {
        render_list_picker_overlay(f, state, kind, colors);
    }
}

fn render_list_picker_overlay(
    f: &mut Frame,
    state: &AppState,
    kind: &PickerKind,
    colors: &ColorPalette,
) {
    match kind {
        PickerKind::History => {
            let items: Vec<&str> = state
                .command_history
                .iter()
                .rev()
                .map(|s| s.as_str())
                .collect();
            let selected = state.picker_selected.min(items.len().saturating_sub(1));
            dialogs::render_list_picker_with_colors(
                f,
                "Command History",
                &items,
                selected,
                "Enter: select  Esc: cancel",
                colors,
            );
        }
        PickerKind::Hotlist => {
            let selected = state
                .picker_selected
                .min(state.cached_hotlist_strings.len().saturating_sub(1));
            dialogs::render_list_picker_with_colors(
                f,
                "Directory Hotlist",
                &state.cached_hotlist_strings,
                selected,
                "Enter: cd  a: add current  d: delete  Esc: close",
                colors,
            );
        }
        PickerKind::CompareMode => {
            const COMPARE_MODES: [&str; 3] = ["Quick", "Size", "Thorough"];
            let items = &COMPARE_MODES[..];
            let selected = state.picker_selected.min(items.len().saturating_sub(1));
            dialogs::render_list_picker_with_colors(
                f,
                "Compare Mode",
                items,
                selected,
                "Enter: select  Esc: cancel",
                colors,
            );
        }
        PickerKind::UserMenu => {
            let selected = state
                .picker_selected
                .min(state.cached_user_menu_strings.len().saturating_sub(1));
            dialogs::render_list_picker_with_colors(
                f,
                "User Menu",
                &state.cached_user_menu_strings,
                selected,
                "Enter: run  Esc: cancel",
                colors,
            );
        }
        PickerKind::ArchiveMenu => {
            const ITEMS: [&str; 2] = ["Extract Archive", "Create Archive"];
            let selected = state.picker_selected.min(ITEMS.len().saturating_sub(1));
            dialogs::render_list_picker_with_colors(
                f,
                "Archive Operations",
                &ITEMS,
                selected,
                "Enter: select  Esc: cancel",
                colors,
            );
        }
    }
}
