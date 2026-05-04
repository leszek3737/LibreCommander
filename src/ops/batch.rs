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

pub fn execute_batch(action: PendingAction) -> BatchReport {
    let label = helpers::action_label(&action);
    execute_batch_with_progress(action, |_| {}, None, label)
}

pub fn execute_batch_with_progress(
    action: PendingAction,
    progress: impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    execute_batch_with_byte_progress(action, progress, cancel, action_label)
}

pub fn execute_batch_with_byte_progress(
    action: PendingAction,
    mut progress: impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
    action_label: &'static str,
) -> BatchReport {
    let mut report = match action {
        PendingAction::Copy { sources, dest } => batch_copy(&sources, &dest, &mut progress, cancel),
        PendingAction::Move { sources, dest } => batch_move(&sources, &dest, &mut progress, cancel),
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

#[allow(clippy::needless_pass_by_value)]
fn report_progress(progress: &mut impl FnMut(BatchProgress), snapshot: ProgressSnapshot<'_>) {
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
        ProgressSnapshot {
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
        ProgressSnapshot {
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
    cancel: Option<Arc<AtomicBool>>,
    on_progress: &mut dyn FnMut(u64),
) -> io::Result<()> {
    match src.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => file_ops::copy_symlink(src, dest).map(|_| ()),
        Ok(meta) if meta.is_dir() => {
            let cancel_token = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
            let (progress_tx, progress_rx) = mpsc::channel::<u64>();
            let (result_tx, result_rx) = mpsc::channel::<io::Result<u64>>();
            thread::scope(|scope| {
                scope.spawn(|| {
                    let result = file_ops::copy_dir_recursive_with_progress(
                        src,
                        dest,
                        &progress_tx,
                        &cancel_token,
                    );
                    let _ = result_tx.send(result);
                });
                wait_for_result_with_progress(result_rx, progress_rx, on_progress).map(|_| ())
            })
        }
        Ok(_) => {
            let cancel_token = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
            let (progress_tx, progress_rx) = mpsc::channel::<u64>();
            let (result_tx, result_rx) = mpsc::channel::<io::Result<u64>>();
            thread::scope(|scope| {
                scope.spawn(|| {
                    let result =
                        file_ops::copy_file_with_progress(src, dest, &progress_tx, &cancel_token);
                    let _ = result_tx.send(result);
                });
                wait_for_result_with_progress(result_rx, progress_rx, on_progress).map(|_| ())
            })
        }
        Err(e) => Err(e),
    }
}

fn move_entry(
    src: &Path,
    dest: &Path,
    cancel: Option<Arc<AtomicBool>>,
    on_progress: &mut dyn FnMut(u64),
) -> io::Result<()> {
    let cancel_token = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let (progress_tx, progress_rx) = mpsc::channel::<u64>();
    let (result_tx, result_rx) = mpsc::channel::<io::Result<()>>();
    thread::scope(|scope| {
        scope.spawn(|| {
            let result = file_ops::move_entry_with_progress(src, dest, &progress_tx, &cancel_token);
            let _ = result_tx.send(result);
        });
        wait_for_result_with_progress(result_rx, progress_rx, on_progress)
    })
}

#[allow(clippy::needless_pass_by_value)]
fn wait_for_result_with_progress<T>(
    result_rx: mpsc::Receiver<io::Result<T>>,
    progress_rx: mpsc::Receiver<u64>,
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
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let operation_cancel = cancel.clone();
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| copy_entry(src, dest, operation_cancel.clone(), on_progress),
        progress,
        cancel,
    )
}

fn batch_move(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let operation_cancel = cancel.clone();
    execute_batch_generic(
        paths,
        dest_dir,
        |src, dest, on_progress| move_entry(src, dest, operation_cancel.clone(), on_progress),
        progress,
        cancel,
    )
}

#[allow(clippy::needless_pass_by_value)]
fn execute_batch_generic<F>(
    sources: &[PathBuf],
    dest_dir: &Path,
    mut action: F,
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport
where
    F: FnMut(&Path, &Path, &mut dyn FnMut(u64)) -> io::Result<()>,
{
    let mut errors: Vec<String> = Vec::new();
    let mut used_dests: HashSet<PathBuf> = HashSet::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let total = sources.len();
    let sizes = helpers::path_sizes(sources);
    let mut bytes_total = helpers::sum_sizes(&sizes);
    let mut bytes_done = 0_u64;
    let start_time = Instant::now();
    let ctx = ProgressCtx {
        total,
        sources,
        sizes: &sizes,
        start_time,
    };

    report_transition(progress, &ctx, 0, bytes_done, bytes_total);

    for (idx, src) in sources.iter().enumerate() {
        if is_canceled(&cancel) {
            canceled = true;
            break;
        }
        let current_total = sizes[idx];
        report_file_active(
            progress,
            &ctx,
            FileProgress {
                idx,
                src,
                bytes_done,
                bytes_total,
                file_bytes: 0,
                file_total: current_total,
            },
        );

        let file_name = src.file_name().unwrap_or_default();
        let target = dest_dir.join(file_name);
        if !used_dests.insert(target.clone()) {
            errors.push(format!(
                "{}: duplicate destination {}",
                src.display(),
                target.display()
            ));
            report_transition(progress, &ctx, idx + 1, bytes_done, bytes_total);
            continue;
        }

        let mut file_bytes_so_far = 0_u64;
        let result = action(src, &target, &mut |byte_delta: u64| {
            file_bytes_so_far = file_bytes_so_far.saturating_add(byte_delta);
            if current_total > 0 {
                file_bytes_so_far = file_bytes_so_far.min(current_total);
            }
            let current_bytes_done = bytes_done.saturating_add(file_bytes_so_far);
            bytes_total = bytes_total.max(current_bytes_done);
            report_file_active(
                progress,
                &ctx,
                FileProgress {
                    idx,
                    src,
                    bytes_done: current_bytes_done,
                    bytes_total,
                    file_bytes: file_bytes_so_far,
                    file_total: current_total.max(file_bytes_so_far),
                },
            );
        });

        if let Err(e) = result {
            if e.kind() == io::ErrorKind::Interrupted && is_canceled(&cancel) {
                canceled = true;
            }
            errors.push(format!("{}: {}", src.display(), e));
        } else {
            success_count += 1;
            bytes_done = bytes_done.saturating_add(current_total.max(file_bytes_so_far));
            bytes_total = bytes_total.max(bytes_done);
        }

        report_transition(progress, &ctx, idx + 1, bytes_done, bytes_total);

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

#[allow(clippy::needless_pass_by_value)]
fn batch_delete(
    paths: &[PathBuf],
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let total = paths.len();
    let sizes = helpers::path_sizes(paths);
    let bytes_total = helpers::sum_sizes(&sizes);
    let mut bytes_done = 0_u64;
    let start_time = Instant::now();

    report_progress(
        progress,
        ProgressSnapshot {
            completed: 0,
            total,
            current: helpers::next_path(paths, 0),
            bytes_done,
            bytes_total,
            current_file_bytes: 0,
            current_file_total: sizes.first().copied().unwrap_or(0),
            start_time,
        },
    );
    for (idx, path) in paths.iter().enumerate() {
        if is_canceled(&cancel) {
            canceled = true;
            break;
        }
        let current_total = sizes[idx];
        report_progress(
            progress,
            ProgressSnapshot {
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
            if e.kind() == io::ErrorKind::Interrupted && is_canceled(&cancel) {
                canceled = true;
            }
            errors.push(format!("{}: {}", path.display(), e));
        } else {
            success_count += 1;
            bytes_done = bytes_done.saturating_add(current_total);
        }
        report_progress(
            progress,
            ProgressSnapshot {
                completed: idx + 1,
                total,
                current: helpers::next_path(paths, idx + 1),
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
mod tests {
    use super::*;
    use crate::app::types::PendingAction;
    use std::fs;
    use std::sync::atomic::Ordering;

    fn make_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn batch_copy_files_to_dest() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "a.txt", b"hello");
        let f2 = make_file(src_dir.path(), "b.txt", b"world");

        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!report.canceled);
        assert!(dest_dir.path().join("a.txt").exists());
        assert!(dest_dir.path().join("b.txt").exists());
    }

    #[test]
    fn batch_copy_duplicate_dest_reports_error() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "same.txt", b"a");
        let f2 = make_file(src_dir.path(), "same.txt", b"b");

        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(!report.canceled);
        assert!(report.errors[0].contains("duplicate destination"));
    }

    #[test]
    fn batch_move_files_to_dest() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "x.txt", b"data");
        let f2 = make_file(src_dir.path(), "y.txt", b"more");

        let action = PendingAction::Move {
            sources: vec![f1.clone(), f2.clone()],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!report.canceled);
        assert!(!f1.exists());
        assert!(!f2.exists());
        assert!(dest_dir.path().join("x.txt").exists());
        assert!(dest_dir.path().join("y.txt").exists());
    }

    #[test]
    fn batch_delete_files() {
        let dir = tempfile::tempdir().unwrap();

        let f1 = make_file(dir.path(), "del1.txt", b"a");
        let f2 = make_file(dir.path(), "del2.txt", b"b");

        let action = PendingAction::Delete {
            paths: vec![f1.clone(), f2.clone()],
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!report.canceled);
        assert!(!f1.exists());
        assert!(!f2.exists());
    }

    #[test]
    fn batch_delete_nonexistent_reports_error() {
        let action = PendingAction::Delete {
            paths: vec![PathBuf::from("/tmp/lc_nonexistent_test_file_xyz")],
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(!report.canceled);
    }

    #[test]
    fn batch_delete_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = make_file(dir.path(), "one.txt", b"1");
        let f2 = make_file(dir.path(), "two.txt", b"2");
        let action = PendingAction::Delete {
            paths: vec![f1, f2],
        };
        let mut updates = Vec::new();

        let report =
            execute_batch_with_progress(action, |progress| updates.push(progress), None, "Delete");

        assert_eq!(report.success_count, 2);
        assert!(!report.canceled);
        assert_eq!(
            updates.first().map(|p| (p.completed, p.total)),
            Some((0, 2))
        );
        assert_eq!(updates.last().map(|p| (p.completed, p.total)), Some((2, 2)));
        assert_eq!(updates.last().map(BatchProgress::percent), Some(1.0));
    }

    #[test]
    fn batch_copy_cancel_stops_between_items() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let f1 = make_file(src_dir.path(), "first.txt", b"1");
        let f2 = make_file(src_dir.path(), "second.txt", b"2");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_progress = Arc::clone(&cancel);
        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch_with_progress(
            action,
            |progress| {
                if progress.completed == 1 {
                    cancel_for_progress.store(true, Ordering::Relaxed);
                }
            },
            Some(cancel),
            "Copy",
        );

        assert_eq!(report.success_count, 1);
        assert!(report.canceled);
        assert!(dest_dir.path().join("first.txt").exists());
        assert!(!dest_dir.path().join("second.txt").exists());
    }

    #[test]
    fn batch_copy_reports_cumulative_byte_progress() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let f1 = make_file(src_dir.path(), "first.txt", b"12345");
        let f2 = make_file(src_dir.path(), "second.txt", b"1234567");
        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };
        let mut updates = Vec::new();

        let report = execute_batch_with_byte_progress(
            action,
            |progress| updates.push(progress),
            None,
            "Copy",
        );

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!report.canceled);
        assert_eq!(updates.first().map(|p| p.bytes_total), Some(12));
        assert_eq!(updates.last().map(|p| p.bytes_done), Some(12));
        assert_eq!(updates.last().map(|p| p.current_file_total), Some(0));
        assert!(updates.iter().any(|p| {
            p.current
                .as_ref()
                .is_some_and(|path| path.file_name().is_some_and(|name| name == "second.txt"))
                && p.bytes_done == 12
                && p.current_file_bytes == 7
                && p.current_file_total == 7
        }));
    }

    #[test]
    fn batch_copy_large_file_progress_never_exceeds_total() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let data = vec![b'x'; 128 * 1024 + 17];
        let file = make_file(src_dir.path(), "large.bin", &data);
        let action = PendingAction::Copy {
            sources: vec![file],
            dest: dest_dir.path().to_path_buf(),
        };
        let mut updates = Vec::new();

        let report = execute_batch_with_byte_progress(
            action,
            |progress| updates.push(progress),
            None,
            "Copy",
        );

        assert_eq!(report.success_count, 1);
        assert!(report.errors.is_empty());
        assert!(updates.iter().all(|p| p.bytes_done <= p.bytes_total));
        assert!(
            updates
                .iter()
                .all(|p| p.current_file_bytes <= p.current_file_total)
        );
        assert!(
            updates
                .windows(2)
                .all(|pair| pair[0].bytes_done <= pair[1].bytes_done)
        );
        assert_eq!(updates.last().map(BatchProgress::byte_percent), Some(100.0));
    }

    #[test]
    fn batch_delete_reports_item_byte_progress() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = make_file(dir.path(), "one.txt", b"123");
        let f2 = make_file(dir.path(), "two.txt", b"1234");
        let action = PendingAction::Delete {
            paths: vec![f1, f2],
        };
        let mut updates = Vec::new();

        let report = execute_batch_with_byte_progress(
            action,
            |progress| updates.push(progress),
            None,
            "Delete",
        );

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!report.canceled);
        assert_eq!(updates.first().map(|p| p.bytes_total), Some(7));
        assert_eq!(updates.last().map(|p| p.bytes_done), Some(7));
        assert_eq!(updates.last().map(BatchProgress::byte_percent), Some(100.0));
    }

    #[test]
    fn format_summary_copy_success() {
        let report = BatchReport {
            errors: vec![],
            success_count: 3,
            canceled: false,
            action_label: "Copy",
        };
        assert_eq!(report.format_summary(), "Copied 3 files");
    }

    #[test]
    fn format_summary_delete_single() {
        let report = BatchReport {
            errors: vec![],
            success_count: 1,
            canceled: false,
            action_label: "Delete",
        };
        assert_eq!(report.format_summary(), "Deleted 1 file");
    }

    #[test]
    fn format_summary_move_partial_error() {
        let report = BatchReport {
            errors: vec!["foo: permission denied".into()],
            success_count: 2,
            canceled: false,
            action_label: "Move",
        };
        assert_eq!(report.format_summary(), "Moved 2 file(s), 1 error(s)");
    }

    #[test]
    fn format_summary_all_errors() {
        let report = BatchReport {
            errors: vec!["a: not found".into(), "b: not found".into()],
            success_count: 0,
            canceled: false,
            action_label: "Delete",
        };
        assert_eq!(report.format_summary(), "Deleted failed: 2 error(s)");
    }

    #[test]
    fn format_summary_single_error() {
        let report = BatchReport {
            errors: vec!["file.txt: not found".into()],
            success_count: 0,
            canceled: false,
            action_label: "Copy",
        };
        assert_eq!(
            report.format_summary(),
            "Copied failed: file.txt: not found"
        );
    }

    #[test]
    fn format_summary_canceled_with_progress() {
        let report = BatchReport {
            errors: vec![],
            success_count: 5,
            canceled: true,
            action_label: "Copy",
        };
        assert_eq!(report.format_summary(), "Copied canceled after 5 file(s)");
    }

    #[test]
    fn format_summary_canceled_no_progress() {
        let report = BatchReport {
            errors: vec![],
            success_count: 0,
            canceled: true,
            action_label: "Move",
        };
        assert_eq!(report.format_summary(), "Moved canceled");
    }

    #[test]
    fn format_summary_unknown_label_passes_through() {
        let report = BatchReport {
            errors: vec![],
            success_count: 2,
            canceled: false,
            action_label: "Foobar",
        };
        assert_eq!(report.format_summary(), "Foobar 2 files");
    }

    #[test]
    fn format_summary_unknown_default_label() {
        let report = BatchReport {
            errors: vec!["e: x".into()],
            success_count: 0,
            canceled: false,
            action_label: "Unknown",
        };
        assert_eq!(report.format_summary(), "Unknown failed: e: x");
    }
}
