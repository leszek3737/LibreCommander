use std::path::PathBuf;

use super::dialogs::{ConfirmDetails, CopyMoveDetails, CopyMoveKind, DialogKind, InputAction};
use super::file_entry::{FileCategory, FileEntry};
use super::modes::AppMode;
use super::panel::{ActivePanel, PanelState};
use super::sorting::{Direction, ListingMode, SortField, SortMode};
use super::test_helpers::TestEntry;
use super::text_input::TextInput;

use crate::app::types::app_state::AppState;

fn test_path(name: impl AsRef<std::path::Path>) -> PathBuf {
    PathBuf::from("/lc-test-fixtures").join(name)
}

fn entry(name: &str) -> TestEntry {
    TestEntry::new(name).path(test_path(name))
}

/// A panel at `/test` whose full (unfiltered) listing is `entries`. Centralizes
/// the `new()` + `set_entries()` setup repeated across the suite (the filtered
/// view becomes the full set, and selection stats are recalculated).
fn panel_from(entries: Vec<FileEntry>) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(entries);
    panel
}

fn panel_with_n_entries(n: u32) -> PanelState {
    let entries = (0..n)
        .map(|i| {
            TestEntry::new(format!("file{i}.txt"))
                .path(test_path(format!("file{i}.txt")))
                .file(100)
                .permissions(0o644)
                .build()
        })
        .collect();
    panel_from(entries)
}

fn panel_with_cursor(n: u32, cursor: usize, scroll_offset: usize) -> PanelState {
    let mut panel = panel_with_n_entries(n);
    panel.cursor = cursor;
    panel.scroll_offset = scroll_offset;
    panel
}

// Table-driven pattern (repeated in test_file_entry_display_permissions below)
#[test]
fn test_file_entry_display_size() {
    let cases: &[(u64, &str)] = &[
        (500, " 500 B"),
        (1500, "1.5 KB"),
        // 1_500_000 / 1_048_576 ≈ 1.43 → display_size truncates (not rounds) → "1.4 MB"
        (1_500_000, "1.4 MB"),
        (1_500_000_000, "1.4 GB"),
        (0, "   0 B"),
    ];
    for &(size, expected) in cases {
        let entry = entry("test.txt").file(size).permissions(0o644).build();
        assert_eq!(entry.display_size(), expected, "size={size}");
    }
}

#[test]
fn test_file_entry_display_permissions() {
    let cases: &[(u32, &str)] = &[
        (0o755, "rwxr-xr-x"),
        (0o644, "rw-r--r--"),
        (0o777, "rwxrwxrwx"),
        (0o000, "---------"),
    ];
    for &(perms, expected) in cases {
        let entry = entry("test.txt").file(100).permissions(perms).build();
        assert_eq!(entry.display_permissions(), expected, "perms=0o{perms:o}");
    }
}

#[test]
fn test_file_entry_display_modified() {
    let entry = entry("test.txt").file(100).permissions(0o644).build();
    let expected = chrono::DateTime::from_timestamp(1_000_000_000, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%d-%m-%y %H:%M")
        .to_string();
    assert_eq!(entry.display_modified(), expected.as_str());
}

#[test]
fn test_sort_mode_default() {
    assert_eq!(
        SortMode::default(),
        SortMode::new(SortField::Name, Direction::Asc)
    );
}

#[test]
fn test_panel_state_new() {
    let path = PathBuf::from("/test");
    let panel = PanelState::new(path.clone());
    assert_eq!(panel.path(), path);
    assert_eq!(panel.listing.filtered_len(), 0);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
    assert_eq!(panel.sort_mode(), SortMode::default());
    assert_eq!(panel.listing_mode(), ListingMode::Long);
    assert!(panel.show_hidden());
    assert!(panel.filter().is_none());
}

#[test]
fn test_panel_state_current_entry_none_when_empty() {
    let panel = PanelState::new(PathBuf::from("/test"));
    assert!(panel.current_entry().is_none());
}

#[test]
fn test_panel_state_current_entry_some() {
    let mut panel = panel_from(vec![
        entry("file1.txt").file(100).permissions(0o644).build(),
    ]);
    panel.cursor = 0;
    assert_eq!(
        panel.current_entry().expect("current entry exists").name,
        "file1.txt"
    );
}

#[test]
fn test_panel_state_current_entry_out_of_bounds() {
    let mut panel = panel_from(vec![
        entry("file1.txt").file(100).permissions(0o644).build(),
    ]);
    panel.cursor = 5;
    assert!(
        panel.current_entry().is_none(),
        "out-of-range cursor yields no current entry"
    );
}

#[test]
fn test_panel_state_toggle_selection_toggle_on() {
    let mut panel = panel_from(vec![
        entry("file1.txt").file(100).permissions(0o644).build(),
    ]);
    panel.cursor = 0;
    panel.toggle_selection();
    assert!(
        panel.current_entry().expect("entry under cursor").selected,
        "toggle selects the cursor entry"
    );
    assert_eq!(
        panel.selected_count(),
        1,
        "one entry selected after toggle on"
    );
    assert_eq!(
        panel.selected_size(),
        100,
        "selected size equals the toggled entry's size"
    );
}

#[test]
fn test_panel_state_toggle_selection_toggle_off() {
    let mut panel = panel_from(vec![
        entry("file1.txt")
            .file(100)
            .permissions(0o644)
            .selected()
            .build(),
    ]);
    panel.cursor = 0;
    assert!(
        panel.current_entry().expect("entry under cursor").selected,
        "entry starts selected"
    );
    panel.toggle_selection();
    assert!(
        !panel.current_entry().expect("entry under cursor").selected,
        "toggle deselects the cursor entry"
    );
    assert_eq!(
        panel.selected_count(),
        0,
        "no entries selected after toggle off"
    );
    assert_eq!(
        panel.selected_size(),
        0,
        "selected size cleared after toggle off"
    );
}

#[test]
fn test_panel_state_set_selection_at_on() {
    let mut panel = panel_from(vec![
        entry("file1.txt").file(100).permissions(0o644).build(),
    ]);

    panel.set_selection_at(0, true);

    assert!(
        panel.current_entry().expect("entry under cursor").selected,
        "set_selection_at(true) selects the entry"
    );
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 100);
}

#[test]
fn test_panel_state_set_selection_at_off() {
    let mut panel = panel_from(vec![
        entry("file1.txt")
            .file(100)
            .permissions(0o644)
            .selected()
            .build(),
    ]);

    panel.set_selection_at(0, false);

    assert!(
        !panel.current_entry().expect("entry under cursor").selected,
        "set_selection_at(false) deselects the entry"
    );
    assert_eq!(panel.selected_count(), 0);
    assert_eq!(panel.selected_size(), 0);
}

#[test]
fn test_panel_state_selected_entries() {
    let panel = panel_from(vec![
        entry("file1.txt")
            .file(100)
            .permissions(0o644)
            .selected()
            .build(),
        entry("file2.txt").file(200).permissions(0o644).build(),
        entry("file3.txt")
            .file(300)
            .permissions(0o644)
            .selected()
            .build(),
    ]);

    let selected: Vec<_> = panel.selected_entries().collect();
    assert_eq!(selected.len(), 2, "two entries are selected");
    assert_eq!(selected[0].name, "file1.txt");
    assert_eq!(selected[1].name, "file3.txt");
}

#[test]
fn test_panel_state_move_cursor_up() {
    let mut panel = panel_with_cursor(2, 1, 0);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_move_cursor_up_at_top() {
    let mut panel = panel_from(vec![
        entry("file1.txt").file(100).permissions(0o644).build(),
    ]);
    panel.cursor = 0;
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_move_cursor_down() {
    let mut panel = panel_with_cursor(2, 0, 0);
    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 1);
}

#[test]
fn test_panel_state_move_cursor_down_wraps_to_top() {
    let mut panel = panel_with_cursor(2, 1, 0);
    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_move_cursor_down_scroll() {
    let mut panel = panel_with_cursor(10, 4, 0);
    panel.move_cursor_down(5);
    assert_eq!(panel.cursor, 5);
    assert_eq!(panel.scroll_offset, 1);
}

#[test]
fn test_panel_state_move_cursor_down_empty() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.cursor = 0;
    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_app_state_new_sets_field_defaults() {
    let state = AppState::new();
    assert_eq!(state.active_panel, ActivePanel::Left);
    assert_eq!(state.mode, AppMode::Normal);
    assert!(!state.should_quit());
    assert!(state.ui.status_message.is_none());
}

#[test]
fn test_app_state_substate_defaults() {
    let state = AppState::new();

    assert_eq!(state.active_panel, ActivePanel::Left);
    assert_eq!(state.mode, AppMode::Normal);
    assert!(!state.should_quit());
    assert!(state.ui.status_message.is_none());
    assert_eq!(state.input.dialog_input.cursor(), 0);
    assert_eq!(state.ui.picker_selected, 0);
    assert_eq!(state.ui.menu_selected, 0);
    assert_eq!(state.ui.menu_item_selected, 0);
    assert!(state.tree.entries.is_empty());
    assert!(state.ui.user_menu_entries.is_empty());
    assert_eq!(state.ui.directory_hotlist.len(), 1);
}

#[test]
fn test_text_input_mutations_clamp_cursor() {
    let mut input = TextInput::new();
    input.set_text("ąb".to_string());
    input.set_cursor(99);

    assert!(input.backspace());
    assert_eq!(input.text(), "ą");
    assert_eq!(input.cursor(), 1);

    input.insert_char('x');
    assert_eq!(input.text(), "ąx");
    assert_eq!(input.cursor(), 2);

    input.set_cursor(99);
    assert!(input.delete_word_backward());
    assert_eq!(input.text(), "");
    assert_eq!(input.cursor(), 0);
}

#[test]
fn test_text_input_set_text_at_end_counts_emoji_zwj_graphemes() {
    let mut input = TextInput::default();
    input.set_text_at_end("a👨‍👩‍👧‍👦b".to_string());

    assert_eq!(input.grapheme_count(), 3);
    assert_eq!(input.cursor(), 3);
}

#[test]
fn test_app_state_active_panel_left() {
    let state = AppState::new();
    let panel = state.active_panel();
    assert_eq!(panel.path(), state.left_panel.path());
}

#[test]
fn test_app_state_active_panel_right() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Right;
    let panel = state.active_panel();
    assert_eq!(panel.path(), state.right_panel.path());
}

#[test]
fn test_app_state_active_panel_mut_left() {
    let mut state = AppState::new();
    let panel = state.active_panel_mut();
    panel.set_path(PathBuf::from("/modified"));
    assert_eq!(state.left_panel.path(), PathBuf::from("/modified"));
}

#[test]
fn test_app_state_active_panel_mut_right() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Right;
    let panel = state.active_panel_mut();
    panel.set_path(PathBuf::from("/modified"));
    assert_eq!(state.right_panel.path(), PathBuf::from("/modified"));
}

#[test]
fn test_app_state_inactive_panel_left() {
    let mut state = AppState::new();
    state.active_panel = ActivePanel::Right;
    let panel = state.inactive_panel();
    assert_eq!(panel.path(), state.left_panel.path());
}

#[test]
fn test_app_state_inactive_panel_right() {
    let state = AppState::new();
    let panel = state.inactive_panel();
    assert_eq!(panel.path(), state.right_panel.path());
}

#[test]
fn test_dialog_kind_confirm() {
    let details = ConfirmDetails::simple("Confirm", "Are you sure?");
    let dialog = DialogKind::Confirm(details);
    let DialogKind::Confirm(cd) = dialog else {
        panic!("Expected Confirm variant");
    };
    assert_eq!(cd.title, "Confirm");
    assert_eq!(cd.message, "Are you sure?");
    assert!(cd.files.is_none());
}

#[test]
fn test_confirm_details_simple() {
    let cd = ConfirmDetails::simple("Delete", "Delete 'foo.txt'?");
    assert_eq!(cd.title, "Delete");
    assert_eq!(cd.message, "Delete 'foo.txt'?");
    assert!(cd.files.is_none());
}

#[test]
fn test_confirm_details_with_files() {
    let files = vec!["/tmp/a.txt".to_string(), "/tmp/b.txt".to_string()];
    let cd = ConfirmDetails::with_files("Delete", "Delete 2 entries?", files);
    let displayed = cd.files.as_ref().expect("files vector is Some");
    assert_eq!(displayed[0], "/tmp/a.txt");
    assert_eq!(displayed[1], "/tmp/b.txt");
}

#[test]
fn test_confirm_details_with_empty_files() {
    let cd = ConfirmDetails::with_files("Delete", "Nothing?", vec![]);
    assert_eq!(cd.files, Some(vec![]));
}

#[test]
fn test_dialog_kind_input() {
    let dialog = DialogKind::Input {
        prompt: "Enter name:".to_string(),
        action: InputAction::Rename,
    };
    let DialogKind::Input { prompt, action } = dialog else {
        panic!("Expected Input variant");
    };
    assert_eq!(prompt, "Enter name:");
    assert_eq!(action, InputAction::Rename);
}

#[test]
fn test_dialog_kind_error() {
    let dialog = DialogKind::Error("Error occurred".to_string());
    let DialogKind::Error(msg) = dialog else {
        panic!("Expected Error variant");
    };
    assert_eq!(msg, "Error occurred");
}

#[test]
fn test_dialog_kind_progress() {
    let dialog = DialogKind::Progress {
        message: "Copying...".to_string(),
        progress_fraction: 0.5,
        cancellable: true,
    };
    let DialogKind::Progress {
        message,
        progress_fraction,
        cancellable,
    } = dialog
    else {
        panic!("Expected Progress variant");
    };
    assert_eq!(message, "Copying...");
    assert_eq!(progress_fraction, 0.5);
    assert!(cancellable);
}

#[test]
fn test_dialog_kind_copy_move() {
    let sources = vec![PathBuf::from("/source1"), PathBuf::from("/source2")];
    let dest = PathBuf::from("/dest");
    let dialog = DialogKind::CopyMove(Box::new(CopyMoveDetails {
        source: sources.clone(),
        dest: dest.clone(),
        kind: CopyMoveKind::Move,
    }));
    let DialogKind::CopyMove(details) = dialog else {
        panic!("Expected CopyMove variant");
    };
    assert_eq!(details.source, sources);
    // `source_display()` is now derived on demand from `source` (file name,
    // falling back to the full path) instead of a stored parallel field.
    assert_eq!(details.source_display(), vec!["source1", "source2"]);
    assert_eq!(details.dest, dest);
    assert!(details.kind.is_move());
}

#[test]
fn test_panel_state_move_cursor_up_scroll_adjust() {
    let mut panel = panel_with_cursor(10, 3, 5);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 2);
    assert_eq!(panel.scroll_offset, 2);
}

#[test]
fn test_panel_state_move_cursor_up_no_scroll_when_visible() {
    let mut panel = panel_with_cursor(10, 5, 3);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 4);
    assert_eq!(panel.scroll_offset, 3);
}

#[test]
fn test_panel_state_move_cursor_down_scroll_new_formula() {
    let mut panel = panel_with_cursor(10, 6, 3);
    panel.move_cursor_down(4);
    assert_eq!(panel.cursor, 7);
    assert_eq!(panel.scroll_offset, 4);
}

#[test]
fn test_panel_state_move_cursor_down_no_scroll_when_visible() {
    let mut panel = panel_with_cursor(10, 3, 0);
    panel.move_cursor_down(5);
    assert_eq!(panel.cursor, 4);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_panel_state_ensure_cursor_visible_below() {
    let mut panel = panel_with_cursor(10, 7, 2);
    panel.ensure_cursor_visible(4);
    assert_eq!(panel.scroll_offset, 4);
}

#[test]
fn test_panel_state_ensure_cursor_visible_above() {
    let mut panel = panel_with_cursor(10, 2, 5);
    panel.ensure_cursor_visible(4);
    assert_eq!(panel.scroll_offset, 2);
}

#[test]
fn test_panel_state_ensure_cursor_visible_already_visible() {
    let mut panel = panel_with_cursor(10, 4, 2);
    panel.ensure_cursor_visible(4);
    assert_eq!(panel.scroll_offset, 2);
}

#[test]
fn test_panel_state_ensure_cursor_visible_edge_case() {
    let mut panel = panel_with_cursor(10, 6, 3);
    panel.ensure_cursor_visible(4);
    assert_eq!(panel.scroll_offset, 3);
}

#[test]
fn test_total_size_computed_by_recalculate() {
    let mut panel = panel_from(vec![
        entry("a.txt").file(100).permissions(0o644).build(),
        entry("b.txt").file(200).permissions(0o644).build(),
        entry("c.txt")
            .file(300)
            .permissions(0o644)
            .selected()
            .build(),
    ]);
    panel.recalculate_selection_stats();
    assert_eq!(panel.total_size(), 600);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 300);
}

#[test]
fn test_hidden_script_is_code() {
    let entry = entry(".script.sh")
        .raw_mode(0o100755)
        .file(100)
        .hidden()
        .build();
    assert_eq!(entry.category(), FileCategory::Code);
}

#[test]
fn test_hidden_archive_is_archive() {
    let entry = entry(".backup.zip")
        .raw_mode(0o100644)
        .file(100)
        .hidden()
        .build();
    assert_eq!(entry.category(), FileCategory::Archive);
}

#[test]
fn test_symlink_overrides_dir() {
    let entry = entry("link_to_dir").raw_mode(0o120777).build();
    assert_eq!(entry.category(), FileCategory::Symlink);
}

#[test]
fn test_symlink_overrides_hidden() {
    let entry = entry(".hidden_link").raw_mode(0o120777).hidden().build();
    assert_eq!(entry.category(), FileCategory::Symlink);
}

#[test]
fn test_executable_without_extension_is_executable() {
    let entry = entry("mybinary").raw_mode(0o100755).file(100).build();
    assert_eq!(entry.category(), FileCategory::Executable);
}

#[test]
fn test_hidden_apk_is_archive() {
    let entry = entry(".app.apk")
        .raw_mode(0o100644)
        .file(100)
        .hidden()
        .build();
    assert_eq!(entry.category(), FileCategory::Archive);
}

#[test]
fn test_total_size_includes_all_entries() {
    let mut panel = panel_from(vec![
        entry("small.txt").file(50).permissions(0o644).build(),
        entry("big.txt")
            .file(5000)
            .permissions(0o644)
            .selected()
            .build(),
    ]);
    panel.recalculate_selection_stats();
    assert_eq!(panel.total_size(), 5050);
    assert_eq!(panel.selected_size(), 5000);
}

#[test]
fn test_panel_state_empty_entries_cursor_scroll_zero() {
    let panel = PanelState::new(PathBuf::from("/test"));
    assert_eq!(panel.listing.filtered_len(), 0);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_panel_state_single_item_cursor() {
    let mut panel = panel_from(vec![entry("only.txt").file(10).permissions(0o644).build()]);

    assert_eq!(panel.cursor, 0);
    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_cursor_stays_at_last_after_entry_removal() {
    let mut panel = panel_from(vec![
        entry("a.txt").file(10).permissions(0o644).build(),
        entry("b.txt").file(10).permissions(0o644).build(),
        entry("c.txt").file(10).permissions(0o644).build(),
    ]);

    // Simulate the listing shrinking to a single entry (e.g. a watcher refresh),
    // leaving the cursor stale at its old index past the new end.
    panel.set_entries(vec![entry("a.txt").file(10).permissions(0o644).build()]);
    panel.cursor = 2;

    // Tests same clamping logic as restore_panel_cursor() in panel_ops.rs
    let max_index = panel.listing.filtered_len().saturating_sub(1);
    panel.cursor = panel.cursor.min(max_index);

    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_move_cursor_down_clamped_at_last() {
    let mut panel = panel_from(vec![
        entry("a.txt").file(10).permissions(0o644).build(),
        entry("b.txt").file(10).permissions(0o644).build(),
    ]);
    panel.cursor = 1;

    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);

    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 1);
}

#[test]
fn test_panel_state_move_cursor_up_clamped_at_zero() {
    let mut panel = panel_from(vec![entry("a.txt").file(10).permissions(0o644).build()]);
    panel.cursor = 0;

    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_current_entry_empty_returns_none() {
    let panel = PanelState::new(PathBuf::from("/test"));
    assert!(panel.current_entry().is_none());
}

#[test]
fn test_panel_state_scroll_offset_with_many_entries() {
    let mut panel = panel_from(
        (0..100)
            .map(|i| {
                TestEntry::new(format!("file{i:03}.txt"))
                    .path(test_path(format!("file{i:03}.txt")))
                    .file(10)
                    .permissions(0o644)
                    .build()
            })
            .collect(),
    );

    let visible_height = 20;

    // Non-wrapping: cursor near bottom of visible window, scroll advances
    panel.cursor = 19;
    panel.scroll_offset = 0;
    panel.move_cursor_down(visible_height);
    assert_eq!(panel.cursor, 20);
    assert_eq!(
        panel.scroll_offset, 1,
        "scroll_offset must advance when cursor exits visible window"
    );

    // Advance further into the list by calling move_cursor_down naturally,
    // so the method itself computes scroll_offset (don't pre-set it)
    for _ in 0..60 {
        panel.move_cursor_down(visible_height);
    }
    assert_eq!(panel.cursor, 80);
    assert_eq!(panel.scroll_offset, 61);

    panel.move_cursor_down(visible_height);
    assert_eq!(panel.cursor, 81);
    assert_eq!(
        panel.scroll_offset, 62,
        "scroll_offset must track cursor deeper in the list"
    );
}

#[test]
fn builder_clears_dir_target_follow_on_type_change() {
    let dir_entry = FileEntry::builder()
        .name("d")
        .path(PathBuf::from("d"))
        .is_dir(true)
        .build()
        .expect("valid test entry");
    let mut cha = dir_entry.cha;
    cha.kind.dir_target = true;
    cha.kind.follow = true;
    assert!(cha.kind.dir_target);
    assert!(cha.kind.follow);

    let cleared = FileEntry::builder()
        .name("d")
        .path(PathBuf::from("d"))
        .cha(cha)
        .is_dir(false)
        .build()
        .expect("valid test entry");
    assert!(!cleared.cha.kind.dir_target);
    assert!(!cleared.cha.kind.follow);

    let link_entry = FileEntry::builder()
        .name("l")
        .path(PathBuf::from("l"))
        .is_symlink(true)
        .build()
        .expect("valid test entry");
    let mut cha = link_entry.cha;
    cha.kind.dir_target = true;
    cha.kind.follow = true;
    assert!(cha.kind.dir_target);
    assert!(cha.kind.follow);

    let cleared = FileEntry::builder()
        .name("l")
        .path(PathBuf::from("l"))
        .cha(cha)
        .is_symlink(false)
        .build()
        .expect("valid test entry");
    assert!(!cleared.cha.kind.dir_target);
    assert!(!cleared.cha.kind.follow);
}

#[test]
fn mtime_none_displays_unknown() {
    let no_mtime = FileEntry::builder()
        .name("unknown.txt")
        .path(PathBuf::from("unknown.txt"))
        .build()
        .expect("valid test entry");
    let expected_epoch = chrono::DateTime::from_timestamp(0, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%d-%m-%y %H:%M")
        .to_string();
    assert_eq!(no_mtime.display_modified(), expected_epoch.as_str());
}

#[test]
fn test_move_cursor_up_wraps_to_last_entry() {
    let mut panel = panel_with_n_entries(5);
    panel.cursor = 0;
    panel.move_cursor_up(3);
    assert_eq!(panel.cursor, 4);
    assert_eq!(panel.scroll_offset, 2);
}

#[test]
fn test_move_cursor_up_wraps_with_single_entry() {
    let mut panel = panel_from(vec![entry("file.txt").file(100).permissions(0o644).build()]);
    panel.cursor = 0;
    panel.move_cursor_up(3);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_move_cursor_down_wraps_to_first_entry() {
    let mut panel = panel_with_n_entries(5);
    panel.cursor = 4;
    panel.move_cursor_down(3);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_move_cursor_down_wraps_with_single_entry() {
    let mut panel = panel_from(vec![entry("file.txt").file(100).permissions(0o644).build()]);
    panel.cursor = 0;
    panel.move_cursor_down(3);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_move_cursor_up_wrap_then_down_wrap() {
    let mut panel = panel_with_n_entries(5);
    panel.cursor = 0;
    panel.move_cursor_up(5);
    assert_eq!(panel.cursor, 4);
    panel.move_cursor_down(5);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_move_cursor_down_wrap_with_many_entries_scroll_check() {
    let mut panel = panel_with_n_entries(20);
    panel.cursor = 19;
    panel.move_cursor_down(5);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_move_cursor_up_wrap_with_many_entries_scroll_check() {
    let mut panel = panel_with_n_entries(20);
    panel.cursor = 0;
    panel.move_cursor_up(5);
    assert_eq!(panel.cursor, 19);
    assert_eq!(panel.scroll_offset, 15);
}

#[test]
fn test_move_cursor_down_with_zero_height_no_scroll_adjust() {
    let mut panel = panel_with_n_entries(5);
    panel.cursor = 2;
    panel.move_cursor_down(0);
    assert_eq!(panel.cursor, 3);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_scroll_offset_beyond_entries_len_clamped_by_ensure_visible() {
    let mut panel = panel_with_cursor(5, 2, 100);
    assert_eq!(panel.scroll_offset, 100);
    panel.ensure_cursor_visible(5);
    assert_eq!(panel.scroll_offset, 2);
}
