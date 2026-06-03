use super::helpers::*;
use crate::input::pickers;
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{AppMode, AppState, CompareMode, DialogKind};

#[test]
fn compare_directories_reports_summary() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("a.txt").build()];
    state.right_panel.listing.entries = vec![TestEntry::new("b.txt").build()];

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
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![
        TestEntry::new("same.txt").build(),
        TestEntry::new("left.txt").build(),
    ];
    state.right_panel.listing.entries = vec![
        TestEntry::new("same.txt").build(),
        TestEntry::new("right.txt").build(),
    ];

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(!state.left_panel.listing.entries[0].selected);
    assert!(state.left_panel.listing.entries[1].selected);
    assert!(!state.right_panel.listing.entries[0].selected);
    assert!(state.right_panel.listing.entries[1].selected);
}

#[test]
fn compare_directories_size_mode_reports_mismatches() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("file.txt").size(5).build()];
    state.right_panel.listing.entries = vec![TestEntry::new("file.txt").size(20).build()];
    pickers::compare_directories(&mut state, CompareMode::Size);
    let left_selected: Vec<_> = state
        .left_panel
        .listing
        .entries
        .iter()
        .filter(|e| e.name != ".." && e.selected)
        .collect();
    assert!(!left_selected.is_empty());
    assert!(left_selected.iter().any(|e| e.name == "file.txt"));
}

#[test]
fn compare_directories_quick_empty_dirs() {
    let mut state = AppState::default();
    pickers::compare_directories(&mut state, CompareMode::Quick);
    assert!(matches!(
        state.mode,
        AppMode::Dialog(DialogKind::Confirm(_))
    ));
    assert_eq!(state.dialog_selection, 0);
}

#[test]
fn compare_mode_picker_maps_index_to_mode() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![TestEntry::new("a.txt").build()];

    for (idx, mode) in CompareMode::ALL.iter().enumerate() {
        state.mode = AppMode::ListPicker(app::types::PickerKind::CompareMode);
        state.picker_selected = idx;
        pickers::handle_list_picker(&mut state, KeyCode::Enter);

        let label = mode.label();
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
