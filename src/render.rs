use std::borrow::Cow;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
};

use lc::{app, ui};

use app::types::{ActivePanel, AppMode, AppState, PickerKind};
use ui::theme::Theme;
use ui::{dialogs, panels, viewer};

fn clamp_selection(selected: usize, items_len: usize) -> usize {
    selected.min(items_len.saturating_sub(1))
}

fn safe_split_at(s: &str, byte_cursor: usize) -> (&str, &str) {
    let pos = byte_cursor.min(s.len());
    if s.is_char_boundary(pos) {
        s.split_at(pos)
    } else {
        let mut p = pos;
        while p > 0 && !s.is_char_boundary(p) {
            p -= 1;
        }
        s.split_at(p)
    }
}

pub(crate) fn render_ui(
    f: &mut Frame,
    state: &AppState,
    viewer_state: &Option<viewer::ViewerState>,
) {
    if state.mode == AppMode::Viewing
        && let Some(vs) = viewer_state
    {
        if vs.is_hex_mode() {
            viewer::render_hex_view(f, f.area(), vs);
        } else {
            viewer::render_viewer(f, f.area(), vs);
        }
        return;
    }

    if state.mode == AppMode::DirectoryTree {
        ui::dir_tree::render_directory_tree(
            f,
            &state.tree_root,
            &state.tree_entries,
            state.tree_selected,
            state.tree_scroll,
        );
        return;
    }

    let size = f.area();

    let bg_block = ratatui::widgets::Block::default().style(Theme::panel_bg());
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

    panels::render_menu_bar(f, main_layout[0]);

    let panel_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    panels::render_panel(
        f,
        panel_area[0],
        &state.left_panel,
        state.active_panel == ActivePanel::Left,
    );
    panels::render_panel(
        f,
        panel_area[1],
        &state.right_panel,
        state.active_panel == ActivePanel::Right,
    );

    let active = if state.active_panel == ActivePanel::Left {
        &state.left_panel
    } else {
        &state.right_panel
    };
    panels::render_status_bar(f, main_layout[2], active);

    let cmd_text: Cow<'_, str> = if state.mode == AppMode::CommandLine {
        let (before, after) = safe_split_at(&state.command_line, state.command_cursor);
        format!("$ {before}_{after}").into()
    } else if state.mode == AppMode::Search {
        let (before, after) = safe_split_at(&state.search_query, state.search_cursor);
        format!("Search: {before}_{after}").into()
    } else if let Some(ref msg) = state.status_message {
        Cow::Borrowed(msg.as_str())
    } else {
        let ap = state.active_panel();
        ap.path.to_string_lossy()
    };
    let cmd_paragraph = ratatui::widgets::Paragraph::new(cmd_text).style(Theme::status_bar());
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar(f, main_layout[4]);

    render_overlays(f, state, main_layout[0]);
}

fn render_overlays(f: &mut Frame, state: &AppState, menu_bar_area: Rect) {
    if let AppMode::Dialog(ref dialog_kind) = state.mode {
        let ui_dialog = to_ui_dialog(dialog_kind, state);
        dialogs::render_dialog(f, &ui_dialog);
    }

    if state.mode == AppMode::Menu {
        ui::menu::render_menu_dropdown(
            f,
            menu_bar_area,
            state.menu_selected,
            state.menu_item_selected,
        );
    }

    if let AppMode::ListPicker(ref kind) = state.mode {
        render_list_picker_overlay(f, state, kind);
    }
}

fn render_list_picker_overlay(f: &mut Frame, state: &AppState, kind: &PickerKind) {
    match kind {
        PickerKind::History => {
            let items: Vec<&str> = state
                .command_history
                .iter()
                .rev()
                .map(|s| s.as_str())
                .collect();
            let selected = clamp_selection(state.picker_selected, items.len());
            dialogs::render_list_picker(
                f,
                "Command History",
                &items,
                selected,
                "Enter: select  Esc: cancel",
            );
        }
        PickerKind::Hotlist => {
            let items: Vec<String> = state
                .directory_hotlist
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            let selected = clamp_selection(state.picker_selected, items.len());
            dialogs::render_list_picker(
                f,
                "Directory Hotlist",
                &items,
                selected,
                "Enter: cd  a: add current  d: delete  Esc: close",
            );
        }
        PickerKind::CompareMode => {
            static COMPARE_MODES: std::sync::LazyLock<[String; 3]> =
                std::sync::LazyLock::new(|| ["Quick".into(), "Size".into(), "Thorough".into()]);
            let items = &COMPARE_MODES[..];
            let selected = clamp_selection(state.picker_selected, items.len());
            dialogs::render_list_picker(
                f,
                "Compare Mode",
                items,
                selected,
                "Enter: select  Esc: cancel",
            );
        }
        PickerKind::UserMenu => {
            let items: Vec<String> = state
                .user_menu_entries
                .iter()
                .map(|e| format!("{}  {}", e.hotkey, e.title))
                .collect();
            let selected = clamp_selection(state.picker_selected, items.len());
            dialogs::render_list_picker(
                f,
                "User Menu",
                &items,
                selected,
                "Enter: run  Esc: cancel",
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
                .map(|fps| fps.iter().map(|p| p.display().to_string()).collect()),
        },
        app::types::DialogKind::Input { prompt, .. } => dialogs::DialogKind::Input {
            title: Cow::Borrowed("Input"),
            prompt: Cow::Borrowed(prompt),
            value: Cow::Borrowed(&state.dialog_input),
            cursor_pos: state.dialog_cursor_pos,
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
        app::types::DialogKind::Progress(msg, pct) => dialogs::DialogKind::Progress {
            title: Cow::Borrowed("Progress"),
            message: Cow::Borrowed(msg),
            percent: *pct * 100.0,
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
                    .unwrap_or_else(|| "(current directory)".to_string()),
                dest.display(),
            );
            dialogs::DialogKind::Confirm {
                title: Cow::Owned(format!("{action} Confirm")),
                message: Cow::Owned(msg),
                selection: state.dialog_selection,
                files: Some(source.iter().map(|p| p.display().to_string()).collect()),
            }
        }
        app::types::DialogKind::Properties { .. } => properties_to_ui_dialog(dialog_kind),
        app::types::DialogKind::OverwriteConfirm { conflicting } => {
            dialogs::DialogKind::OverwriteConfirm {
                selection: state.dialog_selection,
                files: conflicting.as_slice(),
            }
        }
    }
}

fn properties_to_ui_dialog(dialog_kind: &app::types::DialogKind) -> dialogs::DialogKind<'_> {
    let app::types::DialogKind::Properties {
        name,
        size,
        mtime,
        permissions,
        owner,
        group,
        is_dir,
        is_symlink,
    } = dialog_kind
    else {
        return dialogs::DialogKind::Error {
            title: "Error".into(),
            message: "Internal error: properties dialog expected".into(),
        };
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
            .timestamp_opt(i64::try_from(duration.as_secs()).unwrap_or(0), 0)
            .single()
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.into())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    } else {
        "Unknown".to_string()
    };
    dialogs::DialogKind::Properties {
        info: dialogs::PropertiesInfo {
            name: name.to_string(),
            size: app::types::FileEntry::format_size(*size),
            mtime: mtime_str,
            permissions: app::types::FileEntry::display_permissions_raw(*permissions),
            owner: owner.to_string(),
            group: group.to_string(),
            file_type: file_type.to_string(),
        },
    }
}
