use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::types::PendingAction;
use crate::ops::{file_ops, helpers};

pub struct BatchReport {
    pub errors: Vec<String>,
    pub success_count: usize,
    pub canceled: bool,
    pub action_label: &'static str,
}

impl BatchReport {
    #[inline]
    pub fn format_summary(&self) -> String {
        let verb = match self.action_label {
            "Copy" => "Copied",
            "Move" => "Moved",
            "Delete" => "Deleted",
            other => other,
        };
        let error_count = self.errors.len();

        if self.canceled {
            if self.success_count == 0 {
                format!("{verb} canceled")
            } else {
                format!("{verb} canceled after {} file(s)", self.success_count)
            }
        } else if error_count == 0 {
            if self.success_count == 1 {
                format!("{verb} 1 file")
            } else {
                format!("{verb} {} files", self.success_count)
            }
        } else if self.success_count == 0 {
            if error_count == 1 {
                format!("{verb} failed: {}", self.errors[0])
            } else {
                format!("{verb} failed: {error_count} error(s)")
            }
        } else {
            format!(
                "{verb} {} file(s), {error_count} error(s)",
                self.success_count
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgress {
    pub completed: usize,
    pub total: usize,
    pub current: Option<PathBuf>,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_file_bytes: u64,
    pub current_file_total: u64,
    pub start_time: Option<Instant>,
}

impl BatchProgress {
    #[cfg(test)]
    pub fn new(completed: usize, total: usize, current: Option<PathBuf>) -> Self {
        Self {
            completed,
            total,
            current,
            bytes_done: 0,
            bytes_total: 0,
            current_file_bytes: 0,
            current_file_total: 0,
            start_time: None,
        }
    }

    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.completed as f32 / self.total as f32
        }
    }

    pub fn byte_percent(&self) -> f32 {
        if self.bytes_total == 0 {
            self.percent() * 100.0
        } else if self.bytes_done >= self.bytes_total {
            100.0
        } else {
            (self.bytes_done as f32 / self.bytes_total as f32 * 100.0).min(99.99)
        }
    }

    pub fn speed(&self) -> f64 {
        match self.start_time {
            Some(t) => {
                let elapsed = t.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    self.bytes_done as f64 / elapsed
                } else {
                    0.0
                }
            }
            None => 0.0,
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let speed = self.speed();
        if speed <= 0.0 || self.bytes_total <= self.bytes_done {
            return None;
        }
        let remaining = (self.bytes_total - self.bytes_done) as f64 / speed;
        Some(Duration::from_secs_f64(remaining))
    }

    pub fn format_bytes(bytes: u64) -> String {
        crate::app::types::format_size(bytes)
    }
}

#[cfg(test)]
pub fn execute_batch(action: PendingAction) -> BatchReport {
    let label = helpers::action_label(&action);
    execute_batch_with_progress(action, |_| {}, &None, label)
}

#[cfg(test)]
pub fn execute_batch_with_progress(
    action: PendingAction,
    progress: impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    execute_batch_with_byte_progress(action, progress, cancel, action_label)
}

pub fn execute_batch_with_byte_progress(
    action: PendingAction,
    mut progress: impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    let mut report = match action {
        PendingAction::Copy {
            sources,
            dest,
            overwrite,
        } => batch_copy(&sources, &dest, &mut progress, cancel, overwrite),
        PendingAction::Move {
            sources,
            dest,
            overwrite,
        } => batch_move(&sources, &dest, &mut progress, cancel, overwrite),
        PendingAction::Delete { paths } => batch_delete(&paths, &mut progress, cancel),
    };
    report.action_label = action_label;
    report
}

fn is_canceled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

struct ProgressSnapshot<'a> {
    completed: usize,
    total: usize,
    current: Option<&'a Path>,
    bytes_done: u64,
    bytes_total: u64,
    current_file_bytes: u64,
    current_file_total: u64,
    start_time: Instant,
}

#[inline]
fn report_progress(progress: &mut impl FnMut(BatchProgress), snapshot: &ProgressSnapshot<'_>) {
    progress(BatchProgress {
        completed: snapshot.completed,
        total: snapshot.total,
        current: snapshot.current.map(Path::to_path_buf),
        bytes_done: snapshot.bytes_done,
        bytes_total: snapshot.bytes_total,
        current_file_bytes: snapshot.current_file_bytes,
        current_file_total: snapshot.current_file_total,
        start_time: Some(snapshot.start_time),
    });
}

struct ProgressCtx<'a> {
    total: usize,
    sources: &'a [PathBuf],
    sizes: &'a [u64],
    start_time: Instant,
    dest_dir: &'a Path,
    dest_dir_normalized: &'a Path,
}

struct BatchState {
    used_dests: HashSet<PathBuf>,
    bytes_done: u64,
    bytes_total: u64,
    errors: Vec<String>,
    success_count: usize,
    canceled: bool,
}

#[derive(Clone, Copy)]
struct FileProgress<'a> {
    idx: usize,
    src: &'a Path,
    bytes_done: u64,
    bytes_total: u64,
    file_bytes: u64,
    file_total: u64,
}

fn report_transition(
    progress: &mut impl FnMut(BatchProgress),
    ctx: &ProgressCtx<'_>,
    completed: usize,
    bytes_done: u64,
    bytes_total: u64,
) {
    report_progress(
        progress,
        &ProgressSnapshot {
            completed,
            total: ctx.total,
            current: helpers::next_path(ctx.sources, completed),
            bytes_done,
            bytes_total,
            current_file_bytes: 0,
            current_file_total: ctx.sizes.get(completed).copied().unwrap_or(0),
            start_time: ctx.start_time,
        },
    );
}

fn report_file_active(
    progress: &mut impl FnMut(BatchProgress),
    ctx: &ProgressCtx<'_>,
    file: FileProgress<'_>,
) {
    report_progress(
        progress,
        &ProgressSnapshot {
            completed: file.idx,
            total: ctx.total,
            current: Some(file.src),
            bytes_done: file.bytes_done,
            bytes_total: file.bytes_total,
            current_file_bytes: file.file_bytes,
            current_file_total: file.file_total,
            start_time: ctx.start_time,
        },
    );
}

fn copy_entry(
    src: &Path,
    dest: &Path,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(u64),
    overwrite: bool,
) -> io::Result<()> {
    match src.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            file_ops::copy_symlink(src, dest, overwrite).map(|_| ())
        }
        Ok(meta) if meta.is_dir() => {
            let (progress_tx, progress_rx) = mpsc::channel::<u64>();
            let (result_tx, result_rx) = mpsc::channel::<io::Result<u64>>();
            thread::scope(|scope| {
                scope.spawn(|| {
                    let result = file_ops::copy_dir_recursive_with_progress(
                        src,
                        dest,
                        &progress_tx,
                        cancel,
                        overwrite,
                    );
                    let _ = result_tx.send(result);
                });
                wait_for_result_with_progress(&result_rx, &progress_rx, on_progress).map(|_| ())
            })
        }
        Ok(_) => {
            let (progress_tx, progress_rx) = mpsc::channel::<u64>();
            let (result_tx, result_rx) = mpsc::channel::<io::Result<u64>>();
            thread::scope(|scope| {
                scope.spawn(|| {
                    let result = file_ops::copy_file_with_progress(
                        src,
                        dest,
                        &progress_tx,
                        cancel,
                        overwrite,
                    );
                    let _ = result_tx.send(result);
                });
                wait_for_result_with_progress(&result_rx, &progress_rx, on_progress).map(|_| ())
            })
        }
        Err(e) => Err(e),
    }
}

fn move_entry(
    src: &Path,
    dest: &Path,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(u64),
    overwrite: bool,
) -> io::Result<()> {
    let (progress_tx, progress_rx) = mpsc::channel::<u64>();
    let (result_tx, result_rx) = mpsc::channel::<io::Result<()>>();
    thread::scope(|scope| {
        scope.spawn(|| {
            let result =
                file_ops::move_entry_with_progress(src, dest, &progress_tx, cancel, overwrite);
            let _ = result_tx.send(result);
        });
        wait_for_result_with_progress(&result_rx, &progress_rx, on_progress)
    })
}

fn wait_for_result_with_progress<T>(
    result_rx: &mpsc::Receiver<io::Result<T>>,
    progress_rx: &mpsc::Receiver<u64>,
    on_progress: &mut dyn FnMut(u64),
) -> io::Result<T> {
    loop {
        match result_rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => {
                for bytes in progress_rx.try_iter() {
                    on_progress(bytes);
                }
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                for bytes in progress_rx.try_iter() {
                    on_progress(bytes);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                for bytes in progress_rx.try_iter() {
                    on_progress(bytes);
                }
                return Err(io::Error::other("operation worker failed"));
            }
        }
    }
}

fn batch_copy(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    overwrite: bool,
) -> BatchReport {
    let cancel_token = cancel
        .clone()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| copy_entry(src, dest, &cancel_token, on_progress, overwrite),
        progress,
        cancel,
    )
}

fn batch_move(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    overwrite: bool,
) -> BatchReport {
    let cancel_token = cancel
        .clone()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| move_entry(src, dest, &cancel_token, on_progress, overwrite),
        progress,
        cancel,
    )
}

fn execute_batch_generic<F>(
    sources: &[PathBuf],
    dest_dir: &Path,
    mut action: F,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
) -> BatchReport
where
    F: FnMut(&Path, &Path, &mut dyn FnMut(u64)) -> io::Result<()>,
{
    let total = sources.len();
    let sizes = helpers::path_sizes(sources);
    let mut state = BatchState {
        used_dests: HashSet::new(),
        bytes_done: 0,
        bytes_total: helpers::sum_sizes(&sizes),
        errors: Vec::new(),
        success_count: 0,
        canceled: false,
    };
    let start_time = Instant::now();
    let dest_dir_normalized = dest_dir
        .canonicalize()
        .unwrap_or_else(|_| dest_dir.to_path_buf());
    let ctx = ProgressCtx {
        total,
        sources,
        sizes: &sizes,
        start_time,
        dest_dir,
        dest_dir_normalized: &dest_dir_normalized,
    };

    report_transition(progress, &ctx, 0, state.bytes_done, state.bytes_total);

    for (idx, src) in sources.iter().enumerate() {
        if is_canceled(cancel) {
            state.canceled = true;
            break;
        }

        process_batch_entry(idx, src, &ctx, cancel, &mut action, progress, &mut state);

        if state.canceled {
            break;
        }
    }

    BatchReport {
        errors: state.errors,
        success_count: state.success_count,
        canceled: state.canceled,
        action_label: "Unknown",
    }
}

fn process_batch_entry<F>(
    idx: usize,
    src: &Path,
    ctx: &ProgressCtx<'_>,
    cancel: &Option<Arc<AtomicBool>>,
    action: &mut F,
    progress: &mut impl FnMut(BatchProgress),
    state: &mut BatchState,
) where
    F: FnMut(&Path, &Path, &mut dyn FnMut(u64)) -> io::Result<()>,
{
    let current_total = ctx.sizes[idx];
    report_file_active(
        progress,
        ctx,
        FileProgress {
            idx,
            src,
            bytes_done: state.bytes_done,
            bytes_total: state.bytes_total,
            file_bytes: 0,
            file_total: current_total,
        },
    );

    let file_name = src.file_name().unwrap_or_default();
    if file_name.is_empty() {
        state.errors.push(format!(
            "{}: cannot copy/move root or parent directory",
            src.display()
        ));
        state.bytes_done = state.bytes_done.saturating_add(ctx.sizes[idx]);
        report_transition(progress, ctx, idx + 1, state.bytes_done, state.bytes_total);
        return;
    }
    let target = ctx.dest_dir.join(file_name);
    let dedup_key = ctx.dest_dir_normalized.join(file_name);
    if !state.used_dests.insert(dedup_key) {
        state.errors.push(format!(
            "{}: duplicate destination {}",
            src.display(),
            target.display()
        ));
        state.bytes_done = state.bytes_done.saturating_add(ctx.sizes[idx]);
        report_transition(progress, ctx, idx + 1, state.bytes_done, state.bytes_total);
        return;
    }

    let mut file_bytes_so_far = 0_u64;
    let result = {
        let bytes_done = &mut state.bytes_done;
        let bytes_total = &mut state.bytes_total;
        action(src, &target, &mut |byte_delta: u64| {
            file_bytes_so_far = file_bytes_so_far.saturating_add(byte_delta);
            if current_total > 0 {
                file_bytes_so_far = file_bytes_so_far.min(current_total);
            }
            let current_bytes_done = bytes_done.saturating_add(file_bytes_so_far);
            *bytes_total = (*bytes_total).max(current_bytes_done);
            report_file_active(
                progress,
                ctx,
                FileProgress {
                    idx,
                    src,
                    bytes_done: current_bytes_done,
                    bytes_total: *bytes_total,
                    file_bytes: file_bytes_so_far,
                    file_total: current_total.max(file_bytes_so_far),
                },
            );
        })
    };

    state.bytes_done = state.bytes_done.saturating_add(file_bytes_so_far);
    state.bytes_total = state.bytes_total.max(state.bytes_done);

    if let Err(e) = result {
        if e.kind() == io::ErrorKind::Interrupted && is_canceled(cancel) {
            state.canceled = true;
        }
        state.errors.push(format!("{}: {}", src.display(), e));
    } else {
        state.success_count += 1;
        let remainder = current_total.saturating_sub(file_bytes_so_far);
        state.bytes_done = state.bytes_done.saturating_add(remainder);
        state.bytes_total = state.bytes_total.max(state.bytes_done);
    }

    report_transition(progress, ctx, idx + 1, state.bytes_done, state.bytes_total);
}

fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    #[cfg(unix)]
    let mut seen_ids: HashSet<(u64, u64)> = HashSet::new();
    #[cfg(not(unix))]
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut identity_ok: Vec<PathBuf> = Vec::new();
    let mut identity_fail: Vec<PathBuf> = Vec::new();

    for p in paths {
        #[cfg(unix)]
        {
            match p.symlink_metadata() {
                Ok(meta) => {
                    use std::os::unix::fs::MetadataExt;
                    let id = (meta.dev(), meta.ino());
                    if !seen_ids.insert(id) {
                        continue;
                    }
                    identity_ok.push(p.clone());
                }
                Err(_) => {
                    identity_fail.push(p.clone());
                }
            }
        }
        #[cfg(not(unix))]
        {
            if !seen_paths.insert(p.clone()) {
                continue;
            }
            match p.symlink_metadata() {
                Ok(_) => identity_ok.push(p.clone()),
                Err(_) => identity_fail.push(p.clone()),
            }
        }
    }

    identity_ok.sort_by_key(|p| p.components().count());
    let mut filtered: Vec<PathBuf> = Vec::new();
    let mut accepted: HashSet<&Path> = HashSet::with_capacity(identity_ok.len());
    for p in &identity_ok {
        let dominated = p.ancestors().skip(1).any(|a| accepted.contains(a));
        if !dominated {
            accepted.insert(p);
            filtered.push(p.clone());
        }
    }

    identity_fail.sort();
    identity_fail.dedup();
    filtered.extend(identity_fail);
    filtered
}

fn batch_delete(
    paths: &[PathBuf],
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let start_time = Instant::now();

    let filtered = dedup_paths(paths);

    let total = filtered.len();
    let sizes = helpers::path_sizes(&filtered);
    let bytes_total = helpers::sum_sizes(&sizes);
    let mut bytes_done = 0_u64;

    report_progress(
        progress,
        &ProgressSnapshot {
            completed: 0,
            total,
            current: helpers::next_path(&filtered, 0),
            bytes_done,
            bytes_total,
            current_file_bytes: 0,
            current_file_total: sizes.first().copied().unwrap_or(0),
            start_time,
        },
    );
    for (idx, path) in filtered.iter().enumerate() {
        if is_canceled(cancel) {
            canceled = true;
            break;
        }
        let current_total = sizes[idx];
        report_progress(
            progress,
            &ProgressSnapshot {
                completed: idx,
                total,
                current: Some(path),
                bytes_done,
                bytes_total,
                current_file_bytes: 0,
                current_file_total: current_total,
                start_time,
            },
        );
        let result = match path.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => file_ops::delete_file(path),
            Ok(meta) if meta.is_dir() => match cancel.as_deref() {
                Some(cancel) => file_ops::delete_dir_recursive_cancelable(path, cancel),
                None => file_ops::delete_dir_recursive(path),
            },
            Ok(_) => file_ops::delete_file(path),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            if e.kind() == io::ErrorKind::Interrupted && is_canceled(cancel) {
                canceled = true;
            }
            errors.push(format!("{}: {}", path.display(), e));
        } else {
            success_count += 1;
            bytes_done = bytes_done.saturating_add(current_total);
        }
        report_progress(
            progress,
            &ProgressSnapshot {
                completed: idx + 1,
                total,
                current: helpers::next_path(&filtered, idx + 1),
                bytes_done,
                bytes_total,
                current_file_bytes: 0,
                current_file_total: sizes.get(idx + 1).copied().unwrap_or(0),
                start_time,
            },
        );
        if canceled {
            break;
        }
    }

    BatchReport {
        errors,
        success_count,
        canceled,
        action_label: "Unknown",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
