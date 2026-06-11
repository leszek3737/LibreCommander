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
const PIPE_READ_LIMIT: u64 = 50 * 1024 * 1024;
const PIPE_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

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
}

pub struct ViewerLoader {
    pub receiver: mpsc::Receiver<std::io::Result<ViewerState>>,
    pub cancel: Arc<AtomicBool>,
    pub path: PathBuf,
    pub(crate) handle: Option<JoinHandle<()>>,
}

impl ViewerLoader {
    pub fn start(path: PathBuf) -> Self {
        let owned_path = path.clone();
        let inner = CancellableLoader::spawn(move |cancel_flag, tx| {
            if cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let result = ViewerState::open_with_cancel(&owned_path, Some(cancel_flag));
            if !cancel_flag.load(Ordering::Acquire) {
                let _ = tx.send(result);
            }
        });
        Self {
            receiver: inner.receiver,
            cancel: inner.cancel,
            path,
            handle: inner.handle,
        }
    }
}

impl Drop for ViewerLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.handle.take();
    }
}

pub struct ImagePreviewLoader {
    pub file_path: PathBuf,
    pub(crate) receiver: mpsc::Receiver<(u16, u16, Text<'static>)>,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) handle: Option<JoinHandle<()>>,
}

impl ImagePreviewLoader {
    pub fn start(path: PathBuf, width: u16, height: u16) -> Self {
        let file_path = path.clone();
        let inner = CancellableLoader::spawn(move |cancel_flag, tx| {
            if cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let text = run_chafa(&path, width, height, Some(cancel_flag));
            if !cancel_flag.load(Ordering::Acquire) {
                let _ = tx.send((width, height, text));
            }
        });
        Self {
            file_path,
            receiver: inner.receiver,
            cancel: inner.cancel,
            handle: inner.handle,
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
        let _ = self.handle.take();
    }
}

pub(crate) fn run_chafa(
    path: &Path,
    width: u16,
    height: u16,
    cancel: Option<&AtomicBool>,
) -> Text<'static> {
    let size_str = format!("{}x{}", width, height);
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
            let stdout = join_pipe_reader_with_timeout(stdout_reader);
            let stderr = join_pipe_reader_with_timeout(stderr_reader);
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
            let stdout = join_pipe_reader_with_timeout(stdout_reader);
            let stderr = join_pipe_reader_with_timeout(stderr_reader);
            return Ok(Output {
                status,
                stdout,
                stderr,
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait()?;
            let stdout = join_pipe_reader_with_timeout(stdout_reader);
            let stderr = join_pipe_reader_with_timeout(stderr_reader);
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
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let mut limited = pipe.take(PIPE_READ_LIMIT);
            if let Err(e) = limited.read_to_end(&mut bytes) {
                debug_log!("pipe read error: {}", e);
            }
        }
        bytes
    })
}

fn join_pipe_reader_with_timeout(handle: JoinHandle<Vec<u8>>) -> Vec<u8> {
    let deadline = Instant::now() + PIPE_JOIN_TIMEOUT;
    loop {
        if handle.is_finished() {
            return match handle.join() {
                Ok(bytes) => bytes,
                Err(e) => {
                    debug_log!("pipe reader thread panicked: {:?}", e);
                    Vec::new()
                }
            };
        }
        if Instant::now() >= deadline {
            debug_log!("pipe reader join timed out, detaching");
            return Vec::new();
        }
        thread::sleep(Duration::from_millis(10));
    }
}
