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
    /// Parse config string / TOML value; unknown → `Emoji`.
    fn from_value(value: &toml::Value) -> Self {
        let Some(s) = value.as_str() else {
            crate::debug_log!("config: non-string value for icon_theme, using emoji");
            return Self::Emoji;
        };
        match s.trim().to_ascii_lowercase().as_str() {
            "emoji" => Self::Emoji,
            "ascii" => Self::Ascii,
            "nerdfont" | "nerd_font" | "nerd-font" => Self::NerdFont,
            _ => {
                crate::debug_log!("config: invalid value for icon_theme, using emoji");
                Self::Emoji
            }
        }
    }
}

impl<'de> Deserialize<'de> for IconTheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_value(&toml::Value::deserialize(deserializer)?))
    }
}

macro_rules! define_theme_colors {
    ($(($field:ident => $color:expr)),* $(,)?) => {
        #[derive(Debug, Clone, Deserialize, Default)]
        #[serde(default)]
        pub struct ThemeConfig {
            $(pub $field: Option<String>,)*
            #[serde(default)]
            pub icon_theme: IconTheme,
        }

        #[derive(Copy, Clone, Debug, PartialEq)]
        pub struct ColorPalette {
            $(pub $field: Color,)*
            icon_theme: IconTheme,
        }

        pub const DEFAULT_COLORS: ColorPalette = ColorPalette {
            $($field: $color,)*
            icon_theme: IconTheme::Emoji,
        };

        impl ColorPalette {
            pub fn from_config(cfg: &ThemeConfig) -> Self {
                Self {
                    $($field: parse_color_field(
                        stringify!($field),
                        cfg.$field.as_deref(),
                        DEFAULT_COLORS.$field,
                    ),)*
                    icon_theme: cfg.icon_theme,
                }
            }

            pub fn icon_theme(&self) -> IconTheme {
                self.icon_theme
            }
        }

        impl ThemeConfig {
            /// Build a `ThemeConfig` directly from a borrowed `[theme]` TOML
            /// table, avoiding a full clone of the table just to round-trip it
            /// back through serde. Mirrors the `Deserialize` derive: each color
            /// key must be a string (a non-string value is rejected), and
            /// `icon_theme` tolerates bad values by falling back to the default
            /// (see `IconTheme::from_value`).
            fn from_table(table: &toml::Table) -> Result<Self, String> {
                Ok(Self {
                    $($field: match table.get(stringify!($field)) {
                        None => None,
                        Some(value) => match value.as_str() {
                            Some(s) => Some(s.to_string()),
                            None => {
                                return Err(format!(
                                    "[theme].{} must be a string color value",
                                    stringify!($field)
                                ));
                            }
                        },
                    },)*
                    icon_theme: table
                        .get("icon_theme")
                        .map(IconTheme::from_value)
                        .unwrap_or_default(),
                })
            }
        }

        impl Default for ColorPalette {
            fn default() -> Self {
                DEFAULT_COLORS
            }
        }
    };
}

define_theme_colors! {
    (panel_bg => Color::Rgb(0, 0, 128)),
    (status_bar_bg => Color::Rgb(0, 0, 128)),
    (menu_bar_bg => Color::Rgb(0, 0, 128)),
    (dialog_bg => Color::Black),
    (highlight_bg => Color::Cyan),
    (panel_fg => Color::White),
    (status_bar_fg => Color::White),
    (menu_bar_fg => Color::White),
    (dialog_fg => Color::White),
    (highlight_fg => Color::Black),
    (border_active => Color::Yellow),
    (border_inactive => Color::DarkGray),
    (title => Color::LightCyan),
    (error => Color::Red),
    (warning => Color::Yellow),
    (info => Color::Cyan),
    (selected_file_fg => Color::LightYellow),
    (scrollbar_active => Color::Yellow),
    (scrollbar_inactive => Color::DarkGray),
    (function_bar_fg => Color::LightBlue),
    (function_bar_bg => Color::DarkGray),
    (search_match_fg => Color::Black),
    (search_match_bg => Color::LightGreen),
    (search_match_current_fg => Color::Black),
    (search_match_current_bg => Color::Yellow),
    (directory => Color::White),
    (executable => Color::Green),
    (symlink => Color::Cyan),
    (archive => Color::Red),
    (image => Color::Magenta),
    (video => Color::LightMagenta),
    (audio => Color::LightGreen),
    (document => Color::LightYellow),
    (source_code => Color::Yellow),
    (config => Color::LightBlue),
    (font => Color::LightCyan),
    (regular_file => Color::White),
}

pub struct Theme;

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Ok(idx) = s.parse::<u8>() {
        return Some(Color::Indexed(idx));
    }
    parse_named_color(s)
}

fn parse_hex_color(hex: &str) -> Option<Color> {
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
    None
}

fn parse_named_color(s: &str) -> Option<Color> {
    // Case-insensitive lookup without allocating a lowercased copy per parse.
    const NAMED: &[(&str, Color)] = &[
        ("black", Color::Black),
        ("red", Color::Red),
        ("green", Color::Green),
        ("yellow", Color::Yellow),
        ("blue", Color::Blue),
        ("magenta", Color::Magenta),
        ("fuchsia", Color::Magenta),
        ("cyan", Color::Cyan),
        ("aqua", Color::Cyan),
        ("gray", Color::Gray),
        ("grey", Color::Gray),
        ("darkgray", Color::DarkGray),
        ("darkgrey", Color::DarkGray),
        ("dark_gray", Color::DarkGray),
        ("dark_grey", Color::DarkGray),
        ("lightred", Color::LightRed),
        ("light_red", Color::LightRed),
        ("lightgreen", Color::LightGreen),
        ("light_green", Color::LightGreen),
        ("lightyellow", Color::LightYellow),
        ("light_yellow", Color::LightYellow),
        ("lightblue", Color::LightBlue),
        ("light_blue", Color::LightBlue),
        ("lightmagenta", Color::LightMagenta),
        ("light_magenta", Color::LightMagenta),
        ("lightcyan", Color::LightCyan),
        ("light_cyan", Color::LightCyan),
        ("white", Color::White),
        ("orange", Color::Rgb(255, 165, 0)),
        ("purple", Color::Rgb(128, 0, 128)),
        ("brown", Color::Rgb(165, 42, 42)),
        ("pink", Color::Rgb(255, 192, 203)),
        ("navy", Color::Rgb(0, 0, 128)),
        ("teal", Color::Rgb(0, 128, 128)),
        ("olive", Color::Rgb(128, 128, 0)),
        ("maroon", Color::Rgb(128, 0, 0)),
        ("lime", Color::Rgb(0, 255, 0)),
        ("silver", Color::Rgb(192, 192, 192)),
    ];
    NAMED
        .iter()
        .find(|(name, _)| s.eq_ignore_ascii_case(name))
        .map(|&(_, color)| color)
}

/// Resolve a single palette color from its optional config string, falling back
/// to `default` and logging (like the icon-theme path) when a value is present
/// but cannot be parsed.
fn parse_color_field(field: &str, value: Option<&str>, default: Color) -> Color {
    let Some(s) = value else {
        return default;
    };
    match parse_color(s) {
        Some(color) => color,
        None => {
            crate::debug_log!("config: invalid color '{s}' for {field}, using default");
            default
        }
    }
}

/// Style accessors from a palette. Collapses repetitive `Style::default().fg/bg`
/// boilerplate while keeping public method names stable.
macro_rules! theme_styles {
    ($($with_colors:ident => |$c:ident| $body:expr);* $(;)?) => {
        impl Theme {
            $(
                pub fn $with_colors($c: &ColorPalette) -> Style {
                    $body
                }
            )*
        }
    };
}

theme_styles! {
    panel_bg_with_colors => |c| Style::default().bg(c.panel_bg);
    panel_fg_with_colors => |c| Style::default().fg(c.panel_fg);
    panel_with_colors => |c| Style::default().fg(c.panel_fg).bg(c.panel_bg);
    status_bar_with_colors =>
        |c| Style::default().fg(c.status_bar_fg).bg(c.status_bar_bg);
    menu_bar_with_colors =>
        |c| Style::default().fg(c.menu_bar_fg).bg(c.menu_bar_bg);
    dialog_with_colors => |c| Style::default().fg(c.dialog_fg).bg(c.dialog_bg);
    highlight_with_colors =>
        |c| Style::default().fg(c.highlight_fg).bg(c.highlight_bg);
    error_dialog_with_colors => |c| Style::default().fg(c.error).bg(c.dialog_bg);
    help_dialog_with_colors => |c| Style::default().fg(c.info).bg(c.dialog_bg);
    warning_dialog_with_colors =>
        |c| Style::default().fg(c.warning).bg(c.dialog_bg);
    border_active_with_colors => |c| Style::default().fg(c.border_active);
    border_inactive_with_colors => |c| Style::default().fg(c.border_inactive);
    title_with_colors => |c| Style::default().fg(c.title);
    error_with_colors => |c| Style::default().fg(c.error);
    warning_with_colors => |c| Style::default().fg(c.warning);
    info_with_colors => |c| Style::default().fg(c.info);
}

impl Theme {
    pub fn apply_from_value_to_palette(
        raw: &toml::Value,
        colors: &mut ColorPalette,
    ) -> Result<(), String> {
        let Some(theme_val) = raw.get("theme") else {
            return Ok(());
        };
        // Read directly from the borrowed table instead of cloning the whole
        // `[theme]` value just to feed serde.
        let Some(table) = theme_val.as_table() else {
            return Err("Failed to parse [theme] section: expected a table".to_string());
        };
        let cfg = ThemeConfig::from_table(table)?;
        *colors = ColorPalette::from_config(&cfg);
        Ok(())
    }

    pub fn highlight_bold_with_colors(colors: &ColorPalette) -> Style {
        Self::highlight_with_colors(colors).add_modifier(Modifier::BOLD)
    }

    pub fn selected_error_with_colors(colors: &ColorPalette) -> Style {
        Self::highlight_with_colors(colors)
            .fg(colors.error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn panel_file_with_colors(color: Color, colors: &ColorPalette) -> Style {
        Style::default().fg(color).bg(colors.panel_bg)
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

    pub fn panel_item_with_colors(color: Color, bold: bool, colors: &ColorPalette) -> Style {
        let style = Self::panel_file_with_colors(color, colors);
        if bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_color_maps_file_categories_to_theme_colors() {
        let cases = [
            (FileCategory::Dir, DEFAULT_COLORS.directory),
            (FileCategory::Archive, DEFAULT_COLORS.archive),
            (FileCategory::Image, DEFAULT_COLORS.image),
            (FileCategory::Video, DEFAULT_COLORS.video),
            (FileCategory::Audio, DEFAULT_COLORS.audio),
            (FileCategory::Document, DEFAULT_COLORS.document),
            (FileCategory::Code, DEFAULT_COLORS.source_code),
            (FileCategory::Config, DEFAULT_COLORS.config),
            (FileCategory::Font, DEFAULT_COLORS.font),
            (FileCategory::Executable, DEFAULT_COLORS.executable),
            (FileCategory::Symlink, DEFAULT_COLORS.symlink),
            (FileCategory::Other, DEFAULT_COLORS.regular_file),
        ];

        for (category, color) in cases {
            assert_eq!(
                Theme::category_color_with_colors(category, &DEFAULT_COLORS),
                color
            );
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
    fn icon_theme_config_field_ascii() {
        let cfg = ThemeConfig {
            icon_theme: IconTheme::Ascii,
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::Ascii);
    }

    #[test]
    fn icon_theme_config_field_emoji() {
        let cfg = ThemeConfig {
            icon_theme: IconTheme::Emoji,
            ..Default::default()
        };
        assert_eq!(cfg.icon_theme, IconTheme::Emoji);
    }

    #[test]
    fn icon_theme_config_field_nerdfont() {
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
