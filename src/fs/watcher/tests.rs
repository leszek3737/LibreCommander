use super::*;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::mpsc;

type TestResult = Result<(), Box<dyn Error>>;

fn expired_instant() -> Instant {
    Instant::now() - DEBOUNCE_DURATION - Duration::from_millis(1)
}

fn full_channel() -> (mpsc::SyncSender<WatchEvent>, mpsc::Receiver<WatchEvent>) {
    mpsc::sync_channel(0)
}

fn insert_expired_modified(watcher: &Watcher, path: &Path) {
    let mut debounce = watcher.debounce_state.lock().unwrap();
    debounce.insert(
        path.to_path_buf(),
        PendingEntry {
            last_seen: expired_instant(),
            coalesced: Some(WatchEvent::Modified(path.to_path_buf())),
        },
    );
}

fn watch_unwatch_lifecycle(remove_before_unwatch: bool) -> TestResult {
    let tempdir = tempfile::tempdir()?;
    let (event_tx, _event_rx) = mpsc::sync_channel(2048);
    let mut watcher = Watcher::new(Arc::new(event_tx))?;
    let watched_path = tempdir.path().to_path_buf();
    let canonical = watched_path.canonicalize()?;

    watcher.watch(&watched_path)?;

    if remove_before_unwatch {
        std::fs::remove_dir_all(&watched_path)?;
    } else {
        assert_eq!(watcher.watched_dirs(), vec![canonical.clone()]);
    }

    watcher.unwatch(&canonical)?;

    assert!(watcher.watched_dirs().is_empty());
    if remove_before_unwatch {
        assert!(watcher.watchers.is_empty());
    }
    Ok(())
}

#[test]
fn watcher_can_watch_and_unwatch_directory() -> TestResult {
    watch_unwatch_lifecycle(false)
}

#[test]
fn watcher_unwatch_cleans_state_when_directory_vanished() -> TestResult {
    watch_unwatch_lifecycle(true)
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
    let pending: Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>> = Mutex::new(HashMap::new());
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
    insert_expired_modified(&watcher, &path);

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
    insert_expired_modified(&watcher, &path);

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
    let (tx, rx) = full_channel();
    let watcher = Watcher::new(Arc::new(tx))?;

    let first = PathBuf::from("/tmp/first.txt");
    let expired = expired_instant();
    {
        let mut debounce = watcher.debounce_state.lock().unwrap();
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
    let debounce = watcher.debounce_state.lock().unwrap();
    let entry = debounce.get(&PathBuf::from("/tmp/first.txt")).unwrap();
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
        let mut debounce = watcher.debounce_state.lock().unwrap();
        debounce.insert(
            path.clone(),
            PendingEntry {
                last_seen: expired_instant(),
                coalesced: Some(WatchEvent::Modified(path.clone())),
            },
        );
    }

    watcher.flush_pending();
    let filler = rx.try_recv()?;
    assert!(matches!(filler, WatchEvent::Modified(p) if p.as_path() == Path::new("/tmp/fill.txt")));

    watcher.flush_pending();
    assert!(rx.try_recv().is_err());

    {
        let mut debounce = watcher.debounce_state.lock().unwrap();
        let entry = debounce.get_mut(&path).unwrap();
        entry.last_seen = expired_instant();
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
        let mut pending = watcher.pending_from.lock().unwrap();
        pending.insert(
            parent_key.clone(),
            VecDeque::from([PendingFromEntry {
                path: stale.clone(),
                time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
            }]),
        );
    }

    watcher.flush_pending();

    let pending = watcher.pending_from.lock().unwrap();
    assert_eq!(
        pending
            .get(&parent_key)
            .and_then(|v| v.front())
            .map(|e| e.path.clone()),
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

    let map = debounce_state.lock().unwrap();
    let entry = map.get(&path).unwrap();
    assert!(entry.coalesced.is_some());
}

#[test]
fn process_debounce_with_skip_debounce_true_never_suppresses() {
    let debounce_state: Mutex<HashMap<PathBuf, PendingEntry>> = Mutex::new(HashMap::new());
    let path = PathBuf::from("/tmp/skip.txt");
    let event = WatchEvent::Modified(path.clone());

    // First call with no prior entry: emits immediately, nothing to flush.
    let (emit1, flushed1) =
        process_debounce(&debounce_state, &[path.as_path()], Some(&event), true);
    assert!(emit1);
    assert!(flushed1.is_empty());

    // Seed a coalesced entry as if a prior debounced event had been suppressed.
    {
        let mut map = debounce_state.lock().unwrap();
        map.get_mut(&path).unwrap().coalesced = Some(WatchEvent::Modified(path.clone()));
    }

    // Second call within the debounce window: skip_debounce still emits and
    // flushes the prior coalesced event instead of suppressing it.
    let (emit2, flushed2) =
        process_debounce(&debounce_state, &[path.as_path()], Some(&event), true);
    assert!(emit2, "skip_debounce=true should never suppress");
    assert_eq!(flushed2.len(), 1);
    assert!(matches!(&flushed2[0].event, WatchEvent::Modified(p) if p == &path));

    let map = debounce_state.lock().unwrap();
    let entry = map.get(&path).unwrap();
    assert!(
        entry.coalesced.is_none(),
        "skip_debounce=true should leave no coalesced event"
    );
}

#[test]
// Known limitation: FIFO pairing lacks semantic matching. Pairing depends on
// emission order from notify, which may cause incorrect pairs with concurrent renames.
fn multiple_from_same_dir_buffered_and_paired_fifo() {
    let pending: Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>> = Mutex::new(HashMap::new());

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
    let to_b = notify::Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
        paths: vec![PathBuf::from("new_b")],
        attrs: Default::default(),
    };
    let to_a = notify::Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::To)),
        paths: vec![PathBuf::from("new_a")],
        attrs: Default::default(),
    };

    let ev1 = convert_event_with_rename_pairing(from1, &pending);
    assert!(ev1.is_empty());

    let ev2 = convert_event_with_rename_pairing(from2, &pending);
    assert!(
        ev2.is_empty(),
        "second From in same dir should not emit Deleted"
    );

    let ev3 = convert_event_with_rename_pairing(to_b, &pending);
    assert!(
        matches!(ev3.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old_a") && to == &PathBuf::from("new_b")),
        "first To pairs with first From (FIFO)"
    );

    let ev4 = convert_event_with_rename_pairing(to_a, &pending);
    assert!(
        matches!(ev4.as_slice(), [WatchEvent::Renamed { from, to }] if from == &PathBuf::from("old_b") && to == &PathBuf::from("new_a")),
        "second To pairs with second From (FIFO)"
    );
}

#[test]
fn flush_pending_emits_deleted_for_stale_from() -> TestResult {
    let (tx, rx) = mpsc::sync_channel(2048);
    let watcher = Watcher::new(Arc::new(tx))?;

    {
        let mut pending = watcher.pending_from.lock().unwrap();
        pending.insert(
            PathBuf::new(),
            VecDeque::from([PendingFromEntry {
                path: PathBuf::from("/tmp/stale_file.txt"),
                time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
            }]),
        );
    }

    watcher.flush_pending();

    let evt = rx.try_recv()?;
    assert!(
        matches!(evt, WatchEvent::Deleted(p) if p.as_path() == Path::new("/tmp/stale_file.txt"))
    );

    assert!(watcher.pending_from.lock().unwrap().is_empty());
    Ok(())
}

#[test]
fn clear_pending_from_keeps_new_from_for_same_path() {
    let parent_key = PathBuf::new();
    let pending: Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>> = Mutex::new(HashMap::new());
    let new_time = Instant::now();
    {
        let mut map = pending.lock().unwrap();
        map.insert(
            parent_key.clone(),
            VecDeque::from([PendingFromEntry {
                path: PathBuf::from("/tmp/rename.txt"),
                time: new_time,
            }]),
        );
    }

    let old_time = Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1);
    clear_pending_from_if_unchanged(
        &pending,
        &parent_key,
        Path::new("/tmp/rename.txt"),
        old_time,
    );

    let map = pending.lock().unwrap();
    assert!(map.contains_key(&parent_key));
}

#[test]
fn flush_pending_keeps_fresh_from() -> TestResult {
    let (tx, rx) = mpsc::sync_channel(2048);
    let watcher = Watcher::new(Arc::new(tx))?;

    {
        let mut pending = watcher.pending_from.lock().unwrap();
        pending.insert(
            PathBuf::new(),
            VecDeque::from([PendingFromEntry {
                path: PathBuf::from("/tmp/fresh_file.txt"),
                time: Instant::now(),
            }]),
        );
    }

    watcher.flush_pending();

    assert!(rx.try_recv().is_err());
    assert!(!watcher.pending_from.lock().unwrap().is_empty());
    Ok(())
}

#[test]
fn per_parent_rename_pairing_does_not_mismatch_across_dirs() {
    let pending: Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>> = Mutex::new(HashMap::new());

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
fn multiple_from_same_parent_both_buffered() {
    let pending: Mutex<HashMap<PathBuf, VecDeque<PendingFromEntry>>> = Mutex::new(HashMap::new());

    let dir_a = PathBuf::from("/dir_a");

    let from1 = notify::Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
        paths: vec![dir_a.join("first.txt")],
        attrs: Default::default(),
    };
    let from2 = notify::Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(RenameMode::From)),
        paths: vec![dir_a.join("second.txt")],
        attrs: Default::default(),
    };

    let events1 = convert_event_with_rename_pairing(from1, &pending);
    assert!(events1.is_empty());

    let events2 = convert_event_with_rename_pairing(from2, &pending);
    assert!(
        events2.is_empty(),
        "second From should be buffered, not emit Deleted"
    );

    let map = pending.lock().unwrap();
    let entries = map.get(&dir_a).expect("parent key should exist");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, dir_a.join("first.txt"));
    assert_eq!(entries[1].path, dir_a.join("second.txt"));
}

#[test]
fn pause_clears_all_state() -> TestResult {
    let (tx, rx) = mpsc::sync_channel(2048);
    let watcher = Watcher::new(Arc::new(tx))?;

    // Seed debounce_state with an expired entry.
    insert_expired_modified(&watcher, Path::new("/tmp/test_pause_clear.txt"));

    // Seed pending_from with a fresh entry.
    {
        let mut pending = watcher.pending_from.lock().unwrap();
        pending.insert(
            PathBuf::from("/some/dir"),
            VecDeque::from([PendingFromEntry {
                path: PathBuf::from("/some/dir/old.txt"),
                time: Instant::now(),
            }]),
        );
    }

    watcher.pause();

    assert!(
        watcher.debounce_state.lock().unwrap().is_empty(),
        "pause should clear debounce_state"
    );
    assert!(
        watcher.pending_from.lock().unwrap().is_empty(),
        "pause() should clear all pending_from entries"
    );

    watcher.resume();
    watcher.flush_pending();
    assert!(rx.try_recv().is_err(), "no events after clearing state");
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

    let state = debounce.lock().unwrap();
    assert!(state.contains_key(&PathBuf::from("/tmp/test_overflow.txt")));
    Ok(())
}

#[test]
fn reinsert_or_overflow_sets_pending_flag_on_full_queue() -> TestResult {
    let (tx, rx) = full_channel();
    let debounce: Mutex<HashMap<PathBuf, PendingEntry>> = Mutex::new(HashMap::new());
    let overflow_pending = AtomicBool::new(false);

    let event = WatchEvent::Modified(PathBuf::from("/tmp/test_overflow_pending.txt"));
    reinsert_or_overflow(&tx, &debounce, &overflow_pending, event);

    assert!(
        overflow_pending.load(Ordering::Acquire),
        "overflow_pending should be true when Overflow send fails"
    );

    let state = debounce.lock().unwrap();
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
        let mut pending = watcher.pending_from.lock().unwrap();
        pending.insert(
            dir_a.clone(),
            VecDeque::from([PendingFromEntry {
                path: dir_a.join("old_a.txt"),
                time: Instant::now() - PENDING_FROM_TIMEOUT - Duration::from_millis(1),
            }]),
        );
        pending.insert(
            dir_b.clone(),
            VecDeque::from([PendingFromEntry {
                path: dir_b.join("fresh_b.txt"),
                time: Instant::now(),
            }]),
        );
    }

    watcher.flush_pending();

    let event = rx.try_recv()?;
    assert!(matches!(
        &event,
        WatchEvent::Deleted(p) if *p == dir_a.join("old_a.txt")
    ));

    assert!(rx.try_recv().is_err(), "dir_b should not time out yet");

    let pending = watcher.pending_from.lock().unwrap();
    assert!(pending.contains_key(&dir_b));
    assert!(!pending.contains_key(&dir_a));
    Ok(())
}
