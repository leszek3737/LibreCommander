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
    viewer_state: &Option<viewer::ViewerState>,
    viewer_loader: &Option<viewer::ViewerLoader>,
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
            viewer::render_loading_with_colors(f, f.area(), &loader.path, colors);
            return;
        }
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

    let cmd_text: std::borrow::Cow<'_, str> = if state.mode == AppMode::CommandLine {
        let (before, after) =
            safe_split_at(&state.command_line.text, state.command_line.byte_pos());
        format!("$ {before}_{after}").into()
    } else if state.mode == AppMode::Search {
        let (before, after) = safe_split_at(&state.search_query, state.search_cursor);
        format!("Search: {before}_{after}").into()
    } else if let Some(ref msg) = state.status_message {
        std::borrow::Cow::Borrowed(msg.as_str())
    } else {
        let ap = state.active_panel();
        ap.path.to_string_lossy()
    };
    let cmd_paragraph =
        ratatui::widgets::Paragraph::new(cmd_text).style(Theme::status_bar_with_colors(colors));
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar_with_colors(f, main_layout[4], colors);

    render_overlays(f, state, main_layout[0], colors);
}

fn render_overlays(f: &mut Frame, state: &AppState, menu_bar_area: Rect, colors: &ColorPalette) {
    if let AppMode::Dialog(ref dialog_kind) = state.mode {
        let ui_dialog = to_ui_dialog(dialog_kind, state);
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
            static COMPARE_MODES: std::sync::LazyLock<[String; 3]> =
                std::sync::LazyLock::new(|| ["Quick".into(), "Size".into(), "Thorough".into()]);
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
    }
}

fn to_ui_dialog<'a>(
    dialog_kind: &'a app::types::DialogKind,
    state: &'a AppState,
) -> dialogs::DialogKind<'a> {
    match dialog_kind {
        app::types::DialogKind::Confirm(cd) => dialogs::DialogKind::Confirm {
            title: Cow::Borrowed(&cd.title),
            message: Cow::Borrowed(&cd.message),
            selection: state.dialog_selection,
            files: cd
                .files
                .as_ref()
                .map(|fps| fps.iter().map(|p| p.display().to_string()).collect())
                .map_or_else(|| Cow::Borrowed(&[][..]), Cow::Owned),
        },
        app::types::DialogKind::Input { prompt, .. } => dialogs::DialogKind::Input {
            title: Cow::Borrowed("Input"),
            prompt: Cow::Borrowed(prompt),
            value: Cow::Borrowed(&state.dialog_input.text),
            cursor_pos: state.dialog_input.cursor,
        },
        app::types::DialogKind::Error(msg) => dialogs::DialogKind::Error {
            title: Cow::Borrowed("Error"),
            message: Cow::Borrowed(msg),
        },
        app::types::DialogKind::Help {
            message,
            scroll_offset,
        } => dialogs::DialogKind::Help {
            title: Cow::Borrowed("Help"),
            message: Cow::Borrowed(message),
            scroll_offset: *scroll_offset,
        },
        app::types::DialogKind::Progress {
            message,
            progress_fraction,
            cancellable,
        } => dialogs::DialogKind::Progress {
            title: Cow::Borrowed("Progress"),
            message: Cow::Borrowed(message),
            percent: *progress_fraction * 100.0,
            cancellable: *cancellable,
        },
        app::types::DialogKind::CopyMove {
            source,
            dest,
            is_move,
        } => {
            let action = if *is_move { "Move" } else { "Copy" };
            let msg = format!(
                "{} {} item(s)\nfrom: {}\n  to: {}",
                action,
                source.len(),
                source
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                dest.display(),
            );
            dialogs::DialogKind::Confirm {
                title: Cow::Borrowed(if *is_move {
                    "Move Confirm"
                } else {
                    "Copy Confirm"
                }),
                message: Cow::Owned(msg),
                selection: state.dialog_selection,
                files: Cow::Owned(source.iter().map(|p| p.display().to_string()).collect()),
            }
        }
        app::types::DialogKind::Properties { .. } => properties_to_ui_dialog(dialog_kind),
        app::types::DialogKind::OverwriteConfirm { conflicting } => {
            dialogs::DialogKind::OverwriteConfirm {
                selection: state.dialog_selection,
                files: Cow::Borrowed(conflicting),
            }
        }
    }
}

fn properties_to_ui_dialog(dialog_kind: &app::types::DialogKind) -> dialogs::DialogKind<'_> {
    let (name, size, mtime, permissions, owner, group, is_dir, is_symlink) = match dialog_kind {
        app::types::DialogKind::Properties {
            name,
            size,
            mtime,
            permissions,
            owner,
            group,
            is_dir,
            is_symlink,
        } => (
            name,
            size,
            mtime,
            permissions,
            owner,
            group,
            is_dir,
            is_symlink,
        ),
        _ => {
            return dialogs::DialogKind::Error {
                title: Cow::Borrowed("Internal Error"),
                message: Cow::Borrowed("Expected Properties dialog"),
            };
        }
    };
    let file_type = if *is_symlink {
        "Symlink"
    } else if *is_dir {
        "Directory"
    } else {
        "File"
    };
    use chrono::TimeZone;
    let mtime_str = if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
        chrono::Local
            .timestamp_opt(i64::try_from(duration.as_secs()).unwrap_or(i64::MAX), 0)
            .single()
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.into())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    } else {
        "Unknown".to_string()
    };
    dialogs::DialogKind::Properties {
        info: dialogs::PropertiesInfo {
            name: name.clone(),
            size: app::types::FileEntry::format_size(*size),
            mtime: mtime_str,
            permissions: app::types::FileEntry::display_permissions_raw(*permissions),
            owner: owner.clone(),
            group: group.clone(),
            file_type: file_type.to_string(),
        },
    }
}
