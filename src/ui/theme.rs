use crate::app::types::FileCategory;
use ratatui::style::Modifier;
use ratatui::style::{Color, Style};

/// Color theme for the application (Midnight Commander style)
pub struct Theme;

impl Theme {
    // Background colors
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
    pub const HIDDEN_FILE: Color = Color::White;
    pub const ERROR: Color = Color::Red;
    pub const WARNING: Color = Color::Yellow;
    pub const INFO: Color = Color::Cyan;

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
    pub const CODE: Color = Self::SOURCE_CODE;
    pub const CONFIG: Color = Color::LightBlue;
    pub const REGULAR_FILE: Color = Color::White;

    // Styles
    pub fn panel_bg() -> Style {
        Style::default().bg(Self::PANEL_BG)
    }

    pub fn panel_fg() -> Style {
        Style::default().fg(Self::PANEL_FG)
    }

    pub fn panel() -> Style {
        Style::default().fg(Self::PANEL_FG).bg(Self::PANEL_BG)
    }

    pub fn status_bar() -> Style {
        Style::default()
            .fg(Self::STATUS_BAR_FG)
            .bg(Self::STATUS_BAR_BG)
    }

    pub fn menu_bar() -> Style {
        Style::default().fg(Self::MENU_BAR_FG).bg(Self::MENU_BAR_BG)
    }

    pub fn dialog() -> Style {
        Style::default().fg(Self::DIALOG_FG).bg(Self::DIALOG_BG)
    }

    pub fn highlight() -> Style {
        Style::default()
            .fg(Self::HIGHLIGHT_FG)
            .bg(Self::HIGHLIGHT_BG)
    }

    pub fn highlight_bold() -> Style {
        Self::highlight().add_modifier(Modifier::BOLD)
    }

    pub fn error_dialog() -> Style {
        Style::default().fg(Self::ERROR).bg(Self::DIALOG_BG)
    }

    pub fn help_dialog() -> Style {
        Style::default().fg(Self::INFO).bg(Self::DIALOG_BG)
    }

    pub fn warning_dialog() -> Style {
        Style::default().fg(Self::WARNING).bg(Self::DIALOG_BG)
    }

    pub fn progress_bar() -> Style {
        Style::default().fg(Self::INFO).bg(Self::DIALOG_BG)
    }

    pub fn selected_error() -> Style {
        Self::highlight()
            .fg(Self::ERROR)
            .add_modifier(Modifier::BOLD)
    }

    pub fn panel_file(color: Color) -> Style {
        Style::default().fg(color).bg(Self::PANEL_BG)
    }

    pub fn category_color(category: FileCategory) -> Color {
        match category {
            FileCategory::Dir => Self::DIRECTORY,
            FileCategory::Executable => Self::EXECUTABLE,
            FileCategory::Symlink => Self::SYMLINK,
            FileCategory::Hidden => Self::HIDDEN_FILE,
            FileCategory::Archive => Self::ARCHIVE,
            FileCategory::Image => Self::IMAGE,
            FileCategory::Video => Self::VIDEO,
            FileCategory::Audio => Self::AUDIO,
            FileCategory::Document => Self::DOCUMENT,
            FileCategory::Code => Self::SOURCE_CODE,
            FileCategory::Config => Self::CONFIG,
            FileCategory::Other => Self::REGULAR_FILE,
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
        Style::default().fg(Self::BORDER_ACTIVE)
    }

    pub fn border_inactive() -> Style {
        Style::default().fg(Self::BORDER_INACTIVE)
    }

    pub fn title() -> Style {
        Style::default().fg(Self::TITLE)
    }

    pub fn error() -> Style {
        Style::default().fg(Self::ERROR)
    }

    pub fn warning() -> Style {
        Style::default().fg(Self::WARNING)
    }

    pub fn info() -> Style {
        Style::default().fg(Self::INFO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_color_maps_file_categories_to_theme_colors() {
        let cases = [
            (FileCategory::Dir, Theme::DIRECTORY),
            (FileCategory::Executable, Theme::EXECUTABLE),
            (FileCategory::Symlink, Theme::SYMLINK),
            (FileCategory::Hidden, Theme::HIDDEN_FILE),
            (FileCategory::Archive, Theme::ARCHIVE),
            (FileCategory::Image, Theme::IMAGE),
            (FileCategory::Video, Theme::VIDEO),
            (FileCategory::Audio, Theme::AUDIO),
            (FileCategory::Document, Theme::DOCUMENT),
            (FileCategory::Code, Theme::SOURCE_CODE),
            (FileCategory::Config, Theme::CONFIG),
            (FileCategory::Other, Theme::REGULAR_FILE),
        ];

        for (category, color) in cases {
            assert_eq!(Theme::category_color(category), color);
        }
    }

    #[test]
    fn code_color_aliases_source_code_color() {
        assert_eq!(Theme::CODE, Theme::SOURCE_CODE);
    }
}
