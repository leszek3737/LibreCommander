use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::types::{ActivePanel, AppState, ListingMode, SortMode};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedPanel {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_with_fallback")]
    pub listing_mode: ListingMode,
    #[serde(default, deserialize_with = "deserialize_with_fallback")]
    pub sort_mode: SortMode,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub show_hidden: bool,
}

fn deserialize_with_fallback<'de, T, D>(d: D) -> Result<T, D::Error>
where
    T: serde::Deserialize<'de> + Default,
    D: serde::Deserializer<'de>,
{
    match T::deserialize(d) {
        Ok(v) => Ok(v),
        Err(_) => Ok(T::default()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSetup {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub active_panel: String,
    #[serde(default)]
    pub left: PersistedPanel,
    #[serde(default)]
    pub right: PersistedPanel,
    #[serde(default)]
    pub hotlist: Vec<String>,
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("lc")
            .join("config.toml"),
    )
}

fn panel_to_persisted(panel: &crate::app::types::PanelState) -> PersistedPanel {
    PersistedPanel {
        path: Some(panel.path.display().to_string()),
        listing_mode: panel.listing_mode,
        sort_mode: panel.sort_mode,
        filter: panel.filter.clone().unwrap_or_default(),
        show_hidden: panel.show_hidden,
    }
}

pub fn save_setup(state: &AppState) -> io::Result<PathBuf> {
    let Some(path) = config_path() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "HOME is not set"));
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let setup = PersistedSetup {
        version: 1,
        active_panel: match state.active_panel {
            ActivePanel::Left => "left",
            ActivePanel::Right => "right",
        }
        .to_string(),
        left: panel_to_persisted(&state.left_panel),
        right: panel_to_persisted(&state.right_panel),
        hotlist: state
            .directory_hotlist
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
    };

    let content = toml::to_string_pretty(&setup)
        .map_err(|e| io::Error::other(format!("serialize config: {e}")))?;
    fs::write(&path, content)?;
    Ok(path)
}

pub fn load_setup(state: &mut AppState) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Ok(());
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Failed to read config {}: {e}", path.display())),
    };
    let setup: PersistedSetup = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse config {}: {e}", path.display()))?;

    apply_panel(&mut state.left_panel, &setup.left);
    apply_panel(&mut state.right_panel, &setup.right);
    state.active_panel = match setup.active_panel.as_str() {
        "right" => ActivePanel::Right,
        _ => ActivePanel::Left,
    };
    if !setup.hotlist.is_empty() {
        state.directory_hotlist = setup.hotlist.iter().map(PathBuf::from).collect();
    }
    Ok(())
}

fn apply_panel(panel: &mut crate::app::types::PanelState, persisted: &PersistedPanel) {
    if let Some(ref path_str) = persisted.path {
        let path = PathBuf::from(path_str);
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
}
