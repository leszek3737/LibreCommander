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

pub enum WatchEvent {
    Created(PathBuf),
    Deleted(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
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
    debounce_state: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    pending_from: Arc<Mutex<Option<PathBuf>>>,
    path_cache: HashMap<PathBuf, PathBuf>,
}

impl Watcher {
    pub fn new(event_tx: Sender<WatchEvent>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let debounce_state = Arc::new(Mutex::new(HashMap::new()));
        let pending_from = Arc::new(Mutex::new(None));
        let primary = RecommendedWatcher::new(
            make_handler(
                event_tx.clone(),
                Arc::clone(&paused),
                Arc::clone(&debounce_state),
                Arc::clone(&pending_from),
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
                ),
                Config::default(),
            )
            .map_err(|e| notify_to_io(&e))?;
            self.fallback = Some(fallback);
        }
        // Safe: guaranteed Some by is_none() check above or prior call
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
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }
}

fn make_handler(
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
    debounce_state: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    pending_from: Arc<Mutex<Option<PathBuf>>>,
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
                    should_emit(&debounce_state, &[from.as_path(), to.as_path()], true);
                    if let Err(e) = event_tx.send(watch_event) {
                        debug_log!("watcher send failed: {e}");
                    }
                    continue;
                }
            };

            let skip_debounce = matches!(&watch_event, WatchEvent::Deleted(_));
            if let Some(path) = path
                && !should_emit(&debounce_state, &[path.as_path()], skip_debounce)
            {
                continue;
            }

            if let Err(e) = event_tx.send(watch_event) {
                debug_log!("watcher send failed: {e}");
            }
        }
    }
}

fn should_emit(
    debounce_state: &Mutex<HashMap<PathBuf, Instant>>,
    paths: &[&Path],
    skip_debounce: bool,
) -> bool {
    let now = Instant::now();
    let mut debounce = lock_or_recover(debounce_state, "watcher");
    debounce
        .retain(|_, instant| now.duration_since(*instant) < DEBOUNCE_DURATION.saturating_mul(2));
    if skip_debounce {
        for p in paths {
            debounce_upsert(&mut debounce, p, now);
        }
        return true;
    }
    let suppressed = paths.iter().any(|p| {
        debounce
            .get(*p)
            .is_some_and(|last| now.duration_since(*last) < DEBOUNCE_DURATION)
    });
    if !suppressed {
        for p in paths {
            debounce_upsert(&mut debounce, p, now);
        }
    }
    !suppressed
}

fn debounce_upsert(map: &mut HashMap<PathBuf, Instant>, path: &Path, instant: Instant) {
    if let Some(ts) = map.get_mut(path) {
        *ts = instant;
    } else {
        map.insert(path.to_path_buf(), instant);
    }
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
) -> Vec<WatchEvent> {
    match &event.kind {
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)) => {
            if let Some(path) = event.paths.into_iter().next() {
                let mut pending = lock_pending(pending_from);
                if let Some(stale) = pending.take() {
                    debug_log!(
                        "orphan rename From: emitting Deleted for stale path {}",
                        stale.display(),
                    );
                    return vec![WatchEvent::Deleted(stale)];
                }
                *pending = Some(path);
            }
            Vec::new()
        }
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)) => {
            let to_path = event.paths.into_iter().next();
            let from_path = lock_pending(pending_from).take();
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

// Named helper for readability — avoids repeating `.into_iter().map(..).collect()`
// at each of the 6 call sites in `convert_event`.

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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn watcher_can_watch_and_unwatch_directory() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx).expect("create watcher");
        let watched_path = tempdir.path().canonicalize().expect("canonicalize tempdir");

        watcher.watch(tempdir.path()).expect("watch tempdir");
        assert_eq!(watcher.watched_dirs(), vec![watched_path]);

        watcher.unwatch(tempdir.path()).expect("unwatch tempdir");
        assert!(watcher.watched_dirs().is_empty());
    }

    #[test]
    fn watcher_unwatch_cleans_state_when_directory_vanished() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let watched_path = tempdir.path().to_path_buf();
        let canonical = watched_path.canonicalize().expect("canonicalize tempdir");
        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx).expect("create watcher");

        watcher.watch(&watched_path).expect("watch tempdir");
        std::fs::remove_dir_all(&watched_path).expect("remove watched dir");

        watcher.unwatch(&canonical).expect("unwatch vanished dir");

        assert!(watcher.watched_dirs().is_empty());
        assert!(watcher.watchers.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn watcher_unwatch_cleans_state_when_symlink_target_vanished() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let target = tempdir.path().join("target");
        let link = tempdir.path().join("link");
        std::fs::create_dir(&target).expect("create target dir");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let (event_tx, _event_rx) = mpsc::channel();
        let mut watcher = Watcher::new(event_tx).expect("create watcher");
        watcher.watch(&link).expect("watch symlinked dir");
        std::fs::remove_dir_all(&target).expect("remove target dir");

        watcher.unwatch(&link).expect("unwatch vanished target");

        assert!(watcher.watched_dirs().is_empty());
        assert!(watcher.watchers.is_empty());
    }

    #[test]
    fn watcher_pause_and_resume_do_not_panic() {
        let (event_tx, _event_rx) = mpsc::channel();
        let watcher = Watcher::new(event_tx).expect("create watcher");

        watcher.pause();
        watcher.resume();
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
    fn watcher_created_with_primary_only_no_fallback() {
        let (event_tx, _event_rx) = mpsc::channel();
        let watcher = Watcher::new(event_tx).expect("create watcher");
        assert!(watcher.fallback.is_none());
        assert!(watcher.watchers.is_empty());
    }
}
