use crate::*;

#[test]
fn file_name_str_valid_utf8() {
    assert_eq!(
        file_name_str(std::path::Path::new("/home/user/file.txt")),
        Some("file.txt".to_string())
    );
}

#[test]
fn file_name_str_root_returns_none() {
    assert_eq!(file_name_str(std::path::Path::new("/")), None);
}

#[test]
fn file_name_str_non_utf8_returns_lossy() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let bad = OsStr::from_bytes(b"bad\xFFname");
    let path = std::path::Path::new("/tmp").join(bad);
    let result = file_name_str(&path);
    assert!(result.is_some());
}

#[test]
fn config_load_missing_file_ok() {
    let result = app::config::load_settings();
    assert!(result.is_ok());
}
