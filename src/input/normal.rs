use std::io;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};

use lc::app::file_type;
use lc::app::paths::terminal_state_file_path;
use lc::app::types::{AppMode, AppState, FileEntry, InputAction, PickerKind, format_mtime};
use lc::app::{panel_ops, shell};
use lc::ops::archive;
use lc::ui::viewer;

use crate::{enter_tui_stdout, file_name_str, leave_tui_stdout};

pub(crate) fn handle_function_keys<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    terminal: &mut ratatui::Terminal<B>,
) {
    match key {
        KeyCode::F(1) => open_help_dialog(state),
        KeyCode::F(2) => super::menu_actions::open_user_menu(state),
        KeyCode::F(3) => view_current_entry(state, viewer_loader),
        KeyCode::F(4) => launch_editor(state, terminal),
        KeyCode::F(5) => confirm_copy(state),
        KeyCode::F(6) => confirm_move(state),
        KeyCode::F(7) => handle_f7_key(state),
        KeyCode::F(8) => confirm_delete(state),
        KeyCode::F(9) => open_menu_bar(state),
        KeyCode::F(10) => state.request_quit(),
        KeyCode::F(11) => open_rename_dialog(state),
        KeyCode::F(12) => handle_f12_key(state),
        _ => {}
    }
}

fn open_help_dialog(state: &mut AppState) {
    state.mode = AppMode::Dialog(lc::app::types::DialogKind::Help {
        message: lc::app::keymap::build_help_message().to_string(),
        scroll_offset: 0,
    });
}

fn view_current_entry(state: &mut AppState, viewer_loader: &mut Option<viewer::ViewerLoader>) {
    if let Some(entry) = state.active_panel().current_entry()
        && !entry.is_dir()
    {
        open_in_viewer(state, viewer_loader, entry.path.clone());
    }
}

fn confirm_copy(state: &mut AppState) {
    confirm_file_transfer(state, "Copy Confirm", "Copy", |sources, dest| {
        lc::app::types::PendingAction::Copy(lc::app::types::TransferAction {
            sources,
            dest,
            overwrite: false,
        })
    });
}

fn confirm_move(state: &mut AppState) {
    confirm_file_transfer(state, "Move Confirm", "Move", |sources, dest| {
        lc::app::types::PendingAction::Move(lc::app::types::TransferAction {
            sources,
            dest,
            overwrite: false,
        })
    });
}

fn open_menu_bar(state: &mut AppState) {
    state.prev_mode = Some(std::mem::replace(&mut state.mode, AppMode::Menu));
    state.ui.menu_item_selected = 0;
}

fn open_rename_dialog(state: &mut AppState) {
    let entry_name = state
        .active_panel()
        .current_entry()
        .filter(|e| e.name != "..")
        .map(|e| e.name.clone());
    if let Some(name) = entry_name {
        state.input.dialog_input.set_text_at_end(name);
        state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
            prompt: "Rename to:".to_string(),
            action: InputAction::Rename,
        });
    }
}

fn handle_f7_key(state: &mut AppState) {
    if let Some(entry) = state.active_panel().current_entry()
        && is_archive_file(entry)
    {
        show_archive_dialog(state);
    } else {
        state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
            prompt: "Create directory:".to_string(),
            action: InputAction::CreateDirectory,
        });
        state.input.dialog_input.clear();
    }
}

fn handle_f12_key(state: &mut AppState) {
    if let Some(entry) = state.active_panel().current_entry()
        && is_archive_file(entry)
    {
        show_archive_dialog(state);
        return;
    }
    let paths = selected_or_current_paths(state);
    if !paths.is_empty() {
        let dest_input = lc::app::types::TextInput::new();
        state.mode = AppMode::Dialog(lc::app::types::DialogKind::ArchiveCreate(Box::new(
            lc::app::types::ArchiveCreateDetails {
                sources: paths,
                dest_input,
            },
        )));
        return;
    }
    state.ui.picker_selected = 0;
    state.mode = AppMode::ListPicker(PickerKind::ArchiveMenu);
}

pub(crate) fn launch_editor<B: ratatui::backend::Backend>(
    state: &mut AppState,
    terminal: &mut ratatui::Terminal<B>,
) {
    let Some((is_dir, path)) = state
        .active_panel()
        .current_entry()
        .map(|e| (e.is_dir(), e.path.clone()))
    else {
        return;
    };
    if is_dir {
        return;
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    if let Err(e) = leave_tui_stdout() {
        state.ui.status_message = Some(format!("Terminal suspend failed: {e}"));
        return;
    }

    write_terminal_state_marker();
    let spawn_result = spawn_editor(&editor, &path);
    let resume_result = enter_tui_stdout();
    clear_terminal_state_marker();

    if let Some(message) = editor_status_message(spawn_result, resume_result) {
        state.ui.status_message = Some(message);
    }
    if let Err(e) = terminal.clear() {
        lc::debug_log!("terminal.clear() failed after editor: {e}");
    }
    panel_ops::refresh_active(state);
}

/// Parse the `EDITOR` value into argv parts, falling back to `vi` when the
/// value cannot be split or is empty.
fn editor_command_parts(editor: &str) -> Vec<String> {
    shlex::split(editor).unwrap_or_else(|| {
        let fallback: Vec<String> = editor.split_whitespace().map(String::from).collect();
        if fallback.is_empty() {
            vec!["vi".to_string()]
        } else {
            fallback
        }
    })
}

/// Spawn the configured editor on `path`, inheriting the parent stdio so the
/// editor takes over the terminal. Returns the editor's exit status.
fn spawn_editor(editor: &str, path: &std::path::Path) -> io::Result<std::process::ExitStatus> {
    let parts = editor_command_parts(editor);
    let cmd = parts.first().map_or("vi", |s| s.as_str());
    std::process::Command::new(cmd)
        .args(parts.get(1..).unwrap_or_default())
        .arg(path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
}

/// Persist a marker file so a crash mid-edit can detect we left the alternate
/// screen. Best-effort: failures are logged but never surfaced to the user.
fn write_terminal_state_marker() {
    let Some(terminal_state_file) = terminal_state_file_path() else {
        return;
    };
    if let Some(parent) = terminal_state_file.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        lc::debug_log!("failed to create terminal state dir: {e}");
    }
    if let Err(e) = std::fs::write(&terminal_state_file, "alternate_screen") {
        lc::debug_log!("failed to write terminal state file: {e}");
    }
}

/// Remove the terminal-state marker written before launching the editor.
/// Best-effort: a missing or unremovable file is only logged.
fn clear_terminal_state_marker() {
    if let Some(terminal_state_file) = terminal_state_file_path()
        && let Err(e) = std::fs::remove_file(&terminal_state_file)
    {
        lc::debug_log!("failed to remove terminal state file: {e}");
    }
}

/// Build the user-facing status message (if any) from the editor spawn result
/// and the terminal-resume result. Returns `None` when everything succeeded.
fn editor_status_message(
    spawn_result: io::Result<std::process::ExitStatus>,
    resume_result: io::Result<()>,
) -> Option<String> {
    match (spawn_result, resume_result) {
        (Err(e), _) => Some(format!("Editor error: {e}")),
        (Ok(s), Err(e)) => {
            let mut parts = Vec::new();
            if !s.success() {
                parts.push(format!("Editor exited with status: {s}"));
            }
            parts.push(format!("Terminal restore failed: {e}"));
            Some(parts.join("; "))
        }
        (Ok(s), Ok(())) if !s.success() => Some(format!("Editor exited with status: {s}")),
        (Ok(_), Ok(())) => None,
    }
}

pub(crate) fn confirm_file_transfer(
    state: &mut AppState,
    label: &str,
    verb: &str,
    make_pending: impl FnOnce(Vec<PathBuf>, PathBuf) -> lc::app::types::PendingAction,
) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let dest_dir = state.inactive_panel().path().to_path_buf();
    let file_names = display_file_names(&paths);
    let msg = if let [single] = file_names.as_slice() {
        format!("{verb} '{single}' to '{}'?", dest_dir.display())
    } else {
        format!(
            "{verb} {} entries to '{}'?",
            paths.len(),
            dest_dir.display()
        )
    };
    let pending = make_pending(paths, dest_dir);
    open_confirm_dialog(state, label, &msg, file_names, pending);
}

pub(crate) fn confirm_delete(state: &mut AppState) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let file_names = display_file_names(&paths);
    let msg = if let [single] = file_names.as_slice() {
        format!("Delete '{single}'?")
    } else {
        format!("Delete {} entries?", paths.len())
    };
    let pending = lc::app::types::PendingAction::Delete { paths };
    open_confirm_dialog(state, "Delete Confirm", &msg, file_names, pending);
}

/// Open a confirmation dialog with the given label, message and affected file
/// names, and arm `pending_action` to run once the user confirms. Shared by
/// the copy/move/delete confirmation entry points.
fn open_confirm_dialog(
    state: &mut AppState,
    label: &str,
    msg: &str,
    file_names: Vec<String>,
    pending: lc::app::types::PendingAction,
) {
    state.input.dialog_selection = 0;
    state.mode = AppMode::Dialog(lc::app::types::DialogKind::Confirm(
        lc::app::types::ConfirmDetails::with_files(label, msg, file_names),
    ));
    state.ui.pending_action = Some(pending);
}

pub(crate) fn handle_navigation_keys(
    state: &mut AppState,
    key: KeyCode,
    modifiers: KeyModifiers,
    visible: usize,
) {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    match key {
        KeyCode::Up if shift => select_and_move(state, visible, Direction::Up),
        KeyCode::Down if shift => select_and_move(state, visible, Direction::Down),
        KeyCode::Up | KeyCode::Char('k') => {
            state.active_panel_mut().move_cursor_up(visible);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.active_panel_mut().move_cursor_down(visible);
        }
        KeyCode::Home => move_cursor_home(state),
        KeyCode::End => move_cursor_end(state, visible),
        KeyCode::PageUp => page_up(state, visible),
        KeyCode::PageDown => page_down(state, visible),
        KeyCode::Tab => switch_active_panel(state, visible),
        KeyCode::Insert => toggle_selection_and_advance(state, visible),
        _ => {}
    }
}

/// Vertical movement direction for Shift+selection navigation.
#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// Toggle the selection at the cursor, then move one row in `direction`.
/// No-op on an empty (filtered) listing.
fn select_and_move(state: &mut AppState, visible: usize, direction: Direction) {
    let panel = state.active_panel_mut();
    if panel.listing.filtered_is_empty() {
        return;
    }
    panel.toggle_selection_at(panel.cursor);
    match direction {
        Direction::Up => panel.move_cursor_up(visible),
        Direction::Down => panel.move_cursor_down(visible),
    }
}

fn move_cursor_home(state: &mut AppState) {
    let p = state.active_panel_mut();
    p.cursor = 0;
    p.scroll_offset = 0;
}

fn move_cursor_end(state: &mut AppState, visible: usize) {
    let len = state.active_panel().listing.filtered_len();
    if len > 0 {
        let p = state.active_panel_mut();
        p.cursor = len - 1;
        p.ensure_cursor_visible(visible);
    }
}

fn page_up(state: &mut AppState, visible: usize) {
    let p = state.active_panel_mut();
    p.cursor = p.cursor.saturating_sub(visible);
    p.scroll_offset = p.scroll_offset.saturating_sub(visible);
}

fn page_down(state: &mut AppState, visible: usize) {
    let len = state.active_panel().listing.filtered_len();
    let p = state.active_panel_mut();
    p.cursor = p.cursor.saturating_add(visible).min(len.saturating_sub(1));
    p.scroll_offset = p
        .scroll_offset
        .saturating_add(visible)
        .min(len.saturating_sub(visible));
}

fn switch_active_panel(state: &mut AppState, visible: usize) {
    state.active_panel = state.active_panel.toggle();
    let p = state.active_panel_mut();
    let max = p.listing.filtered_len().saturating_sub(1);
    p.cursor = p.cursor.min(max);
    p.ensure_cursor_visible(visible);
}

fn toggle_selection_and_advance(state: &mut AppState, visible: usize) {
    let panel = state.active_panel_mut();
    if panel.listing.filtered_is_empty() {
        return;
    }
    panel.toggle_selection();
    if panel.cursor < panel.listing.filtered_len() - 1 {
        panel.move_cursor_down(visible);
    }
}

pub(crate) fn reposition_cursor_to_entry(
    state: &mut AppState,
    prev_dir_name: Option<&str>,
    visible: usize,
) {
    if let Some(name) = prev_dir_name {
        // Resolve the index first so the borrowing iterator is dropped before the
        // mutable panel access below.
        let idx = state
            .active_panel()
            .listing
            .filtered()
            .position(|e| e.name == name);
        if let Some(idx) = idx {
            let p = state.active_panel_mut();
            p.cursor = idx;
            p.ensure_cursor_visible(visible);
        }
    }
}

pub(crate) fn handle_enter_key<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    visible: usize,
    _terminal: &mut ratatui::Terminal<B>,
) {
    let Some(entry) = state.active_panel().current_entry() else {
        return;
    };
    if entry.is_dir() {
        let path = entry.path.clone();
        let is_dotdot = entry.name == "..";
        let prev_dir_name = if is_dotdot {
            file_name_str(state.active_panel().path())
        } else {
            None
        };
        let p = state.active_panel_mut();
        if p.history().back().map(|p| p.as_path()) != Some(p.path()) {
            p.push_history(p.path().to_path_buf());
        }
        p.set_path(path);
        p.cursor = 0;
        p.scroll_offset = 0;
        panel_ops::refresh_active(state);
        reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
    } else if is_archive_file(entry) {
        open_in_viewer(state, viewer_loader, entry.path.clone());
    }
}

pub(crate) fn show_archive_dialog(state: &mut AppState) {
    let entry = match state.active_panel().current_entry() {
        Some(e) => e,
        None => return,
    };
    let source = entry.path.clone();
    let dest = state.active_panel().path().display().to_string();
    // Listing a large or corrupt archive can be slow, so it runs off the event
    // thread: record the request and show a loading dialog. The main loop spawns
    // the read and builds the extract dialog when it completes (see `bg_load`).
    state.ui.pending_archive_list = Some((source, dest));
    state.mode = AppMode::Dialog(lc::app::types::DialogKind::progress(
        "Listing archive...".to_string(),
        0.0,
        true,
    ));
}

/// Build the extract dialog from a completed background archive listing, or
/// report the failure. Called by the main loop when the `bg_load` finishes.
pub(crate) fn apply_archive_list_result(
    state: &mut AppState,
    source: PathBuf,
    dest: String,
    result: Result<Vec<archive::ArchiveEntry>, archive::ArchiveError>,
) {
    match result {
        Ok(entries) => {
            let mut dest_input = lc::app::types::TextInput::new();
            dest_input.set_text_at_end(dest);
            state.mode = AppMode::Dialog(lc::app::types::DialogKind::ArchiveExtract(Box::new(
                lc::app::types::ArchiveExtractDetails {
                    source,
                    entries,
                    dest_input,
                },
            )));
        }
        Err(e) => {
            state.ui.status_message = Some(format!("Failed to list archive: {e}"));
            state.mode = AppMode::Normal;
        }
    }
}

pub(crate) fn handle_ctrl_keys(state: &mut AppState, key: KeyCode, terminal_height: u16) {
    match key {
        KeyCode::Char('u') => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = state.active_panel.toggle();
        }
        KeyCode::Char('s') => {
            state.mode = AppMode::Search;
            state.input.search_query.clear();
            state.input.search_cursor = 0;
        }
        KeyCode::Char('h') => {
            let visible = panel_ops::panel_visible_height(terminal_height);
            let p = state.active_panel_mut();
            p.set_show_hidden(!p.show_hidden());
            p.cursor = 0;
            p.scroll_offset = 0;
            panel_ops::rebuild_visible_entries(p, visible);
        }
        KeyCode::Char('r') => {
            panel_ops::refresh_active(state);
        }
        KeyCode::Char('o') => {
            if let Err(e) = shell::toggle_external_view(state, panel_ops::refresh_both) {
                state.ui.status_message = Some(format!("External view error: {e}"));
            }
        }
        _ => {}
    }
}

pub(crate) fn handle_alt_keys(state: &mut AppState, key: KeyCode, visible: usize) {
    match key {
        KeyCode::Enter => {
            if let Some(entry) = state.active_panel().current_entry()
                && entry.name != ".."
            {
                state.mode = AppMode::Dialog(lc::app::types::DialogKind::Properties(Box::new(
                    lc::app::types::PropertiesDetails {
                        name: entry.name.clone(),
                        size: entry.size(),
                        mtime: entry.mtime(),
                        permissions: entry.mode_bits(),
                        owner: entry.owner.to_string(),
                        group: entry.group.to_string(),
                        kind: lc::app::types::FileKind::from_metadata_flags(
                            entry.is_dir(),
                            entry.is_symlink(),
                        ),
                        size_str: lc::app::types::FileEntry::format_size(entry.size()),
                        mtime_str: format_mtime(entry.mtime()),
                        permissions_str: lc::app::types::FileEntry::display_permissions_raw(
                            entry.mode_bits(),
                        ),
                    },
                )));
            }
        }
        KeyCode::Backspace => {
            let prev_dir_name = file_name_str(state.active_panel().path());
            let panel = state.active_panel_mut();
            if let Some(prev_path) = panel.pop_history() {
                if prev_path.is_dir() {
                    panel.set_path(prev_path.clone());
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    panel_ops::refresh_active(state);
                    reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
                    state.ui.status_message = Some(format!("cd to {}", prev_path.display()));
                } else {
                    panel.push_history(prev_path);
                }
            }
        }
        KeyCode::Char(c) if ('1'..='9').contains(&c) => {
            panel_ops::navigate_to_hotlist(state, (c as usize) - ('1' as usize));
        }
        KeyCode::Char('c') => {
            state.mode = AppMode::Dialog(lc::app::types::DialogKind::Input {
                prompt: "Quick cd:".to_string(),
                action: InputAction::QuickCd,
            });
            state
                .input
                .dialog_input
                .set_text_at_end(state.active_panel().path().display().to_string());
        }
        KeyCode::Char('x' | 'X') => state.enter_command_line_mode(),
        _ => {}
    }
}

pub(crate) fn selected_or_current_paths(state: &AppState) -> Vec<PathBuf> {
    let panel = state.active_panel();

    let current_entry_fallback = || {
        panel
            .current_entry()
            .filter(|entry| is_not_parent_dir(entry))
            .map(|entry| vec![entry.path.clone()])
            .unwrap_or_default()
    };

    if panel.selected_count() == 0 {
        return current_entry_fallback();
    }

    let selected: Vec<PathBuf> = panel
        .selected_entries()
        .filter(|entry| is_not_parent_dir(entry))
        .map(|entry| entry.path.clone())
        .collect();

    if selected.is_empty() {
        return current_entry_fallback();
    }

    selected
}

fn is_archive_file(entry: &FileEntry) -> bool {
    !entry.is_dir() && file_type::is_archive(&entry.name)
}

fn is_not_parent_dir(entry: &FileEntry) -> bool {
    entry.name != ".."
}

fn open_in_viewer(
    state: &mut AppState,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    path: PathBuf,
) {
    *viewer_loader = Some(viewer::ViewerState::open_background(path));
    state.prev_mode = None;
    state.mode = AppMode::Viewing;
}

/// Render the display name (last path component, falling back to the full path)
/// for each path. Builds the strings directly to avoid an intermediate
/// `Vec<PathBuf>` and the extra path clone it would require.
fn display_file_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            p.file_name().map_or_else(
                || p.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        })
        .collect()
}
