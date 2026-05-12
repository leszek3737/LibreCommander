use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

use lc::{app, fs, menu, ops, ui};

mod input;
mod render;

use app::job_runner::{RunningJob, poll_running_job, start_confirmed_action};
use app::types::{
    ActivePanel, AppMode, AppState, CompareMode, InputAction, PanelState, PickerKind,
};
use app::{dir_tree, paths, shell, user_menu, watcher_sync};
use fs::reader;
use fs::watcher::Watcher;
use menu::{menu_item_count, menu_total_count};

use ui::{DIR_TREE_OVERHEAD_ROWS, LAYOUT_OVERHEAD_ROWS, dialogs, viewer};

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

    refresh_panel(&mut state.left_panel, 0);
    refresh_panel(&mut state.right_panel, 0);
    watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut last_synced_paths);

    let mut dirty = true;

    loop {
        sync_watcher_job_state(&watcher, running_job.is_some(), &mut watcher_paused);
        watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut last_synced_paths);
        if watcher_sync::poll_watcher_events(&mut state, &watch_rx) {
            dirty = true;
        }

        if poll_running_job(&mut state, &mut running_job, refresh_both) {
            let resumed =
                sync_watcher_job_state(&watcher, running_job.is_some(), &mut watcher_paused);
            if resumed {
                refresh_both(&mut state);
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
                handle_command_line(state, *key);
            }
            AppMode::Dialog(_) => {
                handle_dialog(state, viewer_state, running_job, key.code, terminal.size()?);
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
                handle_list_picker(state, key.code);
            }
            AppMode::DirectoryTree => {
                handle_directory_tree(state, viewer_state, key.code, terminal.size()?.height);
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

fn file_names_from_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            p.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| p.clone())
        })
        .collect()
}

fn sync_watcher_job_state(
    watcher: &Option<Watcher>,
    job_running: bool,
    watcher_paused: &mut bool,
) -> bool {
    let Some(watcher) = watcher.as_ref() else {
        return false;
    };

    if job_running && !*watcher_paused {
        watcher.pause();
        *watcher_paused = true;
        false
    } else if !job_running && *watcher_paused {
        watcher.resume();
        *watcher_paused = false;
        true
    } else {
        false
    }
}

pub(crate) fn refresh_panel(panel: &mut PanelState, visible_height: usize) {
    match reader::read_directory(&panel.path, panel.show_hidden) {
        Ok((entries, errors)) => {
            update_panel_read_errors(panel, &errors);
            let current_name = current_panel_entry_name(panel);
            let saved = selected_panel_paths(panel);
            let new_unfiltered = entries;
            let new_filtered = filtered_sorted_entries(
                &new_unfiltered,
                panel.filter.as_deref(),
                panel.sort_mode,
                panel.sort_options,
            );
            panel.unfiltered_entries = new_unfiltered;
            panel.entries = new_filtered;
            restore_panel_selection(panel, &saved);
            panel.recalculate_selection_stats();
            restore_panel_cursor(panel, current_name.as_deref());
            clamp_panel_scroll(panel, visible_height);
        }
        Err(e) => {
            panel.unfiltered_entries.clear();
            panel.entries.clear();
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.last_error = Some(e.to_string());
            panel.recalculate_selection_stats();
        }
    }
}

fn update_panel_read_errors(panel: &mut PanelState, errors: &[io::Error]) {
    if errors.is_empty() {
        panel.last_error = None;
    } else {
        let error_summary = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        panel.last_error = Some(format!(
            "{} file(s) failed to read: {error_summary}",
            errors.len()
        ));
    }
}

fn current_panel_entry_name(panel: &PanelState) -> Option<String> {
    panel
        .entries
        .get(panel.cursor)
        .filter(|e| e.name != "..")
        .map(|e| e.name.clone())
}

fn selected_panel_paths(panel: &PanelState) -> HashSet<PathBuf> {
    panel
        .entries
        .iter()
        .filter(|e| e.selected)
        .map(|e| e.path.clone())
        .collect()
}

fn filtered_sorted_entries(
    entries: &[reader::FileEntry],
    filter: Option<&str>,
    sort_mode: app::types::SortMode,
    sort_options: app::types::SortOptions,
) -> Vec<reader::FileEntry> {
    let mut sort_entries: Vec<reader::FileEntry> = entries
        .iter()
        .filter(|e| {
            if e.name == ".." {
                true
            } else if let Some(filter) = filter {
                ops::FileSearch::matches_pattern(&e.name, filter, false)
            } else {
                true
            }
        })
        .cloned()
        .collect();
    ops::sort_entries(&mut sort_entries, sort_mode, sort_options);
    sort_entries
}

fn restore_panel_selection(panel: &mut PanelState, saved: &HashSet<PathBuf>) {
    for entry in &mut panel.entries {
        entry.selected = saved.contains(&entry.path);
    }
}

fn restore_panel_cursor(panel: &mut PanelState, current_name: Option<&str>) {
    if let Some(name) = current_name
        && let Some(pos) = panel.entries.iter().position(|e| e.name == name)
    {
        panel.cursor = pos;
    }
    if panel.cursor >= panel.entries.len() && !panel.entries.is_empty() {
        panel.cursor = panel.entries.len() - 1;
    }
}

fn clamp_panel_scroll(panel: &mut PanelState, visible_height: usize) {
    let max_scroll = panel.entries.len().saturating_sub(1);
    if panel.scroll_offset > max_scroll {
        panel.scroll_offset = max_scroll;
    }
    if panel.scroll_offset > panel.cursor {
        panel.scroll_offset = panel.cursor;
    }
    panel.ensure_cursor_visible(visible_height);
}

fn current_visible_height() -> usize {
    crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(0)
}

pub(crate) fn refresh_active(state: &mut AppState) {
    let visible = current_visible_height();
    match state.active_panel {
        ActivePanel::Left => refresh_panel(&mut state.left_panel, visible),
        ActivePanel::Right => refresh_panel(&mut state.right_panel, visible),
    }
}

pub(crate) fn refresh_both(state: &mut AppState) {
    let visible = current_visible_height();
    refresh_panel(&mut state.left_panel, visible);
    refresh_panel(&mut state.right_panel, visible);
}

fn set_active_panel(state: &mut AppState, panel: ActivePanel) {
    state.active_panel = panel;
}

pub(crate) fn with_menu_panel<T>(state: &mut AppState, f: impl FnOnce(&mut AppState) -> T) -> T {
    let original = state.active_panel;
    match state.menu_selected {
        0 => set_active_panel(state, ActivePanel::Left),
        4 => set_active_panel(state, ActivePanel::Right),
        _ => {}
    }
    let result = f(state);
    if matches!(state.mode, AppMode::Dialog(_)) {
        state.menu_restore_panel = Some(original);
    } else {
        set_active_panel(state, original);
    }
    result
}

fn handle_directory_tree(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
) {
    let visible_height = directory_tree_visible_height(terminal_height);
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') if state.tree_selected > 0 => {
            state.tree_selected -= 1;
        }
        KeyCode::Down | KeyCode::Char('j')
            if !state.tree_entries.is_empty()
                && state.tree_selected + 1 < state.tree_entries.len() =>
        {
            state.tree_selected += 1;
        }
        KeyCode::Home => {
            state.tree_selected = 0;
            state.tree_scroll = 0;
        }
        KeyCode::End if !state.tree_entries.is_empty() => {
            state.tree_selected = state.tree_entries.len() - 1;
        }
        KeyCode::PageUp => {
            state.tree_selected = state.tree_selected.saturating_sub(visible_height);
            state.tree_scroll = state.tree_scroll.saturating_sub(visible_height);
        }
        KeyCode::PageDown if !state.tree_entries.is_empty() => {
            state.tree_selected =
                (state.tree_selected + visible_height).min(state.tree_entries.len() - 1);
            state.tree_scroll = state
                .tree_scroll
                .saturating_add(visible_height)
                .min(state.tree_entries.len().saturating_sub(visible_height));
        }
        KeyCode::Enter => {
            let selected = state.tree_selected;
            let is_dir = state.tree_entries.get(selected).is_some_and(|e| e.is_dir);
            let is_file = state.tree_entries.get(selected).is_some_and(|e| !e.is_dir);

            if is_dir {
                let show_hidden = state.active_panel().show_hidden;
                let diagnostics = dir_tree::toggle_expand_with_diagnostics(
                    &mut state.tree_entries,
                    selected,
                    show_hidden,
                );
                set_tree_diagnostic_status(&mut state.status_message, &diagnostics);
                // Clamp selection after toggle
                if state.tree_selected >= state.tree_entries.len() && !state.tree_entries.is_empty()
                {
                    state.tree_selected = state.tree_entries.len() - 1;
                }
            } else if is_file {
                let path = state.tree_entries[selected].path.clone();
                if let Ok(vs) = viewer::ViewerState::open(&path) {
                    *viewer_state = Some(vs);
                    state.prev_mode = Some(state.mode.clone());
                    state.mode = AppMode::Viewing;
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(entry) = state.tree_entries.get(state.tree_selected) {
                let target = if entry.is_dir {
                    entry.path.clone()
                } else {
                    entry
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default()
                };
                if !target.as_os_str().is_empty() && target.is_dir() {
                    state.active_panel_mut().path = target;
                    state.active_panel_mut().cursor = 0;
                    state.active_panel_mut().scroll_offset = 0;
                    refresh_active(state);
                    state.mode = AppMode::Normal;
                }
            }
        }
        _ => {}
    }

    let selected = state.tree_selected;
    let scroll = state.tree_scroll;
    let effective = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };
    state.tree_scroll = effective;
}

fn directory_tree_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(DIR_TREE_OVERHEAD_ROWS) as usize
}

pub(crate) fn set_tree_diagnostic_status(
    status_message: &mut Option<String>,
    diagnostics: &[dir_tree::TreeDiagnostic],
) {
    if diagnostics.is_empty() {
        return;
    }

    let first = &diagnostics[0];
    *status_message = Some(format!(
        "Directory tree warning: {}: {}{}",
        first.path.display(),
        first.message,
        if diagnostics.len() > 1 {
            format!(", {} more", diagnostics.len() - 1)
        } else {
            String::new()
        }
    ));
}

fn panel_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(LAYOUT_OVERHEAD_ROWS) as usize
}

fn navigate_to_hotlist(state: &mut AppState, index: usize) {
    if let Some(path) = state.directory_hotlist.get(index).cloned()
        && path.is_dir()
    {
        let panel = state.active_panel_mut();
        panel.path = path.clone();
        panel.cursor = 0;
        panel.scroll_offset = 0;
        refresh_active(state);
        state.status_message = Some(format!("cd to {}", path.display()));
    }
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
        refresh_active(state);
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
    let file_names = file_names_from_paths(&paths);
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
    let file_names = file_names_from_paths(&paths);
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
        refresh_active(state);
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
            refresh_active(state);
        }
        KeyCode::Char('r') => {
            refresh_active(state);
        }
        KeyCode::Char('o') => {
            if let Err(e) = shell::toggle_external_view(state, refresh_both) {
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
                refresh_active(state);
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
            navigate_to_hotlist(state, (c as usize) - ('1' as usize));
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
    let visible = panel_visible_height(terminal_height);
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
                refresh_active(state);
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
        let content_width = terminal_width.saturating_sub(2) as usize;
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

fn command_delete_word_backward(state: &mut AppState) {
    let cursor = state.command_cursor;
    if cursor > 0 {
        let text = &state.command_line[..cursor];
        let word_start = text
            .char_indices()
            .rev()
            .skip_while(|&(_, c)| c.is_whitespace())
            .find(|&(_, c)| c.is_whitespace())
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        state.command_line.drain(word_start..cursor);
        state.command_cursor = word_start;
        state.history_index = None;
    }
}

fn command_execute(state: &mut AppState) {
    let cmd = state.command_line.clone();
    state.mode = AppMode::Normal;
    state.command_line.clear();
    state.command_cursor = 0;
    state.history_index = None;
    if !cmd.is_empty() {
        shell::run_shell_command(state, &cmd, false, refresh_active);
    }
}

fn handle_command_line(state: &mut AppState, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => {
                state.command_cursor = 0;
                return;
            }
            KeyCode::Char('e') => {
                state.command_cursor = state.command_line.len();
                return;
            }
            KeyCode::Char('u') => {
                state.command_line.clear();
                state.command_cursor = 0;
                state.history_index = None;
                return;
            }
            KeyCode::Char('w') => {
                command_delete_word_backward(state);
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.command_cursor = 0;
            state.history_index = None;
        }
        KeyCode::Enter => {
            command_execute(state);
        }
        KeyCode::Backspace => {
            let cursor = state.command_cursor;
            if cursor > 0 {
                let prev = state.command_line[..cursor]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.command_line.drain(prev..cursor);
                state.command_cursor = prev;
                state.history_index = None;
            }
        }
        KeyCode::Left if state.command_cursor > 0 => {
            state.command_cursor = state.command_line[..state.command_cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
        KeyCode::Left => {}
        KeyCode::Right if state.command_cursor < state.command_line.len() => {
            state.command_cursor = state.command_line[state.command_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| state.command_cursor + i)
                .unwrap_or(state.command_line.len());
        }
        KeyCode::Right => {}
        KeyCode::Up if !state.command_history.is_empty() => {
            if state.history_index.is_none() {
                state.command_draft = state.command_line.clone();
            }
            let idx = match state.history_index {
                Some(i) if i > 0 => i - 1,
                Some(i) => i,
                None => state.command_history.len() - 1,
            };
            state.history_index = Some(idx);
            state.command_line = state.command_history[idx].clone();
            state.command_cursor = state.command_line.len();
        }
        KeyCode::Down => {
            if let Some(idx) = state.history_index {
                if idx + 1 < state.command_history.len() {
                    state.history_index = Some(idx + 1);
                    state.command_line = state.command_history[idx + 1].clone();
                } else {
                    state.history_index = None;
                    state.command_line = state.command_draft.clone();
                }
                state.command_cursor = state.command_line.len();
            }
        }
        KeyCode::Char(c) => {
            state.command_line.insert(state.command_cursor, c);
            state.command_cursor += c.len_utf8();
            state.history_index = None;
        }
        _ => {}
    }
}

fn parse_octal_mode(input: &str) -> Option<u32> {
    let mode = u32::from_str_radix(input.trim(), 8).ok()?;
    if mode <= 0o7777 { Some(mode) } else { None }
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

/// Shared helper to dismiss dialog and restore panel
fn dismiss_dialog_and_restore(state: &mut AppState) {
    state.mode = AppMode::Normal;
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

pub(crate) fn dismiss_dialog(state: &mut AppState) {
    state.mode = AppMode::Normal;
    state.pending_action = None;
    state.status_message = None;
    state.dialog_selection = 0;
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

#[cfg(unix)]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(src_meta) = std::fs::symlink_metadata(src) else {
        return false;
    };
    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        return false;
    };
    src_meta.dev() == dest_meta.dev() && src_meta.ino() == dest_meta.ino()
}

#[cfg(not(unix))]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    src == dest
}

fn check_overwrite_conflict(state: &AppState) -> Option<Vec<String>> {
    let action = state.pending_action.as_ref()?;
    let (sources, dest_dir, overwrite) = match action {
        app::types::PendingAction::Copy {
            sources,
            dest,
            overwrite,
        } => (sources, dest, *overwrite),
        app::types::PendingAction::Move {
            sources,
            dest,
            overwrite,
        } => (sources, dest, *overwrite),
        app::types::PendingAction::Delete { .. } => return None,
    };
    if overwrite {
        return None;
    }
    let conflicting: Vec<String> = sources
        .iter()
        .filter_map(|s| {
            let name = s.file_name()?;
            let target = dest_dir.join(name);
            if is_same_file(s, &target) {
                return None;
            }
            if std::fs::symlink_metadata(&target).is_ok() {
                Some(name.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    if conflicting.is_empty() {
        None
    } else {
        Some(conflicting)
    }
}

fn handle_confirm_dialog(state: &mut AppState, running_job: &mut Option<RunningJob>, key: KeyCode) {
    let confirmed = match key {
        KeyCode::Char('y' | 'Y') => Some(true),
        KeyCode::Char('n' | 'N') => Some(false),
        KeyCode::Enter => Some(state.dialog_selection == 0),
        KeyCode::Esc => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Left | KeyCode::Right => {
            state.dialog_selection = if state.dialog_selection == 0 { 1 } else { 0 };
            return;
        }
        _ => return,
    };

    if confirmed == Some(true) {
        if state.pending_action.is_some() {
            if let Some(conflicting) = check_overwrite_conflict(state) {
                state.dialog_selection = 0;
                state.mode =
                    AppMode::Dialog(app::types::DialogKind::OverwriteConfirm { conflicting });
                return;
            }
            start_confirmed_action(state, running_job);
            state.dialog_selection = 0;
            if state.status_message.is_some() {
                state.mode = AppMode::Normal;
                refresh_both(state);
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
        } else {
            dismiss_dialog(state);
            refresh_both(state);
        }
    } else {
        dismiss_dialog(state);
    }
}

fn handle_overwrite_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    match key {
        KeyCode::Esc => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Left => {
            state.dialog_selection = state.dialog_selection.saturating_sub(1);
            return;
        }
        KeyCode::Right => {
            state.dialog_selection = (state.dialog_selection + 1).min(1);
            return;
        }
        KeyCode::Char('o' | 'O') => {
            set_pending_overwrite(state);
        }
        KeyCode::Char('c' | 'C') => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Enter => match state.dialog_selection {
            0 => set_pending_overwrite(state),
            1 => {
                dismiss_dialog(state);
                return;
            }
            _ => return,
        },
        _ => return,
    }
    start_confirmed_action(state, running_job);
    state.dialog_selection = 0;
    if state.status_message.is_some() {
        state.mode = AppMode::Normal;
        refresh_both(state);
        if let Some(panel) = state.menu_restore_panel.take() {
            set_active_panel(state, panel);
        }
    }
}

fn set_pending_overwrite(state: &mut AppState) {
    if let Some(action) = state.pending_action.as_mut() {
        match action {
            app::types::PendingAction::Copy { overwrite, .. }
            | app::types::PendingAction::Move { overwrite, .. } => {
                *overwrite = true;
            }
            app::types::PendingAction::Delete { .. } => {}
        }
    }
}

fn handle_find_file(state: &mut AppState, input: &str, terminal_height: u16) {
    let dir = state.active_panel().path.clone();
    let outcome = ops::FileSearch::search_files_with_diagnostics(&dir, input, true, false);
    let result_count = outcome.matches.len();
    let error_count = outcome.errors.len();
    let truncated = outcome.truncated;
    if let Some(first) = outcome.matches.first()
        && let Some(parent) = first.path.parent()
    {
        state.active_panel_mut().path = parent.to_path_buf();
        refresh_active(state);
        if let Some(pos) = state
            .active_panel()
            .entries
            .iter()
            .position(|e| e.path == first.path)
        {
            state.active_panel_mut().cursor = pos;
            state
                .active_panel_mut()
                .ensure_cursor_visible(panel_visible_height(terminal_height));
        }
    }
    let mut message = if result_count > 0 {
        format!("Found {result_count} match(es) for '{input}'")
    } else {
        format!("No matches for '{input}'")
    };
    if error_count > 0 {
        message.push_str(&format!(", {error_count} error(s)"));
    }
    if let Some(reason) = truncated {
        let label = match reason {
            ops::TruncationReason::DepthLimit => "depth limit",
            ops::TruncationReason::ItemLimit => "item limit",
            ops::TruncationReason::ContentResultLimit => "result limit",
            ops::TruncationReason::FileTooLarge => "file too large",
            ops::TruncationReason::LineTooLong => "line too long",
            ops::TruncationReason::BinaryFile => "binary file",
        };
        message.push_str(&format!(", truncated ({label})"));
    }
    state.status_message = Some(message);
}

fn handle_quick_cd(state: &mut AppState, input: &str) {
    let expanded = fs::path::resolve_user_path(&state.active_panel().path, input);

    if expanded.is_dir() {
        let panel = state.active_panel_mut();
        panel.history.push(panel.path.clone());
        panel.path = expanded.clone();
        panel.cursor = 0;
        panel.scroll_offset = 0;
        refresh_active(state);
        if !state.directory_hotlist.iter().any(|p| p == &expanded) {
            state.directory_hotlist.push(expanded);
        }
    } else {
        state.status_message = Some(format!("Directory not found: {input}"));
    }
}

fn handle_input_action(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    action: &InputAction,
    terminal_height: u16,
) -> bool {
    let input = state.dialog_input.clone();
    match action {
        InputAction::ViewerSearch => {
            if let Some(vs) = viewer_state.as_mut() {
                vs.search(&input, terminal_height.saturating_sub(3) as usize);
            }
            state.mode = AppMode::Viewing;
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            return true;
        }
        InputAction::CreateDirectory if !input.trim().is_empty() => {
            if std::path::Path::new(&input)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                state.status_message = Some("Invalid path: '..' not allowed".to_string());
            } else {
                let target = fs::path::resolve_user_path(&state.active_panel().path, &input);
                if let Err(err) = ops::create_directory(&target) {
                    state.status_message = Some(format!("Create directory failed: {err}"));
                } else {
                    refresh_active(state);
                }
            }
        }
        InputAction::Rename if !input.is_empty() => {
            if let Some(entry) = state.active_panel().current_entry()
                && let Err(err) = ops::rename_entry(&entry.path, &input)
            {
                state.status_message = Some(format!("Rename failed: {err}"));
            }
        }
        InputAction::Chmod if !input.is_empty() => {
            if let Some(mode) = parse_octal_mode(&input) {
                if let Some(entry) = state.active_panel().current_entry()
                    && let Err(err) = ops::chmod(&entry.path, mode)
                {
                    state.status_message = Some(format!("Chmod failed: {err}"));
                }
            } else {
                state.status_message = Some(format!("Invalid octal mode '{input}'"));
            }
        }
        InputAction::Filter => {
            let panel = state.active_panel_mut();
            panel.filter = if input.trim().is_empty() {
                None
            } else {
                Some(input)
            };
        }
        InputAction::QuickCd => handle_quick_cd(state, &input),
        InputAction::FindFile => handle_find_file(state, &input, terminal_height),
        _ => {}
    }
    state.mode = AppMode::Normal;
    state.dialog_input.clear();
    state.dialog_cursor_pos = 0;
    refresh_active(state);
    if let Some(panel) = state.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
    false
}

fn handle_dialog_text_edit(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Backspace if state.dialog_cursor_pos > 0 => {
            state.dialog_cursor_pos -= 1;
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(state.dialog_input.len());
            let next_byte = state.dialog_input[byte_pos..]
                .chars()
                .next()
                .map(|c| byte_pos + c.len_utf8())
                .unwrap_or(state.dialog_input.len());
            state.dialog_input.drain(byte_pos..next_byte);
        }
        KeyCode::Delete => {
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i);
            if let Some(pos) = byte_pos {
                let next_char_end = state.dialog_input[pos..]
                    .chars()
                    .next()
                    .map(|c| pos + c.len_utf8())
                    .unwrap_or(state.dialog_input.len());
                state.dialog_input.drain(pos..next_char_end);
            }
        }
        KeyCode::Char(c) => {
            let byte_pos = state
                .dialog_input
                .char_indices()
                .nth(state.dialog_cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(state.dialog_input.len());
            state.dialog_input.insert(byte_pos, c);
            state.dialog_cursor_pos += 1;
        }
        KeyCode::Left if state.dialog_cursor_pos > 0 => {
            state.dialog_cursor_pos -= 1;
        }
        KeyCode::Right if state.dialog_cursor_pos < state.dialog_input.chars().count() => {
            state.dialog_cursor_pos += 1;
        }
        KeyCode::Home => {
            state.dialog_cursor_pos = 0;
        }
        KeyCode::End => {
            state.dialog_cursor_pos = state.dialog_input.chars().count();
        }
        _ => {}
    }
}

fn handle_input_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    action: &InputAction,
    key: KeyCode,
    terminal_height: u16,
) -> bool {
    match key {
        KeyCode::Enter => handle_input_action(state, viewer_state, action, terminal_height),
        KeyCode::Esc => {
            state.mode = if *action == InputAction::ViewerSearch {
                AppMode::Viewing
            } else {
                AppMode::Normal
            };
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
            false
        }
        _ => {
            handle_dialog_text_edit(state, key);
            false
        }
    }
}

/// Handle error dialog (dismiss on Enter/Esc)
fn handle_error_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

/// Handle progress dialog (cancel on Esc)
fn handle_progress_dialog(state: &mut AppState, running_job: &Option<RunningJob>, key: KeyCode) {
    if key == KeyCode::Esc
        && let Some(job) = running_job.as_ref()
    {
        job.cancel.store(true, Ordering::Relaxed);
        state.status_message = Some("Cancel requested".to_string());
    }
}

/// Handle properties dialog (dismiss on Enter/Esc)
fn handle_properties_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

fn handle_copymove_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    let confirmed = match key {
        KeyCode::Char('y' | 'Y') => Some(true),
        KeyCode::Char('n' | 'N') => Some(false),
        KeyCode::Enter => Some(state.dialog_selection == 0),
        KeyCode::Esc => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Left | KeyCode::Right => {
            state.dialog_selection = if state.dialog_selection == 0 { 1 } else { 0 };
            return;
        }
        _ => return,
    };

    if confirmed == Some(true) {
        let action = if let AppMode::Dialog(app::types::DialogKind::CopyMove {
            source,
            dest,
            is_move,
        }) = &state.mode
        {
            if *is_move {
                app::types::PendingAction::Move {
                    sources: source.clone(),
                    dest: dest.clone(),
                    overwrite: false,
                }
            } else {
                app::types::PendingAction::Copy {
                    sources: source.clone(),
                    dest: dest.clone(),
                    overwrite: false,
                }
            }
        } else {
            return;
        };
        state.pending_action = Some(action);
        if let Some(conflicting) = check_overwrite_conflict(state) {
            state.dialog_selection = 0;
            state.mode = AppMode::Dialog(app::types::DialogKind::OverwriteConfirm { conflicting });
            return;
        }
        start_confirmed_action(state, running_job);
        state.dialog_selection = 0;
        if state.status_message.is_some() {
            state.mode = AppMode::Normal;
            refresh_both(state);
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
        }
    } else {
        dismiss_dialog(state);
    }
}

fn handle_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
    terminal_size: Size,
) {
    // Handle Help dialog specially due to mutable scroll_offset
    if let AppMode::Dialog(app::types::DialogKind::Help {
        message,
        scroll_offset,
    }) = &mut state.mode
    {
        let total_lines = message.lines().count();
        let max_lines = dialogs::help_visible_height(Rect::new(
            0,
            0,
            terminal_size.width,
            terminal_size.height,
        ));
        let should_exit = match key {
            KeyCode::Up | KeyCode::Char('k') => {
                *scroll_offset = scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + 1).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::PageUp => {
                *scroll_offset = scroll_offset.saturating_sub(max_lines);
                false
            }
            KeyCode::PageDown => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + max_lines).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::Home => {
                *scroll_offset = 0;
                false
            }
            KeyCode::End => {
                if total_lines > max_lines {
                    *scroll_offset = total_lines - max_lines;
                }
                false
            }
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => true,
            _ => true, // Any other key exits
        };
        if should_exit {
            state.mode = AppMode::Normal;
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
        }
        return;
    }

    // Extract action early to avoid borrow issues
    let input_action =
        if let AppMode::Dialog(app::types::DialogKind::Input { ref action, .. }) = state.mode {
            Some(*action)
        } else {
            None
        };

    let dk = if let AppMode::Dialog(ref dk) = state.mode {
        dk
    } else {
        return;
    };

    match dk {
        app::types::DialogKind::Confirm(_) => {
            handle_confirm_dialog(state, running_job, key);
        }
        app::types::DialogKind::Input { .. } => {
            if let Some(action) = input_action {
                let _ =
                    handle_input_dialog(state, viewer_state, &action, key, terminal_size.height);
            }
        }
        app::types::DialogKind::Error(_) => {
            handle_error_dialog(state, key);
        }
        app::types::DialogKind::Progress(_, _) => {
            handle_progress_dialog(state, running_job, key);
        }
        app::types::DialogKind::Properties { .. } => {
            handle_properties_dialog(state, key);
        }
        app::types::DialogKind::CopyMove { .. } => {
            handle_copymove_dialog(state, running_job, key);
        }
        app::types::DialogKind::OverwriteConfirm { .. } => {
            handle_overwrite_dialog(state, running_job, key);
        }
        app::types::DialogKind::Help { .. } => {
            dismiss_dialog_and_restore(state);
        }
    }
}

fn handle_history_picker(state: &mut AppState, key: KeyCode, len: usize) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Enter => {
            let idx = len.saturating_sub(1).saturating_sub(state.picker_selected);
            if let Some(cmd) = state.command_history.get(idx).cloned() {
                state.command_cursor = cmd.len();
                state.command_line = cmd;
                state.mode = AppMode::CommandLine;
            } else {
                state.mode = AppMode::Normal;
            }
        }
        _ => {}
    }
}

fn handle_hotlist_picker(state: &mut AppState, key: KeyCode, len: usize) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Enter => {
            if let Some(path) = state.directory_hotlist.get(state.picker_selected).cloned() {
                if path.is_dir() {
                    state.active_panel_mut().path = path;
                    state.active_panel_mut().cursor = 0;
                    state.active_panel_mut().scroll_offset = 0;
                    refresh_active(state);
                } else {
                    state.status_message = Some("Hotlist entry no longer exists".to_string());
                }
                state.mode = AppMode::Normal;
            }
        }
        KeyCode::Char('a') => {
            let cur = state.active_panel().path.clone();
            if state.directory_hotlist.iter().any(|p| p == &cur) {
                state.status_message = Some("Directory already in hotlist".to_string());
            } else {
                state.directory_hotlist.push(cur);
                state.status_message = Some("Added current directory to hotlist".to_string());
            }
        }
        KeyCode::Char('d') if state.picker_selected < state.directory_hotlist.len() => {
            state.directory_hotlist.remove(state.picker_selected);
            if state.picker_selected > 0 && state.picker_selected >= state.directory_hotlist.len() {
                state.picker_selected -= 1;
            }
        }
        _ => {}
    }
}

fn handle_compare_mode_picker(state: &mut AppState, key: KeyCode) {
    const MODES: [CompareMode; 3] = [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
    let len = MODES.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Enter => {
            let chosen = MODES[state.picker_selected.min(len - 1)];
            state.mode = AppMode::Normal;
            compare_directories(state, chosen);
        }
        _ => {}
    }
}

fn handle_user_menu_picker(state: &mut AppState, key: KeyCode) {
    let len = state.user_menu_entries.len();
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up if len > 0 && state.picker_selected > 0 => {
            state.picker_selected -= 1;
        }
        KeyCode::Down if len > 0 && state.picker_selected + 1 < len => {
            state.picker_selected += 1;
        }
        KeyCode::Enter => {
            let idx = state.picker_selected.min(len.saturating_sub(1));
            state.mode = AppMode::Normal;
            if let Some(entry) = state.user_menu_entries.get(idx).cloned() {
                let active_dir = state.active_panel().path.clone();
                let other_dir = state.inactive_panel().path.clone();
                let current_file = state
                    .active_panel()
                    .current_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let tagged: Vec<PathBuf> = state
                    .active_panel()
                    .selected_entries()
                    .into_iter()
                    .filter(|e| e.name != "..")
                    .map(|e| e.path.clone())
                    .collect();
                let ctx = user_menu::SubstContext {
                    current_file: &current_file,
                    active_dir: &active_dir,
                    other_dir: &other_dir,
                    tagged: &tagged,
                };
                let cmd = user_menu::apply_substitutions(&entry.command, &ctx);
                shell::run_shell_command(state, &cmd, true, refresh_active);
            }
        }
        _ => {}
    }
}

fn handle_list_picker(state: &mut AppState, key: KeyCode) {
    let kind = if let AppMode::ListPicker(ref k) = state.mode {
        *k
    } else {
        return;
    };

    match kind {
        PickerKind::History => {
            handle_history_picker(state, key, state.command_history.len());
        }
        PickerKind::Hotlist => {
            handle_hotlist_picker(state, key, state.directory_hotlist.len());
        }
        PickerKind::CompareMode => {
            handle_compare_mode_picker(state, key);
        }
        PickerKind::UserMenu => {
            handle_user_menu_picker(state, key);
        }
    }
}

fn apply_search_filter(panel: &mut PanelState) {
    panel.sync_unfiltered_selection();
    panel.entries = filtered_sorted_entries(
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
            panel.entries = std::mem::take(&mut panel.unfiltered_entries);
            panel.cursor = 0;
            panel.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Enter => {
            state.mode = AppMode::Normal;
            state.search_query.clear();
            let panel = state.active_panel_mut();
            panel.unfiltered_entries.clear();
            refresh_active(state);
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

fn compare_directories(state: &mut AppState, mode: CompareMode) {
    let left_entries = if state.left_panel.unfiltered_entries.is_empty() {
        &state.left_panel.entries
    } else {
        &state.left_panel.unfiltered_entries
    };
    let right_entries = if state.right_panel.unfiltered_entries.is_empty() {
        &state.right_panel.entries
    } else {
        &state.right_panel.unfiltered_entries
    };
    let report = ops::compare_entries(left_entries, right_entries, mode);
    ops::apply_compare_to_panels(&mut state.left_panel, &mut state.right_panel, &report);

    let mode_name = match mode {
        CompareMode::Quick => "Quick",
        CompareMode::Size => "Size",
        CompareMode::Thorough => "Thorough",
    };
    state.status_message = None;
    state.dialog_selection = 0;
    state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(
        app::types::ConfirmDetails::simple(
            "Compare Results",
            &format!(
                "Compare dirs ({mode_name}):\nUnique in left:  {}\nUnique in right: {}\nDiffering:       {}",
                report.unique_left, report.unique_right, report.differing
            ),
        ),
    ));
}

// ---- Type conversion helpers ----

#[cfg(test)]
mod tests;
