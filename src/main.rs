use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEvent,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

use lc::{app, fs, menu, ui};

mod input;
mod render;

use app::job_runner::{RunningJob, poll_running_job};
use app::types::{ActivePanel, AppMode, AppState, InputAction, PanelState};
use app::{panel_ops, paths, shell, watcher_sync};

use ui::viewer;

const EVENT_POLL_TIMEOUT_MS: u64 = 100;

pub(crate) fn file_name_str(p: &std::path::Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = leave_tui_stdout();
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = leave_tui_stdout();
        default_hook(panic_info);
    }));
}

fn enter_tui_stdout() -> io::Result<()> {
    enable_raw_mode()?;
    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide) {
        let _ = disable_raw_mode();
        return Err(err);
    }
    Ok(())
}

fn leave_tui_stdout() -> io::Result<()> {
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
    raw_result.and(screen_result)
}

fn suspend_terminal_stdout() -> io::Result<()> {
    leave_tui_stdout()
}

fn resume_terminal_stdout() -> io::Result<()> {
    enter_tui_stdout()
}

fn terminal_state_file_path() -> PathBuf {
    paths::terminal_state_file_path()
}

fn main() -> io::Result<()> {
    install_panic_hook();
    enter_tui_stdout()?;

    let result = {
        let _guard = TerminalGuard;
        let backend = CrosstermBackend::new(io::stdout());
        match Terminal::new(backend) {
            Ok(mut terminal) => run_app(&mut terminal),
            Err(err) => Err(err),
        }
    };

    if let Err(err) = &result {
        lc::debug_log!("Error: {err}");
        let msg = format!("Error: {err}\n");
        let _ = io::stderr().write_all(msg.as_bytes());
    }
    result
}

fn poll_viewer_loader(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
) -> bool {
    let Some(loader) = viewer_loader.as_ref() else {
        return false;
    };
    match loader.receiver.try_recv() {
        Ok(Ok(vs)) => {
            *viewer_state = Some(vs);
            *viewer_loader = None;
        }
        Ok(Err(e)) => {
            state.status_message = Some(format!("Failed to open file: {e}"));
            state.mode = AppMode::Normal;
            *viewer_loader = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            state.status_message = Some("Viewer load failed: thread panicked".to_string());
            state.mode = AppMode::Normal;
            *viewer_loader = None;
        }
    }
    true
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let terminal_state_file = terminal_state_file_path();

    if std::fs::metadata(&terminal_state_file).is_ok() {
        let leave_result = leave_tui_stdout();
        let resume_result = resume_terminal_stdout();
        if resume_result.is_ok()
            && let Err(e) = std::fs::remove_file(&terminal_state_file)
        {
            lc::debug_log!("failed to remove terminal state file: {e}");
        }
        leave_result?;
        resume_result?;
    }

    let mut state = AppState::new();
    let config_raw = match app::config::load_setup(&mut state) {
        Ok(raw) => raw,
        Err(e) => {
            state.status_message = Some(e);
            None
        }
    };
    if let Some(ref raw) = config_raw
        && let Err(e) = ui::theme::Theme::apply_from_value_to_palette(raw, &mut state.theme_colors)
    {
        state.status_message = Some(e);
    }

    let mut viewer_state: Option<viewer::ViewerState> = None;
    let mut viewer_loader: Option<viewer::ViewerLoader> = None;
    let mut running_job: Option<RunningJob> = None;
    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = match fs::watcher::Watcher::new(watch_tx) {
        Ok(w) => Some(w),
        Err(err) => {
            let msg = format!("watcher disabled: {err}");
            state.status_message = match state.status_message.take() {
                Some(prev) => Some(format!("{prev}; {msg}")),
                None => Some(msg),
            };
            None
        }
    };
    let mut watcher_paused = false;
    let mut last_synced_paths: Option<(PathBuf, PathBuf)> = None;

    panel_ops::refresh_panel(&mut state.left_panel, 0);
    panel_ops::refresh_panel(&mut state.right_panel, 0);
    watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut last_synced_paths);

    let mut dirty = true;

    loop {
        panel_ops::sync_watcher_job_state(&watcher, running_job.is_some(), &mut watcher_paused);
        watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut last_synced_paths);
        if let Some(ref w) = watcher {
            w.flush_pending();
        }
        if watcher_sync::poll_watcher_events(&mut state, &watch_rx) {
            dirty = true;
        }

        if poll_running_job(&mut state, &mut running_job, panel_ops::refresh_both) {
            let resumed = panel_ops::sync_watcher_job_state(
                &watcher,
                running_job.is_some(),
                &mut watcher_paused,
            );
            if resumed {
                panel_ops::refresh_both(&mut state);
            }
            dirty = true;
        }

        if poll_viewer_loader(&mut state, &mut viewer_state, &mut viewer_loader) {
            dirty = true;
        }

        if dirty {
            terminal.draw(|f| render::render_ui(f, &state, &viewer_state, &viewer_loader))?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))? {
            dirty = dispatch_event(
                &mut state,
                &mut viewer_state,
                &mut viewer_loader,
                &mut running_job,
                terminal,
                &event::read()?,
            )?;
        }

        if state.should_quit {
            return Ok(());
        }
    }
}

fn dispatch_event<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    running_job: &mut Option<RunningJob>,
    terminal: &mut Terminal<B>,
    event: &Event,
) -> Result<bool, B::Error> {
    match event {
        Event::Key(key) => dispatch_key_event(
            state,
            viewer_state,
            viewer_loader,
            running_job,
            terminal,
            key,
        ),
        Event::Mouse(mouse_event) => dispatch_mouse_event(
            state,
            viewer_state,
            viewer_loader,
            running_job,
            mouse_event,
            terminal,
        ),
        Event::Resize(_, _) => Ok(true),
        _ => Ok(false),
    }
}

fn dispatch_key_event<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    running_job: &mut Option<RunningJob>,
    terminal: &mut Terminal<B>,
    key: &KeyEvent,
) -> Result<bool, B::Error> {
    let size = terminal.size()?;
    match key.kind {
        KeyEventKind::Press => {}
        KeyEventKind::Repeat if key_repeat_allowed(&state.mode, key.code) => {}
        _ => return Ok(true),
    }
    match &state.mode {
        AppMode::Normal => {
            input::mode_dispatch::handle_normal_mode(
                state,
                viewer_state,
                viewer_loader,
                key.code,
                key.modifiers,
                size.height,
                terminal,
            );
        }
        AppMode::Viewing => {
            input::mode_dispatch::handle_viewer_mode(
                state,
                viewer_state,
                viewer_loader,
                key.code,
                size,
            );
        }
        AppMode::CommandLine => {
            input::command_line::handle_command_line(state, *key);
        }
        AppMode::Dialog(_) => {
            input::dialogs::handle_dialog(state, viewer_state, running_job, key.code, size);
        }
        AppMode::Search if matches!(key.code, KeyCode::F(_)) => {
            input::mode_dispatch::clear_search_state(state);
            input::mode_dispatch::handle_normal_mode(
                state,
                viewer_state,
                viewer_loader,
                key.code,
                key.modifiers,
                size.height,
                terminal,
            );
        }
        AppMode::Search => {
            input::mode_dispatch::handle_search_mode(state, key.code, size.height);
        }
        AppMode::Menu => {
            input::mode_dispatch::handle_menu_mode(
                state,
                viewer_state,
                viewer_loader,
                key.code,
                size.height,
                terminal,
            );
        }
        AppMode::ListPicker(_) => {
            input::pickers::handle_list_picker(state, key.code);
        }
        AppMode::DirectoryTree => {
            input::directory_tree::handle_directory_tree(
                state,
                viewer_state,
                viewer_loader,
                key.code,
                size.height,
            );
        }
    }
    Ok(true)
}

fn key_repeat_allowed(mode: &AppMode, key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Char('j' | 'k')
    ) || matches!(
        mode,
        AppMode::CommandLine
            | AppMode::Dialog(_)
            | AppMode::Search
            | AppMode::Menu
            | AppMode::ListPicker(_)
    ) && matches!(
        key,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Char(_) | KeyCode::Enter
    )
}

fn dispatch_mouse_event<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    running_job: &mut Option<RunningJob>,
    mouse_event: &MouseEvent,
    terminal: &mut Terminal<B>,
) -> Result<bool, B::Error> {
    let size = terminal.size()?;
    if let Some(outcome) = input::mouse::handle_mouse_event(
        state,
        viewer_state,
        viewer_loader,
        running_job,
        *mouse_event,
        size,
    ) {
        match outcome {
            input::mouse::MouseOutcome::Consumed => {}
            input::mouse::MouseOutcome::NormalKey(key) => {
                if matches!(state.mode, AppMode::Search) {
                    input::mode_dispatch::clear_search_state(state);
                }
                input::mode_dispatch::handle_normal_mode(
                    state,
                    viewer_state,
                    viewer_loader,
                    key,
                    KeyModifiers::NONE,
                    size.height,
                    terminal,
                );
            }
            input::mouse::MouseOutcome::MenuAction => {
                input::mode_dispatch::run_selected_menu_action(
                    state,
                    viewer_state,
                    viewer_loader,
                    size.height,
                    terminal,
                );
            }
        }
    }
    Ok(true)
}

pub(crate) fn handle_function_keys<B: ratatui::backend::Backend>(
    state: &mut AppState,
    _viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    key: KeyCode,
    terminal: &mut ratatui::Terminal<B>,
) {
    match key {
        KeyCode::F(1) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Help {
                message: app::keymap::build_help_message().to_string(),
                scroll_offset: 0,
            });
        }
        KeyCode::F(2) => {
            input::menu_actions::open_user_menu(state);
        }
        KeyCode::F(3) => {
            if let Some(entry) = state.active_panel().current_entry()
                && !entry.is_dir()
            {
                let path = entry.path.clone();
                *viewer_loader = Some(viewer::ViewerState::open_background(path));
                state.prev_mode = None;
                state.mode = AppMode::Viewing;
            }
        }
        KeyCode::F(4) => {
            launch_editor(state, terminal);
        }
        KeyCode::F(5) => {
            confirm_file_transfer(state, "Copy Confirm", "Copy", |sources, dest| {
                app::types::PendingAction::Copy {
                    sources,
                    dest,
                    overwrite: false,
                }
            });
        }
        KeyCode::F(6) => {
            confirm_file_transfer(state, "Move Confirm", "Move", |sources, dest| {
                app::types::PendingAction::Move {
                    sources,
                    dest,
                    overwrite: false,
                }
            });
        }
        KeyCode::F(7) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Input {
                prompt: "Create directory:".to_string(),
                default_text: String::new(),
                action: InputAction::CreateDirectory,
            });
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
        }
        KeyCode::F(8) => {
            confirm_delete(state);
        }
        KeyCode::F(9) => {
            state.prev_mode = Some(state.mode.clone());
            state.mode = AppMode::Menu;
            state.menu_item_selected = 0;
        }
        KeyCode::F(10) => {
            state.should_quit = true;
        }
        _ => {}
    }
}

fn launch_editor<B: ratatui::backend::Backend>(
    state: &mut AppState,
    terminal: &mut ratatui::Terminal<B>,
) {
    let entry_info = state
        .active_panel()
        .current_entry()
        .map(|e| (e.is_dir(), e.path.clone()));
    if let Some((is_dir, path)) = entry_info
        && !is_dir
    {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        if let Err(e) = suspend_terminal_stdout() {
            state.status_message = Some(format!("Terminal suspend failed: {e}"));
            return;
        }
        let terminal_state_file = terminal_state_file_path();
        if let Some(parent) = terminal_state_file.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            lc::debug_log!("failed to create terminal state dir: {e}");
        }
        if let Err(e) = std::fs::write(&terminal_state_file, "alternate_screen") {
            lc::debug_log!("failed to write terminal state file: {e}");
        }
        let parts: Vec<String> = shlex::split(&editor).unwrap_or_else(|| {
            let fallback: Vec<String> = editor.split_whitespace().map(String::from).collect();
            if fallback.is_empty() {
                vec!["vi".to_string()]
            } else {
                fallback
            }
        });
        let cmd = parts.first().map_or("vi", |s| s.as_str());
        let status = std::process::Command::new(cmd)
            .args(&parts[1..])
            .arg(&path)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        let resume_result = resume_terminal_stdout();
        if resume_result.is_ok()
            && let Err(e) = std::fs::remove_file(&terminal_state_file)
        {
            lc::debug_log!("failed to remove terminal state file: {e}");
        }
        match (status, resume_result) {
            (Err(e), _) => state.status_message = Some(format!("Editor error: {e}")),
            (_, Err(e)) => {
                state.status_message = Some(format!("Terminal restore failed after editor: {e}"));
            }
            (Ok(s), _) if !s.success() => {
                state.status_message = Some(format!("Editor exited with status: {s}"));
            }
            (Ok(_), Ok(_)) => {}
        }
        if let Err(e) = terminal.clear() {
            lc::debug_log!("terminal.clear() failed after editor: {e}");
        }
        panel_ops::refresh_active(state);
    }
}

fn confirm_file_transfer(
    state: &mut AppState,
    label: &str,
    verb: &str,
    make_pending: impl FnOnce(Vec<PathBuf>, PathBuf) -> app::types::PendingAction,
) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let dest_dir = state.inactive_panel().path.clone();
    let file_names = panel_ops::file_names_from_paths(&paths);
    let msg = if paths.len() == 1 {
        let name = file_name_str(&paths[0]).unwrap_or_default();
        format!("{verb} '{name}' to '{}'?", dest_dir.display())
    } else {
        format!(
            "{verb} {} entries to '{}'?",
            paths.len(),
            dest_dir.display()
        )
    };
    state.dialog_selection = 0;
    state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(
        app::types::ConfirmDetails::with_files(label, &msg, file_names),
    ));
    state.pending_action = Some(make_pending(paths, dest_dir));
}

fn confirm_delete(state: &mut AppState) {
    let paths = selected_or_current_paths(state);
    if paths.is_empty() {
        return;
    }
    let file_names = panel_ops::file_names_from_paths(&paths);
    let msg = if paths.len() == 1 {
        let name = file_name_str(&paths[0]).unwrap_or_default();
        format!("Delete '{name}'?")
    } else {
        format!("Delete {} entries?", paths.len())
    };
    state.dialog_selection = 0;
    state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(
        app::types::ConfirmDetails::with_files("Delete Confirm", &msg, file_names),
    ));
    state.pending_action = Some(app::types::PendingAction::Delete { paths });
}

pub(crate) fn handle_navigation_keys(
    state: &mut AppState,
    key: KeyCode,
    modifiers: KeyModifiers,
    visible: usize,
) {
    match key {
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            if panel.entries.is_empty() {
                return;
            }
            panel.toggle_selection_at(panel.cursor);
            panel.move_cursor_up(visible);
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            if panel.entries.is_empty() {
                return;
            }
            panel.toggle_selection_at(panel.cursor);
            panel.move_cursor_down(visible);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let panel = state.active_panel_mut();
            panel.move_cursor_up(visible);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let panel = state.active_panel_mut();
            panel.move_cursor_down(visible);
        }
        KeyCode::Home => {
            let p = state.active_panel_mut();
            p.cursor = 0;
            p.scroll_offset = 0;
        }
        KeyCode::End => {
            let len = state.active_panel().entries.len();
            if len > 0 {
                let p = state.active_panel_mut();
                p.cursor = len - 1;
                p.ensure_cursor_visible(visible);
            }
        }
        KeyCode::PageUp => {
            let p = state.active_panel_mut();
            p.cursor = p.cursor.saturating_sub(visible);
            p.scroll_offset = p.scroll_offset.saturating_sub(visible);
        }
        KeyCode::PageDown => {
            let len = state.active_panel().entries.len();
            let p = state.active_panel_mut();
            p.cursor = (p.cursor + visible).min(len.saturating_sub(1));
            p.scroll_offset = (p.scroll_offset + visible).min(len.saturating_sub(visible));
        }
        KeyCode::Tab => {
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            let p = state.active_panel_mut();
            let max = p.entries.len().saturating_sub(1);
            p.cursor = p.cursor.min(max);
            p.ensure_cursor_visible(visible);
        }
        KeyCode::Insert => {
            let panel = state.active_panel_mut();
            if panel.entries.is_empty() {
                return;
            }
            panel.toggle_selection();
            if panel.cursor < panel.entries.len() - 1 {
                panel.move_cursor_down(visible);
            }
        }
        _ => {}
    }
}

fn reposition_cursor_to_entry(state: &mut AppState, prev_dir_name: Option<&str>, visible: usize) {
    if let Some(name) = prev_dir_name
        && let Some(idx) = state
            .active_panel()
            .entries
            .iter()
            .position(|e| e.name == name)
    {
        let p = state.active_panel_mut();
        p.cursor = idx;
        p.ensure_cursor_visible(visible);
    }
}

pub(crate) fn handle_enter_key(state: &mut AppState, visible: usize) {
    let entry_info = state
        .active_panel()
        .current_entry()
        .map(|e| (e.is_dir(), e.path.clone(), e.name == ".."));
    if let Some((is_dir, path, is_dotdot)) = entry_info
        && is_dir
    {
        let prev_dir_name = if is_dotdot {
            file_name_str(&state.active_panel().path)
        } else {
            None
        };
        let p = state.active_panel_mut();
        p.history.push(p.path.clone());
        p.path = path;
        p.cursor = 0;
        p.scroll_offset = 0;
        panel_ops::refresh_active(state);
        reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
    }
}

pub(crate) fn handle_ctrl_keys(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Char('u') => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
        }
        KeyCode::Char('s') => {
            let panel = state.active_panel_mut();
            if panel.unfiltered_entries.is_empty() {
                panel.unfiltered_entries = panel.entries.clone();
                panel.path_index.clear();
            }
            state.mode = AppMode::Search;
            state.search_query.clear();
            state.search_cursor = 0;
        }
        KeyCode::Char('h') => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            panel_ops::refresh_active(state);
        }
        KeyCode::Char('r') => {
            panel_ops::refresh_active(state);
        }
        KeyCode::Char('o') => {
            if let Err(e) = shell::toggle_external_view(state, panel_ops::refresh_both) {
                state.status_message = Some(format!("External view error: {e}"));
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
                state.mode = AppMode::Dialog(app::types::DialogKind::Properties {
                    name: entry.name.clone(),
                    size: entry.size(),
                    mtime: entry.mtime(),
                    permissions: entry.mode_bits(),
                    owner: entry.owner.clone(),
                    group: entry.group.clone(),
                    is_dir: entry.is_dir(),
                    is_symlink: entry.is_symlink(),
                });
            }
        }
        KeyCode::Backspace => {
            let prev_dir_name = file_name_str(&state.active_panel().path);
            let panel = state.active_panel_mut();
            if let Some(prev_path) = panel.history.pop()
                && prev_path.is_dir()
            {
                panel.path = prev_path.clone();
                panel.cursor = 0;
                panel.scroll_offset = 0;
                panel_ops::refresh_active(state);
                reposition_cursor_to_entry(state, prev_dir_name.as_deref(), visible);
                state.status_message = Some(format!("cd to {}", prev_path.display()));
            }
        }
        KeyCode::Char(c) if ('1'..='9').contains(&c) => {
            panel_ops::navigate_to_hotlist(state, (c as usize) - ('1' as usize));
        }
        KeyCode::Char('c') => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Input {
                prompt: "Quick cd:".to_string(),
                default_text: state.active_panel().path.display().to_string(),
                action: InputAction::QuickCd,
            });
            state.dialog_input = state.active_panel().path.display().to_string();
            state.dialog_cursor_pos = state.dialog_input.chars().count();
        }
        KeyCode::Char('x' | 'X') => {
            state.command_line.clear();
            state.command_cursor = 0;
            state.history_index = None;
            state.prev_mode = None;
            state.mode = AppMode::CommandLine;
        }
        _ => {}
    }
}

fn selected_or_current_paths(state: &AppState) -> Vec<std::path::PathBuf> {
    let selected: Vec<std::path::PathBuf> = state
        .active_panel()
        .selected_entries()
        .into_iter()
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.path.clone())
        .collect();

    if selected.is_empty() {
        state
            .active_panel()
            .current_entry()
            .filter(|entry| entry.name != "..")
            .map(|entry| vec![entry.path.clone()])
            .unwrap_or_default()
    } else {
        selected
    }
}

pub(crate) fn apply_search_filter(panel: &mut PanelState) {
    panel.sync_unfiltered_selection();
    panel.entries = panel_ops::filtered_sorted_entries(
        &panel.unfiltered_entries,
        panel.filter.as_deref(),
        panel.sort_mode,
        panel.sort_options,
        panel.show_hidden,
    );
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.recalculate_selection_stats();
}

#[cfg(test)]
mod tests;
