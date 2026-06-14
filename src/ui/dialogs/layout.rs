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
    /// Number of visible content rows. `usize` because callers use it as a
    /// line count / index into the wrapped-line `Vec` (avoids repeated casts).
    pub height: usize,
    /// Content width in terminal columns. `u16` because it is fed straight
    /// back into Ratatui geometry (`Rect`, scroll), which is natively `u16`.
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

/// Canonical geometry of the help dialog's content area.
///
/// This is the single source of truth; [`help_visible_height`] and
/// [`help_message_width`] are thin field accessors over it so that callers
/// needing only one field stay readable without duplicating the layout math.
pub fn help_dialog_geometry(area: Rect) -> HelpGeometry {
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
    let content = help_dialog_content_rect(dialog_area);
    let height = content.height as usize;
    // Reserve one column on the right for the scrollbar / right margin, so the
    // wrapped-line count used for scrolling matches what is actually drawn.
    // Only subtract when there is more than one column: at width 0 or 1 there is
    // no room to spare, so the width is returned unchanged (width 0 stays 0).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog must always stay within its parent area.
    fn assert_within(child: Rect, parent: Rect) {
        assert!(child.width <= parent.width, "width overflow: {child:?}");
        assert!(child.height <= parent.height, "height overflow: {child:?}");
        assert!(
            child.x + child.width <= parent.x + parent.width,
            "x overflow: {child:?}"
        );
        assert!(
            child.y + child.height <= parent.y + parent.height,
            "y overflow: {child:?}"
        );
    }

    #[test]
    fn centered_rect_30x5_clamps_to_minimums() {
        let area = Rect::new(0, 0, 30, 5);
        let rect = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, area);
        // MIN_WIDTH (30) / MIN_HEIGHT (5) exactly fill the tiny area.
        assert_eq!(rect.width, 30);
        assert_eq!(rect.height, 5);
        assert_within(rect, area);
    }

    #[test]
    fn centered_rect_zero_area_is_zero_no_panic() {
        let area = Rect::new(0, 0, 0, 0);
        let rect = centered_rect(50, 40, area);
        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 0);
        assert_within(rect, area);
    }

    #[test]
    fn centered_rect_minimal_area_stays_in_bounds() {
        let area = Rect::new(3, 7, 1, 1);
        let rect = centered_rect(50, 40, area);
        assert_within(rect, area);
    }

    #[test]
    fn centered_rect_overflow_percent_clamps_to_area() {
        // Percentages above 100 must never exceed the parent area.
        let area = Rect::new(0, 0, 100, 50);
        let rect = centered_rect(150, 150, area);
        assert_eq!(rect.width, area.width);
        assert_eq!(rect.height, area.height);
        assert_within(rect, area);
    }

    #[test]
    fn help_accessors_agree_with_geometry() {
        let area = Rect::new(0, 0, 80, 24);
        let geo = help_dialog_geometry(area);
        assert_eq!(help_visible_height(area), geo.height);
        assert_eq!(help_message_width(area), geo.width);
    }

    #[test]
    fn help_geometry_zero_area_no_panic() {
        let geo = help_dialog_geometry(Rect::new(0, 0, 0, 0));
        // Just assert it returns without panicking and width stays sane.
        assert!(geo.width <= 1);
    }
}
