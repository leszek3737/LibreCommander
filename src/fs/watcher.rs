use crate::debug_log;
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify::{Watcher as NotifyWatcher, event::RenameMode};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

const DEBOUNCE_DURATION: Duration = Duration::from_millis(300);
const PENDING_FROM_TIMEOUT: Duration = Duration::from_secs(2);

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
    debounce_state: Arc<Mutex<HashMap<PathBuf, PendingEntry>>>,
    pending_from: Arc<Mutex<HashMap<PathBuf, PendingFromEntry>>>,
    overflow_pending: Arc<AtomicBool>,
    path_cache: HashMap<PathBuf, PathBuf>,
}

impl Watcher {
    pub fn new(event_tx: Arc<SyncSender<WatchEvent>>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let debounce_state = Arc::new(Mutex::new(HashMap::new()));
        let pending_from = Arc::new(Mutex::new(HashMap::new()));
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
            let fallback = PollWatcher::new(
                make_handler(
                    Arc::clone(&self.event_tx),
                    Arc::clone(&self.paused),
                    Arc::clone(&self.debounce_state),
                    Arc::clone(&self.pending_from),
                    Arc::clone(&self.overflow_pending),
                ),
                Config::default(),
            )
            .map_err(|e| notify_to_io(&e))?;
            self.fallback = Some(fallback);
        }
        let Some(f) = self.fallback.as_mut() else {
            return Err(io::Error::other(
                "create_fallback: fallback must be initialized",
            ));
        };
        Ok(f)
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
                Ok(path) => {
                    self.path_cache.insert(path.clone(), path.clone());
                    path
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
        self.watchers.retain(|watched, _| {
            watched.as_path() != path && !path_points_to_missing_watch(path, watched)
        });
        self.path_cache
            .retain(|_, v| self.watchers.contains_key(v.as_path()));
    }

    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.watchers.keys().cloned().collect()
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
        let mut pending = lock_or_recover(&self.pending_from, "pending_from");
        if !pending.is_empty() {
            debug_log!("watcher paused: clearing stale pending_from entries");
        }
        pending.clear();
        drop(pending);
        let mut debounce = lock_or_recover(&self.debounce_state, "watcher");
        if !debounce.is_empty() {
            debug_log!(
                "watcher paused: clearing {} debounce entries",
                debounce.len()
            );
        }
        debounce.clear();
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

        let mut debounce = lock_or_recover(&self.debounce_state, "watcher");
        let flushed = flush_expired(&mut debounce);
        drop(debounce);
        send_expired_events(&self.event_tx, &self.debounce_state, flushed);

        let stale_entries: Vec<(PathBuf, PathBuf, Instant)> = {
            let pending = lock_or_recover(&self.pending_from, "pending_from");
            pending
                .iter()
                .filter(|(_, entry)| entry.time.elapsed() >= PENDING_FROM_TIMEOUT)
                .map(|(parent, entry)| (parent.clone(), entry.path.clone(), entry.time))
                .collect()
        };

        if !stale_entries.is_empty() {
            let mut pending = lock_or_recover(&self.pending_from, "pending_from");
            for (parent_key, path, time) in stale_entries {
                debug_log!(
                    "stale rename From timed out: emitting Deleted for {} (parent {})",
                    path.display(),
                    parent_key.display(),
                );
                match try_send_event(&self.event_tx, WatchEvent::Deleted(path.clone())) {
                    SendStatus::Full(_) => {}
                    SendStatus::Sent | SendStatus::Disconnected => {
                        if let Some(entry) = pending.get(&parent_key)
                            && entry.path == path
                            && entry.time == time
                        {
                            pending.remove(&parent_key);
                        }
                    }
                }
            }
        }
    }
}

fn try_send_event(event_tx: &SyncSender<WatchEvent>, event: WatchEvent) -> SendStatus {
    match event_tx.try_send(event) {
        Ok(()) => SendStatus::Sent,
        Err(TrySendError::Full(event)) => {
            debug_log!("watcher event queue full; dropping event");
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
    pending_from: Arc<Mutex<HashMap<PathBuf, PendingFromEntry>>>,
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
            let path = match &watch_event {
                WatchEvent::Created(p) | WatchEvent::Deleted(p) | WatchEvent::Modified(p) => {
                    Some(p.clone())
                }
                WatchEvent::Renamed { from, to } => {
                    let (_, flushed) = process_debounce(
                        &debounce_state,
                        &[from.as_path(), to.as_path()],
                        None,
                        true,
                    );
                    send_expired_events(&event_tx, &debounce_state, flushed);
                    if let SendStatus::Full(evt) = try_send_event(&event_tx, watch_event) {
                        reinsert_or_overflow(&event_tx, &debounce_state, &overflow_pending, evt);
                    }
                    continue;
                }
                WatchEvent::Overflow => {
                    let _ = try_send_event(&event_tx, watch_event);
                    continue;
                }
            };

            let skip_debounce = matches!(&watch_event, WatchEvent::Deleted(_));
            if let Some(path) = path {
                let (emit, flushed) = process_debounce(
                    &debounce_state,
                    &[path.as_path()],
                    if skip_debounce {
                        None
                    } else {
                        Some(&watch_event)
                    },
                    skip_debounce,
                );
                send_expired_events(&event_tx, &debounce_state, flushed);
                if !emit {
                    continue;
                }
            }

            if let SendStatus::Full(evt) = try_send_event(&event_tx, watch_event) {
                reinsert_or_overflow(&event_tx, &debounce_state, &overflow_pending, evt);
            }
        }
    }
}

fn flush_expired(debounce: &mut HashMap<PathBuf, PendingEntry>) -> Vec<ExpiredDebouncedEvent> {
    let now = Instant::now();
    let mut flushed = Vec::new();

    let expired_paths = debounce
        .iter()
        .filter(|(_, entry)| now.duration_since(entry.last_seen) >= DEBOUNCE_DURATION)
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();

    for path in expired_paths {
        if let Some(mut entry) = debounce.remove(&path)
            && let Some(event) = entry.coalesced.take()
        {
            flushed.push(ExpiredDebouncedEvent { path, event });
        }
    }

    flushed
}

fn send_expired_events(
    event_tx: &SyncSender<WatchEvent>,
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    flushed: Vec<ExpiredDebouncedEvent>,
) {
    for expired in flushed {
        let path = expired.path;
        match try_send_event(event_tx, expired.event) {
            SendStatus::Sent | SendStatus::Disconnected => {}
            SendStatus::Full(event) => {
                let mut debounce = lock_or_recover(debounce_state, "watcher");
                debounce.entry(path).or_insert(PendingEntry {
                    last_seen: Instant::now(),
                    coalesced: Some(event),
                });
            }
        }
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
    let mut debounce = lock_or_recover(debounce_state, "watcher");
    debounce.entry(key).or_insert(PendingEntry {
        last_seen: Instant::now(),
        coalesced: Some(event),
    });
    drop(debounce);
    if event_tx.try_send(WatchEvent::Overflow).is_err() {
        overflow_pending.store(true, Ordering::Release);
    }
}

#[cfg(test)]
fn clear_pending_from_if_unchanged(
    pending_from: &Mutex<HashMap<PathBuf, PendingFromEntry>>,
    parent_key: &Path,
    path: &Path,
    time: Instant,
) {
    let mut pending = lock_or_recover(pending_from, "pending_from");
    if let Some(entry) = pending.get(parent_key)
        && entry.path == path
        && entry.time == time
    {
        pending.remove(parent_key);
    }
}

fn lock_pending(
    pending_from: &Mutex<HashMap<PathBuf, PendingFromEntry>>,
) -> std::sync::MutexGuard<'_, HashMap<PathBuf, PendingFromEntry>> {
    lock_or_recover(pending_from, "pending_from")
}

/// Process debounce for watched paths.
///
/// Returns `(should_emit, expired_events)` where:
/// - `should_emit`: caller should forward the current event downstream
///   (`false` means the event was suppressed/coalesced by debounce)
/// - `expired_events`: entries whose debounce window has elapsed;
///   these should always be sent regardless of `should_emit`
fn process_debounce(
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    paths: &[&Path],
    event: Option<&WatchEvent>,
    skip_debounce: bool,
) -> (bool, Vec<ExpiredDebouncedEvent>) {
    let now = Instant::now();
    let mut debounce = lock_or_recover(debounce_state, "watcher");

    let mut flushed = flush_expired(&mut debounce);

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
                    entry.coalesced = Some(evt.clone());
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

fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| {
        debug_log!("{label} mutex poisoned, recovering: {e}");
        e.into_inner()
    })
}

fn convert_event_with_rename_pairing(
    event: notify::Event,
    pending_from: &Mutex<HashMap<PathBuf, PendingFromEntry>>,
) -> Vec<WatchEvent> {
    match &event.kind {
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)) => {
            if let Some(path) = event.paths.into_iter().next() {
                let parent_key = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                let mut pending = lock_pending(pending_from);
                let mut events = Vec::new();
                if let Some(existing) = pending.get(&parent_key) {
                    debug_log!(
                        "orphan rename From: emitting Deleted for stale path {} (parent {})",
                        existing.path.display(),
                        parent_key.display(),
                    );
                    events.push(WatchEvent::Deleted(existing.path.clone()));
                    pending.remove(&parent_key);
                }
                pending.insert(
                    parent_key,
                    PendingFromEntry {
                        path,
                        time: Instant::now(),
                    },
                );
                events
            } else {
                Vec::new()
            }
        }
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)) => {
            let to_path = event.paths.into_iter().next();
            let parent_key = to_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let from_entry = lock_pending(pending_from).remove(&parent_key);
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

fn convert_event(event: notify::Event) -> Vec<WatchEvent> {
    match event.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(_) => map_paths(event.paths, WatchEvent::Created),
        EventKind::Remove(_) => map_paths(event.paths, WatchEvent::Deleted),
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::Both)) => {
            if event.paths.len() == 2 {
                vec![WatchEvent::Renamed {
                    from: event.paths[0].clone(),
                    to: event.paths[1].clone(),
                }]
            } else {
                map_paths(event.paths, WatchEvent::Modified)
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
/// On Linux (inotify), the OS auto-removes watches when a directory is deleted,
/// so subsequent unwatch calls fail with "No watch was found" or EINVAL.
fn is_watch_already_gone_error(e: &notify::Error) -> bool {
    let msg = e.to_string();
    msg.contains("No watch was found") || msg.contains("Invalid argument")
}

fn notify_to_io(err: &notify::Error) -> io::Error {
    io::Error::other(err.to_string())
}

fn path_points_to_missing_watch(path: &Path, watched: &Path) -> bool {
    if path.is_relative()
        && let Ok(current_dir) = std::env::current_dir()
        && current_dir.join(path) == watched
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
mod tests {
    use super::*;
    use std::error::Error;
    use std::sync::mpsc;

    type TestResult = Result<(), Box<dyn Error>>;

    #[test]
    fn watcher_can_watch_and_unwatch_directory() -> TestResult {
        let tempdir = tempfile::tempdir()?;
        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let mut watcher = Watcher::new(Arc::new(event_tx))?;
        let watched_path = tempdir.path().canonicalize()?;

        watcher.watch(tempdir.path())?;
        assert_eq!(watcher.watched_dirs(), vec![watched_path]);

        watcher.unwatch(tempdir.path())?;
        assert!(watcher.watched_dirs().is_empty());
        Ok(())
    }

    #[test]
    fn watcher_unwatch_cleans_state_when_directory_vanished() -> TestResult {
        let tempdir = tempfile::tempdir()?;
        let watched_path = tempdir.path().to_path_buf();
        let canonical = watched_path.canonicalize()?;
        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let mut watcher = Watcher::new(Arc::new(event_tx))?;

        watcher.watch(&watched_path)?;
        std::fs::remove_dir_all(&watched_path)?;

        watcher.unwatch(&canonical)?;

        assert!(watcher.watched_dirs().is_empty());
        assert!(watcher.watchers.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn watcher_unwatch_cleans_state_when_symlink_target_vanished() -> TestResult {
        let tempdir = tempfile::tempdir()?;
        let target = tempdir.path().join("target");
        let link = tempdir.path().join("link");
        std::fs::create_dir(&target)?;
        std::os::unix::fs::symlink(&target, &link)?;

        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let mut watcher = Watcher::new(Arc::new(event_tx))?;
        watcher.watch(&link)?;
        std::fs::remove_dir_all(&target)?;

        watcher.unwatch(&link)?;

        assert!(watcher.watched_dirs().is_empty());
        assert!(watcher.watchers.is_empty());
        Ok(())
    }

    #[test]
    fn watcher_pause_and_resume_do_not_panic() -> TestResult {
        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(event_tx))?;

        watcher.pause();
        watcher.resume();
        Ok(())
    }

    #[test]
    fn convert_event_emits_all_create_paths() {
        let event = notify::Event {
            kind: EventKind::Create(notify::event::CreateKind::Any),
            paths: vec![PathBuf::from("a"), PathBuf::from("b")],
            attrs: Default::default(),
        };

        let events = convert_event(event);

        assert!(matches!(&events[0], WatchEvent::Created(path) if path == &PathBuf::from("a")));
        assert!(matches!(&events[1], WatchEvent::Created(path) if path == &PathBuf::from("b")));
    }

    #[test]
    fn convert_event_maps_split_rename_events() {
        let pending: Mutex<HashMap<PathBuf, PendingFromEntry>> = Mutex::new(HashMap::new());
        let from = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![PathBuf::from("old")],
            attrs: Default::default(),
        };
        let to = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
            paths: vec![PathBuf::from("new")],
            attrs: Default::default(),
        };

        let from_events = convert_event_with_rename_pairing(from, &pending);
        assert!(from_events.is_empty());

        let to_events = convert_event_with_rename_pairing(to, &pending);
        assert!(
            matches!(to_events.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old") && to == &PathBuf::from("new"))
        );
    }

    #[test]
    fn watcher_created_with_primary_only_no_fallback() -> TestResult {
        let (event_tx, _event_rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(event_tx))?;
        assert!(watcher.fallback.is_none());
        assert!(watcher.watchers.is_empty());
        Ok(())
    }

    #[test]
    fn flush_pending_emits_coalesced_event_after_debounce_window() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        let path = PathBuf::from("/tmp/test_file.txt");
        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            debounce.insert(
                path.clone(),
                PendingEntry {
                    last_seen: Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1),
                    coalesced: Some(WatchEvent::Modified(path.clone())),
                },
            );
        }

        watcher.flush_pending();

        let flushed = rx.try_recv()?;
        assert!(matches!(flushed, WatchEvent::Modified(p) if p == path));
        Ok(())
    }

    #[test]
    fn flush_pending_does_not_emit_while_paused() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        watcher.pause();

        let path = PathBuf::from("/tmp/test_file.txt");
        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            debounce.insert(
                path.clone(),
                PendingEntry {
                    last_seen: Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1),
                    coalesced: Some(WatchEvent::Modified(path.clone())),
                },
            );
        }

        watcher.flush_pending();

        assert!(rx.try_recv().is_err());

        watcher.resume();
        watcher.flush_pending();

        let flushed = rx.try_recv()?;
        assert!(matches!(flushed, WatchEvent::Modified(p) if p == path));
        Ok(())
    }

    #[test]
    fn flush_pending_does_not_block_when_queue_is_full() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(0);
        let watcher = Watcher::new(Arc::new(tx))?;

        let first = PathBuf::from("/tmp/first.txt");
        let expired = Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1);
        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            debounce.insert(
                first.clone(),
                PendingEntry {
                    last_seen: expired,
                    coalesced: Some(WatchEvent::Modified(first)),
                },
            );
        }

        watcher.flush_pending();

        assert!(rx.try_recv().is_err());
        let debounce = watcher
            .debounce_state
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(entry) = debounce.get(&PathBuf::from("/tmp/first.txt")) else {
            assert!(debounce.contains_key(&PathBuf::from("/tmp/first.txt")));
            return Ok(());
        };
        assert!(entry.last_seen > expired);
        assert!(matches!(entry.coalesced, Some(WatchEvent::Modified(_))));
        Ok(())
    }

    #[test]
    fn flush_pending_retries_full_debounced_event() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.try_send(WatchEvent::Modified(PathBuf::from("/tmp/fill.txt")))?;
        let watcher = Watcher::new(Arc::new(tx))?;

        let path = PathBuf::from("/tmp/retry.txt");
        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            debounce.insert(
                path.clone(),
                PendingEntry {
                    last_seen: Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1),
                    coalesced: Some(WatchEvent::Modified(path.clone())),
                },
            );
        }

        watcher.flush_pending();
        let filler = rx.try_recv()?;
        assert!(
            matches!(filler, WatchEvent::Modified(p) if p.as_path() == Path::new("/tmp/fill.txt"))
        );

        watcher.flush_pending();
        assert!(rx.try_recv().is_err());

        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            let Some(entry) = debounce.get_mut(&path) else {
                assert!(debounce.contains_key(&path));
                return Ok(());
            };
            entry.last_seen = Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1);
        }

        watcher.flush_pending();

        let retried = rx.try_recv()?;
        assert!(matches!(retried, WatchEvent::Modified(p) if p == path));
        Ok(())
    }

    #[test]
    fn flush_pending_keeps_stale_from_when_queue_is_full() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.try_send(WatchEvent::Modified(PathBuf::from("/tmp/fill.txt")))?;
        let watcher = Watcher::new(Arc::new(tx))?;
        let stale = PathBuf::from("/tmp/stale_file.txt");
        let parent_key = PathBuf::new();

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            pending.insert(
                parent_key.clone(),
                PendingFromEntry {
                    path: stale.clone(),
                    time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
                },
            );
        }

        watcher.flush_pending();

        let pending = watcher
            .pending_from
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        assert_eq!(
            pending.get(&parent_key).map(|e| e.path.clone()),
            Some(stale)
        );
        drop(rx);
        Ok(())
    }

    #[test]
    fn process_debounce_coalesces_suppressed_event() {
        let debounce_state: Mutex<HashMap<PathBuf, PendingEntry>> = Mutex::new(HashMap::new());
        let path = PathBuf::from("/tmp/coalesce.txt");
        let event = WatchEvent::Modified(path.clone());

        let (emit1, flushed1) =
            process_debounce(&debounce_state, &[path.as_path()], Some(&event), false);
        assert!(emit1);
        assert!(flushed1.is_empty());

        let (emit2, flushed2) =
            process_debounce(&debounce_state, &[path.as_path()], Some(&event), false);
        assert!(!emit2);
        assert!(flushed2.is_empty());

        let map = debounce_state.lock().unwrap_or_else(|err| err.into_inner());
        let Some(entry) = map.get(&path) else {
            assert!(map.contains_key(&path));
            return;
        };
        assert!(entry.coalesced.is_some());
    }

    #[test]
    fn stale_from_emits_deleted_and_stores_new_from() {
        let pending: Mutex<HashMap<PathBuf, PendingFromEntry>> = Mutex::new(HashMap::new());

        let from1 = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![PathBuf::from("old_a")],
            attrs: Default::default(),
        };
        let from2 = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![PathBuf::from("old_b")],
            attrs: Default::default(),
        };
        let to = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
            paths: vec![PathBuf::from("new_b")],
            attrs: Default::default(),
        };

        let ev1 = convert_event_with_rename_pairing(from1, &pending);
        assert!(ev1.is_empty());

        let ev2 = convert_event_with_rename_pairing(from2, &pending);
        assert!(matches!(ev2.as_slice(), [WatchEvent::Deleted(p)] if p == &PathBuf::from("old_a")));

        let ev3 = convert_event_with_rename_pairing(to, &pending);
        assert!(
            matches!(ev3.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old_b") && to == &PathBuf::from("new_b"))
        );
    }

    #[test]
    fn flush_pending_emits_deleted_for_stale_from() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            pending.insert(
                PathBuf::new(),
                PendingFromEntry {
                    path: PathBuf::from("/tmp/stale_file.txt"),
                    time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
                },
            );
        }

        watcher.flush_pending();

        let evt = rx.try_recv()?;
        assert!(
            matches!(evt, WatchEvent::Deleted(p) if p.as_path() == Path::new("/tmp/stale_file.txt"))
        );

        assert!(
            watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn clear_pending_from_keeps_new_from_for_same_path() {
        let parent_key = PathBuf::new();
        let pending: Mutex<HashMap<PathBuf, PendingFromEntry>> = Mutex::new(HashMap::new());
        let old_time = Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1);
        let new_time = Instant::now();
        {
            let mut map = pending.lock().unwrap_or_else(|err| err.into_inner());
            map.insert(
                parent_key.clone(),
                PendingFromEntry {
                    path: PathBuf::from("/tmp/rename.txt"),
                    time: new_time,
                },
            );
        }

        clear_pending_from_if_unchanged(
            &pending,
            &parent_key,
            Path::new("/tmp/rename.txt"),
            old_time,
        );

        let map = pending.lock().unwrap_or_else(|err| err.into_inner());
        assert!(map.contains_key(&parent_key));
    }

    #[test]
    fn flush_pending_keeps_fresh_from() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            pending.insert(
                PathBuf::new(),
                PendingFromEntry {
                    path: PathBuf::from("/tmp/fresh_file.txt"),
                    time: Instant::now(),
                },
            );
        }

        watcher.flush_pending();

        assert!(rx.try_recv().is_err());
        assert!(
            !watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn per_parent_rename_pairing_does_not_mismatch_across_dirs() {
        let pending: Mutex<HashMap<PathBuf, PendingFromEntry>> = Mutex::new(HashMap::new());

        let dir_a = PathBuf::from("/dir_a");
        let dir_b = PathBuf::from("/dir_b");

        let from_a = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![dir_a.join("file_in_a.txt")],
            attrs: Default::default(),
        };
        let from_b = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![dir_b.join("file_in_b.txt")],
            attrs: Default::default(),
        };
        let to_b = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
            paths: vec![dir_b.join("renamed_b.txt")],
            attrs: Default::default(),
        };
        let to_a = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
            paths: vec![dir_a.join("renamed_a.txt")],
            attrs: Default::default(),
        };

        let events_a_from = convert_event_with_rename_pairing(from_a, &pending);
        assert!(events_a_from.is_empty());

        let events_b_from = convert_event_with_rename_pairing(from_b, &pending);
        assert!(events_b_from.is_empty());

        let events_b_to = convert_event_with_rename_pairing(to_b, &pending);
        assert_eq!(events_b_to.len(), 1);
        assert!(matches!(
            &events_b_to[0],
            WatchEvent::Renamed { from, to }
            if from == &dir_b.join("file_in_b.txt") && to == &dir_b.join("renamed_b.txt")
        ));

        let events_a_to = convert_event_with_rename_pairing(to_a, &pending);
        assert_eq!(events_a_to.len(), 1);
        assert!(matches!(
            &events_a_to[0],
            WatchEvent::Renamed { from, to }
            if from == &dir_a.join("file_in_a.txt") && to == &dir_a.join("renamed_a.txt")
        ));
    }

    #[test]
    fn per_parent_rename_orphan_from_emits_deleted_for_same_parent_only() {
        let pending: Mutex<HashMap<PathBuf, PendingFromEntry>> = Mutex::new(HashMap::new());

        let dir_a = PathBuf::from("/dir_a");

        let from1 = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![dir_a.join("stale.txt")],
            attrs: Default::default(),
        };
        let from2 = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
            paths: vec![dir_a.join("fresh.txt")],
            attrs: Default::default(),
        };

        let events1 = convert_event_with_rename_pairing(from1, &pending);
        assert!(events1.is_empty());

        let events2 = convert_event_with_rename_pairing(from2, &pending);
        assert_eq!(events2.len(), 1);
        assert!(matches!(
            &events2[0],
            WatchEvent::Deleted(p) if p == &dir_a.join("stale.txt")
        ));
    }

    #[test]
    fn pause_clears_debounce_state() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        let path = PathBuf::from("/tmp/test_pause_clear.txt");
        {
            let mut debounce = watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            debounce.insert(
                path,
                PendingEntry {
                    last_seen: Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1),
                    coalesced: Some(WatchEvent::Modified(PathBuf::from(
                        "/tmp/test_pause_clear.txt",
                    ))),
                },
            );
        }

        watcher.pause();
        assert!(
            watcher
                .debounce_state
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty(),
            "pause should clear debounce_state"
        );

        watcher.resume();
        watcher.flush_pending();
        assert!(rx.try_recv().is_err(), "no events after clearing debounce");
        Ok(())
    }

    #[test]
    fn reinsert_or_overflow_sends_overflow_marker() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(1);
        let debounce: Mutex<HashMap<PathBuf, PendingEntry>> = Mutex::new(HashMap::new());
        let overflow_pending = AtomicBool::new(false);

        let event = WatchEvent::Modified(PathBuf::from("/tmp/test_overflow.txt"));
        reinsert_or_overflow(&tx, &debounce, &overflow_pending, event);

        let overflow = rx.try_recv()?;
        assert!(matches!(overflow, WatchEvent::Overflow));
        assert!(
            !overflow_pending.load(Ordering::Acquire),
            "overflow_pending should be false when send succeeds"
        );

        let state = debounce.lock().unwrap_or_else(|err| err.into_inner());
        assert!(state.contains_key(&PathBuf::from("/tmp/test_overflow.txt")));
        Ok(())
    }

    #[test]
    fn reinsert_or_overflow_sets_pending_flag_on_full_queue() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(0);
        let debounce: Mutex<HashMap<PathBuf, PendingEntry>> = Mutex::new(HashMap::new());
        let overflow_pending = AtomicBool::new(false);

        let event = WatchEvent::Modified(PathBuf::from("/tmp/test_overflow_pending.txt"));
        reinsert_or_overflow(&tx, &debounce, &overflow_pending, event);

        assert!(
            overflow_pending.load(Ordering::Acquire),
            "overflow_pending should be true when Overflow send fails"
        );

        let state = debounce.lock().unwrap_or_else(|err| err.into_inner());
        assert!(state.contains_key(&PathBuf::from("/tmp/test_overflow_pending.txt")));

        assert!(
            rx.try_recv().is_err(),
            "no Overflow should arrive on full queue"
        );
        Ok(())
    }

    #[test]
    fn stale_from_per_parent_times_out_independently() -> TestResult {
        let (tx, rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        let dir_a = PathBuf::from("/dir_a");
        let dir_b = PathBuf::from("/dir_b");

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            pending.insert(
                dir_a.clone(),
                PendingFromEntry {
                    path: dir_a.join("old_a.txt"),
                    time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
                },
            );
            pending.insert(
                dir_b.clone(),
                PendingFromEntry {
                    path: dir_b.join("fresh_b.txt"),
                    time: Instant::now(),
                },
            );
        }

        watcher.flush_pending();

        let event = rx.try_recv()?;
        assert!(matches!(
            &event,
            WatchEvent::Deleted(p) if *p == dir_a.join("old_a.txt")
        ));

        assert!(rx.try_recv().is_err(), "dir_b should not time out yet");

        let pending = watcher
            .pending_from
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        assert!(pending.contains_key(&dir_b));
        assert!(!pending.contains_key(&dir_a));
        Ok(())
    }

    #[test]
    fn pause_clears_pending_from_entries() -> TestResult {
        let (tx, _rx) = mpsc::sync_channel(2048);
        let watcher = Watcher::new(Arc::new(tx))?;

        // Insert a pending_from entry
        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            pending.insert(
                PathBuf::from("/some/dir"),
                PendingFromEntry {
                    path: PathBuf::from("/some/dir/old.txt"),
                    time: Instant::now(),
                },
            );
        }

        watcher.pause();

        let pending = watcher
            .pending_from
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        assert!(
            pending.is_empty(),
            "pause() should clear all pending_from entries"
        );
        Ok(())
    }
}
