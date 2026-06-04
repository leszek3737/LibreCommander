use super::*;
use crate::app::file_type::*;
use crate::app::types::format_time;
use crate::app::types::sanitize_for_display;
use ratatui::style::Color;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn test_timestamp() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

fn create_test_entry(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool) -> FileEntry {
    FileEntry::builder()
        .name(name)
        .path(name)
        .is_dir(is_dir)
        .is_symlink(is_symlink)
        .is_executable(is_exec)
        .size(1024)
        .modified(test_timestamp())
        .owner("user")
        .group("group")
        .is_hidden(name.starts_with('.'))
        .build()
}

// TODO: replace with #[rstest] once it's added as a dev-dependency.
macro_rules! test_file_color {
    ($name:ident, $filename:expr, $is_dir:expr, $is_exec:expr, $is_symlink:expr, $expected:expr, bold) => {
        #[test]
        fn $name() {
            let entry = create_test_entry($filename, $is_dir, $is_exec, $is_symlink);
            let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
            assert_eq!(style.fg, Some($expected));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    };
    ($name:ident, $filename:expr, $is_dir:expr, $is_exec:expr, $is_symlink:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let entry = create_test_entry($filename, $is_dir, $is_exec, $is_symlink);
            let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
            assert_eq!(style.fg, Some($expected));
        }
    };
}
// TODO: add negative test — verify get_file_color returns default Color::White
// for files that match no known extension category.

test_file_color!(
    test_get_file_color_directory,
    "mydir",
    true,
    false,
    false,
    Color::White,
    bold
);
test_file_color!(
    test_get_file_color_code_script,
    "script.sh",
    false,
    true,
    false,
    Color::Yellow
);
test_file_color!(
    test_get_file_color_extensionless_executable,
    "mybinary",
    false,
    true,
    false,
    Color::Green,
    bold
);
test_file_color!(
    test_get_file_color_symlink,
    "link",
    false,
    false,
    true,
    Color::Cyan
);
// TODO: currently get_file_color only adds BOLD for directories and executables.
// If symlinks should also be bold, update the `bold` condition from
// `entry.is_dir() || entry.is_executable()` to include `entry.is_symlink()`.
test_file_color!(
    test_get_file_color_archive,
    "archive.tar.gz",
    false,
    false,
    false,
    Color::Red
);
test_file_color!(
    test_get_file_color_image,
    "photo.png",
    false,
    false,
    false,
    Color::Magenta
);
test_file_color!(
    test_get_file_color_source_code,
    "main.rs",
    false,
    false,
    false,
    Color::Yellow
);
test_file_color!(
    test_get_file_color_hidden_as_other,
    ".hidden",
    false,
    false,
    false,
    Color::White
);
test_file_color!(
    test_get_file_color_config,
    "settings.toml",
    false,
    false,
    false,
    Color::LightBlue
);
test_file_color!(
    test_get_file_color_video,
    "movie.mp4",
    false,
    false,
    false,
    Color::LightMagenta
);
test_file_color!(
    test_get_file_color_audio,
    "song.mp3",
    false,
    false,
    false,
    Color::LightGreen
);
test_file_color!(
    test_get_file_color_font,
    "font.ttf",
    false,
    false,
    false,
    Color::LightCyan
);
test_file_color!(
    test_get_file_color_regular,
    "unknown.xyz",
    false,
    false,
    false,
    Color::White
);
test_file_color!(
    test_get_file_color_document,
    "document.txt",
    false,
    false,
    false,
    Color::LightYellow
);

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
fn test_format_permissions_full() {
    let result = format_permissions(0o755);
    assert_eq!(result, "rwxr-xr-x");
}

#[test]
fn test_format_permissions_readonly() {
    let result = format_permissions(0o444);
    assert_eq!(result, "r--r--r--");
}

#[test]
fn test_format_permissions_no_permissions() {
    let result = format_permissions(0o000);
    assert_eq!(result, "---------");
}

#[test]
fn test_is_archive_tar() {
    assert!(is_archive("file.tar"));
    assert!(is_archive("archive.TAR"));
    assert!(is_archive("backup.tar.gz"));
}

#[test]
fn test_is_archive_zip() {
    assert!(is_archive("files.zip"));
    assert!(is_archive("data.7z"));
    assert!(is_archive("backup.rar"));
}

#[test]
fn test_is_archive_negative() {
    assert!(!is_archive("document.txt"));
    assert!(!is_archive("image.png"));
}

#[test]
fn test_is_image_jpg() {
    assert!(is_image("photo.jpg"));
    assert!(is_image("image.JPEG"));
}

#[test]
fn test_is_image_png() {
    assert!(is_image("screenshot.png"));
    assert!(is_image("icon.PNG"));
}

#[test]
fn test_is_image_negative() {
    assert!(!is_image("document.txt"));
    assert!(!is_image("code.rs"));
}

#[test]
fn test_is_source_code_rust() {
    assert!(is_source_code("main.rs"));
    assert!(is_source_code("lib.RS"));
}

#[test]
fn test_is_source_code_python() {
    assert!(is_source_code("script.py"));
    assert!(is_source_code("module.PY"));
}

#[test]
fn test_is_source_code_js() {
    assert!(is_source_code("app.js"));
    assert!(is_source_code("component.ts"));
}

#[test]
fn test_is_source_code_negative() {
    assert!(!is_source_code("image.png"));
    assert!(!is_source_code("data.ini"));
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
    let result = format_entry_line(&entry, 60, false, &entry.category(), IconTheme::Emoji);
    assert!(result.contains("file.txt"));
}

#[test]
fn test_format_entry_line_selected() {
    let mut entry = create_test_entry("file.txt", false, false, false);
    entry.selected = true;
    let result = format_entry_line(&entry, 60, false, &entry.category(), IconTheme::Emoji);
    assert!(result.starts_with('*'));
}

#[test]
fn test_format_entry_line_with_permissions() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_entry_line(&entry, 60, true, &entry.category(), IconTheme::Emoji);
    assert!(result.contains("file.txt"));
    assert!(result.contains("r"));
}

#[test]
fn test_format_entry_line_width_zero() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_entry_line(&entry, 0, false, &entry.category(), IconTheme::Emoji);
    assert_eq!(result, " ");
}

#[test]
fn test_format_entry_line_width_one() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_entry_line(&entry, 1, false, &entry.category(), IconTheme::Emoji);
    assert_eq!(result, " ");
}

#[test]
fn test_format_entry_line_width_two() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_entry_line(&entry, 2, false, &entry.category(), IconTheme::Emoji);
    assert_eq!(result, " …");
}

#[test]
fn test_format_brief_entry_line_basic() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_brief_entry_line(&entry, 60, &entry.category(), IconTheme::Emoji);
    assert!(result.contains("file.txt"));
}

#[test]
fn test_format_brief_entry_line_selected() {
    let mut entry = create_test_entry("file.txt", false, false, false);
    entry.selected = true;
    let result = format_brief_entry_line(&entry, 60, &entry.category(), IconTheme::Emoji);
    assert!(result.starts_with('*'));
}

#[test]
fn test_format_brief_entry_line_truncation() {
    let entry = create_test_entry(
        "very_long_filename_that_should_be_truncated.txt",
        false,
        false,
        false,
    );
    let result = format_brief_entry_line(&entry, 30, &entry.category(), IconTheme::Emoji);
    assert!(result.contains('…') || UnicodeWidthStr::width(result.as_str()) <= 30);
}

#[test]
fn test_format_brief_entry_line_width_zero() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_brief_entry_line(&entry, 0, &entry.category(), IconTheme::Emoji);
    assert_eq!(result, " ");
}

#[test]
fn test_format_brief_entry_line_width_one() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_brief_entry_line(&entry, 1, &entry.category(), IconTheme::Emoji);
    assert_eq!(result, " ");
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

// TODO: add tests for IconTheme::None — verify that NerdFont and Plain icons
// are returned as expected for each FileCategory when theme is None.

#[test]
fn test_format_entry_line_truncation() {
    let entry = create_test_entry(
        "very_long_filename_that_should_be_truncated.txt",
        false,
        false,
        false,
    );
    let result = format_entry_line(&entry, 47, false, &entry.category(), IconTheme::Emoji);
    assert!(result.contains('…'));
}

#[test]
fn test_format_entry_line_truncation_handles_unicode() {
    let entry = create_test_entry("日本語テストファイル.txt", false, false, false);
    let result = format_entry_line(&entry, 47, false, &entry.category(), IconTheme::Emoji);
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
    let (summary, width) = panel_status_summary(&panel);
    assert_eq!(summary, "");
    assert_eq!(width, 0);
}

#[test]
fn test_panel_status_summary_first_item() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
        create_test_entry("c.txt", false, false, false),
    ];
    panel.cursor = 0;
    let (summary, _) = panel_status_summary(&panel);
    assert!(summary.contains("1/3"));
    assert!(summary.contains("33%"));
}

#[test]
fn test_panel_status_summary_last_item() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
        create_test_entry("c.txt", false, false, false),
    ];
    panel.cursor = 2;
    let (summary, _) = panel_status_summary(&panel);
    assert!(summary.contains("3/3"));
    assert!(summary.contains("100%"));
}

#[test]
fn test_panel_status_summary_with_selection() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
    ];
    panel.cursor = 0;
    panel.set_selected_count(1);
    panel.set_selected_size(1024);
    let (summary, _) = panel_status_summary(&panel);
    assert!(summary.contains("1/2"));
    assert!(summary.contains("1 1.0 KB"));
}

#[test]
fn test_panel_status_summary_no_selection_when_zero() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![create_test_entry("a.txt", false, false, false)];
    panel.cursor = 0;
    panel.set_selected_count(0);
    let (summary, _) = panel_status_summary(&panel);
    assert!(!summary.contains("Sel:"));
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
fn test_format_size_one_byte() {
    assert_eq!(format_size(1), "1 B");
}

#[test]
fn test_format_size_just_under_1kb() {
    assert_eq!(format_size(1023), "1023 B");
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
    panel.listing.entries = vec![
        create_test_entry("file.txt", false, false, false),
        create_test_entry("mydir", true, false, false),
    ];
    let content = render_to_string(80, 24, |f| {
        render_panel(f, f.area(), &panel, true);
    });
    assert!(content.contains("file.txt"));
}

#[test]
fn test_render_panel_empty_no_panic() {
    let panel = PanelState::new(PathBuf::from("/test"));
    let content = render_to_string(80, 24, |f| {
        render_panel(f, f.area(), &panel, false);
    });
    assert!(content.contains("/test"));
}

#[test]
fn test_render_status_bar_no_panic() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.listing.entries = vec![create_test_entry("file.txt", false, false, false)];
    let content = render_to_string(80, 2, |f| {
        render_status_bar(f, f.area(), &panel);
    });
    assert!(content.contains("file.txt"));
}

#[test]
fn test_render_function_bar_no_panic() {
    let content = render_to_string(80, 1, |f| {
        render_function_bar(f, f.area());
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
    let result = sanitize_for_display("\x1b[31mred\x1b[0m");
    assert_eq!(&*result, "red");
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
    assert_eq!(&*result, "a⏎bcd·e  f");
}
