use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

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
    #[serde(default)]
    pub sort_sensitive: bool,
    #[serde(default)]
    pub left: PersistedPanel,
    #[serde(default)]
    pub right: PersistedPanel,
    #[serde(default)]
    pub hotlist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub active_panel: ActivePanel,
    pub dir_first: bool,
    pub sort_sensitive: bool,
    pub left: PersistedPanel,
    pub right: PersistedPanel,
    pub hotlist: Vec<PathBuf>,
}

impl Settings {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            active_panel: state.active_panel,
            dir_first: state.left_panel.sort_options.dir_first,
            sort_sensitive: state.left_panel.sort_options.sort_sensitive,
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
            sort_sensitive: self.sort_sensitive,
        };
        state.left_panel.sort_options = sort_opts;
        state.right_panel.sort_options = sort_opts;
        state.directory_hotlist = self.hotlist.clone();
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
            sort_sensitive: settings.sort_sensitive,
            left: settings.left.clone(),
            right: settings.right.clone(),
            hotlist: settings
                .hotlist
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
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
            sort_sensitive: setup.sort_sensitive,
            left: setup.left,
            right: setup.right,
            hotlist: setup
                .hotlist
                .iter()
                .map(|s| crate::fs::path::clean_path(&crate::fs::path::expand_path(s)))
                .collect(),
        }
    }
}

fn panel_to_persisted(panel: &PanelState) -> PersistedPanel {
    PersistedPanel {
        path: Some(panel.path.display().to_string()),
        listing_mode: panel.listing_mode,
        sort_mode: panel.sort_mode,
        filter: panel.filter.clone().unwrap_or_default(),
        show_hidden: panel.show_hidden,
        show_permissions: panel.show_permissions,
    }
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

pub fn load_setup(state: &mut AppState) -> Result<(), String> {
    if let Some(settings) = load_settings()? {
        settings.apply_to_state(state);
    }
    Ok(())
}

pub fn load_settings() -> Result<Option<Settings>, String> {
    let Some(path) = paths::config_file_path() else {
        return Ok(None);
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to read config {}: {e}", path.display())),
    };
    let setup: PersistedSetup = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse config {}: {e}", path.display()))?;
    Ok(Some(Settings::from(setup)))
}

fn apply_panel(panel: &mut PanelState, persisted: &PersistedPanel) {
    if let Some(ref path_str) = persisted.path {
        let path = crate::fs::path::clean_path(&crate::fs::path::expand_path(path_str));
        let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if resolved.is_dir() {
            panel.path = resolved;
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
        assert_eq!(
            settings.sort_sensitive,
            state.left_panel.sort_options.sort_sensitive
        );
        assert_eq!(settings.left.path, Some(tmp_dir.display().to_string()));
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
            sort_sensitive: false,
            left: PersistedPanel {
                path: Some(tmp_dir.display().to_string()),
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
        assert!(!state.left_panel.sort_options.sort_sensitive);
        assert!(state.right_panel.sort_options.dir_first);
        assert!(!state.right_panel.sort_options.sort_sensitive);
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
            sort_sensitive: false,
            left: PersistedPanel {
                path: Some("/tmp".to_string()),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::ModTimeDesc,
                filter: "log".to_string(),
                show_hidden: true,
                show_permissions: false,
            },
            right: PersistedPanel::default(),
            hotlist: vec!["/tmp".to_string(), "/usr".to_string()],
        };
        let settings = Settings::from(setup.clone());
        let persisted = PersistedSetup::from(&settings);

        assert_eq!(settings.active_panel, ActivePanel::Right);
        assert!(settings.dir_first);
        assert!(!settings.sort_sensitive);
        assert_eq!(
            settings.hotlist,
            vec![PathBuf::from("/tmp"), PathBuf::from("/usr")]
        );
        assert_eq!(persisted.active_panel, setup.active_panel);
        assert_eq!(persisted.dir_first, setup.dir_first);
        assert_eq!(persisted.sort_sensitive, setup.sort_sensitive);
        assert_eq!(persisted.left, setup.left);
        assert_eq!(persisted.right, setup.right);
        assert_eq!(persisted.hotlist, setup.hotlist);
    }
}
