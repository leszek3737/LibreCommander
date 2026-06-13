use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::types::PendingAction;
use crate::ops::{archive, file_ops, helpers};

fn effective_cancel(cancel: &Option<Arc<AtomicBool>>) -> Arc<AtomicBool> {
    cancel
        .clone()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)))
}

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
            "Extract" => "Extracted",
            "Archive" => "Archived",
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

    #[must_use]
    fn with_clamped_bytes(mut self) -> Self {
        self.bytes_done = self.bytes_done.min(self.bytes_total);
        self.current_file_bytes = self.current_file_bytes.min(self.current_file_total);
        self
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
    match action {
        PendingAction::Copy(t) => batch_copy(
            &t.sources,
            &t.dest,
            &mut progress,
            cancel,
            t.overwrite,
            action_label,
        ),
        PendingAction::Move(t) => batch_move(
            &t.sources,
            &t.dest,
            &mut progress,
            cancel,
            t.overwrite,
            action_label,
        ),
        PendingAction::Delete { paths } => {
            batch_delete(&paths, &mut progress, cancel, action_label)
        }
        PendingAction::ExtractArchive { source, dest, .. } => {
            batch_extract_archive(&source, &dest, &mut progress, cancel, action_label)
        }
        PendingAction::CreateArchive {
            sources,
            dest,
            format,
            ..
        } => batch_create_archive(&sources, &dest, format, &mut progress, cancel, action_label),
    }
}

fn is_canceled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

enum EntryKind {
    Symlink,
    Dir,
    File,
}

fn classify_entry(path: &Path) -> io::Result<EntryKind> {
    let meta = path.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        Ok(EntryKind::Symlink)
    } else if meta.is_dir() {
        Ok(EntryKind::Dir)
    } else {
        Ok(EntryKind::File)
    }
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
    progress(
        BatchProgress {
            completed: snapshot.completed,
            total: snapshot.total,
            current: snapshot.current.map(Path::to_path_buf),
            bytes_done: snapshot.bytes_done,
            bytes_total: snapshot.bytes_total,
            current_file_bytes: snapshot.current_file_bytes,
            current_file_total: snapshot.current_file_total,
            start_time: Some(snapshot.start_time),
        }
        .with_clamped_bytes(),
    );
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
            current: ctx.sources.get(completed).map(PathBuf::as_path),
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
    match classify_entry(src)? {
        EntryKind::Symlink => file_ops::copy_symlink(src, dest, overwrite).map(|_| ()),
        EntryKind::Dir => {
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
        EntryKind::File => {
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

#[inline]
fn drain_progress(rx: &mpsc::Receiver<u64>, mut consume: impl FnMut(u64)) {
    for delta in rx.try_iter() {
        consume(delta);
    }
}

fn wait_for_result_with_progress<T>(
    result_rx: &mpsc::Receiver<io::Result<T>>,
    progress_rx: &mpsc::Receiver<u64>,
    mut on_progress: &mut dyn FnMut(u64),
) -> io::Result<T> {
    loop {
        match result_rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => {
                drain_progress(progress_rx, &mut on_progress);
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                drain_progress(progress_rx, &mut on_progress);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                drain_progress(progress_rx, &mut on_progress);
                return Err(io::Error::other(
                    "worker thread disconnected unexpectedly — \
                     the spawned thread may have panicked or dropped the result sender",
                ));
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
    action_label: &'static str,
) -> BatchReport {
    let cancel_token = effective_cancel(cancel);
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| copy_entry(src, dest, &cancel_token, on_progress, overwrite),
        progress,
        cancel,
        action_label,
    )
}

fn batch_move(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    overwrite: bool,
    action_label: &'static str,
) -> BatchReport {
    let cancel_token = effective_cancel(cancel);
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| move_entry(src, dest, &cancel_token, on_progress, overwrite),
        progress,
        cancel,
        action_label,
    )
}

fn execute_batch_generic<F>(
    sources: &[PathBuf],
    dest_dir: &Path,
    mut action: F,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    action_label: &'static str,
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

    // Final check: catch cancel set during progress callback on the last item
    // (the top-of-loop check won't run when there are no more items).
    if !state.canceled && is_canceled(cancel) {
        state.canceled = true;
    }

    BatchReport {
        errors: state.errors,
        success_count: state.success_count,
        canceled: state.canceled,
        action_label,
    }
}

struct ByteProgress<'a, P> {
    progress: &'a mut P,
    ctx: &'a ProgressCtx<'a>,
    bytes_done: u64,
    bytes_total: u64,
    file_bytes_so_far: u64,
    idx: usize,
    current_total: u64,
}

impl<'a, P: FnMut(BatchProgress)> ByteProgress<'a, P> {
    fn on_delta(&mut self, src: &Path, byte_delta: u64) {
        self.file_bytes_so_far = self.file_bytes_so_far.saturating_add(byte_delta);
        if self.current_total > 0 {
            self.file_bytes_so_far = self.file_bytes_so_far.min(self.current_total);
        }
        let current_bytes_done = self.bytes_done.saturating_add(self.file_bytes_so_far);
        self.bytes_total = self.bytes_total.max(current_bytes_done);
        report_file_active(
            self.progress,
            self.ctx,
            FileProgress {
                idx: self.idx,
                src,
                bytes_done: current_bytes_done,
                bytes_total: self.bytes_total,
                file_bytes: self.file_bytes_so_far,
                file_total: self.current_total.max(self.file_bytes_so_far),
            },
        );
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

    let mut bp = ByteProgress {
        progress,
        ctx,
        bytes_done: state.bytes_done,
        bytes_total: state.bytes_total,
        file_bytes_so_far: 0,
        idx,
        current_total,
    };
    let result = action(src, &target, &mut |byte_delta| bp.on_delta(src, byte_delta));

    state.bytes_done = bp.bytes_done.saturating_add(bp.file_bytes_so_far);
    state.bytes_total = bp.bytes_total.max(state.bytes_done);

    if let Err(e) = result {
        if e.kind() == io::ErrorKind::Interrupted && is_canceled(cancel) {
            state.canceled = true;
        }
        state.errors.push(format!("{}: {}", src.display(), e));
    } else {
        state.success_count += 1;
        let remainder = current_total.saturating_sub(bp.file_bytes_so_far);
        state.bytes_done = state.bytes_done.saturating_add(remainder);
        state.bytes_total = state.bytes_total.max(state.bytes_done);
    }

    report_transition(progress, ctx, idx + 1, state.bytes_done, state.bytes_total);
}

#[cfg(unix)]
mod identity {
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    #[derive(Hash, Eq, PartialEq)]
    pub struct Identity {
        dev: u64,
        ino: u64,
    }

    pub fn get_identity(path: &Path) -> io::Result<Identity> {
        let meta = path.symlink_metadata()?;
        Ok(Identity {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
}

#[cfg(not(unix))]
mod identity {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    #[derive(Hash, Eq, PartialEq)]
    pub struct Identity {
        path: PathBuf,
    }

    pub fn get_identity(path: &Path) -> io::Result<Identity> {
        let _meta = path.symlink_metadata()?;
        Ok(Identity {
            path: path.to_path_buf(),
        })
    }
}

use identity::Identity;

fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen_identities: HashSet<Identity> = HashSet::new();
    let mut identity_ok: Vec<PathBuf> = Vec::new();
    let mut identity_fail: Vec<PathBuf> = Vec::new();

    for p in paths {
        match identity::get_identity(p) {
            Ok(id) => {
                if !seen_identities.insert(id) {
                    continue;
                }
                identity_ok.push(p.clone());
            }
            Err(_) => {
                identity_fail.push(p.clone());
            }
        }
    }

    // PERF: sort + O(n·depth) ancestors check. For very large batches (100k+)
    // this may become a bottleneck; profile before considering a trie/hash
    // prefix tree to reduce worst-case from O(n·depth) to amortized O(n).
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
    action_label: &'static str,
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
            current: filtered.first().map(PathBuf::as_path),
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
        let result = match classify_entry(path) {
            Ok(EntryKind::Symlink) => file_ops::delete_file(path),
            Ok(EntryKind::Dir) => match cancel.as_deref() {
                Some(cancel) => file_ops::delete_dir_recursive_cancelable(path, cancel),
                None => file_ops::delete_dir_recursive(path),
            },
            Ok(EntryKind::File) => file_ops::delete_file(path),
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
                current: filtered.get(idx + 1).map(PathBuf::as_path),
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

    // Final check: catch cancel set during the last item's deletion
    if !canceled && is_canceled(cancel) {
        canceled = true;
    }

    BatchReport {
        errors,
        success_count,
        canceled,
        action_label,
    }
}

fn batch_extract_archive(
    source: &Path,
    dest: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    let start_time = Instant::now();
    let source_size = helpers::path_size(source).unwrap_or(0);
    let cancel_token = effective_cancel(cancel);

    report_progress(
        progress,
        &ProgressSnapshot {
            completed: 0,
            total: 1,
            current: Some(source),
            bytes_done: 0,
            bytes_total: source_size,
            current_file_bytes: 0,
            current_file_total: source_size,
            start_time,
        },
    );

    let (progress_tx, progress_rx) = mpsc::channel::<u64>();
    let (result_tx, result_rx) = mpsc::channel::<Result<(), archive::ArchiveError>>();

    let cancel_clone = Arc::clone(&cancel_token);
    let source_buf = source.to_path_buf();
    let dest_buf = dest.to_path_buf();

    thread::scope(|scope| {
        scope.spawn(|| {
            let result = archive::extract::extract_archive(
                &source_buf,
                &dest_buf,
                &progress_tx,
                &cancel_clone,
            );
            let _ = result_tx.send(result);
        });

        let mut bytes_done = 0_u64;
        loop {
            match result_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    let errors = match result {
                        Ok(()) => Vec::new(),
                        Err(ref e) => vec![format!("{}: {e}", source.display())],
                    };
                    let is_interrupt = matches!(&result, Err(archive::ArchiveError::Io(e)) if e.kind() == io::ErrorKind::Interrupted);
                    return BatchReport {
                        errors,
                        success_count: if result.is_ok() { 1 } else { 0 },
                        canceled: is_interrupt && is_canceled(cancel),
                        action_label,
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    report_progress(
                        progress,
                        &ProgressSnapshot {
                            completed: 0,
                            total: 1,
                            current: Some(source),
                            bytes_done,
                            bytes_total: source_size.max(bytes_done),
                            current_file_bytes: bytes_done,
                            current_file_total: source_size.max(bytes_done),
                            start_time,
                        },
                    );
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    return BatchReport {
                        errors: vec![format!(
                            "{}: operation worker disconnected",
                            source.display()
                        )],
                        success_count: 0,
                        canceled: is_canceled(cancel),
                        action_label,
                    };
                }
            }
        }
    })
}

fn batch_create_archive(
    sources: &[PathBuf],
    dest: &Path,
    format: archive::ArchiveFormat,
    progress: &mut impl FnMut(BatchProgress),
    cancel: &Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    let start_time = Instant::now();
    let total_size = helpers::sum_sizes(&helpers::path_sizes(sources));
    let cancel_token = effective_cancel(cancel);

    report_progress(
        progress,
        &ProgressSnapshot {
            completed: 0,
            total: 1,
            current: Some(dest),
            bytes_done: 0,
            bytes_total: total_size,
            current_file_bytes: 0,
            current_file_total: total_size,
            start_time,
        },
    );

    let (progress_tx, progress_rx) = mpsc::channel::<u64>();
    let (result_tx, result_rx) = mpsc::channel::<Result<(), archive::ArchiveError>>();

    let cancel_clone = Arc::clone(&cancel_token);
    let sources_buf = sources.to_vec();
    let dest_buf = dest.to_path_buf();

    thread::scope(|scope| {
        scope.spawn(|| {
            let result = archive::create::create_archive(
                &sources_buf,
                &dest_buf,
                format,
                &progress_tx,
                &cancel_clone,
            );
            let _ = result_tx.send(result);
        });

        let mut bytes_done = 0_u64;
        loop {
            match result_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    let errors = match result {
                        Ok(()) => Vec::new(),
                        Err(ref e) => vec![format!("{}: {e}", dest.display())],
                    };
                    let is_interrupt = matches!(&result, Err(archive::ArchiveError::Io(e)) if e.kind() == io::ErrorKind::Interrupted);
                    return BatchReport {
                        errors,
                        success_count: if result.is_ok() { 1 } else { 0 },
                        canceled: is_interrupt && is_canceled(cancel),
                        action_label,
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    report_progress(
                        progress,
                        &ProgressSnapshot {
                            completed: 0,
                            total: 1,
                            current: Some(dest),
                            bytes_done,
                            bytes_total: total_size.max(bytes_done),
                            current_file_bytes: bytes_done,
                            current_file_total: total_size.max(bytes_done),
                            start_time,
                        },
                    );
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    drain_progress(&progress_rx, |d| bytes_done = bytes_done.saturating_add(d));
                    return BatchReport {
                        errors: vec![format!("{}: operation worker disconnected", dest.display())],
                        success_count: 0,
                        canceled: is_canceled(cancel),
                        action_label,
                    };
                }
            }
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
