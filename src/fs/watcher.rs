use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify::{Watcher as NotifyWatcher, event::RenameMode};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

pub enum WatchEvent {
    Created(PathBuf),
    Deleted(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

pub struct Watcher {
    primary: RecommendedWatcher,
    fallback: PollWatcher,
    watched_dirs: HashSet<PathBuf>,
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
}

impl Watcher {
    pub fn new(event_tx: Sender<WatchEvent>) -> io::Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));
        let primary = RecommendedWatcher::new(
            make_handler(event_tx.clone(), Arc::clone(&paused)),
            Config::default(),
        )
        .map_err(notify_to_io)?;
        let fallback = PollWatcher::new(
            make_handler(event_tx.clone(), Arc::clone(&paused)),
            Config::default(),
        )
        .map_err(notify_to_io)?;

        Ok(Self {
            primary,
            fallback,
            watched_dirs: HashSet::new(),
            event_tx,
            paused,
        })
    }

    pub fn watch(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if self.watched_dirs.contains(&path) {
            return Ok(());
        }

        match self.primary.watch(&path, RecursiveMode::NonRecursive) {
            Ok(()) => {
                self.watched_dirs.insert(path);
                Ok(())
            }
            Err(primary_err) => match self.fallback.watch(&path, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watched_dirs.insert(path);
                    Ok(())
                }
                Err(fallback_err) => Err(io::Error::other(format!(
                    "primary watcher failed: {primary_err}; fallback watcher failed: {fallback_err}"
                ))),
            },
        }
    }

    pub fn unwatch(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let primary_result = self.primary.unwatch(&path).map_err(notify_to_io);
        let fallback_result = self.fallback.unwatch(&path).map_err(notify_to_io);
        self.watched_dirs.remove(&path);

        primary_result.or(fallback_result).or(Ok(()))
    }

    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.watched_dirs.iter().cloned().collect()
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn sender(&self) -> &Sender<WatchEvent> {
        &self.event_tx
    }
}

fn make_handler(
    event_tx: Sender<WatchEvent>,
    paused: Arc<AtomicBool>,
) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |result| {
        if paused.load(Ordering::SeqCst) {
            return;
        }

        let Ok(event) = result else {
            return;
        };

        for watch_event in convert_event(event) {
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

fn notify_to_io(err: notify::Error) -> io::Error {
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
}
