use crate::file_name_str;
use lc::app;

struct IsolatedEnv {
    xdg_config: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
}

/// Wraps a path as the owned `OsString` the env map stores.
fn os(path: &std::path::Path) -> Option<std::ffi::OsString> {
    Some(path.as_os_str().to_owned())
}

impl IsolatedEnv {
    fn new(xdg_config: &std::path::Path) -> Self {
        Self {
            xdg_config: os(xdg_config),
            home: Some(std::ffi::OsString::from("/nonexistent")),
        }
    }

    fn _with_home(home: &str) -> Self {
        Self {
            xdg_config: None,
            home: Some(std::ffi::OsString::from(home)),
        }
    }

    fn empty() -> Self {
        Self {
            xdg_config: None,
            home: None,
        }
    }

    fn xdg(xdg_config: &std::path::Path) -> Self {
        Self {
            xdg_config: os(xdg_config),
            home: None,
        }
    }

    fn var_os(&self, key: &str) -> Option<std::ffi::OsString> {
        match key {
            "XDG_CONFIG_HOME" => self.xdg_config.clone(),
            "HOME" => self.home.clone(),
            _ => None,
        }
    }
}

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

#[cfg(unix)]
#[test]
fn file_name_str_non_utf8_returns_lossy() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    // 0xFF is not valid UTF-8, so to_string_lossy replaces it with U+FFFD.
    let bad = OsStr::from_bytes(b"bad\xFFname");
    let path = std::path::Path::new("/tmp").join(bad);
    let result = file_name_str(&path);
    assert_eq!(result.as_deref(), Some("bad\u{fffd}name"));
}

#[cfg(windows)]
#[test]
fn file_name_str_non_utf8_returns_lossy() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    // 0xD800 is an unpaired high surrogate: invalid UTF-16, so to_string_lossy
    // replaces it with U+FFFD.
    let bad = OsString::from_wide(&[0x0062, 0x0061, 0x0064, 0xD800, 0x006E]);
    let path = std::path::Path::new("C:\\tmp").join(bad);
    let result = file_name_str(&path);
    assert_eq!(result.as_deref(), Some("bad\u{fffd}n"));
}

#[test]
fn config_load_missing_file_ok() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let env = IsolatedEnv::new(tmp.path());
    let result = app::config::load_settings_with_env(|k| env.var_os(k));
    // No config file present -> Ok(None): not an error, and no defaults invented.
    assert_eq!(result.expect("load should succeed"), None);
}

#[test]
fn config_load_invalid_toml() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let config_dir = tmp.path().join("lc");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(config_dir.join("config.toml"), "[[broken toml {{{").expect("write config");
    let env = IsolatedEnv::new(tmp.path());
    let err = app::config::load_settings_with_env(|k| env.var_os(k))
        .expect_err("invalid toml must error");
    assert!(
        err.contains("parse"),
        "error should mention the parse failure: {err}"
    );
}

#[test]
fn config_load_full_toml_parses_all_fields() {
    use lc::app::types::{ActivePanel, Direction, ListingMode, SortField, SortMode};

    let tmp = tempfile::tempdir().expect("tempdir creation");
    let config_dir = tmp.path().join("lc");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let toml = "\
active_panel = \"right\"
dir_first = false
sensitive = true
hotlist = [\"/tmp\"]

[left]
path = \"/tmp\"
listing_mode = \"brief\"
sort_mode = \"size_desc\"
filter = \"rs\"
show_hidden = false
show_permissions = true

[right]
listing_mode = \"long\"
sort_mode = \"name_asc\"
";
    std::fs::write(config_dir.join("config.toml"), toml).expect("write config");
    let env = IsolatedEnv::new(tmp.path());

    let settings = app::config::load_settings_with_env(|k| env.var_os(k))
        .expect("load should succeed")
        .expect("config present");

    assert_eq!(settings.active_panel, ActivePanel::Right);
    assert!(!settings.dir_first);
    // `sensitive` is the canonical field name (alias: `sort_sensitive`).
    assert!(settings.sensitive);
    assert_eq!(settings.left.listing_mode, ListingMode::Brief);
    assert_eq!(
        settings.left.sort_mode,
        SortMode::new(SortField::Size, Direction::Desc)
    );
    assert_eq!(settings.left.filter, "rs");
    assert!(!settings.left.show_hidden);
    assert!(settings.left.show_permissions);
    assert_eq!(settings.right.listing_mode, ListingMode::Long);
    assert_eq!(
        settings.right.sort_mode,
        SortMode::new(SortField::Name, Direction::Asc)
    );
    // "/tmp" exists, so it is canonicalized into a single hotlist entry (the
    // absolute form is platform-dependent, e.g. /private/tmp on macOS).
    assert_eq!(settings.hotlist.len(), 1);
}

#[test]
fn config_fallback_no_xdg_no_home() {
    let env = IsolatedEnv::empty();
    let result = app::config::load_settings_with_env(|k| env.var_os(k));
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn config_xdg_present() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let env = IsolatedEnv::xdg(tmp.path());
    let path = app::paths::config_file_path_with_env(|k| env.var_os(k));
    let expected = tmp.path().join("lc").join("config.toml");
    assert_eq!(path, Some(expected));
}
