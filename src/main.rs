use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
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
#[cfg(test)]
use app::shell;
#[cfg(test)]
use app::types::PanelState;
use app::types::{AppMode, AppState, DialogKind, ViewMode};
use app::{panel_ops, paths, watcher_sync};

use ui::viewer;

#[cfg(test)]
pub(crate) use input::normal::{
    confirm_delete, confirm_file_transfer, launch_editor, reposition_cursor_to_entry,
    selected_or_current_paths,
};
pub(crate) use input::normal::{
    handle_alt_keys, handle_ctrl_keys, handle_enter_key, handle_function_keys,
    handle_navigation_keys,
};

const EVENT_POLL_TIMEOUT_MS: u64 = 33;

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

fn main() {
    install_panic_hook();
    if let Err(err) = enter_tui_stdout() {
        lc::debug_log!("Error: {err}");
        let msg = format!("Error: {err}\n");
        let _ = io::stderr().write_all(msg.as_bytes());
        std::process::exit(1);
    }

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
        std::process::exit(1);
    }
}

fn poll_viewer_loader(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
) -> bool {
    let Some(loader) = viewer_loader.as_ref() else {
        return false;
    };
    let mut changed = false;
    match loader.receiver.try_recv() {
        Ok(Ok(vs)) => {
            *viewer_state = Some(vs);
            *viewer_loader = None;
            changed = true;
        }
        Ok(Err(e)) => {
            state.status_message = Some(format!("Failed to open file: {e}"));
            state.mode = AppMode::Normal;
            *viewer_loader = None;
            changed = true;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            let now = std::time::Instant::now();
            let should_redraw = state.viewer_spinner_frame.is_none_or(|last| {
                now.duration_since(last) >= std::time::Duration::from_millis(200)
            });
            if should_redraw {
                state.viewer_spinner_frame = Some(now);
                changed = true;
            }
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            state.status_message = Some("Viewer load failed: thread panicked".to_string());
            state.mode = AppMode::Normal;
            *viewer_loader = None;
            changed = true;
        }
    }
    changed
}

fn poll_image_preview(
    viewer_state: &mut Option<viewer::ViewerState>,
    image_preview_loader: &mut Option<viewer::ImagePreviewLoader>,
) -> bool {
    let Some(loader) = image_preview_loader.as_ref() else {
        return false;
    };
    match loader.try_recv() {
        Ok((w, h, text)) => {
            let matched = viewer_state
                .as_ref()
                .is_some_and(|vs| vs.file_path == loader.file_path);
            if matched && let Some(vs) = viewer_state.as_mut() {
                vs.set_image_preview(w, h, text);
            }
            *image_preview_loader = None;
            matched
        }
        Err(mpsc::TryRecvError::Empty) => false,
        Err(mpsc::TryRecvError::Disconnected) => {
            *image_preview_loader = None;
            false
        }
    }
}

fn start_image_preview_if_needed(
    viewer_state: &mut Option<viewer::ViewerState>,
    image_preview_loader: &mut Option<viewer::ImagePreviewLoader>,
    terminal_size: (u16, u16),
) {
    let Some(vs) = viewer_state.as_mut() else {
        return;
    };
    if vs.view_mode != ViewMode::Image {
        return;
    }
    let (w, h) = terminal_size;
    if !vs.needs_image_preview(w, h) {
        return;
    }
    if let Some(loader) = image_preview_loader.take() {
        loader.cancel();
    }
    let (cw, ch) = viewer::ViewerState::image_content_size(w, h);
    *image_preview_loader = Some(viewer::ImagePreviewLoader::start(
        vs.file_path.clone(),
        cw,
        ch,
    ));
}

fn pre_draw(
    state: &AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    image_preview_loader: &mut Option<viewer::ImagePreviewLoader>,
    terminal: &Terminal<CrosstermBackend<io::Stdout>>,
) {
    let Ok(size) = terminal.size() else { return };
    start_image_preview_if_needed(
        viewer_state,
        image_preview_loader,
        (size.width, size.height),
    );
    if state.mode == AppMode::Viewing
        && let Some(vs) = viewer_state
    {
        vs.update_wrap_layout(size.width as usize);
    }
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    recover_terminal_state()?;

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
    let mut image_preview_loader: Option<viewer::ImagePreviewLoader> = None;
    let mut running_job: Option<RunningJob> = None;
    let (watch_tx, watch_rx) = mpsc::sync_channel(2048);
    let mut watcher = match fs::watcher::Watcher::new(Arc::new(watch_tx)) {
        Ok(w) => Some(w),
        Err(err) => {
            let msg = format!("watcher disabled: {err}");
            state.status_message = Some(match state.status_message.take() {
                Some(prev) => format!("{prev}; {msg}"),
                None => msg,
            });
            None
        }
    };
    let mut watcher_paused = false;
    let mut watcher_sync_state = watcher_sync::WatcherSyncState::default();

    panel_ops::refresh_panel(&mut state.left_panel, 0);
    panel_ops::refresh_panel(&mut state.right_panel, 0);
    watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut watcher_sync_state);
    let mut dirty = true;

    loop {
        panel_ops::sync_watcher_job_state(&watcher, running_job.is_some(), &mut watcher_paused);
        watcher_sync::sync_watcher_paths(&mut watcher, &state, &mut watcher_sync_state);
        if let Some(ref w) = watcher {
            w.flush_pending();
        }
        if watcher_sync::poll_watcher_events(&mut state, &watch_rx) {
            dirty = true;
        }
        if poll_running_job(&mut state, &mut running_job, panel_ops::refresh_both) {
            panel_ops::sync_watcher_job_state(&watcher, running_job.is_some(), &mut watcher_paused);
            dirty = true;
        }
        if poll_viewer_loader(&mut state, &mut viewer_state, &mut viewer_loader)
            || poll_image_preview(&mut viewer_state, &mut image_preview_loader)
        {
            dirty = true;
        }
        if dirty {
            pre_draw(
                &state,
                &mut viewer_state,
                &mut image_preview_loader,
                terminal,
            );
            if let Err(e) =
                terminal.draw(|f| render::render_ui(f, &state, &viewer_state, &viewer_loader))
            {
                if let Some(j) = running_job.as_mut() {
                    j.shutdown()
                }
                return Err(e);
            }
            dirty = false;
        }
        if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))? {
            let key = match event::read() {
                Ok(k) => k,
                Err(e) => {
                    if let Some(j) = running_job.as_mut() {
                        j.shutdown()
                    }
                    return Err(e);
                }
            };
            dirty = dispatch_event(
                &mut state,
                &mut viewer_state,
                &mut viewer_loader,
                &mut image_preview_loader,
                &mut running_job,
                terminal,
                &key,
            )?;
        }

        if state.should_quit {
            if let Some(j) = running_job.as_mut() {
                j.shutdown()
            }
            return Ok(());
        }
    }
}

fn recover_terminal_state() -> io::Result<()> {
    let terminal_state_file = terminal_state_file_path();
    if std::fs::metadata(&terminal_state_file).is_ok() {
        let leave_result = leave_tui_stdout();
        let resume_result = resume_terminal_stdout();
        if let Err(e) = std::fs::remove_file(&terminal_state_file) {
            lc::debug_log!("failed to remove terminal state file: {e}");
        }
        if let Err(e) = &leave_result {
            lc::debug_log!("leave_tui_stdout failed: {e}");
        }
        resume_result?;
    }
    Ok(())
}

fn dispatch_event<B: ratatui::backend::Backend>(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    viewer_loader: &mut Option<viewer::ViewerLoader>,
    image_preview_loader: &mut Option<viewer::ImagePreviewLoader>,
    running_job: &mut Option<RunningJob>,
    terminal: &mut Terminal<B>,
    event: &Event,
) -> Result<bool, B::Error> {
    match event {
        Event::Key(key) => dispatch_key_event(
            state,
            viewer_state,
            viewer_loader,
            image_preview_loader,
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
    image_preview_loader: &mut Option<viewer::ImagePreviewLoader>,
    running_job: &mut Option<RunningJob>,
    terminal: &mut Terminal<B>,
    key: &KeyEvent,
) -> Result<bool, B::Error> {
    match key.kind {
        KeyEventKind::Press => {}
        KeyEventKind::Repeat if key_repeat_allowed(&state.mode, key.code) => {}
        _ => return Ok(false),
    }
    let size = terminal.size()?;
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
                image_preview_loader,
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
            let visible = panel_ops::panel_visible_height(size.height);
            input::mode_dispatch::clear_search_state(state, visible);
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
    let is_nav = matches!(
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
    );
    if is_nav {
        return true;
    }

    let is_text_edit = matches!(key, KeyCode::Backspace | KeyCode::Delete | KeyCode::Char(_));
    let is_text_mode = matches!(
        mode,
        AppMode::CommandLine | AppMode::Search | AppMode::Menu | AppMode::ListPicker(_)
    );
    if is_text_edit && is_text_mode {
        return true;
    }

    if is_text_edit && matches!(mode, AppMode::Dialog(DialogKind::Input { .. })) {
        return true;
    }

    if key == KeyCode::Enter {
        return false;
    }

    false
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
                    let visible = panel_ops::panel_visible_height(size.height);
                    input::mode_dispatch::clear_search_state(state, visible);
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

#[cfg(test)]
pub(crate) fn apply_search_filter(panel: &mut PanelState) {
    panel_ops::rebuild_visible_entries(panel, panel_ops::current_visible_height());
    panel.cursor = 0;
    panel.scroll_offset = 0;
}

#[cfg(test)]
mod tests;
