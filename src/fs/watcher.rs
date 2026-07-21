use crate::debug_log;
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify::{Watcher as NotifyWatcher, event::RenameMode};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

/// Window for coalescing rapid-fire filesystem events (creates, writes, chmod
/// bursts). 300 ms absorbs the typical jitter from editors, build tools and
/// macOS FSEvents without introducing a perceptible delay in panel refreshes.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(300);

/// Maximum time a "rename-from" entry is kept before we treat the rename as a
/// plain delete.  Two seconds is generous enough for slow network mounts (NFS,
/// SMB, FUSE) where the paired "rename-to" event can arrive with significant
/// latency, while still bounding stale entries in the pending map.
const PENDING_FROM_TIMEOUT: Duration = Duration::from_secs(2);

/// Upper bound on buffered rename-From entries per parent directory. A mass
/// rename (e.g. `mv dir/* elsewhere/`) emits one From per file before any To
/// arrives; without a cap the per-parent queue grows unbounded until the 2 s
/// timeout fires. When the cap is hit we stop buffering and signal `Overflow`
/// instead, mirroring the batch-overflow handling for the event channel — the
/// panel does a full refresh rather than tracking individual renames.
const PENDING_FROM_LIMIT: usize = 1024;

#[derive(Clone)]
pub enum WatchEvent {
    Created(PathBuf),
    Deleted(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    Overflow,
}

struct PendingEntry {
    last_seen: Instant,
    coalesced: Option<WatchEvent>,
}

struct PendingFromEntry {
    path: PathBuf,
    /// Rename-tracker cookie from the backend (Linux inotify sets equal cookies
    /// on the From/To pair). `None` when the backend does not provide one
    /// (macOS FSEvents), in which case pairing falls back to FIFO order.
    cookie: Option<usize>,
    time: Instant,
}

struct ExpiredDebouncedEvent {
    path: PathBuf,
    event: WatchEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhichWatcher {
    Primary,
    Fallback,
}

enum SendStatus {
    Sent,
    Full(WatchEvent),
    Disconnected,
}

pub struct Watcher {
    primary: RecommendedWatcher,
    fallback: Option<PollWatcher>,
    watchers: HashMap<PathBuf, WhichWatcher>,
    event_tx: Arc<SyncSender<WatchEvent>>,
    paused: Arc<AtomicBool>,
    // Shared debounce map — accessed from notify callback thread and main thread
    // (flush_pending). Both go through lock_or_recover to handle poison.
    debounce_state: Arc<Mutex<HashMap<PathBuf, PendingEntry>>>,
    // Unpaired rename-From events awaiting a matching rename-To. Entries are
    // bounded by PENDING_FROM_TIMEOUT (stale ones are emitted as Deleted in
    // flush_pending()) AND by PENDING_FROM_LIMIT per parent (overflow emits an
    // Overflow marker so growth is capped under a mass rename).
    pending_from: Arc<Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>>>,
    overflow_pending: Arc<AtomicBool>,
    // Canonical-path cache: maps original (possibly non-canonical) path to its
    // resolved form. Grows unboundedly — only trimmed on unwatch(). Acceptable
    // because it mirrors the watched_dirs set which is small by design.
    path_cache: HashMap<PathBuf, PathBuf>,
}

impl Watcher {
    pub fn new(event_tx: Arc<SyncSender<WatchEvent>>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let debounce_state = Arc::new(Mutex::new(HashMap::new()));
        let pending_from: Arc<Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let overflow_pending = Arc::new(AtomicBool::new(false));
        let primary = RecommendedWatcher::new(
            make_handler(
                Arc::clone(&event_tx),
                Arc::clone(&paused),
                Arc::clone(&debounce_state),
                Arc::clone(&pending_from),
                Arc::clone(&overflow_pending),
            ),
            Config::default(),
        )
        .map_err(|e| notify_to_io(&e))?;

        Ok(Self {
            primary,
            fallback: None,
            watchers: HashMap::new(),
            event_tx,
            paused,
            debounce_state,
            pending_from,
            overflow_pending,
            path_cache: HashMap::new(),
        })
    }

    fn create_fallback(&mut self) -> io::Result<&mut PollWatcher> {
        if self.fallback.is_none() {
            // Network mounts (the reason this fallback exists) need a short
            // interval; notify's Config::default() polls every 30s.
            let fallback = PollWatcher::new(
                make_handler(
                    Arc::clone(&self.event_tx),
                    Arc::clone(&self.paused),
                    Arc::clone(&self.debounce_state),
                    Arc::clone(&self.pending_from),
                    Arc::clone(&self.overflow_pending),
                ),
                Config::default().with_poll_interval(Duration::from_secs(2)),
            )
            .map_err(|e| notify_to_io(&e))?;
            self.fallback = Some(fallback);
        }
        self.fallback
            .as_mut()
            .ok_or_else(|| io::Error::other("create_fallback: fallback must be initialized"))
    }

    /// Start watching `path` for filesystem events.
    ///
    /// Uses `RecursiveMode::NonRecursive` — only events directly inside `path`
    /// are reported. Child-directory modifications (e.g. a file created inside
    /// a subdirectory) are NOT reported unless that subdirectory is also watched.
    /// Panel entries for child directories show metadata from the parent listing
    /// only; changes within child subtrees require a full panel refresh.
    pub fn watch(&mut self, path: &Path) -> io::Result<()> {
        let original = path.to_path_buf();

        // TOCTOU: if the path vanishes between canonicalize and watch(),
        // watch() fails and the error propagates to the caller. If it
        // vanishes *after* a successful watch(), the OS emits events that
        // trigger cleanup in unwatch/is_watch_already_gone_error.
        let path = path.canonicalize().map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("cannot canonicalize {}: {e}", original.display()),
            )
        })?;

        if self.watchers.contains_key(&path) {
            return Ok(());
        }

        self.path_cache.insert(original, path.clone());

        match self.primary.watch(&path, RecursiveMode::NonRecursive) {
            Ok(()) => {
                self.watchers.insert(path, WhichWatcher::Primary);
                Ok(())
            }
            Err(primary_err) => {
                let fallback = self.create_fallback()?;
                match fallback.watch(&path, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        self.watchers.insert(path, WhichWatcher::Fallback);
                        Ok(())
                    }
                    Err(fallback_err) => Err(io::Error::other(format!(
                        "primary watcher failed: {primary_err}; fallback watcher failed: {fallback_err}"
                    ))),
                }
            }
        }
    }

    pub fn unwatch(&mut self, path: &Path) -> io::Result<()> {
        let path = if let Some(canonical) = self.path_cache.get(path) {
            canonical.clone()
        } else {
            match path.canonicalize() {
                Ok(canonical) => {
                    self.path_cache
                        .insert(path.to_path_buf(), canonical.clone());
                    canonical
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    self.remove_watched_dir_state(path);
                    return Ok(());
                }
                Err(err) => {
                    return Err(io::Error::new(
                        err.kind(),
                        format!("cannot canonicalize {}: {err}", path.display()),
                    ));
                }
            }
        };

        let unwatch_result = match self.watchers.get(&path) {
            Some(WhichWatcher::Primary) => self.primary.unwatch(&path).or_else(|e| {
                // On Linux (inotify), the OS removes watches automatically when the
                // directory is deleted. Subsequent unwatch calls may fail with
                // "No watch was found" or EINVAL. Either way the watch is gone.
                if is_watch_already_gone_error(&e) {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "primary unwatch failed for {}: {e}",
                        path.display()
                    )))
                }
            }),
            Some(WhichWatcher::Fallback) => {
                if let Some(fb) = self.fallback.as_mut() {
                    fb.unwatch(&path).or_else(|e| {
                        if is_watch_already_gone_error(&e) {
                            Ok(())
                        } else {
                            Err(io::Error::other(format!(
                                "fallback unwatch failed for {}: {e}",
                                path.display()
                            )))
                        }
                    })
                } else {
                    Ok(())
                }
            }
            None => Ok(()),
        };

        self.watchers.remove(&path);
        self.path_cache.retain(|_, v| v != &path);
        unwatch_result
    }

    fn remove_watched_dir_state(&mut self, path: &Path) {
        let clean = crate::fs::path::clean_path(path);
        // A deleted symlink can no longer be resolved via `read_link`, so
        // `path_points_to_missing_watch` cannot map it to its canonical watch key
        // and the watcher would leak. Recover the key from `path_cache`, which
        // recorded the symlink -> canonical mapping when the watch was added: any
        // cached spelling that cleans to `clean` names the canonical entry to
        // evict.
        let cached_canonical: Vec<PathBuf> = self
            .path_cache
            .iter()
            .filter(|(k, _)| crate::fs::path::clean_path(k) == clean)
            .map(|(_, v)| v.clone())
            .collect();
        self.watchers.retain(|watched, _| {
            watched.as_path() != clean
                && !cached_canonical.iter().any(|c| c == watched)
                && !path_points_to_missing_watch(&clean, watched)
        });
        self.path_cache
            .retain(|_, v| self.watchers.contains_key(v.as_path()));
    }

    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.watchers.keys().cloned().collect()
    }

    pub fn pause(&self) {
        // Set the flag while holding the debounce lock: the event handler
        // re-checks `paused` under this same lock before emitting, so once
        // `pause()` has returned no in-flight callback can send an event past
        // it (the check-then-send is serialized by the lock).
        {
            let mut debounce = lock_or_recover(&self.debounce_state, "watcher");
            self.paused.store(true, Ordering::Release);
            if !debounce.is_empty() {
                debug_log!(
                    "watcher paused: clearing {} debounce entries",
                    debounce.len()
                );
            }
            debounce.clear();
        }
        let mut pending = lock_or_recover(&self.pending_from, "pending_from");
        if !pending.is_empty() {
            debug_log!("watcher paused: clearing stale pending_from entries");
        }
        pending.clear();
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }

    pub fn flush_pending(&self) {
        if self.paused.load(Ordering::Acquire) {
            return;
        }

        if self.overflow_pending.load(Ordering::Acquire) && event_tx_try_overflow(&self.event_tx) {
            self.overflow_pending.store(false, Ordering::Release);
        }

        // Single lock cycle: flush expired debounced events and reinsert the
        // ones the (full) channel rejected, all under one guard.
        {
            let mut debounce = lock_or_recover(&self.debounce_state, "watcher");
            let flushed = flush_expired(&mut debounce);
            send_expired_events(&self.event_tx, &mut debounce, flushed);
        }

        self.flush_stale_pending_from();
    }

    /// Emit `Deleted` for rename-From entries that timed out without a matching
    /// rename-To. Holds the `pending_from` lock for a single cycle: stale ids
    /// are collected, then each is sent and retracted in place (sends are
    /// non-blocking `try_send`, so holding the guard is cheap).
    fn flush_stale_pending_from(&self) {
        let mut pending = lock_or_recover(&self.pending_from, "pending_from");
        let stale: Vec<(PathBuf, PathBuf, Instant)> = pending
            .iter()
            .flat_map(|(parent, entries)| {
                entries
                    .iter()
                    .filter(|entry| entry.time.elapsed() >= PENDING_FROM_TIMEOUT)
                    .map(move |entry| (parent.clone(), entry.path.clone(), entry.time))
            })
            .collect();

        if stale.is_empty() {
            return;
        }
        // Aggregate log: one line for the whole batch instead of one per entry,
        // which would flood the log on a mass-rename timeout.
        debug_log!(
            "emitting Deleted for {} stale rename-From entries",
            stale.len()
        );

        for (parent_key, path, time) in stale {
            match try_send_event(&self.event_tx, WatchEvent::Deleted(path.clone())) {
                // Channel full: keep the entry and retry on the next flush.
                SendStatus::Full(_) => {}
                SendStatus::Sent | SendStatus::Disconnected => {
                    clear_pending_from_entry(&mut pending, &parent_key, &path, time);
                }
            }
        }
    }
}

/// Running count of consecutive events dropped because the channel was full.
/// Reset on the next successful send. Used to rate-limit the "queue full" log so
/// a sustained burst produces a handful of aggregated lines instead of one per
/// dropped event. Relaxed ordering is fine — this only gates logging.
static DROPPED_ON_FULL: AtomicU64 = AtomicU64::new(0);

/// Log at the start of a full-queue burst and then every `FULL_LOG_INTERVAL`
/// drops, so the message rate stays bounded under heavy load.
const FULL_LOG_INTERVAL: u64 = 256;

fn try_send_event(event_tx: &SyncSender<WatchEvent>, event: WatchEvent) -> SendStatus {
    match event_tx.try_send(event) {
        Ok(()) => {
            DROPPED_ON_FULL.store(0, Ordering::Relaxed);
            SendStatus::Sent
        }
        Err(TrySendError::Full(event)) => {
            let dropped = DROPPED_ON_FULL.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped.is_multiple_of(FULL_LOG_INTERVAL) {
                debug_log!("watcher event queue full; dropped {dropped} event(s) so far");
            }
            SendStatus::Full(event)
        }
        Err(TrySendError::Disconnected(_)) => {
            debug_log!("watcher send failed: receiver disconnected");
            SendStatus::Disconnected
        }
    }
}

/// Try to send an Overflow marker. Returns true if sent successfully.
fn event_tx_try_overflow(event_tx: &SyncSender<WatchEvent>) -> bool {
    event_tx.try_send(WatchEvent::Overflow).is_ok()
}

fn make_handler(
    event_tx: Arc<SyncSender<WatchEvent>>,
    paused: Arc<AtomicBool>,
    debounce_state: Arc<Mutex<HashMap<PathBuf, PendingEntry>>>,
    pending_from: Arc<Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>>>,
    overflow_pending: Arc<AtomicBool>,
) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |result| {
        if paused.load(Ordering::Acquire) {
            return;
        }

        let event = match result {
            Ok(event) => event,
            Err(err) => {
                debug_log!("notify watcher error: {err}");
                return;
            }
        };

        for watch_event in convert_event_with_rename_pairing(event, &pending_from) {
            match watch_event {
                WatchEvent::Renamed { .. } => {
                    handle_rename_event(
                        &event_tx,
                        &debounce_state,
                        &overflow_pending,
                        &paused,
                        watch_event,
                    );
                }
                WatchEvent::Overflow => send_overflow_or_flag(&event_tx, &overflow_pending),
                WatchEvent::Created(_) | WatchEvent::Deleted(_) | WatchEvent::Modified(_) => {
                    handle_path_event(
                        &event_tx,
                        &debounce_state,
                        &overflow_pending,
                        &paused,
                        watch_event,
                    );
                }
            }
        }
    }
}

/// Debounce-bump both rename paths (single lock cycle), then emit the rename.
fn handle_rename_event(
    event_tx: &SyncSender<WatchEvent>,
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    overflow_pending: &AtomicBool,
    paused: &AtomicBool,
    event: WatchEvent,
) {
    if let WatchEvent::Renamed { from, to } = &event {
        let mut debounce = lock_or_recover(debounce_state, "watcher");
        // Re-check under the debounce lock: `pause()` sets the flag while
        // holding this lock and clears the map, so a `true` here means we must
        // not emit anything past the pause.
        if paused.load(Ordering::Acquire) {
            return;
        }
        let (_, flushed) =
            process_debounce(&mut debounce, &[from.as_path(), to.as_path()], None, true);
        send_expired_events(event_tx, &mut debounce, flushed);
    }
    if let SendStatus::Full(evt) = try_send_event(event_tx, event) {
        reinsert_or_overflow(event_tx, debounce_state, overflow_pending, evt);
    }
}

/// Debounce a single-path event (Created/Modified/Deleted) and emit it unless
/// suppressed. Borrows the path/event from `event` so no `PathBuf` is cloned on
/// the hot path; `event` is moved into the send only at the true fan-out point.
fn handle_path_event(
    event_tx: &SyncSender<WatchEvent>,
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    overflow_pending: &AtomicBool,
    paused: &AtomicBool,
    event: WatchEvent,
) {
    let skip_debounce = matches!(&event, WatchEvent::Deleted(_));
    let emit = {
        let path = match &event {
            WatchEvent::Created(p) | WatchEvent::Deleted(p) | WatchEvent::Modified(p) => {
                p.as_path()
            }
            // Renamed/Overflow are routed elsewhere; nothing to do here.
            _ => return,
        };
        let mut debounce = lock_or_recover(debounce_state, "watcher");
        // Re-check under the debounce lock so a `pause()` that has completed
        // (flag set + map cleared under this lock) cannot be raced by a send.
        if paused.load(Ordering::Acquire) {
            return;
        }
        let (emit, flushed) = process_debounce(
            &mut debounce,
            &[path],
            if skip_debounce { None } else { Some(&event) },
            skip_debounce,
        );
        send_expired_events(event_tx, &mut debounce, flushed);
        emit
    };
    if !emit {
        return;
    }
    if let SendStatus::Full(evt) = try_send_event(event_tx, event) {
        reinsert_or_overflow(event_tx, debounce_state, overflow_pending, evt);
    }
}

fn flush_expired(debounce: &mut HashMap<PathBuf, PendingEntry>) -> Vec<ExpiredDebouncedEvent> {
    let now = Instant::now();
    let mut flushed = Vec::new();

    debounce.retain(|path, entry| {
        if now.duration_since(entry.last_seen) >= DEBOUNCE_DURATION {
            if let Some(event) = entry.coalesced.take() {
                flushed.push(ExpiredDebouncedEvent {
                    path: path.clone(),
                    event,
                });
            }
            false
        } else {
            true
        }
    });

    flushed
}

/// Send each expired debounced event; reinsert (into the already-held guard)
/// any the full channel rejected, so they are retried on the next flush.
/// Operates on `&mut` so the caller's lock cycle is not dropped and reacquired.
fn send_expired_events(
    event_tx: &SyncSender<WatchEvent>,
    debounce: &mut HashMap<PathBuf, PendingEntry>,
    flushed: Vec<ExpiredDebouncedEvent>,
) {
    for expired in flushed {
        match try_send_event(event_tx, expired.event) {
            SendStatus::Sent | SendStatus::Disconnected => {}
            SendStatus::Full(event) => coalesce_into(debounce, expired.path, event),
        }
    }
}

/// Store `event` as the coalesced value for `path`, moving it in without an
/// extra clone (the `Entry` API would otherwise need one for `and_modify`).
fn coalesce_into(debounce: &mut HashMap<PathBuf, PendingEntry>, path: PathBuf, event: WatchEvent) {
    match debounce.entry(path) {
        Entry::Occupied(mut slot) => slot.get_mut().coalesced = Some(event),
        Entry::Vacant(slot) => {
            slot.insert(PendingEntry {
                last_seen: Instant::now(),
                coalesced: Some(event),
            });
        }
    }
}

fn send_overflow_or_flag(event_tx: &SyncSender<WatchEvent>, overflow_pending: &AtomicBool) {
    match try_send_event(event_tx, WatchEvent::Overflow) {
        SendStatus::Full(_) => {
            overflow_pending.store(true, Ordering::Release);
        }
        SendStatus::Sent | SendStatus::Disconnected => {}
    }
}

fn reinsert_or_overflow(
    event_tx: &SyncSender<WatchEvent>,
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    overflow_pending: &AtomicBool,
    event: WatchEvent,
) {
    let key = match &event {
        WatchEvent::Created(p)
        | WatchEvent::Deleted(p)
        | WatchEvent::Modified(p)
        | WatchEvent::Renamed { from: p, .. } => p.clone(),
        WatchEvent::Overflow => return,
    };
    {
        let mut debounce = lock_or_recover(debounce_state, "watcher");
        coalesce_into(&mut debounce, key, event);
    }
    send_overflow_or_flag(event_tx, overflow_pending);
}

/// Retract the rename-From entry identified by (`path`, `time`) under
/// `parent_key`, but only if it is still present unchanged. The `time` acts as a
/// version tag: a newer From that reused the same path has a different `time`
/// and is therefore left in place. Shared by `flush_stale_pending_from` and the
/// tests so the retraction logic lives in exactly one spot.
fn clear_pending_from_entry(
    pending: &mut HashMap<PathBuf, VecDeque<PendingFromEntry>>,
    parent_key: &Path,
    path: &Path,
    time: Instant,
) {
    if let Some(entries) = pending.get_mut(parent_key) {
        entries.retain(|e| !(e.path == path && e.time == time));
        if entries.is_empty() {
            pending.remove(parent_key);
        }
    }
}

/// Apply debounce policy to `paths` on an already-locked debounce map. The
/// caller owns the lock cycle (passes `&mut`) so flushing and the follow-up
/// `send_expired_events` happen under one acquisition. Returns whether the event
/// should be emitted now plus any entries that just expired.
fn process_debounce(
    debounce: &mut HashMap<PathBuf, PendingEntry>,
    paths: &[&Path],
    event: Option<&WatchEvent>,
    skip_debounce: bool,
) -> (bool, Vec<ExpiredDebouncedEvent>) {
    let now = Instant::now();

    let mut flushed = flush_expired(debounce);

    if skip_debounce {
        for p in paths {
            if let Some(mut old) = debounce.remove(*p)
                && let Some(evt) = old.coalesced.take()
            {
                flushed.push(ExpiredDebouncedEvent {
                    path: p.to_path_buf(),
                    event: evt,
                });
            }
            debounce.insert(
                p.to_path_buf(),
                PendingEntry {
                    last_seen: now,
                    coalesced: None,
                },
            );
        }
        return (true, flushed);
    }

    let suppressed = paths.iter().any(|p| {
        debounce
            .get(*p)
            .is_some_and(|entry| now.duration_since(entry.last_seen) < DEBOUNCE_DURATION)
    });

    if suppressed {
        if let Some(evt) = event {
            for p in paths {
                if let Some(entry) = debounce.get_mut(*p) {
                    let deleted_wins = matches!(entry.coalesced, Some(WatchEvent::Deleted(_)))
                        && !matches!(evt, WatchEvent::Deleted(_));
                    if !deleted_wins {
                        entry.coalesced = Some(evt.clone());
                    }
                    entry.last_seen = now;
                }
            }
        }
    } else {
        for p in paths {
            debounce.insert(
                p.to_path_buf(),
                PendingEntry {
                    last_seen: now,
                    coalesced: None,
                },
            );
        }
    }

    (!suppressed, flushed)
}

/// Lock a `Mutex`, rebuilding the guarded data on poison.
///
/// # Poison recovery
///
/// A thread panicked while holding this lock, so the guarded data may be
/// partially mutated. Reusing it would let subsequent operations run on a
/// distorted state. The watcher's guarded maps are pure ephemeral caches
/// (debounce/rename bookkeeping rebuilt from incoming events), so on poison we
/// discard the contents (`T::default()`) and clear the poison flag — recovering
/// to a clean, consistent state instead of silently continuing on corrupt data.
/// At worst a few in-flight events are lost, which is preferable to emitting
/// wrong ones, and far preferable to crashing the TUI.
fn lock_or_recover<'a, T: Default>(
    mutex: &'a Mutex<T>,
    label: &str,
) -> std::sync::MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            debug_log!("{label} mutex poisoned; rebuilding guarded state");
            let mut guard = poisoned.into_inner();
            *guard = T::default();
            mutex.clear_poison();
            guard
        }
    }
}

fn convert_event_with_rename_pairing(
    event: notify::Event,
    pending_from: &Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>>,
) -> Vec<WatchEvent> {
    match &event.kind {
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)) => {
            let cookie = event.attrs.tracker();
            let Some(path) = event.paths.into_iter().next() else {
                return Vec::new();
            };
            let parent_key = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let mut pending = lock_or_recover(pending_from, "pending_from");
            let entries = pending.entry(parent_key).or_default();
            if entries.len() >= PENDING_FROM_LIMIT {
                // Bounded: a mass rename would grow this without limit. Stop
                // buffering and signal Overflow (full refresh) — consistent with
                // the channel batch-overflow handling. The unmatched To later
                // surfaces as a plain Created, which the refresh reconciles.
                //
                // Clear the parent's queue before returning: the buffered Froms
                // would otherwise flush after the 2s timeout as up to
                // PENDING_FROM_LIMIT stale Deleted events — an event storm right
                // after we already signalled a full refresh via Overflow. The
                // Overflow-driven refresh reconciles the final state on its own,
                // so dropping the buffered Froms is safe (any still-unmatched To
                // simply surfaces as a Created).
                let dropped = entries.len();
                entries.clear();
                debug_log!(
                    "pending_from limit hit for {}; clearing {} buffered From(s) and emitting Overflow",
                    path.display(),
                    dropped
                );
                return vec![WatchEvent::Overflow];
            }
            entries.push_back(PendingFromEntry {
                path,
                cookie,
                time: Instant::now(),
            });
            Vec::new()
        }
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)) => {
            let to_cookie = event.attrs.tracker();
            let to_path = event.paths.into_iter().next();
            let parent_key = to_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let from_entry = {
                let mut pending = lock_or_recover(pending_from, "pending_from");
                take_paired_from(&mut pending, &parent_key, to_cookie)
            };
            match (from_entry, to_path) {
                (Some(entry), Some(to)) => vec![WatchEvent::Renamed {
                    from: entry.path,
                    to,
                }],
                (None, Some(to)) => vec![WatchEvent::Created(to)],
                _ => Vec::new(),
            }
        }
        _ => convert_event(event),
    }
}

/// Pick the buffered rename-From that pairs with an incoming rename-To.
///
/// Backends that provide a rename-tracker cookie (Linux inotify) let us pair
/// deterministically even when several renames interleave in one directory:
/// match on the cookie. When the To carries no cookie (macOS FSEvents) or none
/// of the buffered Froms match it, fall back to FIFO order — the best heuristic
/// available without rename tracking. Removes the chosen entry and prunes the
/// parent bucket when it empties, all under the caller's single lock.
fn take_paired_from(
    pending: &mut HashMap<PathBuf, VecDeque<PendingFromEntry>>,
    parent_key: &Path,
    to_cookie: Option<usize>,
) -> Option<PendingFromEntry> {
    let entries = pending.get_mut(parent_key)?;
    let taken = match to_cookie {
        // A cookie that matches no buffered From means there is genuinely no pair
        // for this To (e.g. a move-in from outside the watched dir): return None
        // so it surfaces as `Created` and the orphaned From times out to
        // `Deleted`, rather than stealing an unrelated From via FIFO and emitting
        // a bogus `Renamed`. FIFO is only the right heuristic when the backend
        // gives no cookie at all (macOS FSEvents).
        Some(cookie) => match entries.iter().position(|e| e.cookie == Some(cookie)) {
            Some(idx) => entries.remove(idx),
            None => None,
        },
        None => entries.pop_front(),
    };
    if entries.is_empty() {
        pending.remove(parent_key);
    }
    taken
}

fn convert_event(event: notify::Event) -> Vec<WatchEvent> {
    match event.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(_) => map_paths(event.paths, WatchEvent::Created),
        EventKind::Remove(_) => map_paths(event.paths, WatchEvent::Deleted),
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::Both)) => {
            let mut paths = event.paths;
            if paths.len() == 2 {
                let from = paths.swap_remove(0);
                let to = paths.swap_remove(0);
                vec![WatchEvent::Renamed { from, to }]
            } else {
                map_paths(paths, WatchEvent::Modified)
            }
        }
        EventKind::Modify(_) => map_paths(event.paths, WatchEvent::Modified),
        EventKind::Any => map_paths(event.paths, WatchEvent::Modified),
        EventKind::Other => map_paths(event.paths, WatchEvent::Modified),
    }
}

fn map_paths(paths: Vec<PathBuf>, map: impl Fn(PathBuf) -> WatchEvent) -> Vec<WatchEvent> {
    paths.into_iter().map(map).collect()
}

/// Returns true if the unwatch error indicates the watch is already gone.
///
/// - `WatchNotFound`: the watcher's internal map no longer tracks this path.
/// - `Io(...)`: the backend reports the watch/file is already gone. The exact
///   error differs per platform, so the classification is `cfg`-split below.
fn is_watch_already_gone_error(e: &notify::Error) -> bool {
    match &e.kind {
        notify::ErrorKind::WatchNotFound => true,
        notify::ErrorKind::Io(io_err) => is_already_gone_io(io_err),
        _ => false,
    }
}

/// Non-Windows (Linux/macOS): the OS already dropped the watch / the file is
/// gone.
///
/// - Linux inotify: `inotify_rm_watch` returns `EINVAL` (the kernel auto-removed
///   the watch on directory deletion) — std maps `EINVAL` to `InvalidInput`.
/// - macOS kqueue: `EV_DELETE` fails with `ENOENT` — std maps it to `NotFound`.
///
/// Matching on `io::ErrorKind` keeps this portable instead of hardcoding raw
/// errno values.
#[cfg(not(windows))]
fn is_already_gone_io(io_err: &io::Error) -> bool {
    matches!(
        io_err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
    )
}

/// Windows: `ReadDirectoryChangesW` reports the watch is already gone. The
/// canonical code is `ERROR_NOT_FOUND` (1168), which has no dedicated
/// `io::ErrorKind`, so match its raw value as well as the `NotFound` kind that
/// covers `ERROR_FILE_NOT_FOUND` / `ERROR_PATH_NOT_FOUND`. (Hardcoding
/// `EINVAL`/`ENOENT` here was the bug — those errno values do not match the
/// Win32 codes, so normal unwatch was reported as an error.)
#[cfg(windows)]
fn is_already_gone_io(io_err: &io::Error) -> bool {
    const ERROR_NOT_FOUND: i32 = 1168;
    io_err.kind() == io::ErrorKind::NotFound || io_err.raw_os_error() == Some(ERROR_NOT_FOUND)
}

fn notify_to_io(err: &notify::Error) -> io::Error {
    io::Error::other(err.to_string())
}

/// Returns true if `path` likely resolves to `watched` even though `path` no
/// longer exists on disk.
///
/// Two resolution strategies:
/// 1. **Relative path:** join with the current working directory and compare
///    the cleaned result to `watched`.
///
///    *TOCTOU note:* the cwd is read once and used immediately.  In a TUI file
///    manager the process cwd changes only on explicit user navigation, so the
///    race window is negligible.  The alternative — snapshotting cwd at watch
///    time — would add bookkeeping complexity for no practical gain.
///
/// 2. **Dangling symlink:** resolve the symlink target and compare.
fn path_points_to_missing_watch(path: &Path, watched: &Path) -> bool {
    if path.is_relative()
        && let Ok(current_dir) = std::env::current_dir()
        && crate::fs::path::clean_path(&current_dir.join(path)) == watched
    {
        return true;
    }

    std::fs::read_link(path).is_ok_and(|target| {
        let target = if target.is_absolute() {
            normalize_missing_target(&target)
        } else if let Some(parent) = path.parent() {
            normalize_missing_target(&parent.join(target))
        } else {
            target
        };
        target == watched
    })
}

fn normalize_missing_target(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
