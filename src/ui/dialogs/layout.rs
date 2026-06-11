use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, BorderType, Borders},
};

pub(super) const DIALOG_WIDTH_PERCENT: u16 = 50;
pub(super) const DIALOG_HEIGHT_PERCENT: u16 = 40;

const HELP_CONTENT_MIN_HEIGHT: u16 = 3;
const HELP_FOOTER_HEIGHT: u16 = 1;

pub struct HelpGeometry {
    pub height: usize,
    pub width: u16,
}

fn thick_bordered_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
}

pub(super) fn help_dialog_content_rect(dialog_area: Rect) -> Rect {
    let block = thick_bordered_block();
    let inner = block.inner(dialog_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(HELP_CONTENT_MIN_HEIGHT),
            Constraint::Length(HELP_FOOTER_HEIGHT),
        ])
        .split(inner);
    chunks[0]
}

pub fn help_dialog_geometry(area: Rect) -> HelpGeometry {
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    let content = help_dialog_content_rect(dialog_area);
    let height = content.height as usize;
    let width = if content.width > 1 {
        content.width.saturating_sub(1)
    } else {
        content.width
    };
    HelpGeometry { height, width }
}

pub fn help_visible_height(area: Rect) -> usize {
    help_dialog_geometry(area).height
}

pub fn help_message_width(area: Rect) -> u16 {
    help_dialog_geometry(area).width
}

pub(super) fn dialog_block(title: &str, style: Style) -> Block<'_> {
    thick_bordered_block().title(title).style(style)
}

pub fn input_dialog_rect(area: Rect) -> Rect {
    centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area)
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    const MIN_WIDTH: u16 = 30;
    const MIN_HEIGHT: u16 = 5;

    let dialog_width = ((area.width as u32 * percent_x as u32) / 100)
        .max(MIN_WIDTH as u32)
        .min(area.width as u32) as u16;
    let dialog_height = ((area.height as u32 * percent_y as u32) / 100)
        .max(MIN_HEIGHT as u32)
        .min(area.height as u32) as u16;

    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

    Rect::new(x, y, dialog_width, dialog_height)
}
