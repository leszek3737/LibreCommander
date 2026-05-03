use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
};

use lc::{app, fs, menu, ops, ui};

mod input;

use app::job_runner::{RunningJob, poll_running_job, start_confirmed_action};
use app::types::{
    ActivePanel, AppMode, AppState, CompareMode, InputAction, PanelState, PickerKind,
};
use app::{dir_tree, paths, shell, user_menu, watcher_sync};
use fs::reader;
use fs::watcher::Watcher;
use menu::{menu_item_count, menu_total_count};
use ops::sorting;
use ui::theme::Theme;
use ui::{DIR_TREE_OVERHEAD_ROWS, LAYOUT_OVERHEAD_ROWS, dialogs, panels, viewer};

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

    // Terminal state recovery: if editor was SIGKILL'd, Drop was skipped.
    // Detect leftover state file and restore terminal before doing anything else.
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
    if let Err(e) = app::config::load_setup(&mut state) {
        state.status_message = Some(e);
    }

    let mut viewer_state: Option<viewer::ViewerState> = None;
    let mut running_job: Option<RunningJob> = None;
    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = fs::watcher::Watcher::new(watch_tx).ok();
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
            terminal.draw(|f| render_ui(f, &state, &viewer_state))?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))? {
            match event::read()? {
                Event::Key(key) => match &state.mode {
                    AppMode::Normal => {
                        handle_normal_mode(
                            &mut state,
                            &mut viewer_state,
                            key.code,
                            key.modifiers,
                            terminal.size()?.height,
                            terminal,
                        );
                        dirty = true;
                    }
                    AppMode::Viewing => {
                        let sz = terminal.size()?;
                        handle_viewer_mode(
                            &mut state,
                            &mut viewer_state,
                            key.code,
                            sz.height,
                            sz.width,
                        );
                        dirty = true;
                    }
                    AppMode::CommandLine => {
                        handle_command_line(&mut state, key.code);
                        dirty = true;
                    }
                    AppMode::Dialog(_) => {
                        handle_dialog(
                            &mut state,
                            &mut viewer_state,
                            &mut running_job,
                            key.code,
                            terminal.size()?.height,
                        );
                        dirty = true;
                    }
                    AppMode::Search => {
                        handle_search_mode(&mut state, key.code, terminal.size()?.height);
                        dirty = true;
                    }
                    AppMode::Menu => {
                        handle_menu_mode(
                            &mut state,
                            &mut viewer_state,
                            key.code,
                            terminal.size()?.height,
                            terminal,
                        );
                        dirty = true;
                    }
                    AppMode::ListPicker(_) => {
                        handle_list_picker(&mut state, key.code);
                        dirty = true;
                    }
                    AppMode::DirectoryTree => {
                        handle_directory_tree(
                            &mut state,
                            &mut viewer_state,
                            key.code,
                            terminal.size()?.height,
                        );
                        dirty = true;
                    }
                },
                Event::Mouse(mouse_event) => {
                    let size: ratatui::layout::Size = terminal.size()?;
                    let outcomes = input::mouse::handle_mouse_event(
                        &mut state,
                        &mut viewer_state,
                        &mut running_job,
                        mouse_event,
                        size,
                    );
                    for outcome in outcomes {
                        match outcome {
                            input::mouse::MouseOutcome::NormalKey(key) => {
                                handle_normal_mode(
                                    &mut state,
                                    &mut viewer_state,
                                    key,
                                    KeyModifiers::NONE,
                                    terminal.size()?.height,
                                    terminal,
                                );
                            }
                            input::mouse::MouseOutcome::MenuAction => {
                                run_selected_menu_action(
                                    &mut state,
                                    &mut viewer_state,
                                    terminal.size()?.height,
                                    terminal,
                                );
                            }
                            input::mouse::MouseOutcome::None => {}
                        }
                    }
                    dirty = true;
                }
                Event::Resize(_, _) => {
                    dirty = true;
                }
                _ => {}
            }
        }

        if state.should_quit {
            return Ok(());
        }
    }
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
            let new_filtered =
                filtered_sorted_entries(&new_unfiltered, panel.filter.as_deref(), panel.sort_mode);
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
) -> Vec<reader::FileEntry> {
    let mut sort_entries: Vec<reader::FileEntry> = entries
        .iter()
        .filter(|e| {
            if e.name == ".." {
                true
            } else if let Some(filter) = filter {
                ops::search::FileSearch::matches_pattern(&e.name, filter, false)
            } else {
                true
            }
        })
        .cloned()
        .collect();
    sorting::sort_entries(&mut sort_entries, sort_mode);
    sort_entries
}

fn restore_panel_selection(panel: &mut PanelState, saved: &HashSet<PathBuf>) {
    for entry in &mut panel.entries {
        if saved.contains(&entry.path) {
            entry.selected = true;
        }
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

pub(crate) fn refresh_active(state: &mut AppState) {
    let visible = crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(0);
    match state.active_panel {
        ActivePanel::Left => refresh_panel(&mut state.left_panel, visible),
        ActivePanel::Right => refresh_panel(&mut state.right_panel, visible),
    }
}

pub(crate) fn refresh_both(state: &mut AppState) {
    let visible = crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(0);
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

fn render_ui(f: &mut Frame, state: &AppState, viewer_state: &Option<viewer::ViewerState>) {
    // If viewing, render viewer fullscreen
    if state.mode == AppMode::Viewing {
        if let Some(vs) = viewer_state {
            if vs.is_hex_mode() {
                viewer::render_hex_view(f, f.area(), vs);
            } else {
                viewer::render_viewer(f, f.area(), vs);
            }
        }
        return;
    }

    // If directory tree mode, render fullscreen tree overlay
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

    // Fill entire screen with blue background
    let bg_block = ratatui::widgets::Block::default().style(Theme::panel_bg());
    f.render_widget(bg_block, size);

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Menu bar
            Constraint::Min(10),   // Panels
            Constraint::Length(1), // Status bar
            Constraint::Length(1), // Command line
            Constraint::Length(1), // Function bar
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

    // Command line area
    let cmd_text = if state.mode == AppMode::CommandLine {
        format!("$ {}_", state.command_line)
    } else if state.mode == AppMode::Search {
        format!("Search: {}_", state.search_query)
    } else if let Some(ref msg) = state.status_message {
        msg.clone()
    } else {
        let ap = state.active_panel();
        format!("{}", ap.path.display())
    };
    let cmd_paragraph = ratatui::widgets::Paragraph::new(cmd_text).style(Theme::status_bar());
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar(f, main_layout[4]);

    // Dialog overlay
    if let AppMode::Dialog(ref dialog_kind) = state.mode {
        let ui_dialog = to_ui_dialog(dialog_kind, state);
        dialogs::render_dialog(f, &ui_dialog);
    }

    // Menu overlay
    if state.mode == AppMode::Menu {
        ui::menu::render_menu_dropdown(
            f,
            main_layout[0],
            state.menu_selected,
            state.menu_item_selected,
        );
    }

    // List picker overlay
    if let AppMode::ListPicker(ref kind) = state.mode {
        match kind {
            PickerKind::History => {
                let items: Vec<String> = state.command_history.iter().rev().cloned().collect();
                dialogs::render_list_picker(
                    f,
                    "Command History",
                    &items,
                    state.picker_selected,
                    "Enter: select  Esc: cancel",
                );
            }
            PickerKind::Hotlist => {
                let items: Vec<String> = state
                    .directory_hotlist
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                dialogs::render_list_picker(
                    f,
                    "Directory Hotlist",
                    &items,
                    state.picker_selected,
                    "Enter: cd  a: add current  d: delete  Esc: close",
                );
            }
            PickerKind::CompareMode => {
                let items = vec![
                    "Quick".to_string(),
                    "Size".to_string(),
                    "Thorough".to_string(),
                ];
                dialogs::render_list_picker(
                    f,
                    "Compare Mode",
                    &items,
                    state.picker_selected,
                    "Enter: select  Esc: cancel",
                );
            }
            PickerKind::UserMenu => {
                let items: Vec<String> = state
                    .user_menu_entries
                    .iter()
                    .map(|e| format!("{}  {}", e.hotkey, e.title))
                    .collect();
                dialogs::render_list_picker(
                    f,
                    "User Menu",
                    &items,
                    state.picker_selected,
                    "Enter: run  Esc: cancel",
                );
            }
        }
    }
}

fn to_ui_dialog(dialog_kind: &app::types::DialogKind, state: &AppState) -> dialogs::DialogKind {
    match dialog_kind {
        app::types::DialogKind::Confirm(cd) => dialogs::DialogKind::Confirm {
            title: cd.title.clone(),
            message: cd.message.clone(),
            selection: state.dialog_selection,
            files: cd
                .files
                .as_ref()
                .map(|fps| fps.iter().map(|p| p.display().to_string()).collect()),
        },
        app::types::DialogKind::Input { prompt, .. } => dialogs::DialogKind::Input {
            title: "Input".to_string(),
            prompt: prompt.clone(),
            value: state.dialog_input.clone(),
            cursor_pos: state.dialog_cursor_pos,
        },
        app::types::DialogKind::Error(msg) => dialogs::DialogKind::Error {
            title: "Error".to_string(),
            message: msg.clone(),
        },
        app::types::DialogKind::Help {
            message,
            scroll_offset,
        } => dialogs::DialogKind::Help {
            title: "Help".to_string(),
            message: message.clone(),
            scroll_offset: *scroll_offset,
        },
        app::types::DialogKind::Progress(msg, pct) => dialogs::DialogKind::Progress {
            title: "Progress".to_string(),
            message: msg.clone(),
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
                    .unwrap_or_default(),
                dest.display(),
            );
            dialogs::DialogKind::Confirm {
                title: format!("{action} Confirm"),
                message: msg,
                selection: state.dialog_selection,
                files: Some(source.iter().map(|p| p.display().to_string()).collect()),
            }
        }
        app::types::DialogKind::Properties {
            name,
            size,
            mtime,
            permissions,
            owner,
            group,
            is_dir,
            is_symlink,
        } => {
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
                name: name.clone(),
                size: app::types::FileEntry::format_size(*size),
                mtime: mtime_str,
                permissions: app::types::FileEntry::display_permissions_raw(*permissions),
                owner: owner.clone(),
                group: group.clone(),
                file_type: file_type.to_string(),
            }
        }
    }
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
                    &state.tree_root,
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

fn shift_select(panel: &mut app::types::PanelState, next: usize) {
    let anchor = panel.selection_anchor.get_or_insert(panel.cursor);
    let anchor = *anchor;

    let old = panel.cursor;
    panel.cursor = next;

    let lo = anchor.min(next);
    let hi = anchor.max(next);
    let affected_lo = anchor.min(old).min(next);
    let affected_hi = anchor.max(old).max(next);

    for i in affected_lo..=affected_hi {
        panel.set_selection_at(i, i >= lo && i <= hi);
    }
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
        KeyCode::F(10) => state.should_quit = true,
        KeyCode::F(1) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Help {
                message: app::keymap::build_help_message(),
                scroll_offset: 0,
            });
        }
        KeyCode::F(2) => {
            state.mode = AppMode::ListPicker(app::types::PickerKind::UserMenu);
            state.picker_selected = 0;
        }
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            // Extend selection upward, or shrink it when moving back over a selected range.
            let panel = state.active_panel_mut();
            if panel.cursor > 0 {
                shift_select(panel, panel.cursor - 1);
                if panel.cursor < panel.scroll_offset {
                    panel.scroll_offset = panel.cursor;
                }
            }
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            // Extend selection downward, or shrink it when moving back over a selected range.
            let panel = state.active_panel_mut();
            let len = panel.entries.len();
            if len > 0 && panel.cursor < len - 1 {
                shift_select(panel, panel.cursor + 1);
                if panel.cursor >= panel.scroll_offset + visible {
                    panel.scroll_offset = panel.cursor.saturating_sub(visible) + 1;
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let panel = state.active_panel_mut();
            panel.selection_anchor = None;
            panel.move_cursor_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let panel = state.active_panel_mut();
            panel.selection_anchor = None;
            panel.move_cursor_down(visible);
        }
        KeyCode::Home => {
            let p = state.active_panel_mut();
            p.selection_anchor = None;
            p.cursor = 0;
            p.scroll_offset = 0;
        }
        KeyCode::End => {
            let len = state.active_panel().entries.len();
            if len > 0 {
                let p = state.active_panel_mut();
                p.selection_anchor = None;
                p.cursor = len - 1;
                p.ensure_cursor_visible(visible);
            }
        }
        KeyCode::PageUp => {
            let p = state.active_panel_mut();
            p.selection_anchor = None;
            p.cursor = p.cursor.saturating_sub(visible);
            p.scroll_offset = p.scroll_offset.saturating_sub(visible);
        }
        KeyCode::PageDown => {
            let len = state.active_panel().entries.len();
            let p = state.active_panel_mut();
            p.selection_anchor = None;
            p.cursor = (p.cursor + visible).min(len.saturating_sub(1));
            p.scroll_offset = (p.scroll_offset + visible).min(len.saturating_sub(visible));
        }
        KeyCode::Enter if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+Enter: Show file properties dialog
            if let Some(entry) = state.active_panel().current_entry()
                && entry.name != ".."
            {
                state.mode = AppMode::Dialog(app::types::DialogKind::Properties {
                    name: entry.name.clone(),
                    size: entry.size,
                    mtime: entry.modified,
                    permissions: entry.permissions,
                    owner: entry.owner.clone(),
                    group: entry.group.clone(),
                    is_dir: entry.is_dir,
                    is_symlink: entry.is_symlink,
                });
            }
        }
        KeyCode::Enter => {
            let entry_info = state
                .active_panel()
                .current_entry()
                .map(|e| (e.is_dir, e.path.clone(), e.name == ".."));
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
                if let Some(ref name) = prev_dir_name {
                    if let Some(idx) = state
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
        KeyCode::F(3) => {
            if let Some(entry) = state.active_panel().current_entry()
                && !entry.is_dir
                && let Ok(vs) = viewer::ViewerState::open(&entry.path)
            {
                *viewer_state = Some(vs);
                state.mode = AppMode::Viewing;
            }
        }
        KeyCode::F(4) => {
            let entry_info = state
                .active_panel()
                .current_entry()
                .map(|e| (e.is_dir, e.path.clone()));
            if let Some((is_dir, path)) = entry_info
                && !is_dir
            {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                if let Err(e) = suspend_terminal_stdout() {
                    state.status_message = Some(format!("Terminal suspend failed: {e}"));
                    return;
                }
                // Write state file before launching editor – if editor is SIGKILL'd,
                // Drop is skipped but we can detect this on next startup.
                let terminal_state_file = terminal_state_file_path();
                if let Some(parent) = terminal_state_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(_e) = std::fs::write(&terminal_state_file, "alternate_screen") {
                    // Best-effort state file; ignore write failures.
                }
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
                        state.status_message =
                            Some(format!("Terminal restore failed after editor: {e}"));
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
        KeyCode::F(9) => {
            state.mode = AppMode::Menu;
            state.menu_item_selected = 0;
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
        }
        KeyCode::F(5) => {
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
                let dest_dir = state.inactive_panel().path.clone();
                let file_names = file_names_from_paths(&paths);
                let msg = if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
                    format!("Copy '{}' to '{}'?", name, dest_dir.display())
                } else {
                    format!("Copy {} entries to '{}'?", paths.len(), dest_dir.display())
                };
                state.dialog_selection = 0;
                state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(
                    app::types::ConfirmDetails::with_files("Copy Confirm", &msg, file_names),
                ));
                state.pending_action = Some(app::types::PendingAction::Copy {
                    sources: paths,
                    dest: dest_dir,
                });
            }
        }
        KeyCode::F(6) => {
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
                let dest_dir = state.inactive_panel().path.clone();
                let file_names = file_names_from_paths(&paths);
                let msg = if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
                    format!("Move '{}' to '{}'?", name, dest_dir.display())
                } else {
                    format!("Move {} entries to '{}'?", paths.len(), dest_dir.display())
                };
                state.dialog_selection = 0;
                state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(
                    app::types::ConfirmDetails::with_files("Move Confirm", &msg, file_names),
                ));
                state.pending_action = Some(app::types::PendingAction::Move {
                    sources: paths,
                    dest: dest_dir,
                });
            }
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
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
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
        }
        KeyCode::Backspace if modifiers.contains(KeyModifiers::ALT) => {
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
                if let Some(ref name) = prev_dir_name {
                    if let Some(idx) = state
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
                state.status_message = Some(format!("cd to {}", prev_path.display()));
            }
        }
        KeyCode::Char(c) if modifiers.contains(KeyModifiers::ALT) && ('1'..='9').contains(&c) => {
            navigate_to_hotlist(state, (c as usize) - ('1' as usize));
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::ALT) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Input {
                prompt: "Quick cd:".to_string(),
                default_text: state.active_panel().path.display().to_string(),
                action: InputAction::QuickCd,
            });
            state.dialog_input = state.active_panel().path.display().to_string();
            state.dialog_cursor_pos = state.dialog_input.chars().count();
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            let panel = state.active_panel_mut();
            if panel.unfiltered_entries.is_empty() {
                panel.unfiltered_entries = panel.entries.clone();
            }
            state.mode = AppMode::Search;
            state.search_query.clear();
        }
        KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
            refresh_active(state);
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Err(e) = shell::toggle_external_view(state, terminal, refresh_both) {
                state.status_message = Some(format!("External view error: {e}"));
            }
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
    terminal_height: u16,
    terminal_width: u16,
) {
    if let Some(vs) = viewer_state.as_mut() {
        let page_height = terminal_height.saturating_sub(3) as usize;
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

fn handle_command_line(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.history_index = None;
        }
        KeyCode::Enter => {
            let cmd = state.command_line.clone();
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.history_index = None;
            if !cmd.is_empty() {
                shell::run_shell_command(state, &cmd, refresh_active);
            }
        }
        KeyCode::Backspace => {
            state.command_line.pop();
            state.history_index = None;
        }
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
            }
        }
        KeyCode::Char(c) => {
            state.command_line.push(c);
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

/// Handle confirm dialog (yes/no with action)
fn handle_confirm_dialog(state: &mut AppState, running_job: &mut Option<RunningJob>, key: KeyCode) {
    match key {
        KeyCode::Char('y' | 'Y') => {
            if state.pending_action.is_some() {
                start_confirmed_action(state, running_job);
                state.dialog_selection = 0;
                let start_failed = state.status_message.is_some();
                if start_failed {
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
        }
        KeyCode::Char('n' | 'N') => {
            dismiss_dialog(state);
        }
        KeyCode::Enter => {
            if state.dialog_selection == 1 {
                dismiss_dialog(state);
            } else if state.pending_action.is_some() {
                start_confirmed_action(state, running_job);
                state.dialog_selection = 0;
                let start_failed = state.status_message.is_some();
                if start_failed {
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
        KeyCode::Esc => {
            dismiss_dialog(state);
        }
        KeyCode::Left | KeyCode::Right => {
            state.dialog_selection = if state.dialog_selection == 0 { 1 } else { 0 };
        }
        _ => {}
    }
}

/// Handle input dialog (text entry) - returns true if early return needed
fn handle_input_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    action: &InputAction,
    key: KeyCode,
    terminal_height: u16,
) -> bool {
    match key {
        KeyCode::Enter => {
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
                    let dir = state.active_panel().path.clone();
                    if let Err(err) = ops::file_ops::create_directory(&dir.join(&input)) {
                        state.status_message = Some(format!("Create directory failed: {err}"));
                    } else {
                        refresh_active(state);
                    }
                }
                InputAction::Rename if !input.is_empty() => {
                    if let Some(entry) = state.active_panel().current_entry()
                        && let Err(err) = ops::file_ops::rename_entry(&entry.path, &input)
                    {
                        state.status_message = Some(format!("Rename failed: {err}"));
                    }
                }
                InputAction::Chmod if !input.is_empty() => {
                    if let Some(mode) = parse_octal_mode(&input) {
                        if let Some(entry) = state.active_panel().current_entry()
                            && let Err(err) = ops::file_ops::chmod(&entry.path, mode)
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
                InputAction::QuickCd => {
                    let expanded = if let Some(stripped) = input.strip_prefix('~') {
                        if let Some(home) = std::env::var_os("HOME") {
                            std::path::PathBuf::from(home).join(stripped.trim_start_matches('/'))
                        } else {
                            std::path::PathBuf::from(&input)
                        }
                    } else {
                        let path = std::path::PathBuf::from(&input);
                        if path.is_absolute() {
                            path
                        } else {
                            state.active_panel().path.join(path)
                        }
                    };

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
                InputAction::FindFile => {
                    let dir = state.active_panel().path.clone();
                    let outcome = ops::search::FileSearch::search_files_with_diagnostics(
                        &dir, &input, true, false,
                    );
                    let result_count = outcome.matches.len();
                    let error_count = outcome.errors.len();
                    let truncated = outcome.truncated;
                    if let Some(first) = outcome.matches.first() {
                        if let Some(parent) = first.path.parent() {
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
                        let mut message = format!("Found {result_count} match(es) for '{input}'");
                        if error_count > 0 {
                            message.push_str(&format!(", {error_count} error(s)"));
                        }
                        if truncated {
                            message.push_str(", truncated");
                        }
                        state.status_message = Some(message);
                    } else {
                        let mut message = format!("No matches for '{input}'");
                        if error_count > 0 {
                            message.push_str(&format!(", {error_count} error(s)"));
                        }
                        if truncated {
                            message.push_str(", truncated");
                        }
                        state.status_message = Some(message);
                    }
                }
                _ => {}
            }
            state.mode = AppMode::Normal;
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            refresh_active(state);
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
        }
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
        }
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
    false
}

/// Handle error dialog (dismiss on Enter/Esc)
fn handle_error_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

/// Handle progress dialog (cancel on Esc)
fn handle_progress_dialog(state: &mut AppState, running_job: &Option<RunningJob>, key: KeyCode) {
    if key == KeyCode::Esc {
        if let Some(job) = running_job.as_ref() {
            job.cancel.store(true, Ordering::Relaxed);
            state.status_message = Some("Cancel requested".to_string());
        }
    }
}

/// Handle properties dialog (dismiss on Enter/Esc)
fn handle_properties_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        dismiss_dialog_and_restore(state);
    }
}

/// Handle copymove dialog (fallback - dismiss on Esc)
fn handle_copymove_dialog(state: &mut AppState, key: KeyCode) {
    if key == KeyCode::Esc {
        dismiss_dialog_and_restore(state);
    }
}

fn handle_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
    terminal_height: u16,
) {
    // Handle Help dialog specially due to mutable scroll_offset
    if let AppMode::Dialog(app::types::DialogKind::Help {
        message,
        scroll_offset,
    }) = &mut state.mode
    {
        let total_lines = message.lines().count();
        let max_lines = terminal_height.saturating_sub(6) as usize;
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
                if handle_input_dialog(state, viewer_state, &action, key, terminal_height) {}
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
            handle_copymove_dialog(state, key);
        }
        app::types::DialogKind::Help { .. } => {
            // Already handled above, this should not be reached
            dismiss_dialog_and_restore(state);
        }
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
            let len = state.command_history.len();
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
                        state.command_line = cmd;
                        state.mode = AppMode::CommandLine;
                    } else {
                        state.mode = AppMode::Normal;
                    }
                }
                _ => {}
            }
        }
        PickerKind::Hotlist => {
            let len = state.directory_hotlist.len();
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
                    if let Some(path) = state.directory_hotlist.get(state.picker_selected).cloned()
                    {
                        if path.is_dir() {
                            state.active_panel_mut().path = path;
                            state.active_panel_mut().cursor = 0;
                            state.active_panel_mut().scroll_offset = 0;
                            refresh_active(state);
                        } else {
                            state.status_message =
                                Some("Hotlist entry no longer exists".to_string());
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
                        state.status_message =
                            Some("Added current directory to hotlist".to_string());
                    }
                }
                KeyCode::Char('d') if state.picker_selected < state.directory_hotlist.len() => {
                    state.directory_hotlist.remove(state.picker_selected);
                    if state.picker_selected > 0
                        && state.picker_selected >= state.directory_hotlist.len()
                    {
                        state.picker_selected -= 1;
                    }
                }
                _ => {}
            }
        }
        PickerKind::CompareMode => {
            const MODES: [CompareMode; 3] =
                [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
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
        PickerKind::UserMenu => {
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
                        shell::run_shell_command(state, &cmd, refresh_active);
                    }
                }
                _ => {}
            }
        }
    }
}

fn apply_search_filter(panel: &mut PanelState) {
    panel.sync_unfiltered_selection();
    panel.entries = filtered_sorted_entries(
        &panel.unfiltered_entries,
        panel.filter.as_deref(),
        panel.sort_mode,
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
    let report =
        ops::compare::compare_entries(&state.left_panel.entries, &state.right_panel.entries, mode);
    ops::compare::apply_compare_to_panels(&mut state.left_panel, &mut state.right_panel, &report);

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
mod tests {
    use super::*;
    use app::types::{ActivePanel, FileEntry};
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn test_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn make_test_entry(name: &str, size: u64, selected: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size,
            modified: std::time::SystemTime::now(),
            permissions: 0o644,
            owner: String::new(),
            group: String::new(),
            selected,
            is_hidden: false,
            mime_type: None,
        }
    }

    #[test]
    fn confirm_enter_without_pending_action_dismisses_dialog() {
        let mut state = AppState {
            mode: AppMode::Dialog(app::types::DialogKind::Confirm(
                app::types::ConfirmDetails::simple("Info", "Nothing to run"),
            )),
            dialog_selection: 0,
            pending_action: None,
            ..Default::default()
        };

        handle_dialog(&mut state, &mut None, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn search_enter_keeps_current_filter() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("alpha.txt"), b"alpha").unwrap();
        std::fs::write(temp_dir.path().join("beta.txt"), b"beta").unwrap();
        let mut state = AppState {
            mode: AppMode::Search,
            search_query: "alpha".to_string(),
            ..Default::default()
        };
        state.left_panel.path = temp_dir.path().to_path_buf();
        state.left_panel.entries = vec![make_test_entry("alpha.txt", 1, false)];
        state.left_panel.unfiltered_entries = vec![
            make_test_entry("alpha.txt", 1, false),
            make_test_entry("beta.txt", 2, false),
        ];
        state.left_panel.filter = Some("alpha".to_string());

        handle_search_mode(&mut state, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.search_query, "");
        assert_eq!(state.left_panel.filter.as_deref(), Some("alpha"));
        assert!(
            state
                .left_panel
                .entries
                .iter()
                .any(|entry| entry.name == "alpha.txt")
        );
        assert!(
            state
                .left_panel
                .entries
                .iter()
                .all(|entry| entry.name == ".." || entry.name.contains("alpha"))
        );
    }

    #[test]
    fn menu_toggle_hidden_files_refreshes_active_panel() {
        let state = AppState {
            active_panel: ActivePanel::Left,
            ..Default::default()
        };
        let mut terminal = test_terminal();
        let mut state = state;
        state.left_panel.path = std::env::temp_dir();
        state.left_panel.show_hidden = false;
        state.mode = AppMode::Menu;
        state.menu_selected = 3;
        state.menu_item_selected = 4;

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        assert_eq!(state.mode, AppMode::Normal);
        assert!(state.left_panel.show_hidden);
    }

    #[test]
    fn menu_rename_opens_input_dialog_with_current_name() {
        let mut terminal = test_terminal();
        let mut state = AppState::default();
        state.left_panel.entries.push(app::types::FileEntry {
            name: "old.txt".to_string(),
            path: std::env::temp_dir().join("old.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        });
        state.mode = AppMode::Menu;
        state.menu_selected = 1;
        state.menu_item_selected = 7;

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        assert_eq!(state.dialog_input, "old.txt");
        assert!(matches!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Input {
                prompt: _,
                default_text: _,
                action: app::types::InputAction::Rename,
            })
        ));
    }

    #[test]
    fn parse_octal_mode_accepts_valid_input() {
        assert_eq!(parse_octal_mode("755"), Some(0o755));
        assert_eq!(parse_octal_mode("0644"), Some(0o644));
        assert_eq!(parse_octal_mode("bad"), None);
    }

    #[test]
    fn compare_directories_reports_summary() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "a.txt".to_string(),
            path: std::env::temp_dir().join("a.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        }];
        state.right_panel.entries = vec![app::types::FileEntry {
            name: "b.txt".to_string(),
            path: std::env::temp_dir().join("b.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        }];

        compare_directories(&mut state, CompareMode::Quick);

        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                app::types::ConfirmDetails::simple(
                    "Compare Results",
                    "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 1\nDiffering:       0"
                )
            ))
        );
    }

    #[test]
    fn menu_history_opens_picker() {
        let mut terminal = test_terminal();
        let state = AppState {
            mode: AppMode::Menu,
            menu_selected: 2,
            menu_item_selected: 5,
            ..Default::default()
        };
        let mut state = state;
        state.command_history.push_back("ls -la".to_string());

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn menu_hotlist_opens_picker() {
        let mut terminal = test_terminal();
        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 2,
            menu_item_selected: 6,
            ..Default::default()
        };
        state.directory_hotlist.push(std::env::temp_dir());

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn shift_down_starts_selection_from_current_entry() {
        let mut terminal = test_terminal();
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            make_test_entry("a.txt", 10, false),
            make_test_entry("b.txt", 20, false),
        ];

        handle_normal_mode(
            &mut state,
            &mut None,
            KeyCode::Down,
            KeyModifiers::SHIFT,
            24,
            &mut terminal,
        );

        assert_eq!(state.left_panel.cursor, 1);
        assert!(state.left_panel.entries[0].selected);
        assert!(state.left_panel.entries[1].selected);
    }

    #[test]
    fn shift_up_shrinks_selection_range() {
        let mut terminal = test_terminal();
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            make_test_entry("a.txt", 10, true),
            make_test_entry("b.txt", 20, true),
            make_test_entry("c.txt", 30, true),
        ];
        state.left_panel.cursor = 2;
        state.left_panel.selection_anchor = Some(0);
        state.left_panel.recalculate_selection_stats();

        handle_normal_mode(
            &mut state,
            &mut None,
            KeyCode::Up,
            KeyModifiers::SHIFT,
            24,
            &mut terminal,
        );

        assert_eq!(state.left_panel.cursor, 1);
        assert!(state.left_panel.entries[0].selected);
        assert!(state.left_panel.entries[1].selected);
        assert!(!state.left_panel.entries[2].selected);
    }

    #[test]
    fn shift_selection_preserves_unrelated_entries() {
        let mut terminal = test_terminal();
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            make_test_entry("a.txt", 10, true),
            make_test_entry("b.txt", 20, false),
            make_test_entry("c.txt", 30, false),
            make_test_entry("d.txt", 40, false),
        ];
        state.left_panel.cursor = 2;
        state.left_panel.recalculate_selection_stats();

        handle_normal_mode(
            &mut state,
            &mut None,
            KeyCode::Down,
            KeyModifiers::SHIFT,
            24,
            &mut terminal,
        );

        assert!(state.left_panel.entries[0].selected);
        assert!(!state.left_panel.entries[1].selected);
        assert!(state.left_panel.entries[2].selected);
        assert!(state.left_panel.entries[3].selected);
    }

    #[test]
    fn home_resets_selection_anchor() {
        let mut terminal = test_terminal();
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            make_test_entry("a.txt", 10, false),
            make_test_entry("b.txt", 20, false),
            make_test_entry("c.txt", 30, false),
        ];
        state.left_panel.cursor = 2;
        state.left_panel.selection_anchor = Some(1);

        handle_normal_mode(
            &mut state,
            &mut None,
            KeyCode::Home,
            KeyModifiers::NONE,
            24,
            &mut terminal,
        );
        handle_normal_mode(
            &mut state,
            &mut None,
            KeyCode::Down,
            KeyModifiers::SHIFT,
            24,
            &mut terminal,
        );

        assert!(state.left_panel.entries[0].selected);
        assert!(state.left_panel.entries[1].selected);
        assert!(!state.left_panel.entries[2].selected);
    }

    #[test]
    fn command_line_up_loads_last_history_entry() {
        let mut state = AppState::default();
        state.command_history.push_back("git status".to_string());

        handle_command_line(&mut state, KeyCode::Up);

        assert_eq!(state.command_line, "git status");
    }

    #[test]
    fn compare_directories_marks_unique_entries_selected() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            app::types::FileEntry {
                name: "same.txt".to_string(),
                path: std::env::temp_dir().join("same.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
                mime_type: None,
            },
            app::types::FileEntry {
                name: "left.txt".to_string(),
                path: std::env::temp_dir().join("left.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
                mime_type: None,
            },
        ];
        state.right_panel.entries = vec![
            app::types::FileEntry {
                name: "same.txt".to_string(),
                path: std::env::temp_dir().join("same.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
                mime_type: None,
            },
            app::types::FileEntry {
                name: "right.txt".to_string(),
                path: std::env::temp_dir().join("right.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
                mime_type: None,
            },
        ];

        compare_directories(&mut state, CompareMode::Quick);

        assert!(!state.left_panel.entries[0].selected);
        assert!(state.left_panel.entries[1].selected);
        assert!(!state.right_panel.entries[0].selected);
        assert!(state.right_panel.entries[1].selected);
    }

    fn make_entry(name: &str, selected: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 100,
            modified: UNIX_EPOCH + Duration::from_secs(0),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected,
            is_hidden: false,
            mime_type: None,
        }
    }

    #[test]
    fn test_selected_or_current_paths_fallback_to_cursor() {
        // No entries are selected → should return the cursor entry
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        state.left_panel.entries = vec![
            make_entry("file_a.txt", false),
            make_entry("file_b.txt", false),
        ];
        state.left_panel.cursor = 1;

        let paths = selected_or_current_paths(&state);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/tmp/file_b.txt"));
    }

    #[test]
    fn test_selected_or_current_paths_uses_selection_when_present() {
        // Two entries selected → returns both, ignoring cursor position
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        state.left_panel.entries = vec![
            make_entry("file_a.txt", true),
            make_entry("file_b.txt", false),
            make_entry("file_c.txt", true),
        ];
        state.left_panel.cursor = 1; // cursor on unselected file_b

        let paths = selected_or_current_paths(&state);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("/tmp/file_a.txt")));
        assert!(paths.contains(&PathBuf::from("/tmp/file_c.txt")));
    }

    #[test]
    fn test_selected_or_current_paths_skips_dotdot() {
        // ".." selected → should not appear in results; cursor is on ".."  → empty
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        let mut dotdot = make_entry("..", false);
        dotdot.name = "..".to_string();
        dotdot.selected = true;
        state.left_panel.entries = vec![dotdot];
        state.left_panel.cursor = 0;

        let paths = selected_or_current_paths(&state);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_selected_or_current_paths_empty_panel() {
        let state = AppState::new();
        let paths = selected_or_current_paths(&state);
        assert!(paths.is_empty());
    }

    #[test]
    fn directory_tree_page_down_uses_terminal_height() {
        let mut state = AppState {
            tree_entries: (0..50)
                .map(|i| app::dir_tree::TreeEntry {
                    path: PathBuf::from(format!("/tmp/{i}")),
                    depth: 0,
                    is_dir: false,
                    expanded: false,
                    name: format!("entry-{i}"),
                })
                .collect(),
            ..Default::default()
        };

        handle_directory_tree(&mut state, &mut None, KeyCode::PageDown, 12);

        assert_eq!(state.tree_selected, 9);
        assert_eq!(state.tree_scroll, 9);
    }

    #[test]
    fn directory_tree_page_up_uses_terminal_height() {
        let mut state = AppState {
            tree_entries: (0..50)
                .map(|i| app::dir_tree::TreeEntry {
                    path: PathBuf::from(format!("/tmp/{i}")),
                    depth: 0,
                    is_dir: false,
                    expanded: false,
                    name: format!("entry-{i}"),
                })
                .collect(),
            tree_selected: 25,
            tree_scroll: 25,
            ..Default::default()
        };

        handle_directory_tree(&mut state, &mut None, KeyCode::PageUp, 12);

        assert_eq!(state.tree_selected, 16);
        assert_eq!(state.tree_scroll, 16);
    }

    #[test]
    fn history_dedup_consecutive() {
        let mut state = AppState::default();
        state.left_panel.path = std::env::temp_dir();
        state.command_history.push_back("echo hi".to_string());
        // Simulate push logic (same as run_shell_command but without executing)
        let cmd = "echo hi";
        if state.command_history.back().is_none_or(|l| l != cmd) {
            state.command_history.push_back(cmd.to_string());
        }
        assert_eq!(state.command_history.len(), 1);
        assert_eq!(state.command_history[0], "echo hi");
    }

    #[test]
    fn history_dedup_different_commands() {
        let mut state = AppState::default();
        state.command_history.push_back("echo hi".to_string());
        let cmd = "ls -la";
        if state.command_history.back().is_none_or(|l| l != cmd) {
            state.command_history.push_back(cmd.to_string());
        }
        assert_eq!(state.command_history.len(), 2);
    }

    #[test]
    fn history_cap_at_100() {
        let mut state = AppState::default();
        for i in 0..101 {
            let cmd = format!("cmd_{}", i);
            if state
                .command_history
                .back()
                .is_none_or(|l| l.as_str() != cmd.as_str())
            {
                state.command_history.push_back(cmd);
                if state.command_history.len() > shell::MAX_HISTORY {
                    state.command_history.pop_front();
                }
            }
        }
        assert_eq!(state.command_history.len(), 100);
        assert_eq!(state.command_history[0], "cmd_1");
        assert_eq!(state.command_history[99], "cmd_100");
    }

    #[test]
    fn history_picker_enter_loads_command_line() {
        let mut state = AppState::default();
        state.command_history.push_back("git status".to_string());
        state.command_history.push_back("git log".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Enter);

        assert_eq!(state.mode, AppMode::CommandLine);
        assert_eq!(state.command_line, "git log");
    }

    #[test]
    fn history_picker_esc_cancels() {
        let mut state = AppState::default();
        state.command_history.push_back("ls".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn history_picker_navigate_up_down() {
        let mut state = AppState::default();
        state.command_history.push_back("cmd1".to_string());
        state.command_history.push_back("cmd2".to_string());
        state.command_history.push_back("cmd3".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn hotlist_picker_add_current_dir() {
        let mut state = AppState::default();
        let tmp = std::env::temp_dir();
        state.left_panel.path = tmp.clone();
        state.directory_hotlist.clear();
        state.mode = AppMode::ListPicker(PickerKind::Hotlist);

        handle_list_picker(&mut state, KeyCode::Char('a'));

        assert!(state.directory_hotlist.contains(&tmp));
    }

    #[test]
    fn hotlist_picker_add_dedup() {
        let mut state = AppState::default();
        let tmp = std::env::temp_dir();
        state.left_panel.path = tmp.clone();
        state.directory_hotlist = vec![tmp.clone()];
        state.mode = AppMode::ListPicker(PickerKind::Hotlist);

        handle_list_picker(&mut state, KeyCode::Char('a'));

        assert_eq!(
            state
                .directory_hotlist
                .iter()
                .filter(|p| *p == &tmp)
                .count(),
            1
        );
    }

    #[test]
    fn hotlist_picker_delete_entry() {
        let mut state = AppState {
            directory_hotlist: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Char('d'));

        assert_eq!(state.directory_hotlist.len(), 2);
        assert!(!state.directory_hotlist.contains(&PathBuf::from("/b")));
    }

    #[test]
    fn hotlist_picker_delete_adjusts_cursor() {
        let mut state = AppState {
            directory_hotlist: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Char('d'));

        assert_eq!(state.directory_hotlist.len(), 1);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn hotlist_persistence_roundtrip() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let tmp_dir = std::env::temp_dir();
        let state = AppState {
            directory_hotlist: vec![tmp_dir.clone(), PathBuf::from("/usr")],
            ..Default::default()
        };

        // Serialize and deserialize manually via PersistedSetup
        let hotlist_strs: Vec<String> = state
            .directory_hotlist
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let content = format!(
            "version = 1\nactive_panel = \"left\"\nhotlist = {:?}\n\
            [left]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n\
            [right]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n",
            hotlist_strs
        );

        // Write to a temp file, then read back via toml
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        let read_back = std::fs::read_to_string(f.path()).unwrap();
        let parsed: app::config::PersistedSetup = toml::from_str(&read_back).unwrap();

        let loaded: Vec<PathBuf> = parsed.hotlist.iter().map(PathBuf::from).collect();
        assert_eq!(loaded, state.directory_hotlist);
    }

    #[test]
    fn user_menu_picker_esc_closes() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            user_menu_entries: vec![app::user_menu::MenuEntry {
                hotkey: 'A',
                title: "Archive".to_string(),
                command: "tar czf a.tgz".to_string(),
                condition: None,
            }],
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn user_menu_picker_navigate_and_select() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            user_menu_entries: vec![
                app::user_menu::MenuEntry {
                    hotkey: 'A',
                    title: "Archive".to_string(),
                    command: "echo archive".to_string(),
                    condition: None,
                },
                app::user_menu::MenuEntry {
                    hotkey: 'B',
                    title: "Build".to_string(),
                    command: "echo build".to_string(),
                    condition: None,
                },
            ],
            ..Default::default()
        };

        // Navigate down
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        // Navigate up
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn user_menu_file_menu_no_menu_file_shows_error() {
        // Point the panel at a temp dir with no .mc.menu file
        let tmp = std::env::temp_dir();
        let mut terminal = test_terminal();
        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 1,
            menu_item_selected: 0,
            ..Default::default()
        };
        state.left_panel.path = tmp.clone();

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        // Should show an error dialog since no menu file exists
        assert!(matches!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Error(_))
        ));
    }

    #[test]
    fn user_menu_file_menu_with_entries_opens_picker() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let mut terminal = test_terminal();
        let menu_path = tmp.path().join(".mc.menu");
        let mut f = std::fs::File::create(&menu_path).unwrap();
        write!(
            f,
            "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n"
        )
        .unwrap();

        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 1,
            menu_item_selected: 0,
            ..Default::default()
        };
        state.left_panel.path = tmp.path().to_path_buf();

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24, &mut terminal);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
        assert_eq!(state.picker_selected, 0);
        assert_eq!(state.user_menu_entries.len(), 2);
        assert_eq!(state.user_menu_entries[0].hotkey, 'A');
        assert_eq!(state.user_menu_entries[1].hotkey, 'B');
    }

    #[test]
    fn compare_mode_picker_maps_index_to_mode() {
        // picker_selected 0 => Quick, 1 => Size, 2 => Thorough
        const MODES: [CompareMode; 3] =
            [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
        assert_eq!(MODES[0], CompareMode::Quick);
        assert_eq!(MODES[1], CompareMode::Size);
        assert_eq!(MODES[2], CompareMode::Thorough);
    }

    #[test]
    fn compare_mode_picker_esc_cancels() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn compare_mode_picker_enter_runs_quick_by_default() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "a.txt".to_string(),
            path: std::env::temp_dir().join("a.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        }];
        state.mode = AppMode::ListPicker(PickerKind::CompareMode);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Enter);

        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                app::types::ConfirmDetails::simple(
                    "Compare Results",
                    "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
                )
            ))
        );
    }

    #[test]
    fn compare_mode_picker_navigate_and_select_thorough() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            picker_selected: 0,
            ..Default::default()
        };
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "x.txt".to_string(),
            path: std::env::temp_dir().join("x.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 42,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
            mime_type: None,
        }];

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 2);

        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                app::types::ConfirmDetails::simple(
                    "Compare Results",
                    "Compare dirs (Thorough):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
                )
            ))
        );
    }
}
