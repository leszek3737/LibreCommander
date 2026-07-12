use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::paths;
use crate::app::types::{ActivePanel, AppState, ListingMode, PanelState, SortMode, SortOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPanel {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_with_fallback")]
    pub listing_mode: ListingMode,
    #[serde(default, deserialize_with = "deserialize_with_fallback")]
    pub sort_mode: SortMode,
    #[serde(default)]
    pub filter: String,
    #[serde(default = "default_true")]
    pub show_hidden: bool,
    #[serde(default)]
    pub show_permissions: bool,
}

// Hand-written so the `Default` (used when a whole `[left]`/`[right]` table is
// absent) agrees with the per-field `#[serde(default = "default_true")]` and the
// runtime `PanelState` default. A derived `Default` would make `show_hidden`
// false for a missing table but true for a present table with the field omitted.
impl Default for PersistedPanel {
    fn default() -> Self {
        Self {
            path: None,
            listing_mode: ListingMode::default(),
            sort_mode: SortMode::default(),
            filter: String::new(),
            show_hidden: true,
            show_permissions: false,
        }
    }
}

fn default_true() -> bool {
    true
}

// Falls back to `T::default()` on invalid config values, logging via debug_log.
// This runs during deserialization — in a TUI app eprintln! would corrupt
// the alternate screen buffer. debug_log writes to a file instead. Generic over
// the field type so every fallible persisted field shares one implementation.
fn deserialize_with_fallback<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    T::deserialize(d).or_else(|_| {
        crate::debug_log!(
            "config: invalid value for {}, using default",
            std::any::type_name::<T>()
        );
        Ok(T::default())
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSetup {
    #[serde(default)]
    pub active_panel: String,
    #[serde(default = "default_true")]
    pub dir_first: bool,
    #[serde(default, alias = "sort_sensitive")]
    pub sensitive: bool,
    #[serde(default)]
    pub left: PersistedPanel,
    #[serde(default)]
    pub right: PersistedPanel,
    #[serde(default)]
    pub hotlist: Option<Vec<String>>,
}

// Settings and PersistedSetup form a parallel type pair:
//   PersistedSetup/PersistedPanel — serde-shaped, owned strings, optional hotlist.
//   Settings                       — runtime-shaped, enum active_panel, Vec<PathBuf> hotlist.
// The duplication is intentional: PersistedSetup mirrors the TOML schema (strings,
// optional fields), while Settings uses domain types for application logic.
// Conversion between them (From impls) handles mapping and validation.
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
        let sort_options = state.panel(state.active_panel).sort_options();
        Self {
            active_panel: state.active_panel,
            dir_first: sort_options.dir_first,
            sensitive: sort_options.sensitive,
            left: panel_to_persisted(&state.left_panel),
            right: panel_to_persisted(&state.right_panel),
            hotlist: state.ui.directory_hotlist.clone(),
        }
    }

    pub fn apply_to_state(self, state: &mut AppState) {
        apply_panel(&mut state.left_panel, &self.left);
        apply_panel(&mut state.right_panel, &self.right);
        state.active_panel = self.active_panel;
        let sort_opts = SortOptions {
            dir_first: self.dir_first,
            sensitive: self.sensitive,
        };
        state.left_panel.set_sort_options(sort_opts);
        state.right_panel.set_sort_options(sort_opts);
        state.hotlist_set(self.hotlist);
    }
}

impl From<&Settings> for PersistedSetup {
    fn from(settings: &Settings) -> Self {
        Self {
            active_panel: active_panel_to_wire(settings.active_panel).to_string(),
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
            active_panel: active_panel_from_wire(&setup.active_panel),
            dir_first: setup.dir_first,
            sensitive: setup.sensitive,
            left: setup.left,
            right: setup.right,
            hotlist: canonicalize_hotlist(&setup.hotlist.unwrap_or_default()),
        }
    }
}

/// Maps an [`ActivePanel`] to its persisted wire string.
fn active_panel_to_wire(panel: ActivePanel) -> &'static str {
    match panel {
        ActivePanel::Left => "left",
        ActivePanel::Right => "right",
    }
}

/// Parses a persisted `active_panel` string into [`ActivePanel`], defaulting to
/// `Left` (and logging) on any unrecognized non-empty value. Inverse of
/// [`active_panel_to_wire`].
fn active_panel_from_wire(s: &str) -> ActivePanel {
    if s.eq_ignore_ascii_case("right") {
        ActivePanel::Right
    } else if s.eq_ignore_ascii_case("left") {
        ActivePanel::Left
    } else {
        if !s.is_empty() {
            crate::debug_log!("config: invalid active_panel value '{s}', using default Left");
        }
        ActivePanel::Left
    }
}

/// Resolves persisted hotlist strings to canonical paths. Each unique cleaned
/// path is canonicalized at most once — the `fs::canonicalize` syscall result is
/// cached so repeated entries skip redundant I/O. Empty/whitespace strings are
/// dropped, and duplicates (including distinct inputs resolving to the same
/// canonical path) are collapsed to first occurrence, matching the runtime
/// `hotlist_push` API which rejects duplicates.
fn canonicalize_hotlist(raw: &[String]) -> Vec<PathBuf> {
    let mut cache: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    raw.iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let path = crate::fs::path::clean_path(&crate::fs::path::expand_path(s));
            cache
                .entry(path.clone())
                .or_insert_with(|| fs::canonicalize(&path).unwrap_or_else(|_| path.clone()))
                .clone()
        })
        .filter(|canonical| seen.insert(canonical.clone()))
        .collect()
}

fn panel_to_persisted(panel: &PanelState) -> PersistedPanel {
    PersistedPanel {
        path: path_to_utf8_string(panel.path()),
        listing_mode: panel.listing_mode(),
        sort_mode: panel.sort_mode(),
        filter: panel.filter().unwrap_or("").to_string(),
        show_hidden: panel.show_hidden(),
        show_permissions: panel.show_permissions(),
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

    // Follow a symlinked config to its target so the atomic replace updates the
    // pointed-to file rather than clobbering the symlink with a regular file.
    let target = resolve_symlink_target(&path);

    // Stage the temp file in the target's own directory so `rename` stays on a
    // single filesystem (and is therefore atomic). Remove any stale leftover
    // first (crash residue or a pre-planted symlink), then `create_new` so the
    // open can neither truncate an existing file nor follow a symlink —
    // matching the archive handlers' temp-file hardening.
    let temp_path = target.with_extension("toml.tmp");
    let _ = fs::remove_file(&temp_path);
    {
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        // Config can hold user data: keep it private. Preserve an existing
        // file's mode, otherwise default new files to 0600.
        apply_config_permissions(&target, &f)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&temp_path, &target)?;
    // Flush the directory entry so the rename survives a crash/power loss that
    // the success return value would otherwise have promised through.
    sync_parent_dir(&target);
    Ok(path)
}

/// Resolve a (possibly relative) symlink at `path` to its target one level deep,
/// so we write through the link instead of replacing it. Non-symlinks and
/// unreadable links resolve to `path` itself.
fn resolve_symlink_target(path: &Path) -> PathBuf {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match fs::read_link(path) {
            Ok(target) if target.is_absolute() => target,
            Ok(target) => path
                .parent()
                .map(|parent| parent.join(&target))
                .unwrap_or(target),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Give the staged config file 0600, or the existing target's mode if it already
/// exists, so an atomic replace never widens permissions. No-op off Unix.
#[cfg(unix)]
fn apply_config_permissions(target: &Path, file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(target)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o600);
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn apply_config_permissions(_target: &Path, _file: &File) -> io::Result<()> {
    Ok(())
}

/// Best-effort fsync of the directory holding `target` so a completed rename is
/// durable. No-op off Unix (where directory fsync is neither portable nor
/// required for the atomic-replace semantics `rename` already provides).
#[cfg(unix)]
fn sync_parent_dir(target: &Path) {
    if let Some(parent) = target.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent_dir(_target: &Path) {}

pub fn load_setup(state: &mut AppState) -> Result<Option<toml::Value>, String> {
    let Some(raw) = read_config_raw_with_env(&paths::ProcessEnv)? else {
        return Ok(None);
    };
    // clone() is required because toml::Value only implements IntoDeserializer
    // for owned values, so we cannot deserialize from a reference.
    let setup: PersistedSetup = raw
        .clone()
        .try_into()
        .map_err(|e| format!("Failed to parse config: {e}"))?;
    Settings::from(setup).apply_to_state(state);
    Ok(Some(raw))
}

pub fn load_settings() -> Result<Option<Settings>, String> {
    load_settings_with_env(&paths::ProcessEnv)
}

pub fn load_settings_with_env(env: &impl paths::EnvProvider) -> Result<Option<Settings>, String> {
    let Some(raw) = read_config_raw_with_env(env)? else {
        return Ok(None);
    };
    let setup: PersistedSetup = raw
        .try_into()
        .map_err(|e| format!("Failed to parse config: {e}"))?;
    Ok(Some(Settings::from(setup)))
}

fn read_config_raw_with_env(env: &impl paths::EnvProvider) -> Result<Option<toml::Value>, String> {
    let Some(path) = paths::config_file_path_with_env(env) else {
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
    if let Some(path_str) = persisted.path.as_deref()
        && let Some((path, canonical)) = resolve_persisted_path(path_str)
    {
        panel.set_path(path);
        panel.set_canonical_path(canonical);
    }
    panel.set_listing_mode(persisted.listing_mode);
    panel.set_sort_mode(persisted.sort_mode);
    panel.set_filter(if persisted.filter.trim().is_empty() {
        None
    } else {
        Some(persisted.filter.clone())
    });
    panel.set_show_hidden(persisted.show_hidden);
    panel.set_show_permissions(persisted.show_permissions);
}

/// Resolves a persisted panel path into the `(path, canonical)` pair to assign,
/// or `None` when the configured path is unusable (not a directory). Prefers the
/// canonicalized path; on canonicalize failure it falls back to the cleaned raw
/// path when that is a directory.
fn resolve_persisted_path(path_str: &str) -> Option<(PathBuf, Option<PathBuf>)> {
    let path = crate::fs::path::clean_path(&crate::fs::path::expand_path(path_str));
    match fs::canonicalize(&path) {
        Ok(canonical) if canonical.is_dir() => Some((canonical.clone(), Some(canonical))),
        Ok(_) => None,
        Err(_) => {
            crate::debug_log!(
                "config: canonicalize failed for {}, falling back to raw path",
                path.display()
            );
            if path.is_dir() {
                Some((path, None))
            } else {
                crate::debug_log!("configured panel path ignored: {}", path.display());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{Direction, ListingMode, SortField, SortMode};

    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn settings_from_state_captures_persisted_fields() {
        let tmp_dir = std::env::temp_dir();
        let mut state = AppState {
            active_panel: ActivePanel::Right,
            left_panel: PanelState {
                path: tmp_dir.clone(),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::new(SortField::Size, Direction::Desc),
                filter: Some("rs".to_string()),
                show_hidden: false,
                ..PanelState::new(tmp_dir.clone())
            },
            ..AppState::default()
        };
        state.hotlist_set(vec![tmp_dir.clone(), PathBuf::from("/usr")]);

        let settings = Settings::from_state(&state);

        assert_eq!(settings.active_panel, ActivePanel::Right);
        assert_eq!(
            settings.dir_first,
            state.panel(state.active_panel).sort_options().dir_first
        );
        assert_eq!(
            settings.sensitive,
            state.panel(state.active_panel).sort_options().sensitive
        );
        assert_eq!(settings.left.path, tmp_dir.to_str().map(String::from));
        assert_eq!(settings.left.listing_mode, ListingMode::Brief);
        assert_eq!(
            settings.left.sort_mode,
            SortMode::new(SortField::Size, Direction::Desc)
        );
        assert_eq!(settings.left.filter, "rs");
        assert!(!settings.left.show_hidden);
        assert_eq!(settings.hotlist, state.ui.directory_hotlist);
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
                sort_mode: SortMode::new(SortField::Extension, Direction::Asc),
                filter: "txt".to_string(),
                show_hidden: false,
                show_permissions: false,
            },
            right: PersistedPanel::default(),
            hotlist: vec![tmp_dir.clone(), PathBuf::from("/usr")],
        };
        let hotlist = settings.hotlist.clone();
        settings.apply_to_state(&mut state);

        assert_eq!(state.active_panel, ActivePanel::Right);
        assert!(state.left_panel.sort_options().dir_first);
        assert!(!state.left_panel.sort_options().sensitive);
        assert!(state.right_panel.sort_options().dir_first);
        assert!(!state.right_panel.sort_options().sensitive);
        assert_eq!(
            state.left_panel.path(),
            tmp_dir.canonicalize().unwrap_or(tmp_dir)
        );
        assert_eq!(state.left_panel.listing_mode(), ListingMode::Brief);
        assert_eq!(
            state.left_panel.sort_mode(),
            SortMode::new(SortField::Extension, Direction::Asc)
        );
        assert_eq!(state.left_panel.filter(), Some("txt"));
        assert!(!state.left_panel.show_hidden());
        assert_eq!(state.ui.directory_hotlist, hotlist);
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn persisted_setup_roundtrips_through_settings() {
        let hotlist_path = std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let setup = PersistedSetup {
            active_panel: "right".to_string(),
            dir_first: true,
            sensitive: false,
            left: PersistedPanel {
                path: Some("/tmp".to_string()),
                listing_mode: ListingMode::Brief,
                sort_mode: SortMode::new(SortField::ModTime, Direction::Desc),
                filter: "log".to_string(),
                show_hidden: true,
                show_permissions: false,
            },
            right: PersistedPanel::default(),
            hotlist: Some(vec![hotlist_path.clone()]),
        };
        let settings = Settings::from(setup.clone());
        let persisted = PersistedSetup::from(&settings);

        assert_eq!(settings.active_panel, ActivePanel::Right);
        assert!(settings.dir_first);
        assert!(!settings.sensitive);
        assert_eq!(settings.hotlist, vec![PathBuf::from(hotlist_path)]);
        assert_eq!(persisted.active_panel, setup.active_panel);
        assert_eq!(persisted.dir_first, setup.dir_first);
        assert_eq!(persisted.sensitive, setup.sensitive);
        assert_eq!(persisted.left, setup.left);
        assert_eq!(persisted.right, setup.right);
        assert_eq!(persisted.hotlist, setup.hotlist);
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn persisted_setup_canonicalizes_existing_hotlist_paths() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let dirty_path = nested.join("..").join("nested");
        let setup = PersistedSetup {
            active_panel: String::new(),
            dir_first: false,
            sensitive: false,
            left: PersistedPanel::default(),
            right: PersistedPanel::default(),
            hotlist: Some(vec![dirty_path.to_string_lossy().into_owned()]),
        };

        let settings = Settings::from(setup);

        assert_eq!(settings.hotlist, vec![nested.canonicalize().unwrap()]);
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
