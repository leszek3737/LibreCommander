use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::paths;
use crate::app::types::{ActivePanel, AppState, ListingMode, PanelState, SortMode, SortOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PersistedPanel {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_listing_mode_with_fallback")]
    pub listing_mode: ListingMode,
    #[serde(default, deserialize_with = "deserialize_sort_mode_with_fallback")]
    pub sort_mode: SortMode,
    #[serde(default)]
    pub filter: String,
    #[serde(default = "default_true")]
    pub show_hidden: bool,
    #[serde(default)]
    pub show_permissions: bool,
}

// Falls back to default on invalid config values, logging via debug_log.
// This runs during deserialization — in a TUI app eprintln! would corrupt
// the alternate screen buffer. debug_log writes to a file instead.
fn default_true() -> bool {
    true
}

fn deserialize_listing_mode_with_fallback<'de, D>(d: D) -> Result<ListingMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    ListingMode::deserialize(d).or_else(|_| {
        crate::debug_log!("config: invalid value for listing_mode, using default");
        Ok(ListingMode::default())
    })
}

fn deserialize_sort_mode_with_fallback<'de, D>(d: D) -> Result<SortMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    SortMode::deserialize(d).or_else(|_| {
        crate::debug_log!("config: invalid value for sort_mode, using default");
        Ok(SortMode::default())
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSetup {
    #[serde(default)]
    pub active_panel: String,
    #[serde(default = "default_true")]
    pub dir_first: bool,
    #[serde(default, rename = "sort_sensitive", alias = "sensitive")]
    pub sensitive: bool,
    #[serde(default)]
    pub left: PersistedPanel,
    #[serde(default)]
    pub right: PersistedPanel,
    #[serde(default)]
    pub hotlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub active_panel: ActivePanel,
    pub dir_first: bool,
    pub sensitive: bool,
    pub left: PersistedPanel,
    pub right: PersistedPanel,
    pub hotlist: Vec<PathBuf>,
}

impl Settings {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            active_panel: state.active_panel,
            dir_first: state.left_panel.sort_options.dir_first,
            sensitive: state.left_panel.sort_options.sensitive,
            left: panel_to_persisted(&state.left_panel),
            right: panel_to_persisted(&state.right_panel),
            hotlist: state.directory_hotlist.clone(),
        }
    }

    pub fn apply_to_state(&self, state: &mut AppState) {
        apply_panel(&mut state.left_panel, &self.left);
        apply_panel(&mut state.right_panel, &self.right);
        state.active_panel = self.active_panel;
        let sort_opts = SortOptions {
            dir_first: self.dir_first,
            sensitive: self.sensitive,
        };
        state.left_panel.sort_options = sort_opts;
        state.right_panel.sort_options = sort_opts;
        if !self.hotlist.is_empty() || state.directory_hotlist.is_empty() {
            state.hotlist_set(self.hotlist.clone());
        }
    }
}

impl From<&Settings> for PersistedSetup {
    fn from(settings: &Settings) -> Self {
        Self {
            active_panel: match settings.active_panel {
                ActivePanel::Left => "left",
                ActivePanel::Right => "right",
            }
            .to_string(),
            dir_first: settings.dir_first,
            sensitive: settings.sensitive,
            left: settings.left.clone(),
            right: settings.right.clone(),
            hotlist: Some(paths_to_utf8_strings(&settings.hotlist)),
        }
    }
}

impl From<PersistedSetup> for Settings {
    fn from(setup: PersistedSetup) -> Self {
        Self {
            active_panel: match setup.active_panel.as_str() {
                "right" => ActivePanel::Right,
                _ => ActivePanel::Left,
            },
            dir_first: setup.dir_first,
            sensitive: setup.sensitive,
            left: setup.left,
            right: setup.right,
            hotlist: setup
                .hotlist
                .unwrap_or_default()
                .iter()
                .map(|s| crate::fs::path::clean_path(&crate::fs::path::expand_path(s)))
                .collect(),
        }
    }
}

fn panel_to_persisted(panel: &PanelState) -> PersistedPanel {
    PersistedPanel {
        path: path_to_utf8_string(&panel.path),
        listing_mode: panel.listing_mode,
        sort_mode: panel.sort_mode,
        filter: panel.filter.clone().unwrap_or_default(),
        show_hidden: panel.show_hidden,
        show_permissions: panel.show_permissions,
    }
}

fn path_to_utf8_string(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned).or_else(|| {
        crate::debug_log!("config: skipping non-UTF path {}", path.display());
        None
    })
}

fn paths_to_utf8_strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| path_to_utf8_string(path))
        .collect()
}

pub fn save_setup(state: &AppState) -> io::Result<PathBuf> {
    save_settings(&Settings::from_state(state))
}

pub fn save_settings(settings: &Settings) -> io::Result<PathBuf> {
    let Some(path) = paths::config_file_path() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "HOME is not set"));
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let setup = PersistedSetup::from(settings);

    let content = toml::to_string_pretty(&setup)
        .map_err(|e| io::Error::other(format!("serialize config: {e}")))?;

    let temp_path = path.with_extension("toml.tmp");
    {
        let mut f = File::create(&temp_path)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&temp_path, &path)?;
    Ok(path)
}

pub fn load_setup(state: &mut AppState) -> Result<Option<toml::Value>, String> {
    let Some(raw) = read_config_raw()? else {
        return Ok(None);
    };
    let setup: PersistedSetup = raw
        .clone()
        .try_into()
        .map_err(|e| format!("Failed to parse config: {e}"))?;
    Settings::from(setup).apply_to_state(state);
    Ok(Some(raw))
}

pub fn load_settings() -> Result<Option<Settings>, String> {
    let Some(raw) = read_config_raw()? else {
        return Ok(None);
    };
    let setup: PersistedSetup = raw
        .try_into()
        .map_err(|e| format!("Failed to parse config: {e}"))?;
    Ok(Some(Settings::from(setup)))
}

fn read_config_raw() -> Result<Option<toml::Value>, String> {
    let Some(path) = paths::config_file_path() else {
        return Ok(None);
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to read config {}: {e}", path.display())),
    };
    let value: toml::Value = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse config {}: {e}", path.display()))?;
    Ok(Some(value))
}

fn apply_panel(panel: &mut PanelState, persisted: &PersistedPanel) {
    if let Some(ref path_str) = persisted.path {
        let path = crate::fs::path::clean_path(&crate::fs::path::expand_path(path_str));
        let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if resolved.is_dir() {
            panel.path = resolved.clone();
            panel.canonical_path = Some(resolved);
        }
    }
    panel.listing_mode = persisted.listing_mode;
    panel.sort_mode = persisted.sort_mode;
    panel.filter = if persisted.filter.trim().is_empty() {
        None
    } else {
        Some(persisted.filter.clone())
    };
    panel.show_hidden = persisted.show_hidden;
    panel.show_permissions = persisted.show_permissions;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{ListingMode, SortMode};

    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn settings_from_state_captures_persisted_fields() {
        let tmp_dir = std::env::temp_dir();
        let state = AppState {
            active_panel: ActivePanel::Right,
            directory_hotlist: vec![tmp_dir.clone(), PathBuf::from("/usr")],
            left_panel: PanelState {
                path: tmp_dir.clone(),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::SizeDesc,
                filter: Some("rs".to_string()),
                show_hidden: false,
                ..PanelState::new(tmp_dir.clone())
            },
            ..AppState::default()
        };

        let settings = Settings::from_state(&state);

        assert_eq!(settings.active_panel, ActivePanel::Right);
        assert_eq!(settings.dir_first, state.left_panel.sort_options.dir_first);
        assert_eq!(settings.sensitive, state.left_panel.sort_options.sensitive);
        assert_eq!(settings.left.path, tmp_dir.to_str().map(String::from));
        assert_eq!(settings.left.listing_mode, ListingMode::Brief);
        assert_eq!(settings.left.sort_mode, SortMode::SizeDesc);
        assert_eq!(settings.left.filter, "rs");
        assert!(!settings.left.show_hidden);
        assert_eq!(settings.hotlist, state.directory_hotlist);
    }

    #[test]
    fn settings_apply_to_state_updates_persisted_fields() {
        let tmp_dir = std::env::temp_dir();
        let mut state = AppState::default();
        let settings = Settings {
            active_panel: ActivePanel::Right,
            dir_first: true,
            sensitive: false,
            left: PersistedPanel {
                path: tmp_dir.to_str().map(String::from),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::ExtensionAsc,
                filter: "txt".to_string(),
                show_hidden: false,
                show_permissions: false,
            },
            right: PersistedPanel::default(),
            hotlist: vec![tmp_dir.clone(), PathBuf::from("/usr")],
        };
        settings.apply_to_state(&mut state);

        assert_eq!(state.active_panel, ActivePanel::Right);
        assert!(state.left_panel.sort_options.dir_first);
        assert!(!state.left_panel.sort_options.sensitive);
        assert!(state.right_panel.sort_options.dir_first);
        assert!(!state.right_panel.sort_options.sensitive);
        assert_eq!(
            state.left_panel.path,
            tmp_dir.canonicalize().unwrap_or(tmp_dir)
        );
        assert_eq!(state.left_panel.listing_mode, ListingMode::Brief);
        assert_eq!(state.left_panel.sort_mode, SortMode::ExtensionAsc);
        assert_eq!(state.left_panel.filter, Some("txt".to_string()));
        assert!(!state.left_panel.show_hidden);
        assert_eq!(state.directory_hotlist, settings.hotlist);
    }

    #[test]
    fn persisted_setup_roundtrips_through_settings() {
        let setup = PersistedSetup {
            active_panel: "right".to_string(),
            dir_first: true,
            sensitive: false,
            left: PersistedPanel {
                path: Some("/tmp".to_string()),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::ModTimeDesc,
                filter: "log".to_string(),
                show_hidden: true,
                show_permissions: false,
            },
            right: PersistedPanel::default(),
            hotlist: Some(vec!["/tmp".to_string(), "/usr".to_string()]),
        };
        let settings = Settings::from(setup.clone());
        let persisted = PersistedSetup::from(&settings);

        assert_eq!(settings.active_panel, ActivePanel::Right);
        assert!(settings.dir_first);
        assert!(!settings.sensitive);
        assert_eq!(
            settings.hotlist,
            vec![PathBuf::from("/tmp"), PathBuf::from("/usr")]
        );
        assert_eq!(persisted.active_panel, setup.active_panel);
        assert_eq!(persisted.dir_first, setup.dir_first);
        assert_eq!(persisted.sensitive, setup.sensitive);
        assert_eq!(persisted.left, setup.left);
        assert_eq!(persisted.right, setup.right);
        assert_eq!(persisted.hotlist, setup.hotlist);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_setup_skips_non_utf8_paths() {
        let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xFF]));
        let settings = Settings {
            active_panel: ActivePanel::Left,
            dir_first: true,
            sensitive: false,
            left: PersistedPanel {
                path: path_to_utf8_string(&non_utf8),
                ..PersistedPanel::default()
            },
            right: PersistedPanel::default(),
            hotlist: vec![PathBuf::from("/tmp"), non_utf8],
        };

        let persisted = PersistedSetup::from(&settings);

        assert_eq!(persisted.left.path, None);
        assert_eq!(persisted.hotlist, Some(vec!["/tmp".to_string()]));
    }
}
