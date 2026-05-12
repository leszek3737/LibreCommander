use super::*;
use crate::app::file_type::*;
use ratatui::style::Color;
use std::path::PathBuf;

fn create_test_entry(name: &str, is_dir: bool, is_exec: bool, is_symlink: bool) -> FileEntry {
    FileEntry::builder()
        .name(name)
        .path(name)
        .is_dir(is_dir)
        .is_symlink(is_symlink)
        .is_executable(is_exec)
        .size(1024)
        .modified(SystemTime::now())
        .owner("user")
        .group("group")
        .is_hidden(name.starts_with('.'))
        .build()
}

#[test]
fn test_get_file_color_directory() {
    let entry = create_test_entry("mydir", true, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::White));
    assert!(style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn test_get_file_color_code_script() {
    let entry = create_test_entry("script.sh", false, true, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Yellow));
}

#[test]
fn test_get_file_color_extensionless_executable() {
    let entry = create_test_entry("mybinary", false, true, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Green));
    assert!(style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn test_get_file_color_symlink() {
    let entry = create_test_entry("link", false, false, true);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Cyan));
}

#[test]
fn test_get_file_color_archive() {
    let entry = create_test_entry("archive.tar.gz", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Red));
}

#[test]
fn test_get_file_color_image() {
    let entry = create_test_entry("photo.png", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Magenta));
}

#[test]
fn test_get_file_color_source_code() {
    let entry = create_test_entry("main.rs", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::Yellow));
}

#[test]
fn test_get_file_color_hidden() {
    let entry = create_test_entry(".hidden", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::White));
}

#[test]
fn test_get_file_color_regular() {
    let entry = create_test_entry("unknown.xyz", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::White));
}

#[test]
fn test_get_file_color_document() {
    let entry = create_test_entry("document.txt", false, false, false);
    let style = get_file_color(&entry.category(), entry.is_dir() || entry.is_executable());
    assert_eq!(style.fg, Some(Color::LightYellow));
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
    let result = format_size(1536);
    assert!(result.contains("KB"));
}

#[test]
fn test_format_size_megabytes() {
    let result = format_size(1024 * 1024 * 5);
    assert!(result.contains("MB"));
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
fn test_format_time_current() {
    let time = SystemTime::now();
    let result = format_time(time);
    assert!(result.len() >= 14);
    assert!(result.contains("-"));
    assert!(result.contains(":"));
}

#[test]
fn test_format_time_returns_cow() {
    let time = SystemTime::UNIX_EPOCH;
    let result = format_time(time);
    assert!(matches!(result, Cow::Owned(_)));
}

#[test]
fn test_format_entry_line_basic() {
    let entry = create_test_entry("file.txt", false, false, false);
    let result = format_entry_line(&entry, 60, false, &entry.category());
    assert!(result.contains("file.txt"));
}

#[test]
fn test_format_entry_line_selected() {
    let mut entry = create_test_entry("file.txt", false, false, false);
    entry.selected = true;
    let result = format_entry_line(&entry, 60, false, &entry.category());
    assert!(result.starts_with('*'));
}

#[test]
fn test_get_file_icon_directory() {
    let entry = create_test_entry("mydir", true, false, false);
    assert_eq!(get_file_icon(&entry.category()), "📁 ");
}

#[test]
fn test_get_file_icon_document() {
    let entry = create_test_entry("report.pdf", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "📝 ");
}

#[test]
fn test_get_file_icon_archive() {
    let entry = create_test_entry("backup.tar.gz", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "📦 ");
}

#[test]
fn test_get_file_icon_image() {
    let entry = create_test_entry("photo.jpg", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "🖼 ");
}

#[test]
fn test_get_file_icon_audio() {
    let entry = create_test_entry("song.mp3", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "🎵 ");
}

#[test]
fn test_get_file_icon_video() {
    let entry = create_test_entry("movie.mp4", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "🎬 ");
}

#[test]
fn test_get_file_icon_config() {
    let entry = create_test_entry("config.toml", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "⚙ ");
}

#[test]
fn test_get_file_icon_code() {
    let entry = create_test_entry("main.rs", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "💻 ");
}

#[test]
fn test_get_file_icon_default() {
    let entry = create_test_entry("unknown.xyz", false, false, false);
    assert_eq!(get_file_icon(&entry.category()), "📄 ");
}

#[test]
fn test_format_entry_line_truncation() {
    let entry = create_test_entry(
        "very_long_filename_that_should_be_truncated.txt",
        false,
        false,
        false,
    );
    let result = format_entry_line(&entry, 47, false, &entry.category());
    assert!(result.contains('…'));
}

#[test]
fn test_format_entry_line_truncation_handles_unicode() {
    let entry = create_test_entry("日本語テストファイル.txt", false, false, false);
    let result = format_entry_line(&entry, 47, false, &entry.category());
    assert!(result.contains('…'));
    assert!(UnicodeWidthStr::width(result.as_str()) <= 47);
}

#[test]
fn test_panel_state_is_not_send_sync() {
    let panel = PanelState::new(PathBuf::from("/test"));

    assert_eq!(panel.path, PathBuf::from("/test"));
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
    panel.entries = vec![
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
    panel.entries = vec![
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
    panel.entries = vec![
        create_test_entry("a.txt", false, false, false),
        create_test_entry("b.txt", false, false, false),
    ];
    panel.cursor = 0;
    panel.selected_count = 1;
    panel.selected_size = 1024;
    let (summary, _) = panel_status_summary(&panel);
    assert!(summary.contains("Sel: 1"));
    assert!(summary.contains("1.0 KB"));
}

#[test]
fn test_panel_status_summary_no_selection_when_zero() {
    let mut panel = PanelState::new(PathBuf::from("/test"));
    panel.entries = vec![create_test_entry("a.txt", false, false, false)];
    panel.cursor = 0;
    panel.selected_count = 0;
    let (summary, _) = panel_status_summary(&panel);
    assert!(!summary.contains("Sel:"));
}

#[test]
fn test_truncate_name_no_truncation() {
    assert_eq!(truncate_name("hello", 10), "hello");
}

#[test]
fn test_truncate_name_with_ellipsis() {
    let result = truncate_name("hello world", 8);
    assert!(result.ends_with('…'));
    assert!(UnicodeWidthStr::width(result.as_str()) <= 8);
}

#[test]
fn test_truncate_name_unicode() {
    let result = truncate_name("日本語テストファイル", 6);
    assert!(result.ends_with('…'));
    assert!(UnicodeWidthStr::width(result.as_str()) <= 6);
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
