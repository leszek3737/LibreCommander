//! Tests for the background-load offload (P1.5 archive listing, P1.6 tree
//! build): the result-application transitions that the main loop invokes when a
//! `bg_load` completes, plus the request/loading-dialog setup.

use lc::app::dir_tree::{TreeBuildResult, TreeEntry};
use lc::app::types::{AppMode, AppState, DialogKind};
use lc::ops::archive::{ArchiveEntry, ArchiveError};
use std::path::PathBuf;

use crate::input::menu_actions::apply_tree_build_result;
use crate::input::normal::apply_archive_list_result;

fn entry(name: &str, is_dir: bool) -> ArchiveEntry {
    ArchiveEntry {
        name: name.into(),
        size: 10,
        compressed_size: 5,
        modified: None,
        is_dir,
        method: "store".into(),
    }
}

#[test]
fn archive_list_ok_opens_extract_dialog() {
    // Simulate the loading dialog the request installs.
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::progress("Listing archive...".into(), 0.0, true)),
        ..Default::default()
    };

    let entries = vec![entry("a.txt", false), entry("sub/", true)];
    apply_archive_list_result(
        &mut state,
        PathBuf::from("/tmp/x.zip"),
        "/dest".to_string(),
        Ok(entries.clone()),
    );

    match &state.mode {
        AppMode::Dialog(DialogKind::ArchiveExtract(details)) => {
            assert_eq!(details.source, PathBuf::from("/tmp/x.zip"));
            assert_eq!(details.entries, entries);
            assert_eq!(details.dest_input.text(), "/dest");
        }
        other => panic!("expected ArchiveExtract dialog, got {other:?}"),
    }
}

#[test]
fn archive_list_err_reports_and_returns_to_normal() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::progress("Listing archive...".into(), 0.0, true)),
        ..Default::default()
    };

    apply_archive_list_result(
        &mut state,
        PathBuf::from("/tmp/broken.zip"),
        "/dest".to_string(),
        Err(ArchiveError::UnsupportedFormat),
    );

    assert_eq!(state.mode, AppMode::Normal);
    assert!(
        state
            .ui
            .status_message
            .as_deref()
            .is_some_and(|m| m.contains("Failed to list archive"))
    );
}

#[test]
fn tree_build_result_enters_directory_tree() {
    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::progress("Building tree...".into(), 0.0, true)),
        ..Default::default()
    };

    let tree = TreeBuildResult {
        entries: vec![TreeEntry {
            path: PathBuf::from("/root/child"),
            depth: 1,
            is_dir: true,
            expanded: false,
            name: "child".to_string(),
            name_width: 5,
            read_error: false,
        }],
        diagnostics: Vec::new(),
    };
    apply_tree_build_result(&mut state, PathBuf::from("/root"), tree);

    assert_eq!(state.mode, AppMode::DirectoryTree);
    assert_eq!(state.tree.root, PathBuf::from("/root"));
    assert_eq!(state.tree.entries.len(), 1);
    assert_eq!(state.tree.selected, 0);
}

/// End-to-end: a real zip listed on a background `BgLoad` (as the main loop
/// does), polled to completion, then applied — exercising the actual
/// off-event-thread path rather than a synthetic result.
#[test]
fn archive_listing_runs_off_thread_and_opens_dialog() {
    use lc::app::bg_load::BgLoad;
    use lc::ops::archive::list::list_archive;
    use std::io::Write as _;

    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("sample.zip");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"hello world").unwrap();
        writer.finish().unwrap();
    }

    let source = archive_path;
    let dest = dir.path().display().to_string();
    let load = BgLoad::spawn("test-archive-list", move |_cancel| {
        let result = list_archive(&source);
        (source, dest, result)
    })
    .unwrap();

    // Poll like the main loop until the worker publishes its result.
    let mut msg = None;
    for _ in 0..2000 {
        if let Ok(m) = load.try_recv() {
            msg = Some(m);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let (source, dest, result) = msg.expect("background archive listing did not complete");

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::progress("Listing archive...".into(), 0.0, true)),
        ..Default::default()
    };
    apply_archive_list_result(&mut state, source, dest, result);

    match &state.mode {
        AppMode::Dialog(DialogKind::ArchiveExtract(details)) => {
            assert!(details.entries.iter().any(|e| &*e.name == "hello.txt"));
        }
        other => panic!("expected ArchiveExtract dialog, got {other:?}"),
    }
}
