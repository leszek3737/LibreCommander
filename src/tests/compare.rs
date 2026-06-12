use super::helpers::*;
use crate::input::pickers;
use crossterm::event::KeyCode;
use lc::app;
use lc::app::types::{AppMode, AppState, CompareMode, DialogKind};

fn state_with_panels(
    left: Vec<crate::app::types::FileEntry>,
    right: Vec<crate::app::types::FileEntry>,
) -> AppState {
    let mut state = AppState::default();
    state.left_panel.listing.entries = left;
    state.right_panel.listing.entries = right;
    state
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

fn extract_confirm_details(state: &AppState) -> &app::types::ConfirmDetails {
    match &state.mode {
        AppMode::Dialog(DialogKind::Confirm(d)) => d,
        other => panic!("expected Confirm dialog, got {other:?}"),
    }
}

#[test]
fn compare_directories_reports_summary() {
    let mut state = state_with_panels(vec![entry("a.txt").build()], vec![entry("b.txt").build()]);

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert_eq!(
        state.mode,
        AppMode::Dialog(app::types::DialogKind::Confirm(
            app::types::ConfirmDetails::simple(
                "Compare Results",
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 1\nDiffering:       0"
            )
        )),
        "summary dialog should report 1 unique per side, 0 differing"
    );
    assert!(
        state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "a.txt" && e.selected),
        "left panel should mark 'a.txt' as selected after compare"
    );
    assert!(
        state
            .right_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "b.txt" && e.selected),
        "right panel should mark 'b.txt' as selected after compare"
    );
}

#[test]
fn compare_directories_marks_unique_entries_selected() {
    let mut state = state_with_panels(
        vec![entry("same.txt").build(), entry("left.txt").build()],
        vec![entry("same.txt").build(), entry("right.txt").build()],
    );

    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(
        !state.left_panel.listing.entries[0].selected,
        "'same.txt' on left should not be selected"
    );
    assert!(
        state.left_panel.listing.entries[1].selected,
        "'left.txt' on left should be selected"
    );
    assert!(
        !state.right_panel.listing.entries[0].selected,
        "'same.txt' on right should not be selected"
    );
    assert!(
        state.right_panel.listing.entries[1].selected,
        "'right.txt' on right should be selected"
    );
}

#[test]
fn compare_directories_size_mode_reports_mismatches() {
    let mut state = state_with_panels(
        vec![entry("file.txt").file(5).build()],
        vec![entry("file.txt").file(20).build()],
    );

    pickers::compare_directories(&mut state, CompareMode::Size);

    assert!(
        state
            .left_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "file.txt" && e.selected),
        "left panel 'file.txt' should be selected (size mismatch)"
    );
    assert!(
        state
            .right_panel
            .listing
            .entries
            .iter()
            .any(|e| e.name == "file.txt" && e.selected),
        "right panel 'file.txt' should be selected (size mismatch)"
    );
}

#[test]
fn compare_directories_quick_empty_dirs() {
    let mut state = AppState::default();
    pickers::compare_directories(&mut state, CompareMode::Quick);

    assert!(
        matches!(state.mode, AppMode::Dialog(DialogKind::Confirm(_))),
        "empty dirs should produce a confirm dialog"
    );
    let details = extract_confirm_details(&state);
    assert_eq!(
        details.title, "Compare Results",
        "dialog title should be 'Compare Results'"
    );
    assert!(
        details.message.contains("Unique in left:  0"),
        "empty dirs should report 0 unique on left"
    );
    assert!(
        details.message.contains("Unique in right: 0"),
        "empty dirs should report 0 unique on right"
    );
    assert!(
        details.message.contains("Differing:       0"),
        "empty dirs should report 0 differing"
    );
    assert_eq!(
        state.dialog_selection, 0,
        "dialog_selection should default to 0"
    );
}

#[test]
fn compare_mode_picker_maps_index_to_mode() {
    let mut state = AppState::default();
    state.left_panel.listing.entries = vec![entry("a.txt").build()];

    for (idx, mode) in CompareMode::ALL.iter().enumerate() {
        // Reset to picker mode for each iteration — simulates fresh picker invocation
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

#[test]
fn compare_directories_identical_content_mixed_types_symlinks() {
    let mut state = state_with_panels(
        vec![
            entry("file.txt").file(100).build(),
            entry("subdir").build(),
            entry("link.txt").file(100).symlink().build(),
        ],
        vec![
            entry("file.txt").file(100).build(),
            entry("subdir").build(),
            entry("link.txt").file(100).symlink().build(),
        ],
    );

    pickers::compare_directories(&mut state, CompareMode::Quick);

    let details = extract_confirm_details(&state);
    assert!(
        details.message.contains("Unique in left:  0"),
        "identical panels should report 0 unique left"
    );
    assert!(
        details.message.contains("Unique in right: 0"),
        "identical panels should report 0 unique right"
    );
    assert!(
        details.message.contains("Differing:       0"),
        "identical panels should report 0 differing"
    );
    assert!(
        state.left_panel.listing.entries.iter().all(|e| !e.selected),
        "no left entries should be selected when panels are identical"
    );
    assert!(
        state
            .right_panel
            .listing
            .entries
            .iter()
            .all(|e| !e.selected),
        "no right entries should be selected when panels are identical"
    );
}
