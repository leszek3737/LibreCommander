use std::io::{self, Write};
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
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Size;

use lc::{app, fs, menu, ui};

mod input;
mod render;
mod render_dialog_map;

use app::job_runner::{RunningJob, poll_running_job};
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
const SPINNER_TICK_INTERVAL: Duration = Duration::from_millis(200);
const WATCH_CHANNEL_CAPACITY: usize = 2048;

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

pub(crate) fn enter_tui_stdout() -> io::Result<()> {
    enable_raw_mode()?;
    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide) {
        if let Err(e) = disable_raw_mode() {
            lc::debug_log!("failed to disable raw mode: {e}");
        }
        return Err(err);
    }
    Ok(())
}

pub(crate) fn leave_tui_stdout() -> io::Result<()> {
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
    if raw_result.is_err() && screen_result.is_err() {
        lc::debug_log!("leave_tui_stdout: screen error masked by raw error: {screen_result:?}");
    }
    raw_result.and(screen_result)
}

fn fatal(msg: &str, err: &dyn std::fmt::Display) -> ! {
    lc::debug_log!("{msg}: {err}");
    let _ = writeln!(io::stderr(), "Error: {err}");
    std::process::exit(1);
}

fn main() {
    install_panic_hook();
    if let Err(err) = enter_tui_stdout() {
        fatal("enter_tui_stdout", &err);
    }

    let result = {
        let _guard = TerminalGuard;
        let backend = CrosstermBackend::new(io::stdout());
        match Terminal::new(backend) {
            Ok(mut terminal) => run_app(&mut terminal),
            Err(e) => Err(e),
        }
    };

    if let Err(err) = &result {
        fatal("run_app", err);
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
    match loader.try_recv() {
        Ok(Ok(vs)) => {
            *viewer_state = Some(vs);
            *viewer_loader = None;
            changed = true;
        }
        Ok(Err(e)) => {
            state.ui.status_message = Some(format!("Failed to open file: {e}"));
            *viewer_state = None;
            state.mode = AppMode::Normal;
            *viewer_loader = None;
            changed = true;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            let now = std::time::Instant::now();
            let should_redraw = state
                .ui
                .viewer_spinner_last_tick
                .is_none_or(|last| now.duration_since(last) >= SPINNER_TICK_INTERVAL);
            if should_redraw {
                state.ui.viewer_spinner_last_tick = Some(now);
                state.ui.viewer_spinner_frame = state.ui.viewer_spinner_frame.wrapping_add(1);
                changed = true;
            }
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            state.ui.status_message = Some("Viewer load failed: thread panicked".to_string());
            *viewer_state = None;
            state.mode = AppMode::Normal;
            *viewer_loader = None;
            changed = true;
        }
    }
    changed
}

fn poll_image_preview(
    state: &mut AppState,
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
            state.ui.status_message = Some("Image preview failed: thread panicked".to_string());
            *image_preview_loader = None;
            true
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
    term_size: Size,
) {
    start_image_preview_if_needed(
        viewer_state,
        image_preview_loader,
        (term_size.width, term_size.height),
    );
    if let Some(vs) = viewer_state
        && state.mode == AppMode::Viewing
    {
        vs.update_wrap_layout(term_size.width as usize);
    }
}

/// Build the initial `AppState`: load config + apply theme. Any recoverable
/// error is surfaced through `status_message` rather than aborting startup.
fn init_app_state() -> AppState {
    let mut state = AppState::new();
    let config_raw = match app::config::load_setup(&mut state) {
        Ok(raw) => raw,
        Err(e) => {
            state.ui.status_message = Some(e);
            None
        }
    };
    if let Some(ref raw) = config_raw
        && let Err(e) = ui::theme::Theme::apply_from_value_to_palette(raw, &mut state.theme_colors)
    {
        state.ui.status_message = Some(e);
    }
    state
}

/// Start the filesystem watcher, returning the receiver and `Option<Watcher>`.
/// A failed watcher is non-fatal: the app runs without live updates and the
/// reason is appended to `status_message`.
fn init_watcher(
    state: &mut AppState,
) -> (
    Option<fs::watcher::Watcher>,
    mpsc::Receiver<fs::watcher::WatchEvent>,
) {
    let (watch_tx, watch_rx) = mpsc::sync_channel(WATCH_CHANNEL_CAPACITY);
    let watcher = match fs::watcher::Watcher::new(Arc::new(watch_tx)) {
        Ok(w) => Some(w),
        Err(err) => {
            let msg = format!("watcher disabled: {err}");
            state.ui.status_message = Some(match state.ui.status_message.take() {
                Some(prev) => format!("{prev}; {msg}"),
                None => msg,
            });
            None
        }
    };
    (watcher, watch_rx)
}

/// Long-lived loop state that the async-poll and render seams operate on. Groups
/// the watcher bookkeeping + the per-frame viewer/job handles so the per-iter
/// helpers take a small parameter list instead of a dozen `&mut` locals.
/// Result of a background archive listing: `(source, dest, entries-or-error)`.
type ArchiveListMsg = (
    std::path::PathBuf,
    String,
    Result<Vec<lc::ops::archive::ArchiveEntry>, lc::ops::archive::ArchiveError>,
);
/// Result of a background directory-tree build: `(root, tree)`.
type TreeBuildMsg = (std::path::PathBuf, app::dir_tree::TreeBuildResult);

struct AppLoop {
    viewer_state: Option<viewer::ViewerState>,
    viewer_loader: Option<viewer::ViewerLoader>,
    image_preview_loader: Option<viewer::ImagePreviewLoader>,
    running_job: Option<RunningJob>,
    /// In-flight background archive listing (P1.5), if any.
    archive_list_load: Option<app::bg_load::BgLoad<ArchiveListMsg>>,
    /// In-flight background directory-tree build (P1.6), if any.
    tree_load: Option<app::bg_load::BgLoad<TreeBuildMsg>>,
    watcher: Option<fs::watcher::Watcher>,
    watcher_paused: bool,
    watcher_sync_state: watcher_sync::WatcherSyncState,
}

/// True while a background loading dialog ("Listing archive..." / "Building
/// tree...") is the active mode. Used to tell a live load from one the user
/// dismissed with Esc (which drops the loader, cancelling it).
fn is_loading_dialog(state: &AppState) -> bool {
    matches!(state.mode, AppMode::Dialog(DialogKind::Progress { .. }))
}

/// Start any pending background load requested by an input handler, and apply a
/// finished load's result. Returns `true` if the frame must be redrawn.
fn poll_background_loads(loop_state: &mut AppLoop, state: &mut AppState) -> bool {
    let mut dirty = false;

    // --- Archive listing (P1.5) ---
    if let Some((source, dest)) = state.ui.pending_archive_list.take() {
        let src = source.clone();
        match app::bg_load::BgLoad::spawn("archive-list", move |_cancel| {
            (source, dest, lc::ops::archive::list_archive(&src))
        }) {
            Ok(load) => loop_state.archive_list_load = Some(load),
            Err(e) => {
                state.ui.status_message = Some(format!("Failed to start archive listing: {e}"));
                state.mode = AppMode::Normal;
                dirty = true;
            }
        }
    }
    if let Some(load) = loop_state.archive_list_load.as_ref() {
        match load.try_recv() {
            Ok((source, dest, result)) => {
                loop_state.archive_list_load = None;
                // Discard the result if the user dismissed the loading dialog.
                if is_loading_dialog(state) {
                    input::normal::apply_archive_list_result(state, source, dest, result);
                }
                dirty = true;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Dismissed via Esc: drop the loader (cancels + detaches).
                if !is_loading_dialog(state) {
                    loop_state.archive_list_load = None;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                loop_state.archive_list_load = None;
                if is_loading_dialog(state) {
                    state.ui.status_message =
                        Some("Archive listing failed: worker thread panicked".to_string());
                    state.mode = AppMode::Normal;
                }
                dirty = true;
            }
        }
    }

    // --- Directory-tree build (P1.6) ---
    if let Some((path, show_hidden)) = state.ui.pending_tree_build.take() {
        let root = path.clone();
        match app::bg_load::BgLoad::spawn("tree-build", move |_cancel| {
            let tree = app::dir_tree::build_tree_with_diagnostics(
                &path,
                input::menu_actions::TREE_EXPAND_DEPTH,
                show_hidden,
            );
            (root, tree)
        }) {
            Ok(load) => loop_state.tree_load = Some(load),
            Err(e) => {
                state.ui.status_message = Some(format!("Failed to start tree build: {e}"));
                state.mode = AppMode::Normal;
                dirty = true;
            }
        }
    }
    if let Some(load) = loop_state.tree_load.as_ref() {
        match load.try_recv() {
            Ok((root, tree)) => {
                loop_state.tree_load = None;
                if is_loading_dialog(state) {
                    input::menu_actions::apply_tree_build_result(state, root, tree);
                }
                dirty = true;
            }
            Err(mpsc::TryRecvError::Empty) => {
                if !is_loading_dialog(state) {
                    loop_state.tree_load = None;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                loop_state.tree_load = None;
                if is_loading_dialog(state) {
                    state.ui.status_message =
                        Some("Tree build failed: worker thread panicked".to_string());
                    state.mode = AppMode::Normal;
                }
                dirty = true;
            }
        }
    }

    dirty
}

/// Drain every async source feeding the UI for one loop iteration: watcher
/// events, the running job, and the viewer/image-preview loaders. Returns
/// `true` if anything changed and the frame must be redrawn.
fn poll_async(
    loop_state: &mut AppLoop,
    state: &mut AppState,
    watch_rx: &mpsc::Receiver<fs::watcher::WatchEvent>,
) -> bool {
    let mut dirty = false;
    panel_ops::sync_watcher_job_state(
        &loop_state.watcher,
        loop_state.running_job.is_some(),
        &mut loop_state.watcher_paused,
    );
    watcher_sync::sync_watcher_paths(
        &mut loop_state.watcher,
        state,
        &mut loop_state.watcher_sync_state,
    );
    // Flush BEFORE polling, every iteration: debounced Created/Modified events
    // sit in the watcher's debounce map and are only pushed onto the channel by
    // `flush_pending`. If we only flushed after a non-empty poll, an event whose
    // debounce window expired during a quiet period (no further filesystem
    // activity) would never be delivered to the UI. Flushing unconditionally each
    // ~33ms loop tick guarantees expired events surface within one debounce
    // interval, and also drains entries left pending after `sync_watcher_paths`
    // removed their watch.
    if let Some(ref w) = loop_state.watcher {
        w.flush_pending();
    }
    if watcher_sync::poll_watcher_events(state, watch_rx) {
        dirty = true;
    }
    if poll_running_job(state, &mut loop_state.running_job, panel_ops::refresh_both) {
        panel_ops::sync_watcher_job_state(
            &loop_state.watcher,
            loop_state.running_job.is_some(),
            &mut loop_state.watcher_paused,
        );
        dirty = true;
    }
    if poll_viewer_loader(
        state,
        &mut loop_state.viewer_state,
        &mut loop_state.viewer_loader,
    ) || poll_image_preview(
        state,
        &mut loop_state.viewer_state,
        &mut loop_state.image_preview_loader,
    ) {
        dirty = true;
    }
    if poll_background_loads(loop_state, state) {
        dirty = true;
    }
    dirty
}

/// Run image-preview prep then draw a single frame. Keeps the `pre_draw` →
/// `terminal.draw` ordering (image preview / wrap layout must be computed
/// before the immediate-mode render reads them) in one place.
fn render_frame(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    loop_state: &mut AppLoop,
    state: &AppState,
    term_size: Size,
) -> io::Result<()> {
    pre_draw(
        state,
        &mut loop_state.viewer_state,
        &mut loop_state.image_preview_loader,
        term_size,
    );
    terminal.draw(|f| {
        render::render_ui(
            f,
            state,
            loop_state.viewer_state.as_ref(),
            loop_state.viewer_loader.as_ref(),
        )
    })?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    recover_terminal_state()?;

    let mut state = init_app_state();
    let (watcher, watch_rx) = init_watcher(&mut state);
    let mut loop_state = AppLoop {
        viewer_state: None,
        viewer_loader: None,
        image_preview_loader: None,
        running_job: None,
        archive_list_load: None,
        tree_load: None,
        watcher,
        watcher_paused: false,
        watcher_sync_state: watcher_sync::WatcherSyncState::default(),
    };

    panel_ops::refresh_panel(&mut state.left_panel, 0);
    panel_ops::refresh_panel(&mut state.right_panel, 0);
    watcher_sync::sync_watcher_paths(
        &mut loop_state.watcher,
        &state,
        &mut loop_state.watcher_sync_state,
    );
    let mut dirty = true;
    let mut term_size = terminal.size()?;

    loop {
        if poll_async(&mut loop_state, &mut state, &watch_rx) {
            dirty = true;
        }
        if dirty {
            if let Err(e) = render_frame(terminal, &mut loop_state, &state, term_size) {
                shutdown_job(&mut loop_state.running_job);
                return Err(e);
            }
            dirty = false;
        }
        let has_event = match event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS)) {
            Ok(ready) => ready,
            Err(e) => {
                // Mirror the `event::read()` error path below: tear down any
                // running job before propagating, instead of relying solely on
                // the destructor's best-effort reaper.
                shutdown_job(&mut loop_state.running_job);
                return Err(e);
            }
        };
        if has_event {
            let key = match event::read() {
                Ok(k) => k,
                Err(e) => {
                    shutdown_job(&mut loop_state.running_job);
                    return Err(e);
                }
            };
            let mut ctx = input::EventContext {
                state: &mut state,
                viewer_state: &mut loop_state.viewer_state,
                viewer_loader: &mut loop_state.viewer_loader,
                image_preview_loader: &mut loop_state.image_preview_loader,
                running_job: &mut loop_state.running_job,
                term_size,
            };
            dirty = match dispatch_event(&mut ctx, terminal, &key) {
                Ok(d) => d,
                Err(e) => {
                    // Match the other loop exit paths: tear down a running job
                    // before propagating instead of leaving it to the reaper.
                    shutdown_job(ctx.running_job);
                    return Err(e);
                }
            };
            // The dispatch may have processed a `Resize`, which lives only in
            // the context; carry it back so the next render uses the new size.
            term_size = ctx.term_size;
        }

        if state.should_quit() {
            // Restore the terminal before reaping a running job so the user gets
            // their shell back immediately instead of staring at a frozen TUI for
            // up to the reaper's join deadline. The `TerminalGuard`'s leave on
            // drop is idempotent, so doing it here too is safe.
            let _ = leave_tui_stdout();
            shutdown_job(&mut loop_state.running_job);
            return Ok(());
        }
    }
}

fn shutdown_job(job: &mut Option<RunningJob>) {
    if let Some(j) = job.as_mut() {
        j.shutdown();
    }
}

fn recover_terminal_state() -> io::Result<()> {
    let Some(terminal_state_file) = paths::terminal_state_file_path() else {
        return Ok(());
    };
    if std::fs::metadata(&terminal_state_file).is_ok() {
        // Bounce the terminal to recover from a previous external-process exit.
        // The `enter` result is what determines whether the terminal is usable
        // going forward, so it is the one we propagate. A failed `leave` is
        // logged but must NOT mask a successful `enter`: reporting an error
        // while the terminal is actually working would abort the app needlessly.
        let leave = leave_tui_stdout();
        let enter = enter_tui_stdout();
        if let Err(e) = leave {
            lc::debug_log!("failed to leave terminal during recovery: {e}");
        }
        // Propagate a failed re-entry BEFORE clearing the marker, so a recovery
        // that did not actually restore the terminal is retried on next launch
        // instead of being silently forgotten.
        enter?;
        if let Err(e) = std::fs::remove_file(&terminal_state_file) {
            lc::debug_log!("failed to remove terminal state file: {e}");
        }
    }
    Ok(())
}

fn dispatch_event<B: ratatui::backend::Backend>(
    ctx: &mut input::EventContext,
    terminal: &mut Terminal<B>,
    event: &Event,
) -> Result<bool, B::Error> {
    match event {
        Event::Key(key) => dispatch_key_event(ctx, terminal, key),
        Event::Mouse(mouse_event) => dispatch_mouse_event(ctx, terminal, mouse_event),
        Event::Resize(cols, rows) => {
            ctx.term_size = Size::new(*cols, *rows);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn dispatch_key_event<B: ratatui::backend::Backend>(
    ctx: &mut input::EventContext,
    terminal: &mut Terminal<B>,
    key: &KeyEvent,
) -> Result<bool, B::Error> {
    match key.kind {
        KeyEventKind::Press => {}
        KeyEventKind::Repeat if key_repeat_allowed(&ctx.state.mode, key.code) => {}
        _ => return Ok(false),
    }
    // Per-mode dispatch: each arm forwards the shared `EventContext` (plus the
    // terminal/key data each handler needs) to exactly one mode handler. Match
    // on `ctx.state.mode` as a place expression with only non-binding patterns
    // (`Dialog(_)`, `ListPicker(_)`, unit variants): this reads the discriminant
    // without holding a borrow into the arm, so each handler is free to reborrow
    // `ctx.state` mutably. No clone -- the previous `mode.clone()` deep-allocated
    // `Dialog`/`ListPicker` payloads on every keypress.
    match ctx.state.mode {
        AppMode::Normal => {
            input::mode_dispatch::handle_normal_mode(ctx, key.code, key.modifiers, terminal);
        }
        AppMode::Viewing => {
            input::mode_dispatch::handle_viewer_mode(ctx, key.code);
        }
        AppMode::CommandLine => {
            input::command_line::handle_command_line(ctx.state, *key);
        }
        AppMode::Dialog(_) => {
            input::dialogs::handle_dialog(ctx, key.code, key.modifiers);
        }
        AppMode::Search if matches!(key.code, KeyCode::F(_)) => {
            let visible = panel_ops::panel_visible_height(ctx.term_size.height);
            input::mode_dispatch::clear_search_state(ctx.state, visible);
            input::mode_dispatch::handle_normal_mode(ctx, key.code, key.modifiers, terminal);
        }
        AppMode::Search => {
            input::mode_dispatch::handle_search_mode(
                ctx.state,
                key.code,
                key.modifiers,
                ctx.term_size.height,
            );
        }
        AppMode::Menu => {
            input::mode_dispatch::handle_menu_mode(ctx, key.code, terminal);
        }
        AppMode::ListPicker(_) => {
            input::pickers::handle_list_picker(ctx.state, key.code);
        }
        AppMode::DirectoryTree => {
            input::directory_tree::handle_directory_tree(ctx, key.code);
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

    // The archive dialogs carry editable destination fields too, so held
    // Backspace/character keys should repeat there like any other text input.
    if is_text_edit
        && matches!(
            mode,
            AppMode::Dialog(
                DialogKind::Input { .. }
                    | DialogKind::ArchiveExtract(_)
                    | DialogKind::ArchiveCreate(_)
            )
        )
    {
        return true;
    }

    false
}

fn dispatch_mouse_event<B: ratatui::backend::Backend>(
    ctx: &mut input::EventContext,
    terminal: &mut Terminal<B>,
    mouse_event: &MouseEvent,
) -> Result<bool, B::Error> {
    let Some(outcome) = input::mouse::handle_mouse_event(ctx, *mouse_event) else {
        return Ok(false);
    };
    match outcome {
        input::mouse::MouseOutcome::Consumed => {}
        input::mouse::MouseOutcome::NormalKey(key) => {
            if matches!(ctx.state.mode, AppMode::Search) {
                let visible = panel_ops::panel_visible_height(ctx.term_size.height);
                input::mode_dispatch::clear_search_state(ctx.state, visible);
            }
            input::mode_dispatch::handle_normal_mode(ctx, key, KeyModifiers::NONE, terminal);
        }
        input::mouse::MouseOutcome::MenuAction => {
            input::mode_dispatch::run_selected_menu_action(ctx, terminal);
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
