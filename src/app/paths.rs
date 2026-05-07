use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

const APP_NAME: &str = "lc";

pub trait EnvProvider {
    fn var_os(&self, key: &str) -> Option<OsString>;
}

pub struct ProcessEnv;

impl EnvProvider for ProcessEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}

#[derive(Default)]
pub struct MapEnv {
    values: HashMap<String, OsString>,
}

impl MapEnv {
    pub fn new(values: &[(&str, &OsStr)]) -> Self {
        Self {
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_os_string()))
                .collect(),
        }
    }
}

impl EnvProvider for MapEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.values.get(key).cloned()
    }
}

pub fn config_file_path() -> Option<PathBuf> {
    config_file_path_with_env(&ProcessEnv)
}

pub fn config_file_path_with_env(env: &impl EnvProvider) -> Option<PathBuf> {
    config_home(env).map(|dir| dir.join("config.toml"))
}

pub fn user_menu_path() -> Option<PathBuf> {
    user_menu_path_with_env(&ProcessEnv)
}

pub fn user_menu_path_with_env(env: &impl EnvProvider) -> Option<PathBuf> {
    config_home(env).map(|dir| dir.join("menu"))
}

pub fn terminal_state_file_path() -> PathBuf {
    terminal_state_file_path_with_env(&ProcessEnv)
}

pub fn terminal_state_file_path_with_env(env: &impl EnvProvider) -> PathBuf {
    cache_home(env)
        .map(|dir| dir.join("terminal_state"))
        .unwrap_or_else(|| std::env::temp_dir().join("lc_terminal_state"))
}

fn config_home(env: &impl EnvProvider) -> Option<PathBuf> {
    env.var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|dir| dir.join(APP_NAME))
        .or_else(|| {
            env.var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config").join(APP_NAME))
        })
}

pub(crate) fn cache_home(env: &impl EnvProvider) -> Option<PathBuf> {
    env.var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|dir| dir.join(APP_NAME))
        .or_else(|| {
            env.var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache").join(APP_NAME))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn env(values: &[(&str, &str)]) -> MapEnv {
        let values: Vec<(&str, &OsStr)> = values
            .iter()
            .map(|(key, value)| (*key, OsStr::new(value)))
            .collect();
        MapEnv::new(&values)
    }

    #[test]
    fn config_path_uses_xdg_config_home() {
        let env = env(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/user")]);

        assert_eq!(
            config_file_path_with_env(&env),
            Some(PathBuf::from("/xdg/config/lc/config.toml"))
        );
    }

    #[test]
    fn config_path_falls_back_to_home_config() {
        let env = env(&[("HOME", "/home/user")]);

        assert_eq!(
            config_file_path_with_env(&env),
            Some(PathBuf::from("/home/user/.config/lc/config.toml"))
        );
    }

    #[test]
    fn user_menu_path_uses_xdg_config_home() {
        let env = env(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/user")]);

        assert_eq!(
            user_menu_path_with_env(&env),
            Some(PathBuf::from("/xdg/config/lc/menu"))
        );
    }

    #[test]
    fn terminal_state_path_uses_xdg_cache_home() {
        let env = env(&[("XDG_CACHE_HOME", "/xdg/cache"), ("HOME", "/home/user")]);

        assert_eq!(
            terminal_state_file_path_with_env(&env),
            PathBuf::from("/xdg/cache/lc/terminal_state")
        );
    }

    #[test]
    fn terminal_state_path_falls_back_to_home_cache() {
        let env = env(&[("HOME", "/home/user")]);

        assert_eq!(
            terminal_state_file_path_with_env(&env),
            PathBuf::from("/home/user/.cache/lc/terminal_state")
        );
    }

    #[test]
    fn terminal_state_path_falls_back_to_temp_when_no_env() {
        let env = MapEnv::default();

        assert_eq!(
            terminal_state_file_path_with_env(&env),
            std::env::temp_dir().join("lc_terminal_state")
        );
    }
}
