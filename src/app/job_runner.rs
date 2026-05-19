use crate::debug_log;
use crate::ops::helpers::action_label;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::ops;

use super::types::{ActivePanel, AppMode, AppState, DialogKind, FileEntry};

enum JobMessage {
    Progress(ops::batch::BatchProgress),
    Finished {
        report: ops::batch::BatchReport,
    },
    SearchFinished {
        outcome: Box<ops::search::SearchOutcome<FileEntry>>,
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
        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            debug_log!("worker thread panicked during shutdown: {:?}", e);
        }
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            std::thread::spawn(move || {
                if let Err(e) = handle.join() {
                    debug_log!("worker thread panicked during tear-down: {:?}", e);
                }
            });
        }
    }
}

pub fn start_confirmed_action(state: &mut AppState, running_job: &mut Option<RunningJob>) {
    if running_job.is_some() {
        state.status_message = Some("Another job is already running".to_string());
        return;
    }
    let action = match state.pending_action.take() {
        Some(a) => a,
        None => return,
    };

    let action_label = action_label(&action);
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_worker = Arc::clone(&cancel);
    let handle = thread::spawn(move || {
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
    });

    state.active_panel_mut().clear_selection();
    state.status_message = None;
    state.mode = AppMode::Dialog(DialogKind::Progress(
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
        state.status_message = Some("Another job is already running".to_string());
        return;
    }
    let dir = state.active_panel().path.clone();
    let pattern_owned = pattern.to_string();
    let (sender, receiver) = mpsc::sync_channel(64);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = Arc::clone(&cancel);

    let handle = thread::spawn(move || {
        let outcome = ops::FileSearch::search_files_with_diagnostics_cancellable(
            &dir,
            &pattern_owned,
            true,
            false,
            &cancel_clone,
        );
        let _ = sender.send(JobMessage::SearchFinished {
            outcome: Box::new(outcome),
            pattern: pattern_owned,
        });
    });

    let search_origin = state.active_panel;
    state.status_message = None;
    state.mode = AppMode::Dialog(DialogKind::Progress(
        format!("Searching for '{}'...", pattern),
        0.0,
        true,
    ));
    state.dialog_input.clear();
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
                if let Some(progress) = latest_progress.take() {
                    let msg =
                        format_progress_message(&progress, job.cancel.load(Ordering::Relaxed));
                    state.mode = AppMode::Dialog(DialogKind::Progress(
                        msg,
                        progress.byte_percent() / 100.0,
                        true,
                    ));
                    dirty = true;
                }
                finished = Some(report);
            }
            JobMessage::SearchFinished { outcome, pattern } => {
                finished_search = Some((outcome, pattern));
            }
        }
    }

    if let Some(progress) = latest_progress {
        let msg = format_progress_message(&progress, job.cancel.load(Ordering::Relaxed));
        state.mode = AppMode::Dialog(DialogKind::Progress(
            msg,
            progress.byte_percent() / 100.0,
            true,
        ));
        dirty = true;
    }

    if let Some(report) = finished {
        if let Some(mut job) = running_job.take()
            && let Some(handle) = job.handle.take()
            && let Err(panic_payload) = handle.join()
        {
            debug_log!("worker thread panicked after Finished: {:?}", panic_payload);
        }
        finish_running_job(state, &report, refresh_both);
        dirty = true;
    } else if let Some((outcome, pattern)) = finished_search {
        let search_origin = running_job.as_ref().and_then(|j| j.search_origin);
        if let Some(mut job) = running_job.take()
            && let Some(handle) = job.handle.take()
            && let Err(panic_payload) = handle.join()
        {
            debug_log!(
                "search worker panicked after SearchFinished: {:?}",
                panic_payload
            );
        }
        finish_search_job(state, &outcome, &pattern, search_origin, refresh_both);
        dirty = true;
    } else if let Some(job) = running_job.as_mut() {
        // No Finished received — check if worker died (panicked before sending)
        let worker_finished = job.handle.as_ref().is_some_and(JoinHandle::is_finished);
        if worker_finished
            && let Some(handle) = job.handle.take()
            && let Err(panic_payload) = handle.join()
        {
            debug_log!("worker thread panicked (no Finished): {:?}", panic_payload);
            let _ = running_job.take();
            state.mode = AppMode::Normal;
            if let Some(panel) = state.menu_restore_panel.take() {
                state.active_panel = panel;
            }
            state.status_message = Some("Operation failed: worker thread panicked".to_string());
            refresh_both(state);
            dirty = true;
        }
    }

    dirty
}

fn format_progress_message(progress: &ops::batch::BatchProgress, canceling: bool) -> String {
    let current = progress
        .current
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "done".to_string());

    let mut message = format!("{} of {}: {current}", progress.completed, progress.total);
    if progress.bytes_total > 0 {
        message.push_str(&format!(
            " | {} / {} | {}/s",
            ops::batch::BatchProgress::format_bytes(progress.bytes_done),
            ops::batch::BatchProgress::format_bytes(progress.bytes_total),
            ops::batch::BatchProgress::format_bytes(progress.speed() as u64),
        ));
        if let Some(eta) = progress.eta() {
            message.push_str(&format!(" | ETA {}", format_duration_short(eta)));
        }
    }

    if canceling {
        format!("Canceling: {message}")
    } else {
        message
    }
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
    state.status_message = Some(report.format_summary());
    state.mode = AppMode::Normal;
    if let Some(panel) = state.menu_restore_panel.take() {
        state.active_panel = panel;
    }
    refresh_both(state);
}

fn finish_search_job(
    state: &mut AppState,
    outcome: &ops::search::SearchOutcome<FileEntry>,
    pattern: &str,
    search_origin: Option<ActivePanel>,
    refresh_both: fn(&mut AppState),
) {
    let result_count = outcome.matches.len();
    if let Some(first) = outcome.matches.first()
        && let Some(parent) = first.path.parent()
    {
        let path = parent.to_path_buf();
        match search_origin {
            Some(ActivePanel::Left) => state.left_panel.set_path(path),
            Some(ActivePanel::Right) => state.right_panel.set_path(path),
            None => state.active_panel_mut().set_path(path),
        };
        refresh_both(state);

        let panel = match search_origin {
            Some(ActivePanel::Left) => &mut state.left_panel,
            Some(ActivePanel::Right) => &mut state.right_panel,
            None => state.active_panel_mut(),
        };
        if let Some(pos) = panel
            .listing
            .entries
            .iter()
            .position(|e| e.path == first.path)
        {
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
    state.status_message = Some(msg);
    state.mode = AppMode::Normal;
    if let Some(panel) = state.menu_restore_panel.take() {
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
