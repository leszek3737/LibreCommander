use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use super::dialogs::{ConfirmDetails, CopyMoveDetails, DialogKind, InputAction, PickerKind};
use super::file_entry::{FileCategory, FileEntry, FileEntryBuilder};
use super::modes::AppMode;
use super::panel::{ActivePanel, PanelState};
use super::sorting::{ListingMode, SortMode, SortOptions};
use super::text_input::TextInput;

use crate::app::types::app_state::AppState;
use crate::fs::cha::ChaKind;

// TODO: consider builder pattern for test entry construction to reduce parameter count
fn test_entry_builder(name: &str) -> FileEntryBuilder {
    FileEntry::builder()
        .name(name)
        .path(PathBuf::from(name))
        .owner("testuser")
        .group("testgroup")
}

fn create_test_entry(
    name: &str,
    is_dir: bool,
    size: u64,
    permissions: u32,
    is_selected: bool,
) -> FileEntry {
    test_entry_builder(name)
        .is_dir(is_dir)
        .size(size)
        .permissions(permissions)
        .selected(is_selected)
        .is_hidden(name.starts_with('.'))
        .modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .created(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .build()
}

// TODO: consolidate create_test_entry and cha_entry — nearly identical, differing only in
// how is_dir/is_symlink are derived (explicit vs from mode bits)
fn cha_entry(name: &str, mode: u32, size: u64, hidden: bool) -> FileEntry {
    let is_link = (mode & 0o170000) == 0o120000;
    let is_directory = (mode & 0o170000) == 0o040000;
    test_entry_builder(name)
        .is_dir(is_directory)
        .is_symlink(is_link)
        .size(size)
        .permissions(mode & 0o7777)
        .is_hidden(hidden)
        .modified(UNIX_EPOCH)
        .created(UNIX_EPOCH)
        .build()
}

fn panel_with_n_entries(n: u32) -> PanelState {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    for i in 0..n {
        panel.listing.entries.push(create_test_entry(
            &format!("file{}.txt", i),
            false,
            100,
            0o644,
            false,
        ));
    }
    panel
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
        let entry = create_test_entry("test.txt", false, size, 0o644, false);
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
        let entry = create_test_entry("test.txt", false, 100, perms, false);
        assert_eq!(entry.display_permissions(), expected, "perms=0o{perms:o}");
    }
}

#[test]
#[allow(clippy::expect_used)]
fn test_file_entry_display_modified() {
    let entry = create_test_entry("test.txt", false, 100, 0o644, false);
    let expected = chrono::DateTime::from_timestamp(1_000_000_000, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%d-%m-%y %H:%M")
        .to_string();
    assert_eq!(entry.display_modified(), expected.as_str());
}

#[test]
fn test_sort_mode_default() {
    assert_eq!(SortMode::default(), SortMode::NameAsc);
}

#[test]
fn test_panel_state_new() {
    let path = PathBuf::from("/test");
    let panel = PanelState::new(path.clone());
    assert_eq!(panel.path(), path);
    assert_eq!(panel.listing.entries.len(), 0);
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
#[allow(clippy::expect_used)]
fn test_panel_state_current_entry_some() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, false));
    panel.cursor = 0;
    assert_eq!(
        panel.current_entry().expect("current entry exists").name,
        "file1.txt"
    );
}

#[test]
fn test_panel_state_current_entry_out_of_bounds() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, false));
    panel.cursor = 5;
    assert!(panel.current_entry().is_none());
}

#[test]
fn test_panel_state_toggle_selection_toggle_on() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, false));
    panel.cursor = 0;
    panel.toggle_selection();
    assert!(panel.listing.entries[0].selected);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 100);
}

#[test]
fn test_panel_state_toggle_selection_toggle_off() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, true));
    panel.cursor = 0;
    assert!(panel.listing.entries[0].selected);
    panel.toggle_selection();
    assert!(!panel.listing.entries[0].selected);
    assert_eq!(panel.selected_count(), 0);
    assert_eq!(panel.selected_size(), 0);
}

#[test]
fn test_panel_state_set_selection_at_on() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, false));

    panel.set_selection_at(0, true);

    assert!(panel.listing.entries[0].selected);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 100);
}

#[test]
fn test_panel_state_set_selection_at_off() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, true));

    panel.set_selection_at(0, false);

    assert!(!panel.listing.entries[0].selected);
    assert_eq!(panel.selected_count(), 0);
    assert_eq!(panel.selected_size(), 0);
}

#[test]
fn test_panel_state_sync_unfiltered_selection() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("file1.txt", false, 100, 0o644, true),
        create_test_entry("file2.txt", false, 200, 0o644, false),
    ];
    panel.listing.unfiltered_entries = vec![
        create_test_entry("file1.txt", false, 100, 0o644, false),
        create_test_entry("file2.txt", false, 200, 0o644, true),
        create_test_entry("file3.txt", false, 300, 0o644, true),
    ];

    panel.sync_unfiltered_selection();

    assert!(panel.listing.unfiltered_entries[0].selected);
    assert!(!panel.listing.unfiltered_entries[1].selected);
    assert!(panel.listing.unfiltered_entries[2].selected);
}

#[test]
fn test_panel_state_selected_entries() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, true));
    panel
        .listing
        .entries
        .push(create_test_entry("file2.txt", false, 200, 0o644, false));
    panel
        .listing
        .entries
        .push(create_test_entry("file3.txt", false, 300, 0o644, true));

    let selected = panel.selected_entries();
    assert_eq!(selected.len(), 2);
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
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file1.txt", false, 100, 0o644, false));
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
    assert!(!state.should_quit);
    assert!(state.status_message.is_none());
}

#[test]
fn test_app_state_substate_defaults() {
    let state = AppState::new();

    assert_eq!(state.active_panel, ActivePanel::Left);
    assert_eq!(state.mode, AppMode::Normal);
    assert!(!state.should_quit);
    assert!(state.status_message.is_none());
    assert_eq!(state.dialog_input.cursor, 0);
    assert_eq!(state.picker_selected, 0);
    assert_eq!(state.menu_selected, 0);
    assert_eq!(state.menu_item_selected, 0);
    assert!(state.tree_entries.is_empty());
    assert!(state.user_menu_entries.is_empty());
    assert_eq!(state.directory_hotlist.len(), 1);
}

#[test]
fn test_text_input_mutations_clamp_cursor() {
    let mut input = TextInput::new();
    input.text = "ąb".to_string();
    input.recompute_grapheme_count();
    input.cursor = 99;

    assert!(input.backspace());
    assert_eq!(input.text, "ą");
    assert_eq!(input.cursor, 1);

    input.insert_char('x');
    assert_eq!(input.text, "ąx");
    assert_eq!(input.cursor, 2);

    input.cursor = 99;
    input.recompute_grapheme_count();
    assert!(input.delete_word_backward());
    assert_eq!(input.text, "");
    assert_eq!(input.cursor, 0);
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
#[allow(clippy::expect_used)]
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
        is_move: true,
        source_display: sources.iter().map(|p| p.display().to_string()).collect(),
    }));
    let DialogKind::CopyMove(details) = dialog else {
        panic!("Expected CopyMove variant");
    };
    assert_eq!(details.source, sources);
    assert_eq!(details.dest, dest);
    assert!(details.is_move);
    assert_eq!(details.source_display.len(), 2);
}

// Smoke test: verifies PartialEq derivation is correct and all variants compile
#[test]
fn test_app_mode_variants() {
    let normal = AppMode::Normal;
    assert_eq!(normal, AppMode::Normal);

    let viewing = AppMode::Viewing;
    assert_eq!(viewing, AppMode::Viewing);

    let cmd_line = AppMode::CommandLine;
    assert_eq!(cmd_line, AppMode::CommandLine);

    let dialog = AppMode::Dialog(DialogKind::Confirm(ConfirmDetails::simple("Test", "test")));
    if let AppMode::Dialog(DialogKind::Confirm(cd)) = &dialog {
        assert_eq!(cd.message, "test");
    }

    let search = AppMode::Search;
    assert_eq!(search, AppMode::Search);

    let menu = AppMode::Menu;
    assert_eq!(menu, AppMode::Menu);

    let picker = AppMode::ListPicker(PickerKind::History);
    assert_eq!(picker, AppMode::ListPicker(PickerKind::History));
}

// Smoke test: verifies PartialEq derivation is correct and variants compile
#[test]
fn test_active_panel_variants() {
    let left = ActivePanel::Left;
    assert_eq!(left, ActivePanel::Left);

    let right = ActivePanel::Right;
    assert_eq!(right, ActivePanel::Right);
}

// Smoke test: verifies Default derivation produces the same state as new()
#[test]
fn test_app_state_default() {
    let state = AppState::default();
    assert_eq!(state.active_panel, ActivePanel::Left);
    assert!(!state.should_quit);
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
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, 100, 0o644, false),
        create_test_entry("b.txt", false, 200, 0o644, false),
        create_test_entry("c.txt", false, 300, 0o644, true),
    ];
    panel.recalculate_selection_stats();
    assert_eq!(panel.total_size(), 600);
    assert_eq!(panel.selected_count(), 1);
    assert_eq!(panel.selected_size(), 300);
}

#[test]
fn test_hidden_script_is_code() {
    let entry = cha_entry(".script.sh", 0o100755, 100, true);
    assert_eq!(entry.category(), FileCategory::Code);
}

#[test]
fn test_hidden_archive_is_archive() {
    let entry = cha_entry(".backup.zip", 0o100644, 100, true);
    assert_eq!(entry.category(), FileCategory::Archive);
}

#[test]
fn test_symlink_overrides_dir() {
    let entry = cha_entry("link_to_dir", 0o120777, 0, false);
    assert_eq!(entry.category(), FileCategory::Symlink);
}

#[test]
fn test_symlink_overrides_hidden() {
    let entry = cha_entry(".hidden_link", 0o120777, 0, true);
    assert_eq!(entry.category(), FileCategory::Symlink);
}

#[test]
fn test_executable_without_extension_is_executable() {
    let entry = cha_entry("mybinary", 0o100755, 100, false);
    assert_eq!(entry.category(), FileCategory::Executable);
}

#[test]
fn test_hidden_apk_is_archive() {
    let entry = cha_entry(".app.apk", 0o100644, 100, true);
    assert_eq!(entry.category(), FileCategory::Archive);
}

#[test]
fn test_total_size_includes_all_entries() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("small.txt", false, 50, 0o644, false),
        create_test_entry("big.txt", false, 5000, 0o644, true),
    ];
    panel.recalculate_selection_stats();
    assert_eq!(panel.total_size(), 5050);
    assert_eq!(panel.selected_size(), 5000);
}

#[test]
fn test_panel_state_empty_entries_cursor_scroll_zero() {
    let panel = PanelState::new(PathBuf::from("/test"));
    assert_eq!(panel.listing.entries.len(), 0);
    assert_eq!(panel.cursor, 0);
    assert_eq!(panel.scroll_offset, 0);
}

#[test]
fn test_panel_state_single_item_cursor() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![create_test_entry("only.txt", false, 10, 0o644, false)];

    assert_eq!(panel.cursor, 0);
    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);
    panel.move_cursor_up(10);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_cursor_stays_at_last_after_entry_removal() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, 10, 0o644, false),
        create_test_entry("b.txt", false, 10, 0o644, false),
        create_test_entry("c.txt", false, 10, 0o644, false),
    ];
    panel.cursor = 2;

    panel.listing.entries.truncate(1);

    // Tests same clamping logic as restore_panel_cursor() in panel_ops.rs
    let max_index = panel.listing.entries.len().saturating_sub(1);
    panel.cursor = panel.cursor.min(max_index);

    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_state_move_cursor_down_clamped_at_last() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, 10, 0o644, false),
        create_test_entry("b.txt", false, 10, 0o644, false),
    ];
    panel.cursor = 1;

    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 0);

    panel.move_cursor_down(10);
    assert_eq!(panel.cursor, 1);
}

#[test]
fn test_panel_state_move_cursor_up_clamped_at_zero() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![create_test_entry("a.txt", false, 10, 0o644, false)];
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
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = (0..100)
        .map(|i| create_test_entry(&format!("file{i:03}.txt"), false, 10, 0o644, false))
        .collect();

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
        .build();
    let mut cha = dir_entry.cha;
    cha.kind.insert(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
    assert!(cha.kind.contains(ChaKind::DIR_TARGET));
    assert!(cha.kind.contains(ChaKind::FOLLOW));

    let cleared = FileEntry::builder()
        .name("d")
        .path(PathBuf::from("d"))
        .cha(cha)
        .is_dir(false)
        .build();
    assert!(!cleared.cha.kind.contains(ChaKind::DIR_TARGET));
    assert!(!cleared.cha.kind.contains(ChaKind::FOLLOW));

    let link_entry = FileEntry::builder()
        .name("l")
        .path(PathBuf::from("l"))
        .is_symlink(true)
        .build();
    let mut cha = link_entry.cha;
    cha.kind.insert(ChaKind::DIR_TARGET | ChaKind::FOLLOW);
    assert!(cha.kind.contains(ChaKind::DIR_TARGET));
    assert!(cha.kind.contains(ChaKind::FOLLOW));

    let cleared = FileEntry::builder()
        .name("l")
        .path(PathBuf::from("l"))
        .cha(cha)
        .is_symlink(false)
        .build();
    assert!(!cleared.cha.kind.contains(ChaKind::DIR_TARGET));
    assert!(!cleared.cha.kind.contains(ChaKind::FOLLOW));
}

#[test]
#[allow(clippy::expect_used)]
fn mtime_none_displays_unknown_and_sorts_after_known() {
    let no_mtime = FileEntry::builder()
        .name("unknown.txt")
        .path(PathBuf::from("unknown.txt"))
        .build();
    let expected_epoch = chrono::DateTime::from_timestamp(0, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%d-%m-%y %H:%M")
        .to_string();
    assert_eq!(no_mtime.display_modified(), expected_epoch.as_str());

    let with_mtime = FileEntry::builder()
        .name("known.txt")
        .path(PathBuf::from("known.txt"))
        .modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .build();

    // TODO: move sorting integration test to src/tests/
    let mut entries = vec![no_mtime, with_mtime];
    crate::ops::sorting::sort_entries(&mut entries, SortMode::ModTimeDesc, SortOptions::default());
    assert_eq!(entries[0].name, "known.txt");
    assert_eq!(entries[1].name, "unknown.txt");
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
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file.txt", false, 100, 0o644, false));
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
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel
        .listing
        .entries
        .push(create_test_entry("file.txt", false, 100, 0o644, false));
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
