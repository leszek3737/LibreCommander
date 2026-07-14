use std::ffi::OsString;
use std::path::PathBuf;

const APP_NAME: &str = "lc";

#[must_use]
pub fn config_file_path() -> Option<PathBuf> {
    config_file_path_with_env(|k| std::env::var_os(k))
}

#[must_use]
pub fn user_menu_path() -> Option<PathBuf> {
    user_menu_path_with_env(|k| std::env::var_os(k))
}

#[must_use]
pub fn terminal_state_file_path() -> Option<PathBuf> {
    terminal_state_file_path_with_env(|k| std::env::var_os(k))
}

#[must_use]
pub fn config_file_path_with_env(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    config_home(&env).map(|dir| dir.join("config.toml"))
}

#[must_use]
pub fn user_menu_path_with_env(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    config_home(&env).map(|dir| dir.join("menu"))
}

#[must_use]
pub fn terminal_state_file_path_with_env(
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    cache_home(&env).map(|dir| dir.join("terminal_state"))
}

fn xdg_dir(
    env: &impl Fn(&str) -> Option<OsString>,
    xdg_key: &str,
    home_subdir: &str,
    platform_fn: fn() -> Option<PathBuf>,
) -> Option<PathBuf> {
    validate_path_env(env, xdg_key)
        .map(|dir| dir.join(APP_NAME))
        .or_else(|| {
            validate_path_env(env, "HOME").map(|home| home.join(home_subdir).join(APP_NAME))
        })
        .or_else(platform_fn)
}

fn validate_path_env(env: &impl Fn(&str) -> Option<OsString>, key: &str) -> Option<PathBuf> {
    env(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn config_home(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    xdg_dir(env, "XDG_CONFIG_HOME", ".config", platform_config_home)
}

#[must_use]
pub(crate) fn cache_home(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    xdg_dir(env, "XDG_CACHE_HOME", ".cache", platform_cache_home)
}

/// Generate a platform fallback for an app directory.
///
/// On Windows, HOME/XDG are often unset, so fall back to the OS-specific dir.
/// On other platforms HOME is always available, so this returns `None`.
macro_rules! platform_home {
    ($name:ident, $dirs_fn:path) => {
        #[cfg(windows)]
        fn $name() -> Option<PathBuf> {
            $dirs_fn().map(|dir| dir.join(APP_NAME))
        }

        #[cfg(not(windows))]
        fn $name() -> Option<PathBuf> {
            None
        }
    };
}

platform_home!(platform_config_home, dirs::config_dir);
platform_home!(platform_cache_home, dirs::cache_dir);

// XDG/HOME semantics under test are Unix-shaped (absolute `/...` paths, no
// platform fallback); on Windows the same inputs are rejected as relative and
// the AppData fallback (tested below) takes over.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_map<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        let map: HashMap<&str, &str> = values.iter().copied().collect();
        move |key| map.get(key).map(|v| OsString::from(*v))
    }

    #[test]
    fn config_path_uses_xdg_config_home() {
        let env = env_map(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/user")]);

        assert_eq!(
            config_file_path_with_env(env),
            Some(PathBuf::from("/xdg/config/lc/config.toml"))
        );
    }

    #[test]
    fn config_path_falls_back_to_home_config() {
        let env = env_map(&[("HOME", "/home/user")]);

        assert_eq!(
            config_file_path_with_env(env),
            Some(PathBuf::from("/home/user/.config/lc/config.toml"))
        );
    }

    #[test]
    fn user_menu_path_uses_xdg_config_home() {
        let env = env_map(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/user")]);

        assert_eq!(
            user_menu_path_with_env(env),
            Some(PathBuf::from("/xdg/config/lc/menu"))
        );
    }

    #[test]
    fn terminal_state_path_uses_xdg_cache_home() {
        let env = env_map(&[("XDG_CACHE_HOME", "/xdg/cache"), ("HOME", "/home/user")]);

        assert_eq!(
            terminal_state_file_path_with_env(env),
            Some(PathBuf::from("/xdg/cache/lc/terminal_state"))
        );
    }

    #[test]
    fn terminal_state_path_falls_back_to_home_cache() {
        let env = env_map(&[("HOME", "/home/user")]);

        assert_eq!(
            terminal_state_file_path_with_env(env),
            Some(PathBuf::from("/home/user/.cache/lc/terminal_state"))
        );
    }

    #[test]
    fn config_path_returns_none_when_home_empty() {
        let env = env_map(&[("HOME", "")]);
        let result = config_file_path_with_env(env);
        assert!(result.is_none());
    }

    #[test]
    fn terminal_state_path_returns_none_when_no_env() {
        let env = env_map(&[]);
        assert_eq!(terminal_state_file_path_with_env(env), None);
    }

    #[test]
    fn config_path_rejects_relative_xdg_config_home() {
        let env = env_map(&[
            ("XDG_CONFIG_HOME", "relative/config"),
            ("HOME", "/home/user"),
        ]);

        assert_eq!(
            config_file_path_with_env(env),
            Some(PathBuf::from("/home/user/.config/lc/config.toml"))
        );
    }

    #[test]
    fn terminal_state_path_rejects_relative_xdg_cache_home() {
        let env = env_map(&[("XDG_CACHE_HOME", "relative/cache"), ("HOME", "/home/user")]);

        assert_eq!(
            terminal_state_file_path_with_env(env),
            Some(PathBuf::from("/home/user/.cache/lc/terminal_state"))
        );
    }

    #[test]
    fn config_path_rejects_relative_home() {
        let env = env_map(&[("HOME", "relative/home")]);
        assert!(config_file_path_with_env(env).is_none());
    }

    #[test]
    fn terminal_state_path_rejects_relative_home() {
        let env = env_map(&[("HOME", "relative/home")]);
        assert_eq!(terminal_state_file_path_with_env(env), None);
    }
}

#[cfg(all(test, windows))]
#[allow(clippy::expect_used)]
mod windows_tests {
    use super::*;

    /// With no HOME/XDG in the environment, Windows falls back to the OS
    /// config dir (AppData) instead of returning None.
    #[test]
    fn config_path_falls_back_to_platform_dir_without_env() {
        let path = config_file_path_with_env(|_: &str| None).expect("AppData fallback");
        assert!(path.ends_with("lc\\config.toml") || path.ends_with("lc/config.toml"));
    }
}
