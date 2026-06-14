use crate::input::dialogs;
use crossterm::event::KeyCode;
use lc::app::types::{
    ActivePanel, AppMode, AppState, ArchiveExtractDetails, DialogKind, InputState, PendingAction,
    TextInput, TransferAction, UiState,
};
use lc::ops::archive::ArchiveFormat;
use ratatui::layout::Size;
use std::path::{Path, PathBuf};

fn setup_src_dest(src: &Path, dest: &Path, files: &[&str]) {
    std::fs::create_dir_all(src).unwrap();
    std::fs::create_dir_all(dest).unwrap();
    for name in files {
        std::fs::write(src.join(name), b"x").unwrap();
    }
}

fn setup_dest_files(dest: &Path, files: &[&str]) {
    std::fs::create_dir_all(dest).unwrap();
    for name in files {
        std::fs::write(dest.join(name), b"x").unwrap();
    }
}

#[test]
fn check_overwrite_no_conflicts_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["new.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("new.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_one_conflict_returns_some() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["clash.txt"]);
    setup_dest_files(&dest, &["clash.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("clash.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["clash.txt"]);
}

#[test]
fn check_overwrite_all_conflicts_returns_all_names() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["a.txt", "b.txt"]);
    setup_dest_files(&dest, &["a.txt", "b.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("a.txt"), src.join("b.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts.iter().any(|s| s == "a.txt"));
    assert!(conflicts.iter().any(|s| s == "b.txt"));
}

#[test]
fn check_overwrite_source_equals_dest_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("same.txt");
    std::fs::write(&file, b"data").unwrap();

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![file],
                dest: tmp.path().to_path_buf(),
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[cfg(unix)]
#[test]
fn check_overwrite_broken_symlink_at_dest_is_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["link.txt"]);

    std::os::unix::fs::symlink("/nonexistent/broken", dest.join("link.txt")).unwrap();

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("link.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["link.txt"]);
}

#[test]
fn check_overwrite_move_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["file.txt"]);
    setup_dest_files(&dest, &["file.txt"]);
    let state = AppState {
        active_panel: ActivePanel::Left,
        ui: UiState {
            pending_action: Some(PendingAction::Move(TransferAction {
                sources: vec![src.join("file.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert_eq!(conflicts, Some(vec!["file.txt".into()]));
}

#[test]
fn check_overwrite_move_same_file_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    setup_src_dest(&src, &src, &["file.txt"]);
    let file = src.join("file.txt");
    let state = AppState {
        active_panel: ActivePanel::Left,
        ui: UiState {
            pending_action: Some(PendingAction::Move(TransferAction {
                sources: vec![file],
                dest: src,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

#[test]
fn check_overwrite_move_overwrite_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["file.txt"]);
    setup_dest_files(&dest, &["file.txt"]);
    let state = AppState {
        active_panel: ActivePanel::Left,
        ui: UiState {
            pending_action: Some(PendingAction::Move(TransferAction {
                sources: vec![src.join("file.txt")],
                dest,
                overwrite: true,
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

#[test]
fn check_overwrite_delete_no_conflict() {
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Delete {
                paths: vec![PathBuf::from("/tmp/nonexistent")],
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state);
    assert!(conflicts.is_none());
}

fn create_test_tar_gz(dir: &Path, entries: &[&str]) -> PathBuf {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let archive_path = dir.join("archive.tar.gz");
    let file = std::fs::File::create(&archive_path).unwrap();
    let enc = GzEncoder::new(file, Compression::fast());
    let mut builder = tar::Builder::new(enc);
    for name in entries {
        let data = b"test content";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, &data[..]).unwrap();
    }
    builder.finish().unwrap();
    archive_path
}

#[test]
fn check_overwrite_extract_archive_no_conflict_empty_dest() {
    let tmp = tempfile::tempdir().unwrap();
    let src = create_test_tar_gz(tmp.path(), &["file1.txt", "file2.txt"]);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::ExtractArchive {
                source: src,
                dest,
                overwrite: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_extract_archive_conflict_with_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let src = create_test_tar_gz(tmp.path(), &["file1.txt", "file2.txt"]);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("file1.txt"), b"existing").unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::ExtractArchive {
                source: src,
                dest,
                overwrite: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["file1.txt"]);
}

#[test]
fn check_overwrite_extract_archive_overwrite_true_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = create_test_tar_gz(tmp.path(), &["file1.txt"]);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("file1.txt"), b"existing").unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::ExtractArchive {
                source: src,
                dest,
                overwrite: true,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_create_archive_dest_exists_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("out.tar.gz");
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, b"data").unwrap();
    std::fs::write(&dest, b"existing").unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::CreateArchive {
                sources: vec![file],
                dest,
                format: ArchiveFormat::TarGz,
                overwrite: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["out.tar.gz"]);
}

#[test]
fn check_overwrite_create_archive_dest_not_exists_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("out.tar.gz");
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, b"data").unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::CreateArchive {
                sources: vec![file],
                dest,
                format: ArchiveFormat::TarGz,
                overwrite: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_create_archive_overwrite_true_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("out.tar.gz");
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, b"data").unwrap();
    std::fs::write(&dest, b"existing").unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::CreateArchive {
                sources: vec![file],
                dest,
                format: ArchiveFormat::TarGz,
                overwrite: true,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_copy_directory_source_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("mydir");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::create_dir_all(dest.join("mydir")).unwrap();

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src_dir],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["mydir"]);
}

#[test]
fn check_overwrite_copy_directory_source_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("mydir");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src_dir],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[cfg(unix)]
#[test]
fn check_overwrite_copy_symlink_source_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let target = tmp.path().join("target.txt");
    std::fs::write(&target, b"target").unwrap();
    let link = src.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    std::fs::write(dest.join("link.txt"), b"existing").unwrap();

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![link],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["link.txt"]);
}

#[test]
fn check_overwrite_copy_empty_sources_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![],
                dest: dir.path().to_path_buf(),
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = dialogs::check_overwrite_conflict(&state);
    assert!(result.is_none());
}

#[test]
fn check_overwrite_move_empty_sources_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Move(TransferAction {
                sources: vec![],
                dest: dir.path().to_path_buf(),
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = dialogs::check_overwrite_conflict(&state);
    assert!(result.is_none());
}

#[test]
fn archive_extract_enter_with_conflict_shows_overwrite_dialog_without_starting_action() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = create_test_tar_gz(tmp.path(), &["file1.txt"]);
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("file1.txt"), b"existing").unwrap();

    let mut dest_input = TextInput::new();
    dest_input.set_text_at_end(dest.display().to_string());

    let mut state = AppState {
        mode: AppMode::Dialog(DialogKind::ArchiveExtract(Box::new(
            ArchiveExtractDetails {
                source: archive,
                entries: vec![],
                dest_input,
            },
        ))),
        input: InputState {
            dialog_selection: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut running_job = None;

    {
        let mut viewer_state = None;
        let mut viewer_loader = None;
        let mut image_preview_loader = None;
        let mut ctx = crate::input::EventContext {
            state: &mut state,
            viewer_state: &mut viewer_state,
            viewer_loader: &mut viewer_loader,
            image_preview_loader: &mut image_preview_loader,
            running_job: &mut running_job,
            term_size: Size::new(80, 24),
        };
        dialogs::handle_dialog(&mut ctx, KeyCode::Enter);
    }

    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::OverwriteConfirm(..))
    ));
    assert!(running_job.is_none());
    assert!(matches!(
        state.ui.pending_action,
        Some(PendingAction::ExtractArchive {
            overwrite: false,
            ..
        })
    ));
}

#[test]
fn check_overwrite_pending_action_none_returns_none() {
    let state = AppState {
        ui: UiState {
            pending_action: None,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_copy_overwrite_true_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src, &dest, &["file.txt"]);
    setup_dest_files(&dest, &["file.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("file.txt")],
                dest,
                overwrite: true,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}

#[test]
fn check_overwrite_copy_duplicate_sources_different_dirs_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let src_a = tmp.path().join("a");
    let src_b = tmp.path().join("b");
    let dest = tmp.path().join("dest");
    setup_src_dest(&src_a, &dest, &["same.txt"]);
    setup_src_dest(&src_b, &dest, &["same.txt"]);
    setup_dest_files(&dest, &["same.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src_a.join("same.txt"), src_b.join("same.txt")],
                dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    let conflicts = dialogs::check_overwrite_conflict(&state).unwrap();
    assert_eq!(conflicts, vec!["same.txt", "same.txt"]);
}

#[test]
fn check_overwrite_copy_nonexistent_dest_no_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let nonexistent_dest = tmp.path().join("nonexistent");
    setup_src_dest(&src, &nonexistent_dest, &["file.txt"]);

    let state = AppState {
        ui: UiState {
            pending_action: Some(PendingAction::Copy(TransferAction {
                sources: vec![src.join("file.txt")],
                dest: nonexistent_dest,
                overwrite: false,
            })),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(dialogs::check_overwrite_conflict(&state).is_none());
}
