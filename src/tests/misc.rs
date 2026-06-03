use crate::file_name_str;
use lc::app;

struct IsolatedEnv {
    xdg_config: std::ffi::OsString,
    home: std::ffi::OsString,
}

impl IsolatedEnv {
    fn new(xdg_config: &std::path::Path) -> Self {
        Self {
            xdg_config: xdg_config.as_os_str().to_owned(),
            home: std::ffi::OsString::from("/nonexistent"),
        }
    }
}

impl app::paths::EnvProvider for IsolatedEnv {
    fn var_os(&self, key: &str) -> Option<std::ffi::OsString> {
        match key {
            "XDG_CONFIG_HOME" => Some(self.xdg_config.clone()),
            "HOME" => Some(self.home.clone()),
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
    let tmp = tempfile::tempdir().unwrap();
    let env = IsolatedEnv::new(tmp.path());
    let result = app::config::load_settings_with_env(&env);
    assert!(result.is_ok());
}
