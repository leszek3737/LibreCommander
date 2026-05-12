use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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

use menu::{menu_item_count, menu_total_count};

use ui::viewer;

const EVENT_POLL_TIMEOUT_MS: u64 = 100;
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

#[allow(clippy::print_stderr)]
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
        eprintln!("Error: {err}");
    }
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let terminal_state_file = terminal_state_file_path();

    if std::fs::metadata(&terminal_state_file).is_ok() {
        let leave_result = leave_tui_stdout();
        let resume_result = resume_terminal_stdout();
        if resume_result.is_ok() {
            let _ = std::fs::remove_file(&terminal_state_file);
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
        && let Err(e) = ui::theme::Theme::apply_from_value(raw)
    {
        state.status_message = Some(e);
    }

    let mut viewer_state: Option<viewer::ViewerState> = None;
    let mut running_job: Option<RunningJob> = None;
    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = match fs::watcher::Watcher::new(watch_tx) {
        Ok(w) => Some(w),
        Err(err) => {
            state.status_message = Some(format!("watcher disabled: {err}"));
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

        if dirty {
            terminal.draw(|f| render::render_ui(f, &state, &viewer_state))?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))? {
            dirty = dispatch_event(
                &mut state,
                &mut viewer_state,
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

fn dispatch_event(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    event: &Event,
) -> io::Result<bool> {
    match event {
        Event::Key(key) => match &state.mode {
            AppMode::Normal => {
                handle_normal_mode(
                    state,
                    viewer_state,
                    key.code,
                    key.modifiers,
                    terminal.size()?.height,
                    terminal,
                );
            }
            AppMode::Viewing => {
                let sz = terminal.size()?;
                handle_viewer_mode(state, viewer_state, key.code, sz, sz.width);
            }
            AppMode::CommandLine => {
                input::command_line::handle_command_line(state, *key);
            }
            AppMode::Dialog(_) => {
                input::dialogs::handle_dialog(
                    state,
                    viewer_state,
                    running_job,
                    key.code,
                    terminal.size()?,
                );
            }
            AppMode::Search => {
                handle_search_mode(state, key.code, terminal.size()?.height);
            }
            AppMode::Menu => {
                handle_menu_mode(
                    state,
                    viewer_state,
                    key.code,
                    terminal.size()?.height,
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
                    key.code,
                    terminal.size()?.height,
                );
            }
        },
        Event::Mouse(mouse_event) => {
            let size: ratatui::layout::Size = terminal.size()?;
            if let Some(outcome) = input::mouse::handle_mouse_event(
                state,
                viewer_state,
                running_job,
                *mouse_event,
                size,
            ) {
                match outcome {
                    input::mouse::MouseOutcome::Consumed => {}
                    input::mouse::MouseOutcome::NormalKey(key) => {
                        handle_normal_mode(
                            state,
                            viewer_state,
                            key,
                            KeyModifiers::NONE,
                            terminal.size()?.height,
                            terminal,
                        );
                    }
                    input::mouse::MouseOutcome::MenuAction => {
                        run_selected_menu_action(
                            state,
                            viewer_state,
                            terminal.size()?.height,
                            terminal,
                        );
                    }
                }
            }
        }
        Event::Resize(_, _) => {}
        _ => return Ok(false),
    }
    Ok(true)
}

fn handle_function_keys<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
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
                && let Ok(vs) = viewer::ViewerState::open(&entry.path)
            {
                *viewer_state = Some(vs);
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
        if let Some(parent) = terminal_state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&terminal_state_file, "alternate_screen");
        let mut parts = editor.split_whitespace();
        let cmd = parts.next().unwrap_or("vi");
        let status = std::process::Command::new(cmd)
            .args(parts)
            .arg(&path)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        let resume_result = resume_terminal_stdout();
        if resume_result.is_ok() {
            let _ = std::fs::remove_file(&terminal_state_file);
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
        let _ = terminal.clear();
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
        let name = paths[0]
            .file_name()
            .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
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
        let name = paths[0]
            .file_name()
            .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
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

fn handle_navigation_keys(
    state: &mut AppState,
    key: KeyCode,
    modifiers: KeyModifiers,
    visible: usize,
) {
    match key {
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            if panel.cursor > 0 {
                panel.toggle_selection_at(panel.cursor);
                panel.cursor -= 1;
                if panel.cursor < panel.scroll_offset {
                    panel.scroll_offset = panel.cursor;
                }
            }
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            let panel = state.active_panel_mut();
            let len = panel.entries.len();
            if len > 0 {
                panel.toggle_selection_at(panel.cursor);
                if panel.cursor < len - 1 {
                    panel.cursor += 1;
                    if panel.cursor >= panel.scroll_offset + visible {
                        panel.scroll_offset = panel.cursor.saturating_sub(visible) + 1;
                    }
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let panel = state.active_panel_mut();
            panel.move_cursor_up();
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
            state.active_panel_mut().toggle_selection();
            state.active_panel_mut().move_cursor_down(visible);
        }
        _ => {}
    }
}

fn handle_enter_key(state: &mut AppState, visible: usize) {
    let entry_info = state
        .active_panel()
        .current_entry()
        .map(|e| (e.is_dir(), e.path.clone(), e.name == ".."));
    if let Some((is_dir, path, is_dotdot)) = entry_info
        && is_dir
    {
        let prev_dir_name = if is_dotdot {
            state
                .active_panel()
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        } else {
            None
        };
        let p = state.active_panel_mut();
        p.history.push(p.path.clone());
        p.path = path;
        p.cursor = 0;
        p.scroll_offset = 0;
        panel_ops::refresh_active(state);
        if let Some(ref name) = prev_dir_name
            && let Some(idx) = state
                .active_panel()
                .entries
                .iter()
                .position(|e| &e.name == name)
        {
            let p = state.active_panel_mut();
            p.cursor = idx;
            p.ensure_cursor_visible(visible);
        }
    }
}

fn handle_ctrl_keys(state: &mut AppState, key: KeyCode) {
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
            }
            state.mode = AppMode::Search;
            state.search_query.clear();
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

fn handle_alt_keys(state: &mut AppState, key: KeyCode, visible: usize) {
    match key {
        KeyCode::Enter => {
            if let Some(entry) = state.active_panel().current_entry()
                && entry.name != ".."
            {
                state.mode = AppMode::Dialog(app::types::DialogKind::Properties {
                    name: entry.name.clone(),
                    size: entry.len(),
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
            let prev_dir_name = state
                .active_panel()
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            let panel = state.active_panel_mut();
            if let Some(prev_path) = panel.history.pop()
                && prev_path.is_dir()
            {
                panel.path = prev_path.clone();
                panel.cursor = 0;
                panel.scroll_offset = 0;
                panel_ops::refresh_active(state);
                if let Some(ref name) = prev_dir_name
                    && let Some(idx) = state
                        .active_panel()
                        .entries
                        .iter()
                        .position(|e| &e.name == name)
                {
                    let p = state.active_panel_mut();
                    p.cursor = idx;
                    p.ensure_cursor_visible(visible);
                }
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
        _ => {}
    }
}

pub(crate) fn handle_normal_mode<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    modifiers: KeyModifiers,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<B>,
) {
    let visible = panel_ops::panel_visible_height(terminal_height);
    match key {
        KeyCode::F(_) => {
            handle_function_keys(state, viewer_state, key, terminal);
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
        KeyCode::Char('u' | 's' | 'h' | 'r' | 'o') if modifiers.contains(KeyModifiers::CONTROL) => {
            handle_ctrl_keys(state, key);
        }
        KeyCode::Enter | KeyCode::Backspace | KeyCode::Char(_)
            if modifiers.contains(KeyModifiers::ALT) =>
        {
            handle_alt_keys(state, key, visible);
        }
        _ => {
            if let KeyCode::Char(c) = key
                && modifiers.is_empty()
            {
                state.search_query.push(c);
                state.mode = AppMode::Search;
                let filter_query = state.search_query.clone();
                let panel = state.active_panel_mut();
                if panel.unfiltered_entries.is_empty() {
                    panel.unfiltered_entries = panel.entries.clone();
                }
                panel.filter = Some(filter_query);
                panel.cursor = 0;
                panel.scroll_offset = 0;
                panel_ops::refresh_active(state);
            }
        }
    }
}
fn handle_viewer_mode(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_size: Size,
    terminal_width: u16,
) {
    if let Some(vs) = viewer_state.as_mut() {
        let page_height = terminal_size.height.saturating_sub(3) as usize;
        let content_width = terminal_width as usize;
        vs.update_wrap_layout(content_width);
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
            KeyCode::Left => vs.scroll_left(4),
            KeyCode::Right => vs.scroll_right(4, content_width),
            KeyCode::Char('l') => vs.toggle_line_numbers(),
            KeyCode::Char('w') => vs.toggle_wrap(),
            KeyCode::Char('h') => vs.toggle_hex_mode(),
            KeyCode::Char('n') => vs.next_match(page_height),
            KeyCode::Char('N') => vs.prev_match(page_height),
            KeyCode::Char('/') => {
                state.dialog_input = vs.search_query.clone().unwrap_or_default();
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(app::types::DialogKind::Input {
                    prompt: "Viewer search:".to_string(),
                    default_text: state.dialog_input.clone(),
                    action: InputAction::ViewerSearch,
                });
            }
            _ => {}
        }
    } else {
        state.mode = AppMode::Normal;
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

fn apply_search_filter(panel: &mut PanelState) {
    panel.sync_unfiltered_selection();
    panel.entries = panel_ops::filtered_sorted_entries(
        &panel.unfiltered_entries,
        panel.filter.as_deref(),
        panel.sort_mode,
        panel.sort_options,
    );
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.recalculate_selection_stats();
}

fn handle_search_mode(state: &mut AppState, key: KeyCode, _terminal_height: u16) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.search_query.clear();
            let panel = state.active_panel_mut();
            panel.filter = None;
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel_ops::refresh_active(state);
        }
        KeyCode::Enter => {
            state.mode = AppMode::Normal;
            state.search_query.clear();
            let panel = state.active_panel_mut();
            panel.unfiltered_entries.clear();
            panel_ops::refresh_active(state);
        }
        KeyCode::Backspace => {
            state.search_query.pop();
            let filter_query = if state.search_query.is_empty() {
                None
            } else {
                Some(state.search_query.clone())
            };
            let panel = state.active_panel_mut();
            panel.filter = filter_query;
            apply_search_filter(panel);
        }
        KeyCode::Char(c) => {
            state.search_query.push(c);
            let filter_query = state.search_query.clone();
            let panel = state.active_panel_mut();
            panel.filter = Some(filter_query);
            apply_search_filter(panel);
        }
        _ => {}
    }
}

fn run_selected_menu_action<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<B>,
) {
    let previous_discriminant = std::mem::discriminant(&state.mode);
    if let Some(action_key) = execute_menu_action(state) {
        state.mode = AppMode::Normal;
        handle_normal_mode(
            state,
            viewer_state,
            action_key,
            KeyModifiers::NONE,
            terminal_height,
            terminal,
        );
    } else if std::mem::discriminant(&state.mode) == previous_discriminant {
        state.mode = AppMode::Normal;
    }
}

fn handle_menu_mode<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
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
            state.mode = AppMode::Normal;
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
            run_selected_menu_action(state, viewer_state, terminal_height, terminal);
        }
        _ => {}
    }
}

pub(crate) fn execute_menu_action(state: &mut AppState) -> Option<KeyCode> {
    input::menu_actions::execute_menu_action(state)
}

// ---- Type conversion helpers ----

#[cfg(test)]
mod tests;
