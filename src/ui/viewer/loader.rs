use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ansi_to_tui::IntoText;
use ratatui::text::Text;

use super::open::ViewerState;
use crate::debug_log;

const CHAFA_TIMEOUT: Duration = Duration::from_secs(10);
const PIPE_READ_LIMIT: u64 = 50 * 1024 * 1024;
/// Join budget for the background pipe readers, collected only after the chafa
/// child has already exited (so the writers are closed and `read_to_end`
/// returns as fast as the OS drains the buffers). Matched to `CHAFA_TIMEOUT`
/// rather than a tight 2s so a large pipe or scheduling delay on the reader
/// thread cannot truncate chafa's output into an empty preview. Runs on the
/// background image-preview loader thread, never the UI event loop.
const PIPE_JOIN_TIMEOUT: Duration = CHAFA_TIMEOUT;
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Owns a background worker plus its cancellation flag and result channel.
///
/// Held as a field by the public loaders rather than unpacked into them, so the
/// cancel/result plumbing and the drop semantics live in one place.
struct CancellableLoader<T> {
    receiver: mpsc::Receiver<T>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> CancellableLoader<T> {
    fn spawn<F>(work: F) -> Self
    where
        F: FnOnce(&AtomicBool, mpsc::Sender<T>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let handle = thread::spawn(move || work(&cancel_flag, tx));
        Self {
            receiver: rx,
            cancel,
            handle: Some(handle),
        }
    }

    fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn from_parts(
        receiver: mpsc::Receiver<T>,
        cancel: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            receiver,
            cancel,
            handle,
        }
    }
}

impl<T> Drop for CancellableLoader<T> {
    fn drop(&mut self) {
        // Signal cancellation and *detach* the worker — deliberately not join.
        // Drops run on the event thread (e.g. when the viewer closes), which
        // must never block; a worker can be mid-`read()` on a slow device.
        // Every worker polls `cancel` between units of work and will not send a
        // stale result once it is set, so detaching is deterministic: the thread
        // observes the flag and terminates on its own.
        self.cancel.store(true, Ordering::Release);
        drop(self.handle.take());
    }
}

/// Shared guard around a worker body: skip the work if cancellation already
/// happened, and only publish the result if cancellation has not happened by
/// the time the work finishes.
fn run_guarded<T, F>(cancel: &AtomicBool, tx: &mpsc::Sender<T>, work: F)
where
    F: FnOnce(&AtomicBool) -> T,
{
    if cancel.load(Ordering::Acquire) {
        return;
    }
    let result = work(cancel);
    if !cancel.load(Ordering::Acquire) {
        let _ = tx.send(result);
    }
}

pub struct ViewerLoader {
    inner: CancellableLoader<std::io::Result<ViewerState>>,
    pub path: Arc<Path>,
}

impl ViewerLoader {
    pub fn start(path: PathBuf) -> Self {
        // Share one allocation between the worker and the struct instead of
        // cloning the path into a second owned `PathBuf` before spawning.
        let path: Arc<Path> = Arc::from(path);
        let worker_path = Arc::clone(&path);
        let inner = CancellableLoader::spawn(move |cancel, tx| {
            run_guarded(cancel, &tx, |c| {
                ViewerState::open_with_cancel(&worker_path, Some(c))
            });
        });
        Self { inner, path }
    }

    pub fn try_recv(&self) -> Result<std::io::Result<ViewerState>, mpsc::TryRecvError> {
        self.inner.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn from_parts(
        receiver: mpsc::Receiver<std::io::Result<ViewerState>>,
        cancel: Arc<AtomicBool>,
        path: PathBuf,
        handle: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            inner: CancellableLoader::from_parts(receiver, cancel, handle),
            path: Arc::from(path),
        }
    }
}

pub struct ImagePreviewLoader {
    pub file_path: PathBuf,
    inner: CancellableLoader<(u16, u16, Text<'static>)>,
}

impl ImagePreviewLoader {
    pub fn start(path: PathBuf, width: u16, height: u16) -> Self {
        let file_path = path.clone();
        let inner = CancellableLoader::spawn(move |cancel, tx| {
            run_guarded(cancel, &tx, |c| {
                let text = run_chafa(&path, width, height, Some(c));
                (width, height, text)
            });
        });
        Self { file_path, inner }
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn try_recv(&self) -> Result<(u16, u16, Text<'static>), mpsc::TryRecvError> {
        self.inner.try_recv()
    }

    #[cfg(test)]
    pub(crate) fn from_parts(
        file_path: PathBuf,
        receiver: mpsc::Receiver<(u16, u16, Text<'static>)>,
        cancel: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            file_path,
            inner: CancellableLoader::from_parts(receiver, cancel, handle),
        }
    }
}

pub(crate) fn run_chafa(
    path: &Path,
    width: u16,
    height: u16,
    cancel: Option<&AtomicBool>,
) -> Text<'static> {
    let size_str = format!("{width}x{height}");
    let child = Command::new("chafa")
        .arg("-f")
        .arg("symbols")
        .arg("--probe")
        .arg("off")
        .arg("--passthrough")
        .arg("none")
        .arg("--polite")
        .arg("on")
        .arg("--size")
        .arg(&size_str)
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child.and_then(|c| wait_for_chafa_output(c, cancel)) {
        Ok(out) if out.status.success() => match out.stdout.into_text() {
            Ok(text) => text,
            Err(e) => Text::raw(format!("Failed to parse ANSI: {e}")),
        },
        Ok(out) => {
            let err_msg = String::from_utf8_lossy(&out.stderr);
            Text::raw(format!("Chafa error: {err_msg}"))
        }
        Err(e) => Text::raw(format!("Failed to execute chafa (is it installed?): {e}")),
    }
}

fn wait_for_chafa_output(mut child: Child, cancel: Option<&AtomicBool>) -> std::io::Result<Output> {
    let stdout_rx = read_pipe_in_background(child.stdout.take());
    let stderr_rx = read_pipe_in_background(child.stderr.take());
    let deadline = Instant::now() + CHAFA_TIMEOUT;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(collect_output(status, &stdout_rx, &stderr_rx, None));
        }

        let cancelled = cancel.is_some_and(|flag| flag.load(Ordering::Acquire));
        let timed_out = Instant::now() >= deadline;
        if cancelled || timed_out {
            let _ = child.kill();
            let status = child.wait()?;
            // A genuine timeout (not a user cancel) substitutes a message when
            // chafa produced no diagnostics of its own.
            let fallback = (!cancelled && timed_out).then_some(b"Chafa timed out".as_slice());
            return Ok(collect_output(status, &stdout_rx, &stderr_rx, fallback));
        }

        thread::sleep(CHILD_POLL_INTERVAL);
    }
}

/// Assembles the [`Output`] from the exited child and its two pipe readers,
/// optionally substituting `stderr_fallback` when stderr came back empty.
fn collect_output(
    status: ExitStatus,
    stdout_rx: &mpsc::Receiver<Vec<u8>>,
    stderr_rx: &mpsc::Receiver<Vec<u8>>,
    stderr_fallback: Option<&[u8]>,
) -> Output {
    let stdout = collect_pipe_reader(stdout_rx);
    let mut stderr = collect_pipe_reader(stderr_rx);
    if stderr.is_empty()
        && let Some(fallback) = stderr_fallback
    {
        stderr = fallback.to_vec();
    }
    Output {
        status,
        stdout,
        stderr,
    }
}

fn read_pipe_in_background<R>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let mut limited = pipe.take(PIPE_READ_LIMIT);
            if let Err(e) = limited.read_to_end(&mut bytes) {
                debug_log!("pipe read error: {}", e);
            }
        }
        let _ = tx.send(bytes);
    });
    rx
}

/// Collects a background pipe reader's bytes, blocking up to
/// [`PIPE_JOIN_TIMEOUT`] on the channel (no busy-wait). On timeout the reader
/// thread is detached and the bytes read so far are dropped.
fn collect_pipe_reader(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<u8> {
    match rx.recv_timeout(PIPE_JOIN_TIMEOUT) {
        Ok(bytes) => bytes,
        Err(_) => {
            debug_log!("pipe reader join timed out, detaching");
            Vec::new()
        }
    }
}
