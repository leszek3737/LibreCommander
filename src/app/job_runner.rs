use crate::debug_log;
use crate::ops::helpers::action_label;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ops;

use super::types::{ActivePanel, AppMode, AppState, DialogKind, FileEntry};

/// Status shown when an action is requested while another job is in flight.
const ANOTHER_JOB_RUNNING: &str = "Another job is already running";

/// Max time the last-resort reaper waits for a worker to finish before
/// detaching it. See `RunningJob`'s `Drop` impl for the rationale.
const REAPER_JOIN_DEADLINE: Duration = Duration::from_secs(5);
/// Poll cadence while the reaper waits for the worker to finish.
const REAPER_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Joins a finished (or finishing) worker thread, swallowing and logging any
/// panic payload tagged with `context`.
///
/// Returns `true` if the worker exited cleanly, `false` if it panicked. Every
/// job-teardown path funnels its `join()` through here so a worker panic is
/// contained (logged) instead of propagating into the event loop.
fn handle_worker_result(handle: JoinHandle<()>, context: &str) -> bool {
    match handle.join() {
        Ok(()) => true,
        Err(panic_payload) => {
            // `join()` yields `Box<dyn Any>`; `{:?}` on it only prints "Any".
            // Downcast to the common panic payload types to log the real message.
            let msg = panic_payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic_payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic payload");
            debug_log!("{context}: {msg}");
            false
        }
    }
}

enum JobMessage {
    Progress(ops::batch::BatchProgress),
    Finished {
        report: ops::batch::BatchReport,
    },
    SearchFinished {
        outcome: Box<ops::search::SearchOutcome<FileEntry, ops::search::SearchError>>,
        pattern: String,
    },
}

pub struct RunningJob {
    receiver: Receiver<JobMessage>,
    pub cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    search_origin: Option<ActivePanel>,
}

impl RunningJob {
    pub fn shutdown(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Drain the channel we still own while waiting, bounded by the same
            // deadline as the Drop reaper. The progress channel is a bounded
            // `sync_channel`, so a worker blocked in `send()` on a full channel
            // would never reach its next `cancel` check; an unbounded `join()`
            // here would then deadlock teardown (we wait for the worker, the
            // worker waits for a free slot). Draining lets it proceed and observe
            // `cancel`.
            let start = std::time::Instant::now();
            while !handle.is_finished() && start.elapsed() < REAPER_JOIN_DEADLINE {
                while self.receiver.try_recv().is_ok() {}
                std::thread::sleep(REAPER_POLL_INTERVAL);
            }
            if handle.is_finished() {
                if let Err(e) = handle.join() {
                    debug_log!("worker thread panicked during shutdown: {:?}", e);
                }
            } else {
                debug_log!(
                    "worker thread did not finish within deadline during shutdown — detaching"
                );
            }
        }
    }
}

// The event loop already calls `shutdown()` (via `shutdown_job`) on every exit
// path, so by the time a job is dropped normally its `handle` is already taken
// and joined and this reaper does nothing. The reaper only runs as a
// last-resort safety net when a job is dropped during panic unwinding, where a
// failed spawn or a worker holding a lock just means we log and detach rather
// than block teardown.
//
// Cancellation-granularity trade-off (`REAPER_JOIN_DEADLINE`):
// `cancel` is cooperative — the worker only observes it between its own cancel
// checks (in `ops::batch`). A single long syscall (e.g. copying one huge file)
// can run well past the 5 s deadline without ever re-checking `cancel`, so the
// reaper detaches it. Detaching is acceptable here because this path only runs
// during panic unwinding: the process is already heading for exit, and the OS
// reclaims the worker's file descriptors at process teardown. Finer-grained
// cancellation would have to live in the worker (`ops::batch`, outside this
// module); blocking teardown on a chunked copy instead would risk hanging a
// panicking process, so we keep the bounded wait + detach.
impl Drop for RunningJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Resource leak risk: if the reaper spawn fails the worker
            // thread is detached — its resources (including file
            // descriptors held by ops) remain alive until process exit.
            let result = std::thread::Builder::new()
                .name("job-reaper".into())
                .spawn(move || {
                    let start = std::time::Instant::now();
                    while !handle.is_finished() && start.elapsed() < REAPER_JOIN_DEADLINE {
                        std::thread::sleep(REAPER_POLL_INTERVAL);
                    }
                    if handle.is_finished() {
                        if let Err(e) = handle.join() {
                            debug_log!("worker thread panicked during tear-down: {:?}", e);
                        }
                    } else {
                        debug_log!("worker thread did not finish within 5 s — detaching reaper");
                    }
                });
            if let Err(e) = result {
                debug_log!(
                    "job-reaper spawn failed (resource leak): {:?} — \
                     consider calling shutdown() explicitly before dropping",
                    e
                );
            }
        }
    }
}

pub fn start_confirmed_action(state: &mut AppState, running_job: &mut Option<RunningJob>) {
    if running_job.is_some() {
        state.ui.status_message = Some(ANOTHER_JOB_RUNNING.to_string());
        return;
    }
    let action = match state.ui.pending_action.take() {
        Some(a) => a,
        None => return,
    };

    let action_label = action_label(&action);
    let (sender, receiver) = mpsc::sync_channel(128);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_worker = Arc::clone(&cancel);
    let handle = match thread::Builder::new()
        .name("batch-worker".into())
        .spawn(move || {
            let progress_sender = sender.clone();
            let report = ops::batch::execute_batch_with_byte_progress(
                action,
                move |progress| {
                    let _ = progress_sender.send(JobMessage::Progress(progress));
                },
                &Some(cancel_for_worker),
                action_label,
            );
            let _ = sender.send(JobMessage::Finished { report });
        }) {
        Ok(handle) => handle,
        Err(e) => {
            // OS refused a new thread (resource exhaustion): report and bail
            // instead of panicking as bare `thread::spawn` would.
            debug_log!("failed to spawn batch worker: {e}");
            state.ui.status_message = Some(format!("Failed to start {action_label}: {e}"));
            return;
        }
    };

    state.active_panel_mut().clear_selection();
    state.ui.status_message = None;
    state.mode = AppMode::Dialog(DialogKind::progress(
        format!("{action_label} starting..."),
        0.0,
        true,
    ));
    *running_job = Some(RunningJob {
        receiver,
        cancel,
        handle: Some(handle),
        search_origin: None,
    });
}

pub fn start_search_job(state: &mut AppState, running_job: &mut Option<RunningJob>, pattern: &str) {
    if running_job.is_some() {
        state.ui.status_message = Some(ANOTHER_JOB_RUNNING.to_string());
        return;
    }
    let dir = state.active_panel().path().to_path_buf();
    let pattern_owned = pattern.to_string();
    let (sender, receiver) = mpsc::sync_channel(64);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = Arc::clone(&cancel);

    let handle = match thread::Builder::new()
        .name("search-worker".into())
        .spawn(move || {
            let outcome = ops::search_files(&dir, &pattern_owned, true, false, Some(&cancel_clone));
            let _ = sender.send(JobMessage::SearchFinished {
                outcome: Box::new(outcome),
                pattern: pattern_owned,
            });
        }) {
        Ok(handle) => handle,
        Err(e) => {
            debug_log!("failed to spawn search worker: {e}");
            state.ui.status_message = Some(format!("Failed to start search: {e}"));
            return;
        }
    };

    let search_origin = state.active_panel;
    state.ui.status_message = None;
    state.mode = AppMode::Dialog(DialogKind::progress(
        format!("Searching for '{}'...", pattern),
        0.0,
        true,
    ));
    state.input.dialog_input.clear();
    *running_job = Some(RunningJob {
        receiver,
        cancel,
        handle: Some(handle),
        search_origin: Some(search_origin),
    });
}

pub fn poll_running_job(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    refresh_both: fn(&mut AppState),
) -> bool {
    let Some(job) = running_job.as_mut() else {
        return false;
    };
    let mut dirty = false;
    let mut finished = None;
    let mut finished_search = None;
    let mut latest_progress: Option<ops::batch::BatchProgress> = None;

    while let Ok(message) = job.receiver.try_recv() {
        match message {
            JobMessage::Progress(progress) => {
                latest_progress = Some(progress);
            }
            JobMessage::Finished { report } => {
                // `finish_running_job` (run unconditionally below when `finished`
                // is set) switches the mode to `AppMode::Normal`, and the caller
                // only renders after this function returns — so any progress
                // dialog built here would be overwritten before it could be
                // displayed. Just drop the pending progress so the second pass
                // below also skips the now-pointless formatting.
                latest_progress = None;
                finished = Some(report);
            }
            JobMessage::SearchFinished { outcome, pattern } => {
                finished_search = Some((outcome, pattern));
            }
        }
    }

    // Handles the case where Progress arrived but Finished did not (or arrives in
    // a later poll cycle). When `Finished` was seen this cycle, the arm above
    // cleared `latest_progress`, because `finish_running_job` owns the final mode
    // and would overwrite any dialog built here before the next render.
    if let Some(progress) = latest_progress {
        let msg = format_progress_message(&progress, job.cancel.load(Ordering::Relaxed));
        state.mode = AppMode::Dialog(DialogKind::progress(
            msg,
            progress.byte_percent() / 100.0,
            true,
        ));
        dirty = true;
    }

    // Handle `Finished` and `SearchFinished` as independent `if`s rather than an
    // `if/else if` chain: a worker sends exactly one of them today, but should the
    // protocol ever emit both in a single poll cycle, the second would otherwise
    // be silently dropped. The `handled_final` flag keeps the no-report fallback
    // below mutually exclusive with both terminal paths.
    let mut handled_final = false;
    if let Some(report) = finished {
        if let Some(mut job) = running_job.take()
            && let Some(handle) = job.handle.take()
        {
            handle_worker_result(handle, "worker thread panicked after Finished");
        }
        finish_running_job(state, &report, refresh_both);
        dirty = true;
        handled_final = true;
    }
    if let Some((outcome, pattern)) = finished_search {
        // Capture `search_origin` in a local before `take()` consumes the job.
        let search_origin = running_job.as_ref().and_then(|j| j.search_origin);
        if let Some(mut job) = running_job.take()
            && let Some(handle) = job.handle.take()
        {
            handle_worker_result(handle, "search worker panicked after SearchFinished");
        }
        finish_search_job(state, &outcome, &pattern, search_origin, refresh_both);
        dirty = true;
        handled_final = true;
    }
    if !handled_final && let Some(job) = running_job.as_mut() {
        let worker_finished = job.handle.as_ref().is_some_and(JoinHandle::is_finished);
        if worker_finished && let Some(handle) = job.handle.take() {
            let exited_cleanly =
                handle_worker_result(handle, "worker thread panicked (no Finished)");
            if exited_cleanly {
                debug_log!("worker exited normally without sending Finished — cleaning up");
            }
            let _ = running_job.take();
            state.mode = AppMode::Normal;
            if let Some(panel) = state.ui.menu_restore_panel.take() {
                state.active_panel = panel;
            }
            state.ui.status_message = Some(if exited_cleanly {
                "Operation completed (worker finished without report)".to_string()
            } else {
                "Operation failed: worker thread panicked".to_string()
            });
            refresh_both(state);
            dirty = true;
        }
    }

    dirty
}

fn format_progress_message(progress: &ops::batch::BatchProgress, canceling: bool) -> String {
    use std::fmt::Write;

    let mut buf = String::with_capacity(128);
    if canceling {
        buf.push_str("Canceling: ");
    }
    let _ = write!(buf, "{} of {}: ", progress.completed, progress.total);
    if let Some(path) = progress.current.as_ref().and_then(|p| p.file_name()) {
        let _ = buf.write_str(&path.to_string_lossy());
    } else {
        buf.push_str("done");
    }
    if progress.bytes_total > 0 {
        buf.push_str(" | ");
        let _ = write!(
            buf,
            "{} / {} | {}/s",
            crate::app::types::format_size(progress.bytes_done),
            crate::app::types::format_size(progress.bytes_total),
            crate::app::types::format_size(progress.speed().round() as u64),
        );
        if let Some(eta) = progress.eta() {
            buf.push_str(" | ETA ");
            let _ = write!(buf, "{}", format_duration_short(eta));
        }
    }
    buf
}

fn format_duration_short(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn finish_running_job(
    state: &mut AppState,
    report: &ops::batch::BatchReport,
    refresh_both: fn(&mut AppState),
) {
    state.ui.status_message = Some(report.format_summary());
    state.mode = AppMode::Normal;
    if let Some(panel) = state.ui.menu_restore_panel.take() {
        state.active_panel = panel;
    }
    refresh_both(state);
}

fn finish_search_job(
    state: &mut AppState,
    outcome: &ops::search::SearchOutcome<FileEntry, ops::search::SearchError>,
    pattern: &str,
    search_origin: Option<ActivePanel>,
    refresh_both: fn(&mut AppState),
) {
    let result_count = outcome.matches.len();
    if let Some(first) = outcome.matches.first()
        && let Some(parent) = first.path.parent()
    {
        let path = parent.to_path_buf();
        {
            state.panel_or_active_mut(search_origin).set_path(path);
        }
        refresh_both(state);

        let panel = state.panel_or_active_mut(search_origin);
        // Resolve the index first so the borrowing iterator is dropped before the
        // mutable cursor update below.
        let pos = panel.listing.filtered().position(|e| e.path == first.path);
        if let Some(pos) = pos {
            panel.cursor = pos;
            panel.ensure_cursor_visible(crate::app::panel_ops::current_visible_height());
        }
    } else {
        refresh_both(state);
    }
    let mut msg = if result_count > 0 {
        format!("Found {result_count} match(es) for '{pattern}'")
    } else {
        format!("No matches for '{pattern}'")
    };
    if !outcome.errors.is_empty() {
        msg.push_str(&format!(", {} error(s)", outcome.errors.len()));
    }
    if let Some(reason) = outcome.truncated {
        let label = match reason {
            ops::search::TruncationReason::DepthLimit => "depth limit",
            ops::search::TruncationReason::ItemLimit => "item limit",
            ops::search::TruncationReason::ContentResultLimit => "result limit",
            ops::search::TruncationReason::FileTooLarge => "file too large",
            ops::search::TruncationReason::LineTooLong => "line too long",
            ops::search::TruncationReason::BinaryFile => "binary file",
        };
        msg.push_str(&format!(", truncated ({label})"));
    }
    state.ui.status_message = Some(msg);
    state.mode = AppMode::Normal;
    if let Some(panel) = state.ui.menu_restore_panel.take() {
        state.active_panel = panel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn format_duration_short_uses_clock_format() {
        assert_eq!(format_duration_short(Duration::from_secs(15)), "00:15");
        assert_eq!(format_duration_short(Duration::from_secs(75)), "01:15");
        assert_eq!(
            format_duration_short(Duration::from_secs(3_665)),
            "01:01:05"
        );
    }

    #[test]
    fn format_progress_message_uses_item_fallback_without_bytes() {
        let progress = ops::batch::BatchProgress::new(3, 10, Some(PathBuf::from("file.txt")));

        assert_eq!(
            format_progress_message(&progress, false),
            "3 of 10: file.txt"
        );
        assert_eq!(
            format_progress_message(&progress, true),
            "Canceling: 3 of 10: file.txt"
        );
    }
}
