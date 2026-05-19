use ratatui::style::Modifier;
use ratatui::style::{Color, Style};
use serde::Deserialize;

use crate::app::types::FileCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub enum IconTheme {
    #[default]
    Emoji,
    Ascii,
    NerdFont,
}

impl IconTheme {
    fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "emoji" => Some(Self::Emoji),
            "ascii" => Some(Self::Ascii),
            "nerdfont" | "nerd_font" | "nerd-font" => Some(Self::NerdFont),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for IconTheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;
        let Some(value) = value.as_str() else {
            crate::debug_log!("config: non-string value for icon_theme, using emoji");
            return Ok(Self::Emoji);
        };
        Ok(Self::from_config_str(value).unwrap_or_else(|| {
            crate::debug_log!("config: invalid value for icon_theme, using emoji");
            Self::Emoji
        }))
    }
}

macro_rules! define_theme_colors {
    ($(($method:ident, $with_method:ident, $field:ident => $color:expr)),* $(,)?) => {
        #[derive(Debug, Clone, Deserialize, Default)]
        #[serde(default)]
        pub struct ThemeConfig {
            $(pub $field: Option<String>,)*
            #[serde(default)]
            pub icon_theme: IconTheme,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub struct ColorPalette {
            $($field: Color,)*
            icon_theme: IconTheme,
        }

        pub const DEFAULT_COLORS: ColorPalette = ColorPalette {
            $($field: $color,)*
            icon_theme: IconTheme::Emoji,
        };

        impl ColorPalette {
            pub fn from_config(cfg: &ThemeConfig) -> Self {
                let mut palette = Self {
                    $($field: cfg.$field
                        .as_ref()
                        .and_then(|s| parse_color(s))
                        .unwrap_or(DEFAULT_COLORS.$field),)*
                    icon_theme: DEFAULT_COLORS.icon_theme,
                };
                palette.icon_theme = cfg.icon_theme;
                palette
            }

            pub fn icon_theme(&self) -> IconTheme {
                self.icon_theme
            }
        }

        impl Default for ColorPalette {
            fn default() -> Self {
                DEFAULT_COLORS
            }
        }

        pub struct Theme;

        impl Theme {
            $(
            pub fn $method() -> Color {
                DEFAULT_COLORS.$field
            }

            pub fn $with_method(colors: &ColorPalette) -> Color {
                colors.$field
            }
            )*
        }
    };
}

define_theme_colors! {
    (panel_bg_color, panel_bg_color_with_colors, panel_bg => Color::Rgb(0, 0, 128)),
    (status_bar_bg, status_bar_bg_with_colors, status_bar_bg => Color::Rgb(0, 0, 128)),
    (menu_bar_bg, menu_bar_bg_with_colors, menu_bar_bg => Color::Rgb(0, 0, 128)),
    (dialog_bg, dialog_bg_with_colors, dialog_bg => Color::Black),
    (highlight_bg, highlight_bg_with_colors, highlight_bg => Color::Cyan),
    (panel_fg_color, panel_fg_color_with_colors, panel_fg => Color::White),
    (status_bar_fg, status_bar_fg_with_colors, status_bar_fg => Color::White),
    (menu_bar_fg, menu_bar_fg_with_colors, menu_bar_fg => Color::White),
    (dialog_fg, dialog_fg_with_colors, dialog_fg => Color::White),
    (highlight_fg, highlight_fg_with_colors, highlight_fg => Color::Black),
    (border_active_color, border_active_color_with_colors, border_active => Color::Yellow),
    (border_inactive_color, border_inactive_color_with_colors, border_inactive => Color::DarkGray),
    (title_color, title_color_with_colors, title => Color::LightCyan),
    (error_color, error_color_with_colors, error => Color::Red),
    (warning_color, warning_color_with_colors, warning => Color::Yellow),
    (info_color, info_color_with_colors, info => Color::Cyan),
    (selected_file_fg, selected_file_fg_with_colors, selected_file_fg => Color::LightYellow),
    (scrollbar_active, scrollbar_active_with_colors, scrollbar_active => Color::Yellow),
    (scrollbar_inactive, scrollbar_inactive_with_colors, scrollbar_inactive => Color::DarkGray),
    (function_bar_fg, function_bar_fg_with_colors, function_bar_fg => Color::LightBlue),
    (function_bar_bg, function_bar_bg_with_colors, function_bar_bg => Color::DarkGray),
    (search_match_fg, search_match_fg_with_colors, search_match_fg => Color::Black),
    (search_match_bg, search_match_bg_with_colors, search_match_bg => Color::LightGreen),
    (search_match_current_fg, search_match_current_fg_with_colors, search_match_current_fg => Color::Black),
    (search_match_current_bg, search_match_current_bg_with_colors, search_match_current_bg => Color::Yellow),
    (directory, directory_with_colors, directory => Color::White),
    (executable, executable_with_colors, executable => Color::Green),
    (symlink, symlink_with_colors, symlink => Color::Cyan),
    (archive, archive_with_colors, archive => Color::Red),
    (image, image_with_colors, image => Color::Magenta),
    (video, video_with_colors, video => Color::LightMagenta),
    (audio, audio_with_colors, audio => Color::LightGreen),
    (document, document_with_colors, document => Color::LightYellow),
    (source_code, source_code_with_colors, source_code => Color::Yellow),
    (config, config_with_colors, config => Color::LightBlue),
    (font, font_with_colors, font => Color::LightCyan),
    (regular_file, regular_file_with_colors, regular_file => Color::White),
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    if let Ok(idx) = s.parse::<u8>() {
        return Some(Color::Indexed(idx));
    }
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
        "orange" => Some(Color::Rgb(255, 165, 0)),
        "purple" => Some(Color::Rgb(128, 0, 128)),
        "brown" => Some(Color::Rgb(165, 42, 42)),
        "pink" => Some(Color::Rgb(255, 192, 203)),
        "navy" => Some(Color::Rgb(0, 0, 128)),
        "teal" => Some(Color::Rgb(0, 128, 128)),
        "olive" => Some(Color::Rgb(128, 128, 0)),
        "maroon" => Some(Color::Rgb(128, 0, 0)),
        "aqua" => Some(Color::Cyan),
        "fuchsia" => Some(Color::Magenta),
        "lime" => Some(Color::Rgb(0, 255, 0)),
        "silver" => Some(Color::Rgb(192, 192, 192)),
        _ => None,
    }
}

impl Theme {
    #[deprecated(note = "Use Theme::panel_bg_color() instead")]
    pub const PANEL_BG: Color = Color::Rgb(0, 0, 128);
    #[deprecated(note = "Use Theme::status_bar_bg() instead")]
    pub const STATUS_BAR_BG: Color = Color::Rgb(0, 0, 128);
    #[deprecated(note = "Use Theme::menu_bar_bg() instead")]
    pub const MENU_BAR_BG: Color = Color::Rgb(0, 0, 128);
    #[deprecated(note = "Use Theme::dialog_bg() instead")]
    pub const DIALOG_BG: Color = Color::Black;
    #[deprecated(note = "Use Theme::highlight_bg() instead")]
    pub const HIGHLIGHT_BG: Color = Color::Cyan;

    #[deprecated(note = "Use Theme::panel_fg_color() instead")]
    pub const PANEL_FG: Color = Color::White;
    #[deprecated(note = "Use Theme::status_bar_fg() instead")]
    pub const STATUS_BAR_FG: Color = Color::White;
    #[deprecated(note = "Use Theme::menu_bar_fg() instead")]
    pub const MENU_BAR_FG: Color = Color::White;
    #[deprecated(note = "Use Theme::dialog_fg() instead")]
    pub const DIALOG_FG: Color = Color::White;
    #[deprecated(note = "Use Theme::highlight_fg() instead")]
    pub const HIGHLIGHT_FG: Color = Color::Black;

    #[deprecated(note = "Use Theme::border_active_color() instead")]
    pub const BORDER_ACTIVE: Color = Color::Yellow;
    #[deprecated(note = "Use Theme::border_inactive_color() instead")]
    pub const BORDER_INACTIVE: Color = Color::DarkGray;
    #[deprecated(note = "Use Theme::title_color() instead")]
    pub const TITLE: Color = Color::LightCyan;
    #[deprecated(note = "Use Theme::error_color() instead")]
    pub const ERROR: Color = Color::Red;
    #[deprecated(note = "Use Theme::warning_color() instead")]
    pub const WARNING: Color = Color::Yellow;
    #[deprecated(note = "Use Theme::info_color() instead")]
    pub const INFO: Color = Color::Cyan;

    #[deprecated(note = "Use Theme::selected_file_fg() instead")]
    pub const SELECTED_FILE_FG: Color = Color::LightYellow;
    #[deprecated(note = "Use Theme::scrollbar_active() instead")]
    pub const SCROLLBAR_ACTIVE: Color = Color::Yellow;
    #[deprecated(note = "Use Theme::scrollbar_inactive() instead")]
    pub const SCROLLBAR_INACTIVE: Color = Color::DarkGray;
    #[deprecated(note = "Use Theme::function_bar_fg() instead")]
    pub const FUNCTION_BAR_FG: Color = Color::LightBlue;
    #[deprecated(note = "Use Theme::function_bar_bg() instead")]
    pub const FUNCTION_BAR_BG: Color = Color::DarkGray;
    #[deprecated(note = "Use Theme::search_match_fg() instead")]
    pub const SEARCH_MATCH_FG: Color = Color::Black;
    #[deprecated(note = "Use Theme::search_match_bg() instead")]
    pub const SEARCH_MATCH_BG: Color = Color::LightGreen;
    #[deprecated(note = "Use Theme::search_match_current_fg() instead")]
    pub const SEARCH_MATCH_CURRENT_FG: Color = Color::Black;
    #[deprecated(note = "Use Theme::search_match_current_bg() instead")]
    pub const SEARCH_MATCH_CURRENT_BG: Color = Color::Yellow;

    #[deprecated(note = "Use Theme::directory() instead")]
    pub const DIRECTORY: Color = Color::White;
    #[deprecated(note = "Use Theme::executable() instead")]
    pub const EXECUTABLE: Color = Color::Green;
    #[deprecated(note = "Use Theme::symlink() instead")]
    pub const SYMLINK: Color = Color::Cyan;
    #[deprecated(note = "Use Theme::archive() instead")]
    pub const ARCHIVE: Color = Color::Red;
    #[deprecated(note = "Use Theme::image() instead")]
    pub const IMAGE: Color = Color::Magenta;
    #[deprecated(note = "Use Theme::video() instead")]
    pub const VIDEO: Color = Color::LightMagenta;
    #[deprecated(note = "Use Theme::audio() instead")]
    pub const AUDIO: Color = Color::LightGreen;
    #[deprecated(note = "Use Theme::document() instead")]
    pub const DOCUMENT: Color = Color::LightYellow;
    #[deprecated(note = "Use Theme::source_code() instead")]
    pub const SOURCE_CODE: Color = Color::Yellow;
    #[deprecated(note = "Use Theme::config() instead")]
    pub const CONFIG: Color = Color::LightBlue;
    #[deprecated(note = "Use Theme::font() instead")]
    pub const FONT: Color = Color::LightCyan;
    #[deprecated(note = "Use Theme::regular_file() instead")]
    pub const REGULAR_FILE: Color = Color::White;

    pub fn apply_from_value(raw: &toml::Value) -> Result<ColorPalette, String> {
        let mut colors = DEFAULT_COLORS;
        Self::apply_from_value_to_palette(raw, &mut colors)?;
        Ok(colors)
    }

    pub fn apply_from_value_to_palette(
        raw: &toml::Value,
        colors: &mut ColorPalette,
    ) -> Result<(), String> {
        let Some(theme_val) = raw.get("theme") else {
            return Ok(());
        };
        let cfg: ThemeConfig = ThemeConfig::deserialize(theme_val.clone())
            .map_err(|e| format!("Failed to parse [theme] section: {e}"))?;
        *colors = ColorPalette::from_config(&cfg);
        Ok(())
    }

    #[deprecated(note = "Use ColorPalette::icon_theme() for active theme state")]
    pub fn icon_theme() -> IconTheme {
        DEFAULT_COLORS.icon_theme
    }

    pub fn panel_bg() -> Style {
        Self::panel_bg_with_colors(&DEFAULT_COLORS)
    }

    pub fn panel_bg_with_colors(colors: &ColorPalette) -> Style {
        Style::default().bg(colors.panel_bg)
    }

    pub fn panel_fg() -> Style {
        Self::panel_fg_with_colors(&DEFAULT_COLORS)
    }

    pub fn panel_fg_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.panel_fg)
    }

    pub fn panel() -> Style {
        Self::panel_with_colors(&DEFAULT_COLORS)
    }

    pub fn panel_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.panel_fg).bg(colors.panel_bg)
    }

    pub fn status_bar() -> Style {
        Self::status_bar_with_colors(&DEFAULT_COLORS)
    }

    pub fn status_bar_with_colors(colors: &ColorPalette) -> Style {
        Style::default()
            .fg(colors.status_bar_fg)
            .bg(colors.status_bar_bg)
    }

    pub fn menu_bar() -> Style {
        Self::menu_bar_with_colors(&DEFAULT_COLORS)
    }

    pub fn menu_bar_with_colors(colors: &ColorPalette) -> Style {
        Style::default()
            .fg(colors.menu_bar_fg)
            .bg(colors.menu_bar_bg)
    }

    pub fn dialog() -> Style {
        Self::dialog_with_colors(&DEFAULT_COLORS)
    }

    pub fn dialog_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.dialog_fg).bg(colors.dialog_bg)
    }

    pub fn highlight() -> Style {
        Self::highlight_with_colors(&DEFAULT_COLORS)
    }

    pub fn highlight_with_colors(colors: &ColorPalette) -> Style {
        Style::default()
            .fg(colors.highlight_fg)
            .bg(colors.highlight_bg)
    }

    pub fn highlight_bold() -> Style {
        Self::highlight_bold_with_colors(&DEFAULT_COLORS)
    }

    pub fn highlight_bold_with_colors(colors: &ColorPalette) -> Style {
        Self::highlight_with_colors(colors).add_modifier(Modifier::BOLD)
    }

    pub fn error_dialog() -> Style {
        Self::error_dialog_with_colors(&DEFAULT_COLORS)
    }

    pub fn error_dialog_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.error).bg(colors.dialog_bg)
    }

    pub fn help_dialog() -> Style {
        Self::help_dialog_with_colors(&DEFAULT_COLORS)
    }

    pub fn help_dialog_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.info).bg(colors.dialog_bg)
    }

    pub fn warning_dialog() -> Style {
        Self::warning_dialog_with_colors(&DEFAULT_COLORS)
    }

    pub fn warning_dialog_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.warning).bg(colors.dialog_bg)
    }

    pub fn progress_bar() -> Style {
        Self::progress_bar_with_colors(&DEFAULT_COLORS)
    }

    pub fn progress_bar_with_colors(colors: &ColorPalette) -> Style {
        Self::help_dialog_with_colors(colors)
    }

    pub fn selected_error() -> Style {
        Self::selected_error_with_colors(&DEFAULT_COLORS)
    }

    pub fn selected_error_with_colors(colors: &ColorPalette) -> Style {
        Self::highlight_with_colors(colors)
            .fg(colors.error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn panel_file(color: Color) -> Style {
        Self::panel_file_with_colors(color, &DEFAULT_COLORS)
    }

    pub fn panel_file_with_colors(color: Color, colors: &ColorPalette) -> Style {
        Style::default().fg(color).bg(colors.panel_bg)
    }

    pub fn category_color(category: FileCategory) -> Color {
        Self::category_color_with_colors(category, &DEFAULT_COLORS)
    }

    pub fn category_color_with_colors(category: FileCategory, colors: &ColorPalette) -> Color {
        match category {
            FileCategory::Dir => colors.directory,
            FileCategory::Archive => colors.archive,
            FileCategory::Image => colors.image,
            FileCategory::Video => colors.video,
            FileCategory::Audio => colors.audio,
            FileCategory::Document => colors.document,
            FileCategory::Code => colors.source_code,
            FileCategory::Config => colors.config,
            FileCategory::Font => colors.font,
            FileCategory::Executable => colors.executable,
            FileCategory::Symlink => colors.symlink,
            FileCategory::Other => colors.regular_file,
        }
    }

    pub fn panel_item(color: Color, bold: bool) -> Style {
        Self::panel_item_with_colors(color, bold, &DEFAULT_COLORS)
    }

    pub fn panel_item_with_colors(color: Color, bold: bool, colors: &ColorPalette) -> Style {
        let style = Self::panel_file_with_colors(color, colors);
        if bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn border_active() -> Style {
        Self::border_active_with_colors(&DEFAULT_COLORS)
    }

    pub fn border_active_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.border_active)
    }

    pub fn border_inactive() -> Style {
        Self::border_inactive_with_colors(&DEFAULT_COLORS)
    }

    pub fn border_inactive_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.border_inactive)
    }

    pub fn title() -> Style {
        Self::title_with_colors(&DEFAULT_COLORS)
    }

    pub fn title_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.title)
    }

    pub fn error() -> Style {
        Self::error_with_colors(&DEFAULT_COLORS)
    }

    pub fn error_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.error)
    }

    pub fn warning() -> Style {
        Self::warning_with_colors(&DEFAULT_COLORS)
    }

    pub fn warning_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.warning)
    }

    pub fn info() -> Style {
        Self::info_with_colors(&DEFAULT_COLORS)
    }

    pub fn info_with_colors(colors: &ColorPalette) -> Style {
        Style::default().fg(colors.info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_color_maps_file_categories_to_theme_colors() {
        let cases = [
            (FileCategory::Dir, Theme::directory()),
            (FileCategory::Archive, Theme::archive()),
            (FileCategory::Image, Theme::image()),
            (FileCategory::Video, Theme::video()),
            (FileCategory::Audio, Theme::audio()),
            (FileCategory::Document, Theme::document()),
            (FileCategory::Code, Theme::source_code()),
            (FileCategory::Config, Theme::config()),
            (FileCategory::Font, Theme::font()),
            (FileCategory::Executable, Theme::executable()),
            (FileCategory::Symlink, Theme::symlink()),
            (FileCategory::Other, Theme::regular_file()),
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
    fn parse_color_css_named() {
        assert_eq!(parse_color("orange"), Some(Color::Rgb(255, 165, 0)));
        assert_eq!(parse_color("purple"), Some(Color::Rgb(128, 0, 128)));
        assert_eq!(parse_color("brown"), Some(Color::Rgb(165, 42, 42)));
        assert_eq!(parse_color("pink"), Some(Color::Rgb(255, 192, 203)));
        assert_eq!(parse_color("navy"), Some(Color::Rgb(0, 0, 128)));
        assert_eq!(parse_color("teal"), Some(Color::Rgb(0, 128, 128)));
        assert_eq!(parse_color("olive"), Some(Color::Rgb(128, 128, 0)));
        assert_eq!(parse_color("maroon"), Some(Color::Rgb(128, 0, 0)));
        assert_eq!(parse_color("aqua"), Some(Color::Cyan));
        assert_eq!(parse_color("fuchsia"), Some(Color::Magenta));
        assert_eq!(parse_color("lime"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_color("silver"), Some(Color::Rgb(192, 192, 192)));
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
        let c = &DEFAULT_COLORS;
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
        let colors = ColorPalette::from_config(&cfg);
        assert_eq!(colors.panel_bg, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.directory, Color::Cyan);
        assert_eq!(colors.error, DEFAULT_COLORS.error);
        assert_eq!(colors.panel_fg, DEFAULT_COLORS.panel_fg);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn theme_config_from_toml_invalid_icon_theme_keeps_colors() {
        let raw: toml::Value = toml::from_str(
            r##"
            [theme]
            panel_bg = "#112233"
            icon_theme = "bad-value"
            "##,
        )
        .unwrap();
        let mut colors = ColorPalette::default();

        Theme::apply_from_value_to_palette(&raw, &mut colors).unwrap();

        assert_eq!(colors.panel_bg, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.icon_theme(), IconTheme::Emoji);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn theme_config_from_toml_non_string_icon_theme_keeps_colors() {
        let raw: toml::Value = toml::from_str(
            r##"
            [theme]
            panel_bg = "#112233"
            icon_theme = true
            "##,
        )
        .unwrap();
        let mut colors = ColorPalette::default();

        Theme::apply_from_value_to_palette(&raw, &mut colors).unwrap();

        assert_eq!(colors.panel_bg, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.icon_theme(), IconTheme::Emoji);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn apply_from_value_returns_palette() {
        let raw: toml::Value = toml::from_str(
            r##"
            [theme]
            panel_bg = "#112233"
            icon_theme = "ascii"
            "##,
        )
        .unwrap();

        let colors = Theme::apply_from_value(&raw).unwrap();

        assert_eq!(colors.panel_bg, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(colors.icon_theme(), IconTheme::Ascii);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn theme_config_from_toml_accepts_nerd_font_alias() {
        let raw: toml::Value = toml::from_str(
            r##"
            [theme]
            icon_theme = "nerd_font"
            "##,
        )
        .unwrap();
        let mut colors = ColorPalette::default();

        Theme::apply_from_value_to_palette(&raw, &mut colors).unwrap();

        assert_eq!(colors.icon_theme(), IconTheme::NerdFont);
    }

    #[test]
    fn icon_theme_default_is_emoji() {
        let cfg: ThemeConfig = ThemeConfig {
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::Emoji);
    }

    #[test]
    fn icon_theme_deserialize_ascii() {
        let cfg = ThemeConfig {
            icon_theme: IconTheme::Ascii,
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::Ascii);
    }

    #[test]
    fn icon_theme_deserialize_emoji() {
        let cfg = ThemeConfig {
            icon_theme: IconTheme::Emoji,
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::Emoji);
    }

    #[test]
    fn icon_theme_deserialize_nerdfont() {
        let cfg = ThemeConfig {
            icon_theme: IconTheme::NerdFont,
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::NerdFont);
    }

    #[test]
    fn icon_theme_ascii_mapping() {
        use crate::app::types::FileCategory;
        use crate::ui::panels::get_file_icon_with_theme;
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Dir, IconTheme::Ascii),
            "D"
        );
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Symlink, IconTheme::Ascii),
            "@"
        );
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Executable, IconTheme::Ascii),
            "*"
        );
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Other, IconTheme::Ascii),
            "."
        );
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Dir, IconTheme::Emoji),
            "📁"
        );
        assert_eq!(
            get_file_icon_with_theme(&FileCategory::Dir, IconTheme::NerdFont),
            ""
        );
    }
}
