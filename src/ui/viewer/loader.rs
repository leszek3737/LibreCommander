use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ansi_to_tui::IntoText;
use ratatui::text::Text;

use super::open::ViewerState;
use crate::debug_log;

const CHAFA_TIMEOUT: Duration = Duration::from_secs(10);

pub struct ViewerLoader {
    pub receiver: mpsc::Receiver<std::io::Result<ViewerState>>,
    pub cancel: Arc<AtomicBool>,
    pub path: PathBuf,
    pub(crate) handle: Option<JoinHandle<()>>,
}

impl ViewerLoader {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let owned_path = path.clone();
        let handle = thread::spawn(move || {
            if cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let result = ViewerState::open_with_cancel(&owned_path, Some(&cancel_flag));
            if !cancel_flag.load(Ordering::Acquire) {
                let _ = tx.send(result);
            }
        });
        Self {
            receiver: rx,
            cancel,
            path,
            handle: Some(handle),
        }
    }
}

impl Drop for ViewerLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn run_chafa(
    path: &Path,
    width: u16,
    height: u16,
    cancel: Option<&AtomicBool>,
) -> Text<'static> {
    let size_str = format!("{}x{}", width, height);
    // Keep terminal probing and passthrough disabled. If chafa talks directly
    // to the terminal, crossterm can read those responses as viewer input and
    // open the search dialog instead of showing the image preview.
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
            Err(e) => Text::raw(format!("Failed to parse ANSI: {}", e)),
        },
        Ok(out) => {
            let err_msg = String::from_utf8_lossy(&out.stderr);
            Text::raw(format!("Chafa error: {}", err_msg))
        }
        Err(e) => Text::raw(format!("Failed to execute chafa (is it installed?): {}", e)),
    }
}

fn wait_for_chafa_output(mut child: Child, cancel: Option<&AtomicBool>) -> std::io::Result<Output> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = read_pipe_in_background(stdout);
    let stderr_reader = read_pipe_in_background(stderr);
    let deadline = Instant::now() + CHAFA_TIMEOUT;

    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = join_pipe_reader(stdout_reader);
            let stderr = join_pipe_reader(stderr_reader);
            return Ok(Output {
                status,
                stdout,
                stderr,
            });
        }
        if let Some(flag) = cancel
            && flag.load(Ordering::Acquire)
        {
            let _ = child.kill();
            let status = child.wait()?;
            let stdout = join_pipe_reader(stdout_reader);
            let stderr = join_pipe_reader(stderr_reader);
            return Ok(Output {
                status,
                stdout,
                stderr,
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait()?;
            let stdout = join_pipe_reader(stdout_reader);
            let stderr = join_pipe_reader(stderr_reader);
            return Ok(Output {
                status,
                stdout,
                stderr: if stderr.is_empty() {
                    b"Chafa timed out".to_vec()
                } else {
                    stderr
                },
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_pipe_in_background<R>(pipe: Option<R>) -> JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    // NOTE: read_to_end blocks the thread until the pipe closes. This means
    // the pipe reader thread is not individually cancelable — it will only
    // exit when the child process terminates and closes its stdout/stderr.
    // The cancel flag in wait_for_chafa_output kills the child process,
    // which indirectly unblocks these readers.
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn join_pipe_reader(handle: JoinHandle<Vec<u8>>) -> Vec<u8> {
    match handle.join() {
        Ok(bytes) => bytes,
        Err(e) => {
            debug_log!("pipe reader thread panicked: {:?}", e);
            Vec::new()
        }
    }
}

// TODO: ViewerLoader and ImagePreviewLoader share an identical structure
// (cancel flag, background thread, channel). Extract a common abstraction
// (e.g. a generic CancellableLoader<T>) to eliminate the duplication.
pub struct ImagePreviewLoader {
    pub file_path: PathBuf,
    pub(crate) receiver: mpsc::Receiver<(u16, u16, Text<'static>)>,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) handle: Option<JoinHandle<()>>,
}

impl ImagePreviewLoader {
    pub fn start(path: PathBuf, width: u16, height: u16) -> Self {
        let file_path = path.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let handle = thread::spawn(move || {
            if cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let text = run_chafa(&path, width, height, Some(&cancel_flag));
            if !cancel_flag.load(Ordering::Acquire) {
                let _ = tx.send((width, height, text));
            }
        });
        Self {
            file_path,
            receiver: rx,
            cancel,
            handle: Some(handle),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn try_recv(&self) -> Result<(u16, u16, Text<'static>), mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for ImagePreviewLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
