use crate::debug_log;
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify::{Watcher as NotifyWatcher, event::RenameMode};
use std::collections::{HashMap, HashSet};
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
    watched_dirs: HashSet<PathBuf>,
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
            watched_dirs: HashSet::new(),
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

        if self.watched_dirs.contains(&path) {
            return Ok(());
        }

        match self.primary.watch(&path, RecursiveMode::NonRecursive) {
            Ok(()) => {
                self.watched_dirs.insert(path.clone());
                self.watchers.insert(path, WhichWatcher::Primary);
                Ok(())
            }
            Err(primary_err) => {
                let fallback = self.create_fallback()?;
                match fallback.watch(&path, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        self.watched_dirs.insert(path.clone());
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
        let path = path.canonicalize().map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("cannot canonicalize {}: {e}", path.display()),
            )
        })?;

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

        self.watched_dirs.remove(&path);
        self.watchers.remove(&path);
        result
    }

    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.watched_dirs.iter().cloned().collect()
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
    move |result| {
        if paused.load(Ordering::Relaxed) {
            return;
        }

        let Ok(event) = result else {
            return;
        };

        for watch_event in convert_event(event) {
            let path = match &watch_event {
                WatchEvent::Created(p) | WatchEvent::Deleted(p) | WatchEvent::Modified(p) => {
                    Some(p.clone())
                }
                WatchEvent::Renamed { from, to } => {
                    // For renames, debounce both paths independently
                    let now = Instant::now();
                    let mut debounce = debounce_state.lock().unwrap_or_else(|e| {
                        debug_log!("watcher mutex poisoned (rename debounce), recovering: {e}");
                        e.into_inner()
                    });
                    let from_allowed = debounce
                        .get(from)
                        .is_none_or(|last| now.duration_since(*last) >= Duration::from_millis(300));
                    let to_allowed = debounce
                        .get(to)
                        .is_none_or(|last| now.duration_since(*last) >= Duration::from_millis(300));
                    if from_allowed && to_allowed {
                        debounce.insert(from.clone(), now);
                        debounce.insert(to.clone(), now);
                        drop(debounce);
                        let _ = event_tx.send(watch_event);
                    }
                    continue;
                }
            };

            if let Some(path) = path {
                let now = Instant::now();
                let mut debounce = debounce_state.lock().unwrap_or_else(|e| {
                    debug_log!("watcher mutex poisoned (debounce), recovering: {e}");
                    e.into_inner()
                });
                if let Some(last) = debounce.get(&path) {
                    if now.duration_since(*last) < Duration::from_millis(300) {
                        continue;
                    }
                }
                debounce.insert(path, now);
                drop(debounce);
            }

            let _ = event_tx.send(watch_event);
        }
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
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)) => {
            map_paths(event.paths, WatchEvent::Deleted)
        }
        EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)) => {
            map_paths(event.paths, WatchEvent::Created)
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

#[cfg(test)]
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
        assert_eq!(watcher.watched_dirs(), vec![watched_path.clone()]);

        watcher.unwatch(tempdir.path()).expect("unwatch tempdir");
        assert!(watcher.watched_dirs().is_empty());
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

        assert!(
            matches!(convert_event(from).as_slice(), [WatchEvent::Deleted(path)] if path == &PathBuf::from("old"))
        );
        assert!(
            matches!(convert_event(to).as_slice(), [WatchEvent::Created(path)] if path == &PathBuf::from("new"))
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
