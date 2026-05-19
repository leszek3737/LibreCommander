use super::helpers::*;
use crate::input::pickers;
use crate::*;
use app::types::CompareMode;

#[test]
fn compare_directories_reports_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        app::types::FileEntry::builder()
            .name("a.txt")
            .path(tmp.path().join("a.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    ];
    state.right_panel.listing.entries = vec![
        app::types::FileEntry::builder()
            .name("b.txt")
            .path(tmp.path().join("b.txt"))
            .cha(crate::fs::cha::Cha::dummy_dir())
            .build(),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 1\nDiffering:       0"
            )
        ))
    );
}

#[test]
fn compare_directories_marks_unique_entries_selected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("same.txt")
            .path(tmp.path().join("same.txt"))
            .build(),
        TestEntry::new("left.txt")
            .path(tmp.path().join("left.txt"))
            .build(),
    ];
    state.right_panel.listing.entries = vec![
        TestEntry::new("same.txt")
            .path(tmp.path().join("same.txt"))
            .build(),
        TestEntry::new("right.txt")
            .path(tmp.path().join("right.txt"))
            .build(),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(!state.left_panel.listing.entries[0].selected);
    assert!(state.left_panel.listing.entries[1].selected);
    assert!(!state.right_panel.listing.entries[0].selected);
    assert!(state.right_panel.listing.entries[1].selected);
}

#[test]
fn compare_directories_size_mode_reports_mismatches() {
    let left_dir = tempfile::tempdir().unwrap();
    let right_dir = tempfile::tempdir().unwrap();
    std::fs::write(left_dir.path().join("file.txt"), "short").unwrap();
    std::fs::write(right_dir.path().join("file.txt"), "longer content here").unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(left_dir.path().to_path_buf());
    state.right_panel.set_path(right_dir.path().to_path_buf());
    state.left_panel.listing.entries = vec![
        TestEntry::new("file.txt")
            .path(left_dir.path().join("file.txt"))
            .size(5)
            .build(),
    ];
    state.right_panel.listing.entries = vec![
        TestEntry::new("file.txt")
            .path(right_dir.path().join("file.txt"))
            .size(20)
            .build(),
    ];
    pickers::compare_directories(&mut state, CompareMode::Size);
    let left_selected: Vec<_> = state
        .left_panel
        .listing
        .entries
        .iter()
        .filter(|e| e.name != ".." && e.selected)
        .collect();
    assert!(!left_selected.is_empty());
}

#[test]
fn compare_directories_by_content_zero_length() {
    let left_dir = tempfile::tempdir().unwrap();
    let right_dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.set_path(left_dir.path().to_path_buf());
    state.right_panel.set_path(right_dir.path().to_path_buf());
    pickers::compare_directories(&mut state, CompareMode::Quick);
}

#[test]
fn compare_mode_picker_maps_index_to_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
    ];

    let modes = ["Quick", "Size", "Thorough"];
    for (idx, label) in modes.iter().enumerate() {
        state.mode = AppMode::ListPicker(app::types::PickerKind::CompareMode);
        state.picker_selected = idx;
        pickers::handle_list_picker(&mut state, KeyCode::Enter);

        match &state.mode {
            AppMode::Dialog(app::types::DialogKind::Confirm(details)) => {
                let expected = format!("Compare dirs ({label}):");
                assert!(
                    details.message.contains(&expected),
                    "mode {label}: expected '{expected}' in '{}'",
                    details.message
                );
            }
            other => panic!("expected Confirm dialog for {label}, got {other:?}"),
        }
    }
}

#[test]
fn compare_mode_picker_esc_cancels() {
    let mut state = AppState {
        mode: AppMode::ListPicker(app::types::PickerKind::CompareMode),
        picker_selected: 1,
        ..Default::default()
    };

    pickers::handle_list_picker(&mut state, KeyCode::Esc);

    assert_eq!(state.mode, AppMode::Normal);
}

#[test]
fn compare_mode_picker_enter_runs_quick_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("a.txt")
            .path(tmp.path().join("a.txt"))
            .build(),
    ];
    state.mode = AppMode::ListPicker(app::types::PickerKind::CompareMode);
    state.picker_selected = 0;

    pickers::handle_list_picker(&mut state, KeyCode::Enter);

    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
            )
        ))
    );
}

#[test]
fn compare_mode_picker_navigate_and_select_thorough() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState {
        mode: AppMode::ListPicker(app::types::PickerKind::CompareMode),
        picker_selected: 0,
        ..Default::default()
    };
    state.left_panel.listing.entries = vec![
        TestEntry::new("x.txt")
            .size(42)
            .path(tmp.path().join("x.txt"))
            .build(),
    ];

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 1);

    pickers::handle_list_picker(&mut state, KeyCode::Down);
    assert_eq!(state.picker_selected, 2);

    pickers::handle_list_picker(&mut state, KeyCode::Enter);
    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Thorough):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
            )
        ))
    );
}
