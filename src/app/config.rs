use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::types::{ActivePanel, AppState, ListingMode, SortMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPanel {
    pub path: String,
    pub show_hidden: bool,
    pub listing_mode: String,
    pub sort_mode: String,
    pub filter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSetup {
    pub version: u32,
    pub active_panel: String,
    pub left: PersistedPanel,
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
        path: panel.path.display().to_string(),
        show_hidden: panel.show_hidden,
        listing_mode: match panel.listing_mode {
            ListingMode::Long => "long",
            ListingMode::Brief => "brief",
        }
        .to_string(),
        sort_mode: match panel.sort_mode {
            SortMode::NameAsc => "name_asc",
            SortMode::NameDesc => "name_desc",
            SortMode::ExtensionAsc => "extension_asc",
            SortMode::ExtensionDesc => "extension_desc",
            SortMode::SizeAsc => "size_asc",
            SortMode::SizeDesc => "size_desc",
            SortMode::ModTimeAsc => "mod_time_asc",
            SortMode::ModTimeDesc => "mod_time_desc",
        }
        .to_string(),
        filter: panel.filter.clone().unwrap_or_default(),
    }
}

fn persisted_to_listing_mode(mode: &str) -> ListingMode {
    match mode {
        "brief" => ListingMode::Brief,
        _ => ListingMode::Long,
    }
}

fn persisted_to_sort_mode(mode: &str) -> SortMode {
    match mode {
        "name_desc" => SortMode::NameDesc,
        "extension_asc" => SortMode::ExtensionAsc,
        "extension_desc" => SortMode::ExtensionDesc,
        "size_asc" => SortMode::SizeAsc,
        "size_desc" => SortMode::SizeDesc,
        "mod_time_asc" => SortMode::ModTimeAsc,
        "mod_time_desc" => SortMode::ModTimeDesc,
        _ => SortMode::NameAsc,
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

pub fn load_setup(state: &mut AppState) {
    let Some(path) = config_path() else {
        return;
    };
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(setup) = toml::from_str::<PersistedSetup>(&content) else {
        return;
    };

    apply_panel(&mut state.left_panel, &setup.left);
    apply_panel(&mut state.right_panel, &setup.right);
    state.active_panel = match setup.active_panel.as_str() {
        "right" => ActivePanel::Right,
        _ => ActivePanel::Left,
    };
    if !setup.hotlist.is_empty() {
        state.directory_hotlist = setup.hotlist.iter().map(PathBuf::from).collect();
    }
}

fn apply_panel(panel: &mut crate::app::types::PanelState, persisted: &PersistedPanel) {
    let path = PathBuf::from(&persisted.path);
    if path.is_dir() {
        panel.path = path;
    }
    panel.show_hidden = persisted.show_hidden;
    panel.listing_mode = persisted_to_listing_mode(&persisted.listing_mode);
    panel.sort_mode = persisted_to_sort_mode(&persisted.sort_mode);
    panel.filter = if persisted.filter.trim().is_empty() {
        None
    } else {
        Some(persisted.filter.clone())
    };
}
