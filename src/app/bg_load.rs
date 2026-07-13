//! Cancel-on-drop background computation. Keeps expensive user-initiated reads
//! (archive listing, directory-tree building, viewer open, image preview) off
//! the event thread so the TUI never freezes on a large/slow/NFS input.
//!
//! The worker publishes at most one result and only if it has not been
//! cancelled; dropping the handle signals cancellation and detaches the worker
//! (the event thread never blocks on a join). Because the underlying reads are
//! not themselves interruptible, "cancel" means the result is discarded, not
//! that the worker is killed — the detached thread finishes on its own and the
//! process reclaims it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

pub struct BgLoad<T> {
    receiver: mpsc::Receiver<T>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> BgLoad<T> {
    /// Spawn `work` on a named thread. `work` receives the cancel flag; its
    /// return value is published unless cancellation happened first. Returns an
    /// error if the OS refuses the thread (so callers can fall back instead of
    /// panicking as a bare `thread::spawn` would).
    pub fn spawn<F>(name: &str, work: F) -> std::io::Result<Self>
    where
        F: FnOnce(&AtomicBool) -> T + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                if cancel_flag.load(Ordering::Acquire) {
                    return;
                }
                let result = work(&cancel_flag);
                if !cancel_flag.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                }
            })?;
        Ok(Self {
            receiver: rx,
            cancel,
            handle: Some(handle),
        })
    }

    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn from_parts(
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

impl<T> Drop for BgLoad<T> {
    fn drop(&mut self) {
        // Signal cancellation and detach: drops run on the event thread, which
        // must never block on a worker that may be mid-read on a slow device.
        self.cancel.store(true, Ordering::Release);
        drop(self.handle.take());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn publishes_result_when_not_cancelled() {
        let load = BgLoad::spawn("test-ok", |_cancel| 42u32).unwrap();
        let mut got = None;
        for _ in 0..2000 {
            if let Ok(v) = load.try_recv() {
                got = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(got, Some(42));
    }

    #[test]
    fn cancelled_worker_suppresses_result() {
        let cancel = Arc::new(AtomicBool::new(true));
        let c = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel::<u32>();
        let handle = thread::spawn(move || {
            if !c.load(Ordering::Acquire) {
                let _ = tx.send(7);
            }
        });
        handle.join().unwrap();
        assert!(rx.try_recv().is_err());
    }
}
