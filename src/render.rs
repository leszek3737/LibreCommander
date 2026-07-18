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

/// Build a `prefix` + text-with-cursor line (e.g. `"$ before_after"`).
/// Shared by the command-line and search variants of the bottom bar.
fn cursor_line(prefix: &str, text: &str, byte_pos: usize) -> String {
    let (before, after) = safe_split_at(text, byte_pos);
    format!("{prefix}{before}_{after}")
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

    match &state.mode {
        AppMode::Viewing => {
            if let Some(vs) = viewer_state {
                render_active_viewer(f, vs, colors);
                return;
            }
            if let Some(loader) = viewer_loader {
                viewer::render_loading_with_colors(
                    f,
                    f.area(),
                    &loader.path,
                    colors,
                    state.ui.viewer_spinner_frame,
                );
                return;
            }
            // Viewing mode entered but no state or loader ready yet — fall through
            // to the normal panel layout so the screen isn't blank.
        }
        AppMode::DirectoryTree => {
            ui::dir_tree::render_directory_tree_with_colors(
                f,
                &state.tree.root,
                &state.tree.entries,
                state.tree.selected,
                state.tree.scroll,
                colors,
            );
            return;
        }
        _ => {}
    }

    // A modal overlay can be open while the viewer is still active (e.g. the
    // viewer search dialog, which switches `mode` to `Dialog`). Draw the viewer
    // as the background so the overlay sits over the file being viewed rather
    // than over the panels.
    if let Some(vs) = viewer_state {
        render_active_viewer(f, vs, colors);
        render_overlays(f, state, f.area(), colors);
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

    let active = state.active_panel();
    panels::render_status_bar_with_colors(f, main_layout[2], active, colors);

    // Per-frame alloc for command bar — low cost (short strings, discarded each frame).
    let cmd_text: Cow<'_, str> = if state.mode == AppMode::CommandLine {
        cursor_line(
            "$ ",
            state.input.command_line.text(),
            state.input.command_line.byte_pos(),
        )
        .into()
    } else if state.mode == AppMode::Search {
        cursor_line(
            "Search: ",
            &state.input.search_query,
            state.input.search_cursor,
        )
        .into()
    } else if let Some(ref msg) = state.ui.status_message {
        Cow::Borrowed(msg.as_str())
    } else {
        active.path().to_string_lossy()
    };
    let cmd_paragraph =
        ratatui::widgets::Paragraph::new(cmd_text).style(Theme::status_bar_with_colors(colors));
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar_with_colors(f, main_layout[4], colors);

    render_overlays(f, state, main_layout[0], colors);
}

/// Renders the active viewer full-screen, dispatching on its view mode. Shared
/// by the `Viewing` mode and by the overlay-over-viewer background path.
fn render_active_viewer(f: &mut Frame, vs: &viewer::ViewerState, colors: &ColorPalette) {
    match vs.view_mode {
        ViewMode::Hex => viewer::render_hex_view_with_colors(f, f.area(), vs, colors),
        ViewMode::Image => viewer::render_image_view_with_colors(f, f.area(), vs, colors),
        ViewMode::Text => viewer::render_viewer_with_colors(f, f.area(), vs, colors),
    }
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
            state.ui.menu_selected,
            state.ui.menu_item_selected,
            colors,
        );
    }

    if let AppMode::ListPicker(ref kind) = state.mode {
        render_list_picker_overlay(f, state, kind, colors);
    }
}

fn clamp_selected(selected: usize, len: usize) -> usize {
    selected.min(len.saturating_sub(1))
}

/// Shared hint shown by pickers that simply select or cancel.
const SELECT_CANCEL_HINT: &str = "Enter: select  Esc: cancel";

/// Clamp the selection against `items` and render the overlay. Collapses the
/// per-`PickerKind` items -> clamp -> render boilerplate into one path.
fn render_picker<T: AsRef<str>>(
    f: &mut Frame,
    title: &str,
    items: &[T],
    selected: usize,
    hint: &str,
    colors: &ColorPalette,
) {
    let selected = clamp_selected(selected, items.len());
    dialogs::render_list_picker_with_colors(f, title, items, selected, hint, colors);
}

fn render_list_picker_overlay(
    f: &mut Frame,
    state: &AppState,
    kind: &PickerKind,
    colors: &ColorPalette,
) {
    let selected = state.ui.picker_selected;
    match kind {
        PickerKind::History => {
            render_picker(
                f,
                "Command History",
                &state.ui.cached_history_strings,
                selected,
                SELECT_CANCEL_HINT,
                colors,
            );
        }
        PickerKind::Hotlist => render_picker(
            f,
            "Directory Hotlist",
            &state.ui.cached_hotlist_strings,
            selected,
            "Enter: cd  a: add current  d: delete  Esc: close",
            colors,
        ),
        PickerKind::CompareMode => {
            const COMPARE_MODES: [&str; 3] = ["Quick", "Size", "Thorough"];
            render_picker(
                f,
                "Compare Mode",
                &COMPARE_MODES,
                selected,
                SELECT_CANCEL_HINT,
                colors,
            );
        }
        PickerKind::UserMenu => render_picker(
            f,
            "User Menu",
            &state.ui.cached_user_menu_strings,
            selected,
            "Enter: run  Esc: cancel",
            colors,
        ),
        PickerKind::ArchiveMenu => {
            const ITEMS: [&str; 2] = ["Extract Archive", "Create Archive"];
            render_picker(
                f,
                "Archive Operations",
                &ITEMS,
                selected,
                SELECT_CANCEL_HINT,
                colors,
            );
        }
    }
}
