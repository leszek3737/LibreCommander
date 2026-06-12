use crate::file_name_str;
use lc::app;

struct IsolatedEnv {
    xdg_config: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
}

impl IsolatedEnv {
    fn new(xdg_config: &std::path::Path) -> Self {
        Self {
            xdg_config: Some(xdg_config.as_os_str().to_owned()),
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
            xdg_config: Some(xdg_config.as_os_str().to_owned()),
            home: None,
        }
    }
}

impl app::paths::EnvProvider for IsolatedEnv {
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

#[test]
fn file_name_str_non_utf8_returns_lossy() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let bad = OsStr::from_bytes(b"bad\xFFname");
    let path = std::path::Path::new("/tmp").join(bad);
    let result = file_name_str(&path);
    assert_eq!(result.as_deref(), Some("bad\u{fffd}name"));
}

#[test]
fn config_load_missing_file_ok() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let env = IsolatedEnv::new(tmp.path());
    let result = app::config::load_settings_with_env(&env);
    assert!(result.is_ok());
}

#[test]
fn config_load_invalid_toml() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let config_dir = tmp.path().join("lc");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(config_dir.join("config.toml"), "[[broken toml {{{").expect("write config");
    let env = IsolatedEnv::new(tmp.path());
    let result = app::config::load_settings_with_env(&env);
    assert!(result.is_err());
}

#[test]
fn config_fallback_no_xdg_no_home() {
    let env = IsolatedEnv::empty();
    let result = app::config::load_settings_with_env(&env);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn config_xdg_present() {
    let tmp = tempfile::tempdir().expect("tempdir creation");
    let env = IsolatedEnv::xdg(tmp.path());
    let path = app::paths::config_file_path_with_env(&env);
    let expected = tmp.path().join("lc").join("config.toml");
    assert_eq!(path, Some(expected));
}
