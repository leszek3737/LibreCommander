#![allow(clippy::expect_used)]

use super::*;
use crate::app::types::format_time;
use crate::app::types::sanitize_for_display;
use crate::ui::theme::{DEFAULT_COLORS, IconTheme};
use ratatui::style::Color;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn test_timestamp() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

fn entry_with(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool, size: u64) -> FileEntry {
    use crate::app::types::test_helpers::TestEntry;
    let mut e = TestEntry::new(name)
        .path(name)
        .modified(test_timestamp())
        .owner("user")
        .group("group");
    if is_dir {
        e = e.len(size);
    } else {
        e = e.file(size);
    }
    if is_symlink {
        e = e.symlink();
    }
    if name.starts_with('.') {
        e = e.hidden();
    }
    if is_exec {
        e = e.executable(true);
    }
    e.build()
}

fn create_test_entry(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool) -> FileEntry {
    entry_with(name, is_dir, is_exec, is_symlink, 1024)
}

/// Render a long-mode entry line into a fresh buffer, matching how
/// `render_panel_with_colors` drives `format_entry_line`.
fn entry_line(entry: &FileEntry, width: usize, show_permissions: bool) -> String {
    let mut suffix = String::new();
    let mut out = String::new();
    format_entry_line(
        entry,
        width,
        show_permissions,
        &entry.category(),
        IconTheme::default(),
        &mut suffix,
        &mut out,
    );
    out
}

/// Render a brief-mode entry line into a fresh buffer.
fn brief_line(entry: &FileEntry, width: usize) -> String {
    let mut out = String::new();
    format_brief_entry_line(
        entry,
        width,
        &entry.category(),
        IconTheme::default(),
        &mut out,
    );
    out
}

// Data-driven case table; splitting into smaller fns would destroy readability without reducing complexity.
#[allow(clippy::too_many_lines)]
#[test]
fn file_color_table_maps_categories_with_intended_bold() {
    // One row per (name, flags, size) input. `bold` is exactly what the panel
    // passes to `get_file_color` (`is_dir || is_exec`); we assert both the
    // foreground color and whether BOLD is set, so the table documents intent.
    struct Case<'a> {
        name: &'a str,
        is_dir: bool,
        is_exec: bool,
        is_symlink: bool,
        size: u64,
        fg: Color,
    }

    let long_name: String = "a".repeat(300);
    let cases = [
        Case {
            name: "mydir",
            is_dir: true,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.directory,
        },
        Case {
            name: "script.sh",
            is_dir: false,
            is_exec: true,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.source_code,
        },
        Case {
            name: "mybinary",
            is_dir: false,
            is_exec: true,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.executable,
        },
        Case {
            name: "link",
            is_dir: false,
            is_exec: false,
            is_symlink: true,
            size: 1024,
            fg: DEFAULT_COLORS.symlink,
        },
        Case {
            name: "archive.tar.gz",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.archive,
        },
        Case {
            name: "photo.png",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.image,
        },
        Case {
            name: "main.rs",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.source_code,
        },
        Case {
            name: ".hidden",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.regular_file,
        },
        Case {
            name: "settings.toml",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.config,
        },
        Case {
            name: "movie.mp4",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.video,
        },
        Case {
            name: "song.mp3",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.audio,
        },
        Case {
            name: "font.ttf",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.font,
        },
        Case {
            name: "unknown.xyz",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.regular_file,
        },
        Case {
            name: "random.qzx",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.regular_file,
        },
        Case {
            name: "document.txt",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.document,
        },
        // Edge cases.
        // u64::MAX size must not change the (size-independent) color.
        Case {
            name: "huge.qzx",
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: u64::MAX,
            fg: DEFAULT_COLORS.regular_file,
        },
        // dir + executable -> directory wins (file_type::category precedence).
        Case {
            name: "bindir",
            is_dir: true,
            is_exec: true,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.directory,
        },
        // symlink + executable -> symlink wins.
        Case {
            name: "exelink",
            is_dir: false,
            is_exec: true,
            is_symlink: true,
            size: 1024,
            fg: DEFAULT_COLORS.symlink,
        },
        // 300-char name, no extension -> Other.
        Case {
            name: long_name.as_str(),
            is_dir: false,
            is_exec: false,
            is_symlink: false,
            size: 1024,
            fg: DEFAULT_COLORS.regular_file,
        },
        // Empty names are unrepresentable: FileEntry::build() asserts a non-empty name, so
        // the panel never receives one. The empty-string → Other mapping is verified at the
        // category level in `compute_category_empty_name_is_other` below.
    ];

    for case in cases {
        let entry = entry_with(
            case.name,
            case.is_dir,
            case.is_exec,
            case.is_symlink,
            case.size,
        );
        let bold = entry.is_dir() || entry.is_executable();
        let style = get_file_color_with_palette(&entry.category(), bold, &DEFAULT_COLORS);
        assert_eq!(style.fg, Some(case.fg), "fg mismatch for {:?}", case.name);
        assert_eq!(
            style.add_modifier.contains(Modifier::BOLD),
            bold,
            "bold mismatch for {:?}",
            case.name
        );
    }
}

#[test]
fn shorten_home_with_replaces_home_prefix_only() {
    assert_eq!(shorten_home_with("/home/u/docs", "/home/u"), "~/docs");
    assert_eq!(shorten_home_with("/home/u", "/home/u"), "~");
    // Prefix must end on a path-component boundary.
    assert_eq!(
        shorten_home_with("/home/user2/x", "/home/u"),
        "/home/user2/x"
    );
    assert_eq!(shorten_home_with("/etc", "/home/u"), "/etc");
    // A root home dir must not swallow every absolute path.
    assert_eq!(shorten_home_with("/etc", "/"), "/etc");
}

#[test]
fn test_format_size_zero() {
    assert_eq!(format_size(0), "0 B");
}

#[test]
fn test_format_size_bytes() {
    assert_eq!(format_size(500), "500 B");
}

#[test]
fn test_format_size_kilobytes() {
    assert_eq!(format_size(1536), "1.5 KB");
}

#[test]
fn test_format_size_megabytes() {
    assert_eq!(format_size(1024 * 1024 * 5), "5.0 MB");
}

#[test]
fn test_format_size_one_byte() {
    assert_eq!(format_size(1), "1 B");
}

#[test]
fn test_format_size_just_under_1kb() {
    assert_eq!(format_size(1023), "1023 B");
}

#[test]
fn test_format_size_exact_1kb() {
    assert_eq!(format_size(1024), "1.0 KB");
}

#[test]
fn test_format_size_exact_1mb() {
    assert_eq!(format_size(1024 * 1024), "1.0 MB");
}

#[test]
fn test_format_size_exact_1gb() {
    assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
}

#[test]
fn test_format_size_u64_max() {
    let result = format_size(u64::MAX);
    assert!(result.contains("EB"));
}

#[test]
fn test_format_permissions_full() {
    let result = FileEntry::display_permissions_raw(0o755);
    assert_eq!(result, "rwxr-xr-x");
}

#[test]
fn test_format_permissions_readonly() {
    let result = FileEntry::display_permissions_raw(0o444);
    assert_eq!(result, "r--r--r--");
}

#[test]
fn test_format_permissions_no_permissions() {
    let result = FileEntry::display_permissions_raw(0o000);
    assert_eq!(result, "---------");
}

#[test]
fn test_format_time_contains_date_separators() {
    let result = format_time(test_timestamp());
    assert!(result.contains("-"));
    assert!(result.contains(":"));
}

#[test]
fn test_format_time_unix_epoch_is_non_empty() {
    let time = SystemTime::UNIX_EPOCH;
    let result = format_time(time);
    assert!(!result.is_empty());
}

#[test]
fn test_format_entry_line_basic() {
    let entry = create_test_entry("file.txt", false, false, false);
    assert!(entry_line(&entry, 60, false).contains("file.txt"));
}

#[test]
fn test_format_entry_line_selected() {
    let mut entry = create_test_entry("file.txt", false, false, false);
    entry.selected = true;
    assert!(entry_line(&entry, 60, false).starts_with('*'));
}

#[test]
fn test_format_entry_line_with_permissions() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = entry_line(&entry, 60, true);
    assert!(result.contains("file.txt"));
    assert!(result.contains("r"));
}

#[test]
fn test_format_entry_line_tiny_width_renders_only_marker() {
    // At width 0 or 1 there is only room for the 1-column selection marker:
    // a space for an unselected entry, '*' for a selected one.
    let unselected = create_test_entry("file.txt", false, false, false);
    let mut selected = create_test_entry("file.txt", false, false, false);
    selected.selected = true;

    assert_eq!(entry_line(&unselected, 0, false), " ");
    assert_eq!(entry_line(&unselected, 1, false), " ");
    assert_eq!(entry_line(&selected, 0, false), "*");
    assert_eq!(entry_line(&selected, 1, false), "*");
}

#[test]
fn test_format_entry_line_width_two() {
    // Width 2 leaves the marker plus a single ellipsis cell for the name.
    let entry = create_test_entry("file.txt", false, false, false);
    assert_eq!(entry_line(&entry, 2, false), " …");
}

#[test]
fn test_format_brief_entry_line_basic() {
    let entry = create_test_entry("file.txt", false, false, false);
    assert!(brief_line(&entry, 60).contains("file.txt"));
}

#[test]
fn test_format_brief_entry_line_selected() {
    let mut entry = create_test_entry("file.txt", false, false, false);
    entry.selected = true;
    assert!(brief_line(&entry, 60).starts_with('*'));
}

#[test]
fn test_format_brief_entry_line_truncation() {
    let entry = create_test_entry(
        "very_long_filename_that_should_be_truncated.txt",
        false,
        false,
        false,
    );
    let result = brief_line(&entry, 30);
    assert!(result.contains('…'));
    assert!(UnicodeWidthStr::width(result.as_str()) <= 30);
}

#[test]
fn test_format_brief_entry_line_tiny_width_renders_only_marker() {
    // Same contract as the long-mode line: widths 0 and 1 render just the marker.
    let unselected = create_test_entry("file.txt", false, false, false);
    let mut selected = create_test_entry("file.txt", false, false, false);
    selected.selected = true;

    assert_eq!(brief_line(&unselected, 0), " ");
    assert_eq!(brief_line(&unselected, 1), " ");
    assert_eq!(brief_line(&selected, 0), "*");
    assert_eq!(brief_line(&selected, 1), "*");
}

#[test]
fn test_get_file_icon_directory() {
    let entry = create_test_entry("mydir", true, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "📁"
    );
}

#[test]
fn test_get_file_icon_document() {
    let entry = create_test_entry("report.pdf", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "📝"
    );
}

#[test]
fn test_get_file_icon_archive() {
    let entry = create_test_entry("backup.tar.gz", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "📦"
    );
}

#[test]
fn test_get_file_icon_image() {
    let entry = create_test_entry("photo.jpg", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "🖼"
    );
}

#[test]
fn test_get_file_icon_audio() {
    let entry = create_test_entry("song.mp3", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "🎵"
    );
}

#[test]
fn test_get_file_icon_video() {
    let entry = create_test_entry("movie.mp4", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "🎬"
    );
}

#[test]
fn test_get_file_icon_config() {
    let entry = create_test_entry("config.toml", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "⚙"
    );
}

#[test]
fn test_get_file_icon_code() {
    let entry = create_test_entry("main.rs", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "💻"
    );
}

#[test]
fn test_get_file_icon_default() {
    let entry = create_test_entry("unknown.xyz", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "📄"
    );
}

#[test]
fn test_get_file_icon_symlink() {
    let entry = create_test_entry("link", false, false, true);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "🔗"
    );
}

#[test]
fn test_get_file_icon_executable() {
    let entry = create_test_entry("mybinary", false, true, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "⚡"
    );
}

#[test]
fn test_get_file_icon_font() {
    let entry = create_test_entry("font.ttf", false, false, false);
    assert_eq!(
        get_file_icon_with_theme(&entry.category(), IconTheme::Emoji),
        "🔤"
    );
}

#[test]
fn test_get_file_icon_ascii_theme() {
    let cases: &[(FileCategory, &str)] = &[
        (FileCategory::Dir, "D"),
        (FileCategory::Symlink, "@"),
        (FileCategory::Executable, "*"),
        (FileCategory::Code, "{"),
        (FileCategory::Config, "#"),
        (FileCategory::Archive, "A"),
        (FileCategory::Image, "I"),
        (FileCategory::Video, "V"),
        (FileCategory::Audio, "~"),
        (FileCategory::Document, "="),
        (FileCategory::Font, "F"),
        (FileCategory::Other, "."),
    ];
    for &(cat, expected) in cases {
        assert_eq!(
            get_file_icon_with_theme(&cat, IconTheme::Ascii),
            expected,
            "Ascii icon mismatch for {cat:?}"
        );
    }
}

#[test]
fn test_get_file_icon_nerdfont_theme() {
    let cases: &[(FileCategory, &str)] = &[
        (FileCategory::Dir, "\u{F07B}"),
        (FileCategory::Symlink, "\u{F481}"),
        (FileCategory::Executable, "\u{F489}"),
        (FileCategory::Code, "\u{EAC4}"),
        (FileCategory::Config, "\u{E615}"),
        (FileCategory::Archive, "\u{F410}"),
        (FileCategory::Image, "\u{F03E}"),
        (FileCategory::Video, "\u{F03D}"),
        (FileCategory::Audio, "\u{F001}"),
        (FileCategory::Document, "\u{F15C}"),
        (FileCategory::Font, "\u{F031}"),
        (FileCategory::Other, "\u{F0F6}"),
    ];
    for &(cat, expected) in cases {
        assert_eq!(
            get_file_icon_with_theme(&cat, IconTheme::NerdFont),
            expected,
            "NerdFont icon mismatch for {cat:?}"
        );
    }
}

#[test]
fn test_format_entry_line_truncation() {
    let entry = create_test_entry(
        "very_long_filename_that_should_be_truncated.txt",
        false,
        false,
        false,
    );
    assert!(entry_line(&entry, 47, false).contains('…'));
}

#[test]
fn test_format_entry_line_truncation_handles_unicode() {
    let entry = create_test_entry("日本語テストファイル.txt", false, false, false);
    let result = entry_line(&entry, 47, false);
    assert!(result.contains('…'));
    assert!(UnicodeWidthStr::width(result.as_str()) <= 47);
}

#[test]
fn test_new_panel_state() {
    let panel = PanelState::new(PathBuf::from("/test"));

    assert_eq!(panel.path(), PathBuf::from("/test"));
    assert_eq!(panel.cursor, 0);
}

#[test]
fn test_panel_status_summary_empty_panel() {
    let panel = PanelState::new(PathBuf::from("/test"));
    let mut buf = String::new();
    let width = panel_status_summary(&panel, &mut buf);
    assert_eq!(buf, "");
    assert_eq!(width, 0);
}

#[test]
fn test_panel_status_summary_first_item() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
        create_test_entry("c.txt", false, false, false),
    ]);
    panel.cursor = 0;
    let mut buf = String::new();
    let _ = panel_status_summary(&panel, &mut buf);
    assert_eq!(buf, " 1/3 33% ");
}

#[test]
fn test_panel_status_summary_last_item() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
        create_test_entry("c.txt", false, false, false),
    ]);
    panel.cursor = 2;
    let mut buf = String::new();
    let _ = panel_status_summary(&panel, &mut buf);
    assert_eq!(buf, " 3/3 100% ");
}

#[test]
fn test_panel_status_summary_with_selection() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
    ]);
    panel.cursor = 0;
    panel.set_selected_count(1);
    panel.set_selected_size(1024);
    let mut buf = String::new();
    let _ = panel_status_summary(&panel, &mut buf);
    assert_eq!(buf, " 1/2 50% (1 1.0 KB) ");
}

#[test]
fn test_panel_status_summary_no_selection_when_zero() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![create_test_entry("a.txt", false, false, false)]);
    panel.cursor = 0;
    panel.set_selected_count(0);
    let mut buf = String::new();
    let _ = panel_status_summary(&panel, &mut buf);
    assert_eq!(buf, " 1/1 100% ");
}

#[test]
fn test_truncate_to_width_wide_char_fits_exactly() {
    let result = truncate_to_width("中a", 2);
    assert_eq!(&*result, "中");
    assert_eq!(UnicodeWidthStr::width(&*result), 2);
}

#[test]
fn test_truncate_to_width_wide_char_in_middle_fits_exactly() {
    let result = truncate_to_width("a中b", 3);
    assert_eq!(&*result, "a中");
    assert_eq!(UnicodeWidthStr::width(&*result), 3);
}

#[test]
fn test_truncate_to_width_narrow_chars_use_ellipsis() {
    let result = truncate_to_width("abcde", 3);
    assert_eq!(&*result, "ab…");
    assert_eq!(UnicodeWidthStr::width(&*result), 3);
}

#[test]
fn test_truncate_to_width_wide_char_doesnt_fit() {
    let result = truncate_to_width("a中bc", 3);
    assert_eq!(&*result, "a中");
    assert_eq!(UnicodeWidthStr::width(&*result), 3);
}

#[test]
fn test_truncate_name_no_truncation() {
    assert_eq!(truncate_name("hello", 10), "hello");
}

#[test]
fn test_truncate_name_with_ellipsis() {
    let result = truncate_name("hello world", 8);
    assert!(result.ends_with('…'));
    assert!(UnicodeWidthStr::width(&*result) <= 8);
}

#[test]
fn test_truncate_name_unicode() {
    let result = truncate_name("日本語テストファイル", 6);
    assert_eq!(&*result, "日本語");
    assert_eq!(UnicodeWidthStr::width(&*result), 6);
}

#[test]
fn test_truncate_name_empty() {
    let result = truncate_name("", 5);
    assert_eq!(&*result, "");
}

fn render_to_string(width: u16, height: u16, f: impl FnOnce(&mut ratatui::Frame<'_>)) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(f).unwrap();
    let buffer = terminal.backend().buffer();
    buffer.content().iter().map(|c| c.symbol()).collect()
}

#[test]
fn test_render_panel_no_panic() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![
        create_test_entry("file.txt", false, false, false),
        create_test_entry("mydir", true, false, false),
    ]);
    let content = render_to_string(80, 24, |f| {
        render_panel_with_colors(
            f,
            f.area(),
            &panel,
            true,
            &DEFAULT_COLORS,
            IconTheme::default(),
        );
    });
    assert!(content.contains("file.txt"));
}

#[test]
fn test_render_panel_empty_no_panic() {
    let panel = PanelState::new(PathBuf::from("/test"));
    let content = render_to_string(80, 24, |f| {
        render_panel_with_colors(
            f,
            f.area(),
            &panel,
            false,
            &DEFAULT_COLORS,
            IconTheme::default(),
        );
    });
    assert!(content.contains("/test"));
}

#[test]
fn test_render_status_bar_no_panic() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.set_entries(vec![create_test_entry("file.txt", false, false, false)]);
    let content = render_to_string(80, 2, |f| {
        render_status_bar_with_colors(f, f.area(), &panel, &DEFAULT_COLORS);
    });
    assert!(content.contains("file.txt"));
}

#[test]
fn test_render_function_bar_no_panic() {
    let content = render_to_string(80, 1, |f| {
        render_function_bar_with_colors(f, f.area(), &DEFAULT_COLORS);
    });
    assert!(content.contains("F1"));
}

#[test]
fn test_sanitize_clean_string_unchanged() {
    let input = "hello_world.txt";
    let result = sanitize_for_display(input);
    assert!(matches!(result, Cow::Borrowed("hello_world.txt")));
}

#[test]
fn test_sanitize_newline_replaced() {
    let result = sanitize_for_display("file\nname");
    assert_eq!(&*result, "file⏎name");
}

#[test]
fn test_sanitize_carriage_return_stripped() {
    let result = sanitize_for_display("file\rname");
    assert_eq!(&*result, "filename");
}

#[test]
fn test_sanitize_tab_replaced() {
    let result = sanitize_for_display("file\tname");
    assert_eq!(&*result, "file  name");
}

#[test]
fn test_sanitize_ansi_escape_stripped() {
    // ESC becomes ·; remaining CSI body stays (no full ANSI state machine).
    let result = sanitize_for_display("\x1b[31mred\x1b[0m");
    assert_eq!(&*result, "·[31mred·[0m");
}

#[test]
fn test_sanitize_null_byte_replaced() {
    let result = sanitize_for_display("file\x00name");
    assert_eq!(&*result, "file·name");
}

#[test]
fn test_sanitize_bare_escape_replaced() {
    let result = sanitize_for_display("file\x1bname");
    assert_eq!(&*result, "file·name");
}

#[test]
fn test_sanitize_unicode_preserved() {
    let result = sanitize_for_display("日本語.txt");
    assert!(matches!(result, Cow::Borrowed("日本語.txt")));
}

#[test]
fn test_sanitize_del_replaced() {
    let result = sanitize_for_display("file\x7F");
    assert_eq!(&*result, "file·");
}

#[test]
fn test_sanitize_mixed_control_chars() {
    let result = sanitize_for_display("a\nb\rc\x1b[32md\x00e\tf");
    assert_eq!(&*result, "a⏎bc·[32md·e  f");
}

#[test]
fn compute_category_empty_name_is_other() {
    // An empty name cannot be stored in FileEntry (the builder asserts non-empty),
    // but compute_category itself must still handle it gracefully and return Other.
    use crate::app::types::compute_category;
    use crate::fs::Cha;
    let cha = Cha::regular_file(0);
    assert_eq!(compute_category(&cha, ""), FileCategory::Other);
}
