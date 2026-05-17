use crate::debug_log;
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify::{Watcher as NotifyWatcher, event::RenameMode};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const DEBOUNCE_DURATION: Duration = Duration::from_millis(300);
const PENDING_FROM_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub enum WatchEvent {
    Created(PathBuf),
    Deleted(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

struct PendingEntry {
    last_seen: Instant,
    coalesced: Option<WatchEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhichWatcher {
    Primary,
    Fallback,
}

pub struct Watcher {
    primary: RecommendedWatcher,
    fallback: Option<PollWatcher>,
    watchers: HashMap<PathBuf, WhichWatcher>,
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
    debounce_state: Arc<Mutex<HashMap<PathBuf, PendingEntry>>>,
    pending_from: Arc<Mutex<Option<PathBuf>>>,
    pending_from_time: Arc<Mutex<Option<Instant>>>,
    path_cache: HashMap<PathBuf, PathBuf>,
}

impl Watcher {
    pub fn new(event_tx: Sender<WatchEvent>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let debounce_state = Arc::new(Mutex::new(HashMap::new()));
        let pending_from = Arc::new(Mutex::new(None));
        let pending_from_time = Arc::new(Mutex::new(None));
        let primary = RecommendedWatcher::new(
            make_handler(
                event_tx.clone(),
                Arc::clone(&paused),
                Arc::clone(&debounce_state),
                Arc::clone(&pending_from),
                Arc::clone(&pending_from_time),
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
            pending_from_time,
            path_cache: HashMap::new(),
        })
    }

    fn create_fallback(&mut self) -> io::Result<&mut PollWatcher> {
        if self.fallback.is_none() {
            let fallback = PollWatcher::new(
                make_handler(
                    self.event_tx.clone(),
                    Arc::clone(&self.paused),
                    Arc::clone(&self.debounce_state),
                    Arc::clone(&self.pending_from),
                    Arc::clone(&self.pending_from_time),
                ),
                Config::default(),
            )
            .map_err(|e| notify_to_io(&e))?;
            self.fallback = Some(fallback);
        }
        #[allow(clippy::unwrap_used)]
        Ok(self.fallback.as_mut().unwrap())
    }

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

        match self.watchers.get(&path) {
            Some(WhichWatcher::Primary) => {
                let _ = self.primary.unwatch(&path);
            }
            Some(WhichWatcher::Fallback) => {
                if let Some(fb) = self.fallback.as_mut() {
                    let _ = fb.unwatch(&path);
                }
            }
            None => {}
        }

        self.watchers.remove(&path);
        self.path_cache.retain(|_, v| v != &path);
        Ok(())
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
        if pending.is_some() {
            debug_log!("watcher paused: clearing stale pending_from");
        }
        *pending = None;
        *lock_or_recover(&self.pending_from_time, "pending_from_time") = None;
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }

    pub fn flush_pending(&self) {
        if self.paused.load(Ordering::Acquire) {
            return;
        }

        let mut debounce = lock_or_recover(&self.debounce_state, "watcher");
        let flushed = flush_expired(&mut debounce);
        drop(debounce);
        for evt in flushed {
            if let Err(e) = self.event_tx.send(evt) {
                debug_log!("watcher send failed: {e}");
            }
        }

        let mut pending = lock_or_recover(&self.pending_from, "pending_from");
        let mut pending_time = lock_or_recover(&self.pending_from_time, "pending_from_time");
        if let (Some(path), Some(time)) = (pending.as_ref(), pending_time.as_ref())
            && time.elapsed() >= PENDING_FROM_TIMEOUT
        {
            debug_log!(
                "stale rename From timed out: emitting Deleted for {}",
                path.display(),
            );
            if let Err(e) = self.event_tx.send(WatchEvent::Deleted(path.clone())) {
                debug_log!("watcher send failed: {e}");
            }
            *pending = None;
            *pending_time = None;
        }
    }
}

fn make_handler(
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
    debounce_state: Arc<Mutex<HashMap<PathBuf, PendingEntry>>>,
    pending_from: Arc<Mutex<Option<PathBuf>>>,
    pending_from_time: Arc<Mutex<Option<Instant>>>,
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

        for watch_event in
            convert_event_with_rename_pairing(event, &pending_from, &pending_from_time)
        {
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
                    for evt in flushed {
                        if let Err(e) = event_tx.send(evt) {
                            debug_log!("watcher send failed: {e}");
                        }
                    }
                    if let Err(e) = event_tx.send(watch_event) {
                        debug_log!("watcher send failed: {e}");
                    }
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
                for evt in flushed {
                    if let Err(e) = event_tx.send(evt) {
                        debug_log!("watcher send failed: {e}");
                    }
                }
                if !emit {
                    continue;
                }
            }

            if let Err(e) = event_tx.send(watch_event) {
                debug_log!("watcher send failed: {e}");
            }
        }
    }
}

fn flush_expired(debounce: &mut HashMap<PathBuf, PendingEntry>) -> Vec<WatchEvent> {
    let now = Instant::now();
    let mut flushed = Vec::new();
    debounce.retain(|_, entry| {
        if now.duration_since(entry.last_seen) >= DEBOUNCE_DURATION {
            if let Some(evt) = entry.coalesced.take() {
                flushed.push(evt);
            }
            false
        } else {
            true
        }
    });
    flushed
}

fn process_debounce(
    debounce_state: &Mutex<HashMap<PathBuf, PendingEntry>>,
    paths: &[&Path],
    event: Option<&WatchEvent>,
    skip_debounce: bool,
) -> (bool, Vec<WatchEvent>) {
    let now = Instant::now();
    let mut debounce = lock_or_recover(debounce_state, "watcher");

    let mut flushed = flush_expired(&mut debounce);

    if skip_debounce {
        for p in paths {
            if let Some(mut old) = debounce.remove(*p)
                && let Some(evt) = old.coalesced.take()
            {
                flushed.push(evt);
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

fn lock_pending(
    pending_from: &Mutex<Option<PathBuf>>,
) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
    lock_or_recover(pending_from, "pending_from")
}

fn convert_event_with_rename_pairing(
    event: notify::Event,
    pending_from: &Mutex<Option<PathBuf>>,
    pending_from_time: &Mutex<Option<Instant>>,
) -> Vec<WatchEvent> {
    match &event.kind {
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)) => {
            if let Some(path) = event.paths.into_iter().next() {
                let mut pending = lock_pending(pending_from);
                let mut pending_time = lock_or_recover(pending_from_time, "pending_from_time");
                let mut events = Vec::new();
                if let Some(stale) = pending.take() {
                    debug_log!(
                        "orphan rename From: emitting Deleted for stale path {}",
                        stale.display(),
                    );
                    events.push(WatchEvent::Deleted(stale));
                }
                *pending = Some(path);
                *pending_time = Some(Instant::now());
                events
            } else {
                Vec::new()
            }
        }
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)) => {
            let to_path = event.paths.into_iter().next();
            let from_path = lock_pending(pending_from).take();
            let _ = lock_or_recover(pending_from_time, "pending_from_time").take();
            match (from_path, to_path) {
                (Some(from), Some(to)) => vec![WatchEvent::Renamed { from, to }],
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
        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx)?;
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
        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx)?;

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

        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx)?;
        watcher.watch(&link)?;
        std::fs::remove_dir_all(&target)?;

        watcher.unwatch(&link)?;

        assert!(watcher.watched_dirs().is_empty());
        assert!(watcher.watchers.is_empty());
        Ok(())
    }

    #[test]
    fn watcher_pause_and_resume_do_not_panic() -> TestResult {
        let (event_tx, _event_rx) = mpsc::channel();
        let watcher = Watcher::new(event_tx)?;

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
        let pending: Mutex<Option<PathBuf>> = Mutex::new(None);
        let pending_time: Mutex<Option<Instant>> = Mutex::new(None);
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

        let from_events = convert_event_with_rename_pairing(from, &pending, &pending_time);
        assert!(from_events.is_empty());

        let to_events = convert_event_with_rename_pairing(to, &pending, &pending_time);
        assert!(
            matches!(to_events.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old") && to == &PathBuf::from("new"))
        );
    }

    #[test]
    fn watcher_created_with_primary_only_no_fallback() -> TestResult {
        let (event_tx, _event_rx) = mpsc::channel();
        let watcher = Watcher::new(event_tx)?;
        assert!(watcher.fallback.is_none());
        assert!(watcher.watchers.is_empty());
        Ok(())
    }

    #[test]
    fn flush_pending_emits_coalesced_event_after_debounce_window() -> TestResult {
        let (tx, rx) = mpsc::channel();
        let watcher = Watcher::new(tx)?;

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
        let (tx, rx) = mpsc::channel();
        let watcher = Watcher::new(tx)?;

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

        watcher.pause();
        watcher.flush_pending();

        assert!(rx.try_recv().is_err());

        watcher.resume();
        watcher.flush_pending();

        let flushed = rx.try_recv()?;
        assert!(matches!(flushed, WatchEvent::Modified(p) if p == path));
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
        let pending: Mutex<Option<PathBuf>> = Mutex::new(None);
        let pending_time: Mutex<Option<Instant>> = Mutex::new(None);

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

        let ev1 = convert_event_with_rename_pairing(from1, &pending, &pending_time);
        assert!(ev1.is_empty());

        let ev2 = convert_event_with_rename_pairing(from2, &pending, &pending_time);
        assert!(matches!(ev2.as_slice(), [WatchEvent::Deleted(p)] if p == &PathBuf::from("old_a")));

        let ev3 = convert_event_with_rename_pairing(to, &pending, &pending_time);
        assert!(
            matches!(ev3.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old_b") && to == &PathBuf::from("new_b"))
        );
    }

    #[test]
    fn flush_pending_emits_deleted_for_stale_from() -> TestResult {
        let (tx, rx) = mpsc::channel();
        let watcher = Watcher::new(tx)?;

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            *pending = Some(PathBuf::from("/tmp/stale_file.txt"));
        }
        {
            let mut pending_time = watcher
                .pending_from_time
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            *pending_time = Some(Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1));
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
                .is_none()
        );
        assert!(
            watcher
                .pending_from_time
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn flush_pending_keeps_fresh_from() -> TestResult {
        let (tx, rx) = mpsc::channel();
        let watcher = Watcher::new(tx)?;

        {
            let mut pending = watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            *pending = Some(PathBuf::from("/tmp/fresh_file.txt"));
        }
        {
            let mut pending_time = watcher
                .pending_from_time
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            *pending_time = Some(Instant::now());
        }

        watcher.flush_pending();

        assert!(rx.try_recv().is_err());
        assert!(
            watcher
                .pending_from
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_some()
        );
        Ok(())
    }
}
