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
//!
//! To keep that detach behaviour from accumulating threads under rapid
//! cancel/respawn (e.g. holding an arrow key through slow-loading archives),
//! `spawn` bounds the number of concurrently live workers. At capacity it
//! returns an error so the caller falls back to a synchronous read rather than
//! spawning another thread.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

/// Upper bound on concurrently *live* worker threads.
///
/// The event loop keeps at most one load per kind in flight (archive listing,
/// tree build), so cancelling-and-respawning drops the old handle, which only
/// *signals* cancellation — the underlying I/O is not interruptible, so a
/// detached worker keeps running until it finishes on its own. Holding an arrow
/// key through a directory of slow-loading archives can therefore accumulate
/// detached workers. This cap prevents runaway accumulation: once it is reached
/// `spawn` returns an error so the caller falls back to a synchronous read
/// instead of spawning yet another thread.
const MAX_LIVE_WORKERS: usize = 8;
static LIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);

pub struct BgLoad<T> {
    receiver: mpsc::Receiver<T>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> BgLoad<T> {
    /// Spawn `work` on a named thread. `work` receives the cancel flag; its
    /// return value is published unless cancellation happened first. Returns an
    /// error if the OS refuses the thread or the worker cap is reached (so
    /// callers can fall back instead of panicking as a bare `thread::spawn`
    /// would).
    ///
    /// Cancellation is *best-effort*: there is an inherent TOCTOU window
    /// between the post-work `cancel_flag` check and `tx.send()`, so a result
    /// may still be published if `cancel()` lands in that gap. A value received
    /// via `try_recv()` therefore does not guarantee non-cancellation — callers
    /// that care must check their own dismiss flag rather than inferring it
    /// from the channel.
    pub fn spawn<F>(name: &str, work: F) -> std::io::Result<Self>
    where
        F: FnOnce(&AtomicBool) -> T + Send + 'static,
    {
        // Bound the number of concurrently live worker threads so rapid
        // cancel/respawn cannot accumulate detached workers faster than they
        // drain. The permit is held for the worker's lifetime and released on
        // thread exit (drop only signals cancellation — it does not kill the
        // detached I/O), so this counts running threads, not held handles. At
        // capacity the caller gets an error and falls back to a sync read.
        if LIVE_WORKERS.fetch_add(1, Ordering::AcqRel) >= MAX_LIVE_WORKERS {
            LIVE_WORKERS.fetch_sub(1, Ordering::Release);
            return Err(std::io::Error::other("bg-load worker cap reached"));
        }

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        let handle = match thread::Builder::new().name(name.to_owned()).spawn(move || {
            // RAII guard: releases the live-worker permit on every exit
            // path, including an early return on pre-work cancellation and
            // a panic inside `work`.
            struct WorkerGuard;
            impl Drop for WorkerGuard {
                fn drop(&mut self) {
                    LIVE_WORKERS.fetch_sub(1, Ordering::Release);
                }
            }
            let _guard = WorkerGuard;

            if cancel_flag.load(Ordering::Acquire) {
                return;
            }
            let result = work(&cancel_flag);
            if !cancel_flag.load(Ordering::Acquire) {
                let _ = tx.send(result);
            }
        }) {
            Ok(h) => h,
            // The thread was never created, so no worker will run to release
            // the permit acquired above — undo it and propagate the error.
            Err(e) => {
                LIVE_WORKERS.fetch_sub(1, Ordering::Release);
                return Err(e);
            }
        };
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
        // Drive the real `BgLoad::spawn` + `cancel` path: a barrier lets the
        // worker reach the post-work checkpoint, the main thread cancels, then
        // the worker is released to its (now-cancelled) `send`. The result
        // must never be published, so `try_recv` reports an empty channel.
        use std::sync::Barrier;
        let at_work = Arc::new(Barrier::new(2));
        let released = Arc::new(Barrier::new(2));
        let at_work_w = Arc::clone(&at_work);
        let released_w = Arc::clone(&released);
        let load = BgLoad::spawn("test-cancel", move |_cancel| -> u32 {
            // Signal "work done" and wait for the cancel signal to land before
            // returning, so the post-work cancel check observes cancellation.
            at_work_w.wait();
            released_w.wait();
            7
        })
        .unwrap();

        at_work.wait();
        load.cancel();
        released.wait();

        // Give the worker a moment to run its post-work cancel check + (not)
        // send, then drain the channel of anything that slipped through the
        // TOCTOU window. A correctly cancelled worker never publishes.
        let mut leaked: Option<u32> = None;
        for _ in 0..200 {
            match load.try_recv() {
                Ok(v) => {
                    leaked = Some(v);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(leaked.is_none(), "cancelled worker published {leaked:?}");
    }
}
