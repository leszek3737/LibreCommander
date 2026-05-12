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
}

impl Watcher {
    pub fn new(event_tx: Sender<WatchEvent>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let debounce_state = Arc::new(Mutex::new(HashMap::new()));
        let primary = RecommendedWatcher::new(
            make_handler(
                event_tx.clone(),
                Arc::clone(&paused),
                Arc::clone(&debounce_state),
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
        })
    }

    fn create_fallback(&mut self) -> io::Result<&mut PollWatcher> {
        if self.fallback.is_none() {
            let fallback = PollWatcher::new(
                make_handler(
                    self.event_tx.clone(),
                    Arc::clone(&self.paused),
                    Arc::clone(&self.debounce_state),
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
        let path = path.canonicalize().map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("cannot canonicalize {}: {e}", path.display()),
            )
        })?;

        if self.watchers.contains_key(&path) {
            return Ok(());
        }

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
        let path = match path.canonicalize() {
            Ok(path) => path,
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
        };

        let result = match self.watchers.get(&path) {
            Some(WhichWatcher::Primary) => {
                self.primary.unwatch(&path).map_err(|e| notify_to_io(&e))
            }
            Some(WhichWatcher::Fallback) => self
                .fallback
                .as_mut()
                .ok_or_else(|| io::Error::other("fallback watcher not initialized"))?
                .unwatch(&path)
                .map_err(|e| notify_to_io(&e)),
            None => Ok(()),
        };

        if result.is_ok() {
            self.watchers.remove(&path);
        }
        result
    }

    fn remove_watched_dir_state(&mut self, path: &Path) {
        let removed = self
            .watchers
            .keys()
            .find(|watched| {
                watched.as_path() == path || path_points_to_missing_watch(path, watched)
            })
            .cloned();

        if let Some(path) = removed {
            self.watchers.remove(&path);
        }
    }

    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.watchers.keys().cloned().collect()
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
}

#[allow(clippy::print_stderr)]
fn make_handler(
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
    debounce_state: Arc<Mutex<HashMap<PathBuf, Instant>>>,
) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    let pending_from: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    move |result| {
        if paused.load(Ordering::Relaxed) {
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
                    if should_emit(&debounce_state, &[from.as_path(), to.as_path()]) {
                        let _ = event_tx.send(watch_event);
                    }
                    continue;
                }
            };

            if let Some(path) = path
                && !should_emit(&debounce_state, &[path.as_path()])
            {
                continue;
            }

            let _ = event_tx.send(watch_event);
        }
    }
}

fn should_emit(debounce_state: &Mutex<HashMap<PathBuf, Instant>>, paths: &[&Path]) -> bool {
    let now = Instant::now();
    let mut debounce = debounce_state.lock().unwrap_or_else(|e| {
        debug_log!("watcher mutex poisoned, recovering: {e}");
        e.into_inner()
    });
    let suppressed = paths.iter().any(|p| {
        debounce
            .get(*p)
            .is_some_and(|last| now.duration_since(*last) < Duration::from_millis(300))
    });
    if !suppressed {
        for p in paths {
            debounce.insert(p.to_path_buf(), now);
        }
    }
    !suppressed
}

fn lock_pending(
    pending_from: &Mutex<Option<PathBuf>>,
) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
    pending_from.lock().unwrap_or_else(|e| {
        debug_log!("pending_from mutex poisoned, recovering: {e}");
        e.into_inner()
    })
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
