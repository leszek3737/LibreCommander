use std::sync::OnceLock;

use ratatui::style::Modifier;
use ratatui::style::{Color, Style};
use serde::Deserialize;

use crate::app::types::FileCategory;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ThemeConfig {
    pub panel_bg: Option<String>,
    pub status_bar_bg: Option<String>,
    pub menu_bar_bg: Option<String>,
    pub dialog_bg: Option<String>,
    pub highlight_bg: Option<String>,
    pub panel_fg: Option<String>,
    pub status_bar_fg: Option<String>,
    pub menu_bar_fg: Option<String>,
    pub dialog_fg: Option<String>,
    pub highlight_fg: Option<String>,
    pub border_active: Option<String>,
    pub border_inactive: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub info: Option<String>,
    pub selected_file_fg: Option<String>,
    pub scrollbar_active: Option<String>,
    pub scrollbar_inactive: Option<String>,
    pub function_bar_fg: Option<String>,
    pub function_bar_bg: Option<String>,
    pub search_match_fg: Option<String>,
    pub search_match_bg: Option<String>,
    pub search_match_current_fg: Option<String>,
    pub search_match_current_bg: Option<String>,
    pub directory: Option<String>,
    pub executable: Option<String>,
    pub symlink: Option<String>,
    pub archive: Option<String>,
    pub image: Option<String>,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub document: Option<String>,
    pub source_code: Option<String>,
    pub config: Option<String>,
    pub font: Option<String>,
    pub regular_file: Option<String>,
}

struct ThemeColors {
    panel_bg: Color,
    status_bar_bg: Color,
    menu_bar_bg: Color,
    dialog_bg: Color,
    highlight_bg: Color,
    panel_fg: Color,
    status_bar_fg: Color,
    menu_bar_fg: Color,
    dialog_fg: Color,
    highlight_fg: Color,
    border_active: Color,
    border_inactive: Color,
    title: Color,
    error: Color,
    warning: Color,
    info: Color,
    selected_file_fg: Color,
    scrollbar_active: Color,
    scrollbar_inactive: Color,
    function_bar_fg: Color,
    function_bar_bg: Color,
    search_match_fg: Color,
    search_match_bg: Color,
    search_match_current_fg: Color,
    search_match_current_bg: Color,
    directory: Color,
    executable: Color,
    symlink: Color,
    archive: Color,
    image: Color,
    video: Color,
    audio: Color,
    document: Color,
    source_code: Color,
    config: Color,
    font: Color,
    regular_file: Color,
}

const DEFAULT_COLORS: ThemeColors = ThemeColors {
    panel_bg: Color::Rgb(0, 0, 128),
    status_bar_bg: Color::Rgb(0, 0, 128),
    menu_bar_bg: Color::Rgb(0, 0, 128),
    dialog_bg: Color::Black,
    highlight_bg: Color::Cyan,
    panel_fg: Color::White,
    status_bar_fg: Color::White,
    menu_bar_fg: Color::White,
    dialog_fg: Color::White,
    highlight_fg: Color::Black,
    border_active: Color::Yellow,
    border_inactive: Color::DarkGray,
    title: Color::LightCyan,
    error: Color::Red,
    warning: Color::Yellow,
    info: Color::Cyan,
    selected_file_fg: Color::LightYellow,
    scrollbar_active: Color::Yellow,
    scrollbar_inactive: Color::DarkGray,
    function_bar_fg: Color::LightBlue,
    function_bar_bg: Color::DarkGray,
    search_match_fg: Color::Black,
    search_match_bg: Color::LightGreen,
    search_match_current_fg: Color::Black,
    search_match_current_bg: Color::Yellow,
    directory: Color::White,
    executable: Color::Green,
    symlink: Color::Cyan,
    archive: Color::Red,
    image: Color::Magenta,
    video: Color::LightMagenta,
    audio: Color::LightGreen,
    document: Color::LightYellow,
    source_code: Color::Yellow,
    config: Color::LightBlue,
    font: Color::LightCyan,
    regular_file: Color::White,
};

static THEME_COLORS: OnceLock<ThemeColors> = OnceLock::new();

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Hex: #RRGGBB or #rgb
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    // Indexed: 0-255
    if let Ok(idx) = s.parse::<u8>() {
        return Some(Color::Indexed(idx));
    }
    // Named colors
    match s.to_ascii_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" | "dark_gray" | "dark_grey" => Some(Color::DarkGray),
        "lightred" | "light_red" => Some(Color::LightRed),
        "lightgreen" | "light_green" => Some(Color::LightGreen),
        "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "lightblue" | "light_blue" => Some(Color::LightBlue),
        "lightmagenta" | "light_magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    }
}

impl ThemeColors {
    fn from_config(cfg: &ThemeConfig) -> Self {
        let resolve = |opt: &Option<String>, fallback: Color| -> Color {
            opt.as_deref().and_then(parse_color).unwrap_or(fallback)
        };
        Self {
            panel_bg: resolve(&cfg.panel_bg, DEFAULT_COLORS.panel_bg),
            status_bar_bg: resolve(&cfg.status_bar_bg, DEFAULT_COLORS.status_bar_bg),
            menu_bar_bg: resolve(&cfg.menu_bar_bg, DEFAULT_COLORS.menu_bar_bg),
            dialog_bg: resolve(&cfg.dialog_bg, DEFAULT_COLORS.dialog_bg),
            highlight_bg: resolve(&cfg.highlight_bg, DEFAULT_COLORS.highlight_bg),
            panel_fg: resolve(&cfg.panel_fg, DEFAULT_COLORS.panel_fg),
            status_bar_fg: resolve(&cfg.status_bar_fg, DEFAULT_COLORS.status_bar_fg),
            menu_bar_fg: resolve(&cfg.menu_bar_fg, DEFAULT_COLORS.menu_bar_fg),
            dialog_fg: resolve(&cfg.dialog_fg, DEFAULT_COLORS.dialog_fg),
            highlight_fg: resolve(&cfg.highlight_fg, DEFAULT_COLORS.highlight_fg),
            border_active: resolve(&cfg.border_active, DEFAULT_COLORS.border_active),
            border_inactive: resolve(&cfg.border_inactive, DEFAULT_COLORS.border_inactive),
            title: resolve(&cfg.title, DEFAULT_COLORS.title),
            error: resolve(&cfg.error, DEFAULT_COLORS.error),
            warning: resolve(&cfg.warning, DEFAULT_COLORS.warning),
            info: resolve(&cfg.info, DEFAULT_COLORS.info),
            selected_file_fg: resolve(&cfg.selected_file_fg, DEFAULT_COLORS.selected_file_fg),
            scrollbar_active: resolve(&cfg.scrollbar_active, DEFAULT_COLORS.scrollbar_active),
            scrollbar_inactive: resolve(&cfg.scrollbar_inactive, DEFAULT_COLORS.scrollbar_inactive),
            function_bar_fg: resolve(&cfg.function_bar_fg, DEFAULT_COLORS.function_bar_fg),
            function_bar_bg: resolve(&cfg.function_bar_bg, DEFAULT_COLORS.function_bar_bg),
            search_match_fg: resolve(&cfg.search_match_fg, DEFAULT_COLORS.search_match_fg),
            search_match_bg: resolve(&cfg.search_match_bg, DEFAULT_COLORS.search_match_bg),
            search_match_current_fg: resolve(
                &cfg.search_match_current_fg,
                DEFAULT_COLORS.search_match_current_fg,
            ),
            search_match_current_bg: resolve(
                &cfg.search_match_current_bg,
                DEFAULT_COLORS.search_match_current_bg,
            ),
            directory: resolve(&cfg.directory, DEFAULT_COLORS.directory),
            executable: resolve(&cfg.executable, DEFAULT_COLORS.executable),
            symlink: resolve(&cfg.symlink, DEFAULT_COLORS.symlink),
            archive: resolve(&cfg.archive, DEFAULT_COLORS.archive),
            image: resolve(&cfg.image, DEFAULT_COLORS.image),
            video: resolve(&cfg.video, DEFAULT_COLORS.video),
            audio: resolve(&cfg.audio, DEFAULT_COLORS.audio),
            document: resolve(&cfg.document, DEFAULT_COLORS.document),
            source_code: resolve(&cfg.source_code, DEFAULT_COLORS.source_code),
            config: resolve(&cfg.config, DEFAULT_COLORS.config),
            font: resolve(&cfg.font, DEFAULT_COLORS.font),
            regular_file: resolve(&cfg.regular_file, DEFAULT_COLORS.regular_file),
        }
    }
}

/// Color theme for the application (Midnight Commander style)
pub struct Theme;

impl Theme {
    // Background colors — kept as pub const for backward compatibility
    pub const PANEL_BG: Color = Color::Rgb(0, 0, 128);
    pub const STATUS_BAR_BG: Color = Color::Rgb(0, 0, 128);
    pub const MENU_BAR_BG: Color = Color::Rgb(0, 0, 128);
    pub const DIALOG_BG: Color = Color::Black;
    pub const HIGHLIGHT_BG: Color = Color::Cyan;

    // Foreground colors
    pub const PANEL_FG: Color = Color::White;
    pub const STATUS_BAR_FG: Color = Color::White;
    pub const MENU_BAR_FG: Color = Color::White;
    pub const DIALOG_FG: Color = Color::White;
    pub const HIGHLIGHT_FG: Color = Color::Black;

    // Special colors
    pub const BORDER_ACTIVE: Color = Color::Yellow;
    pub const BORDER_INACTIVE: Color = Color::DarkGray;
    pub const TITLE: Color = Color::LightCyan;
    pub const ERROR: Color = Color::Red;
    pub const WARNING: Color = Color::Yellow;
    pub const INFO: Color = Color::Cyan;

    // UI element colors
    pub const SELECTED_FILE_FG: Color = Color::LightYellow;
    pub const SCROLLBAR_ACTIVE: Color = Color::Yellow;
    pub const SCROLLBAR_INACTIVE: Color = Color::DarkGray;
    pub const FUNCTION_BAR_FG: Color = Color::LightBlue;
    pub const FUNCTION_BAR_BG: Color = Color::DarkGray;
    pub const SEARCH_MATCH_FG: Color = Color::Black;
    pub const SEARCH_MATCH_BG: Color = Color::LightGreen;
    pub const SEARCH_MATCH_CURRENT_FG: Color = Color::Black;
    pub const SEARCH_MATCH_CURRENT_BG: Color = Color::Yellow;

    // File type colors
    pub const DIRECTORY: Color = Color::White;
    pub const EXECUTABLE: Color = Color::Green;
    pub const SYMLINK: Color = Color::Cyan;
    pub const ARCHIVE: Color = Color::Red;
    pub const IMAGE: Color = Color::Magenta;
    pub const VIDEO: Color = Color::LightMagenta;
    pub const AUDIO: Color = Color::LightGreen;
    pub const DOCUMENT: Color = Color::LightYellow;
    pub const SOURCE_CODE: Color = Color::Yellow;
    pub const CONFIG: Color = Color::LightBlue;
    pub const FONT: Color = Color::LightCyan;
    pub const REGULAR_FILE: Color = Color::White;

    pub fn load_from_config() {
        let cfg = match crate::app::paths::config_file_path() {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let full: toml::Value = match toml::from_str(&content) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    match full.get("theme").cloned() {
                        Some(theme_val) => match ThemeConfig::deserialize(theme_val) {
                            Ok(cfg) => cfg,
                            Err(_) => return,
                        },
                        None => return,
                    }
                }
                Err(_) => return,
            },
            None => return,
        };
        let colors = ThemeColors::from_config(&cfg);
        let _ = THEME_COLORS.set(colors);
    }

    fn colors() -> &'static ThemeColors {
        THEME_COLORS.get().unwrap_or(&DEFAULT_COLORS)
    }

    // Styles

    /// Returns a bg-only `Style` intended for merging with a fg-only style via
    /// Ratatui's `Style::patch`. Used by callers that set border/block backgrounds
    /// independently of foreground: `ui::menu`, `ui::dir_tree`, main panel block.
    pub fn panel_bg() -> Style {
        Style::default().bg(Self::colors().panel_bg)
    }

    /// Returns an fg-only `Style` intended for merging with a bg-only style via
    /// Ratatui's `Style::patch`. Used by `ui::menu` for border styling where
    /// background comes from the container block.
    pub fn panel_fg() -> Style {
        Style::default().fg(Self::colors().panel_fg)
    }

    pub fn panel() -> Style {
        Style::default()
            .fg(Self::colors().panel_fg)
            .bg(Self::colors().panel_bg)
    }

    pub fn status_bar() -> Style {
        Style::default()
            .fg(Self::colors().status_bar_fg)
            .bg(Self::colors().status_bar_bg)
    }

    pub fn menu_bar() -> Style {
        Style::default()
            .fg(Self::colors().menu_bar_fg)
            .bg(Self::colors().menu_bar_bg)
    }

    pub fn dialog() -> Style {
        Style::default()
            .fg(Self::colors().dialog_fg)
            .bg(Self::colors().dialog_bg)
    }

    pub fn highlight() -> Style {
        Style::default()
            .fg(Self::colors().highlight_fg)
            .bg(Self::colors().highlight_bg)
    }

    pub fn highlight_bold() -> Style {
        Self::highlight().add_modifier(Modifier::BOLD)
    }

    pub fn error_dialog() -> Style {
        Style::default()
            .fg(Self::colors().error)
            .bg(Self::colors().dialog_bg)
    }

    /// Style for help dialogs — info color on dialog background.
    /// Also used as the base for `progress_bar()`.
    pub fn help_dialog() -> Style {
        Style::default()
            .fg(Self::colors().info)
            .bg(Self::colors().dialog_bg)
    }

    pub fn warning_dialog() -> Style {
        Style::default()
            .fg(Self::colors().warning)
            .bg(Self::colors().dialog_bg)
    }

    /// Style for progress bars — delegates to `help_dialog()`.
    /// Separate method so the style can diverge independently in future.
    pub fn progress_bar() -> Style {
        Self::help_dialog()
    }

    pub fn selected_error() -> Style {
        Self::highlight()
            .fg(Self::colors().error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn panel_file(color: Color) -> Style {
        Style::default().fg(color).bg(Self::colors().panel_bg)
    }

    pub fn category_color(category: FileCategory) -> Color {
        let c = Self::colors();
        match category {
            FileCategory::Dir => c.directory,
            FileCategory::Archive => c.archive,
            FileCategory::Image => c.image,
            FileCategory::Video => c.video,
            FileCategory::Audio => c.audio,
            FileCategory::Document => c.document,
            FileCategory::Code => c.source_code,
            FileCategory::Config => c.config,
            FileCategory::Font => c.font,
            FileCategory::Executable => c.executable,
            FileCategory::Symlink => c.symlink,
            FileCategory::Other => c.regular_file,
        }
    }

    pub fn panel_item(color: Color, bold: bool) -> Style {
        let style = Self::panel_file(color);
        if bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn border_active() -> Style {
        Style::default().fg(Self::colors().border_active)
    }

    pub fn border_inactive() -> Style {
        Style::default().fg(Self::colors().border_inactive)
    }

    pub fn title() -> Style {
        Style::default().fg(Self::colors().title)
    }

    pub fn error() -> Style {
        Style::default().fg(Self::colors().error)
    }

    pub fn warning() -> Style {
        Style::default().fg(Self::colors().warning)
    }

    pub fn info() -> Style {
        Style::default().fg(Self::colors().info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_color_maps_file_categories_to_theme_colors() {
        let cases = [
            (FileCategory::Dir, Theme::DIRECTORY),
            (FileCategory::Archive, Theme::ARCHIVE),
            (FileCategory::Image, Theme::IMAGE),
            (FileCategory::Video, Theme::VIDEO),
            (FileCategory::Audio, Theme::AUDIO),
            (FileCategory::Document, Theme::DOCUMENT),
            (FileCategory::Code, Theme::SOURCE_CODE),
            (FileCategory::Config, Theme::CONFIG),
            (FileCategory::Font, Theme::FONT),
            (FileCategory::Executable, Theme::EXECUTABLE),
            (FileCategory::Symlink, Theme::SYMLINK),
            (FileCategory::Other, Theme::REGULAR_FILE),
        ];

        for (category, color) in cases {
            assert_eq!(Theme::category_color(category), color);
        }
    }

    #[test]
    fn parse_color_named() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("Blue"), Some(Color::Blue));
        assert_eq!(parse_color("light_cyan"), Some(Color::LightCyan));
        assert_eq!(parse_color("darkgray"), Some(Color::DarkGray));
    }

    #[test]
    fn parse_color_hex() {
        assert_eq!(parse_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_color("#F00"), Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn parse_color_indexed() {
        assert_eq!(parse_color("0"), Some(Color::Indexed(0)));
        assert_eq!(parse_color("128"), Some(Color::Indexed(128)));
        assert_eq!(parse_color("255"), Some(Color::Indexed(255)));
    }

    #[test]
    fn parse_color_invalid() {
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("#GG0000"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn defaults_match_when_no_config() {
        // Without calling load_from_config, colors() returns DEFAULT_COLORS
        let c = Theme::colors();
        assert_eq!(c.panel_bg, Color::Rgb(0, 0, 128));
        assert_eq!(c.directory, Color::White);
        assert_eq!(c.error, Color::Red);
    }

    #[test]
    fn theme_config_from_toml() {
        let cfg = ThemeConfig {
            panel_bg: Some("#000000".to_string()),
            directory: Some("yellow".to_string()),
            error: Some("#FF00FF".to_string()),
            ..Default::default()
        };
        assert_eq!(cfg.panel_bg.as_deref(), Some("#000000"));
        assert_eq!(cfg.directory.as_deref(), Some("yellow"));
        assert_eq!(cfg.error.as_deref(), Some("#FF00FF"));
        assert_eq!(cfg.warning, None);
    }

    #[test]
    fn theme_colors_from_config_overrides() {
        let cfg = ThemeConfig {
            panel_bg: Some("#112233".to_string()),
            directory: Some("cyan".to_string()),
            ..Default::default()
        };
        let colors = ThemeColors::from_config(&cfg);
        assert_eq!(colors.panel_bg, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.directory, Color::Cyan);
        // Unset fields fall back to defaults
        assert_eq!(colors.error, DEFAULT_COLORS.error);
        assert_eq!(colors.panel_fg, DEFAULT_COLORS.panel_fg);
    }
}
