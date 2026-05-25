use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use ansi_to_tui::IntoText;
use ratatui::text::Text;

use super::open::ViewerState;

pub struct ViewerLoader {
    pub receiver: mpsc::Receiver<std::io::Result<ViewerState>>,
    pub cancel: Arc<AtomicBool>,
    pub path: PathBuf,
    pub(crate) _handle: Option<JoinHandle<()>>,
}

impl ViewerLoader {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let owned_path = path.clone();
        let handle = thread::spawn(move || {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let result = ViewerState::open_with_cancel(&owned_path, Some(&cancel_flag));
            if !cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send(result);
            }
        });
        Self {
            receiver: rx,
            cancel,
            path,
            _handle: Some(handle),
        }
    }
}

impl Drop for ViewerLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self._handle.take();
    }
}

pub fn run_chafa(path: &Path, width: u16, height: u16) -> Text<'static> {
    let size_str = format!("{}x{}", width, height);
    match std::process::Command::new("chafa")
        .arg("-f")
        .arg("symbols")
        .arg("--size")
        .arg(&size_str)
        .arg("--")
        .arg(path)
        .output()
    {
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

pub struct ImagePreviewLoader {
    pub file_path: PathBuf,
    pub(crate) receiver: mpsc::Receiver<(u16, u16, Text<'static>)>,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) _handle: Option<JoinHandle<()>>,
}

impl ImagePreviewLoader {
    pub fn start(path: PathBuf, width: u16, height: u16) -> Self {
        let file_path = path.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let handle = thread::spawn(move || {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let text = run_chafa(&path, width, height);
            if !cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send((width, height, text));
            }
        });
        Self {
            file_path,
            receiver: rx,
            cancel,
            _handle: Some(handle),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn try_recv(&self) -> Result<(u16, u16, Text<'static>), mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for ImagePreviewLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self._handle.take();
    }
}
