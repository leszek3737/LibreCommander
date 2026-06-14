use std::cell::RefCell;
use std::fmt::Write as _;
use std::sync::OnceLock;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Gauge, Paragraph, Wrap},
};

use crate::ui::dialogs::PropertiesInfo;
use crate::ui::theme::{ColorPalette, Theme};

use super::layout::dialog_block;
use super::text::truncate_path;

const OK_BUTTON_LABEL: &str = "[ OK ]";
const CLOSE_HINT_LABEL: &str = "[ Press Enter or Esc to close ]";
const CANCELING_PREFIX: &str = "Canceling:";
const PROPERTIES_NAME_MAX_WIDTH: usize = 30;

/// Title prefix for the properties dialog (`"Properties — {name}"`).
const PROPERTIES_TITLE_PREFIX: &str = "Properties — ";

/// Spare capacity reserved per properties line so a typical value can be
/// appended without reallocating the line buffer.
const PROP_VALUE_CAPACITY_HINT: usize = 24;

/// The fields rendered, in order, by the properties dialog.
///
/// Bundling the six label prefixes into one cohesive type keeps the labels and
/// their ordering together instead of scattering parallel `const` strings.
#[derive(Clone, Copy)]
enum PropField {
    Name,
    Type,
    Size,
    Modified,
    Permissions,
    Owner,
}

impl PropField {
    /// The `"Label: "` prefix rendered before the field value.
    const fn prefix(self) -> &'static str {
        match self {
            Self::Name => "Name: ",
            Self::Type => "Type: ",
            Self::Size => "Size: ",
            Self::Modified => "Modified: ",
            Self::Permissions => "Permissions: ",
            Self::Owner => "Owner: ",
        }
    }
}

static PERCENT_LABELS: OnceLock<[String; 101]> = OnceLock::new();

fn percent_label(n: u16) -> &'static str {
    let labels = PERCENT_LABELS.get_or_init(|| std::array::from_fn(|i| format!("{i}%")));
    // Defensive clamp: the table only holds 0..=100. Even though callers clamp
    // today, never index out of range if an unclamped percentage slips through.
    &labels[(n as usize).min(100)]
}

fn centered_paragraph<'a>(text: &'a str, style: Style) -> Paragraph<'a> {
    Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Center)
}

pub fn render_error_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::error_dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Minimum-size guard: on tiny terminals the bordered block can leave no
    // inner room. Degrade gracefully to just the border rather than laying out
    // into a zero-sized area.
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let message_paragraph = Paragraph::new(message)
        // `trim: true`: this is a centered message, so trimming leading
        // whitespace keeps it visually centered as it wraps. (Contrast with
        // `render_properties_dialog`, which uses `trim: false` to preserve the
        // left-aligned field columns.)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Theme::error_with_colors(colors));
    f.render_widget(message_paragraph, chunks[0]);

    let ok_btn = centered_paragraph(OK_BUTTON_LABEL, Theme::selected_error_with_colors(colors));
    f.render_widget(ok_btn, chunks[1]);
}

pub fn render_progress_dialog(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    percent: f32,
    cancellable: bool,
    colors: &ColorPalette,
) {
    let block = dialog_block(title, Theme::dialog_with_colors(colors));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Minimum-size guard (see `render_error_dialog`): skip the inner layout when
    // the bordered block leaves no room, so a tiny terminal shows just the
    // border instead of a broken layout.
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let msg_min = if inner.height <= 3 { 1 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(msg_min),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg_paragraph = Paragraph::new(message)
        // `trim: true`: centered status message; trim leading whitespace for
        // clean centering (cf. `trim: false` in `render_properties_dialog`).
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(msg_paragraph, chunks[0]);

    let clamped = (percent.clamp(0.0, 100.0).round()) as u16;
    let gauge = Gauge::default()
        .gauge_style(Theme::progress_bar_with_colors(colors))
        .percent(clamped)
        .label(percent_label(clamped));
    f.render_widget(gauge, chunks[1]);

    let hint_text = if !cancellable {
        ""
    } else if message.starts_with(CANCELING_PREFIX) {
        "Canceled"
    } else {
        "Esc: cancel after current item"
    };
    if !hint_text.is_empty() {
        let hint = centered_paragraph(hint_text, Theme::warning_with_colors(colors));
        f.render_widget(hint, chunks[2]);
    }
}

thread_local! {
    /// Render-local memo for the properties-dialog title.
    ///
    /// Reformatting `"Properties — {name}"` every frame is wasteful while the
    /// same file's dialog stays open, so the title is cached keyed by the
    /// (already truncated) display name. This is purely a render optimization:
    /// it never touches `AppState` and the rendered output is identical to
    /// recomputing the title every frame.
    ///
    /// `None` means the cache has never been populated (first call). Using
    /// `Option` rather than a `("", "")` sentinel avoids a false cache hit when
    /// `display_name` is the empty string (e.g. width-0 truncation on the first
    /// frame), which would leave the title as `""` instead of `"Properties — "`.
    static PROPERTIES_TITLE: RefCell<Option<(String, String)>> =
        const { RefCell::new(None) };
}

/// Runs `body` with the cached properties-dialog title for `display_name`,
/// recomputing the title only when the name changed since the last frame.
fn with_properties_title<R>(display_name: &str, body: impl FnOnce(&str) -> R) -> R {
    PROPERTIES_TITLE.with_borrow_mut(|cache| {
        let needs_recompute = cache
            .as_ref()
            .is_none_or(|(key, _)| key.as_str() != display_name);
        if needs_recompute {
            let mut title =
                String::with_capacity(PROPERTIES_TITLE_PREFIX.len() + display_name.len());
            title.push_str(PROPERTIES_TITLE_PREFIX);
            title.push_str(display_name);
            *cache = Some((display_name.to_owned(), title));
        }
        match cache.as_ref() {
            Some((_, title)) => body(title.as_str()),
            // cache was set to Some above by the needs_recompute branch
            None => unreachable!("cache set to Some above"),
        }
    })
}

/// Builds a single `"Label: value"` line, pre-sizing the buffer so the value
/// can be appended without a reallocation (replacing a per-line `format!`).
fn prop_line(field: PropField, value: impl std::fmt::Display) -> Line<'static> {
    let prefix = field.prefix();
    let mut s = String::with_capacity(prefix.len() + PROP_VALUE_CAPACITY_HINT);
    s.push_str(prefix);
    let _ = write!(s, "{value}");
    Line::from(s)
}

/// Builds the combined `"Owner: owner:group"` line.
fn owner_line(owner: &str, group: &str) -> Line<'static> {
    let prefix = PropField::Owner.prefix();
    let mut s = String::with_capacity(prefix.len() + owner.len() + group.len() + 1);
    s.push_str(prefix);
    let _ = write!(s, "{owner}:{group}");
    Line::from(s)
}

pub fn render_properties_dialog(
    f: &mut Frame,
    area: Rect,
    info: &PropertiesInfo<'_>,
    colors: &ColorPalette,
) {
    let display_name = truncate_path(&info.name, PROPERTIES_NAME_MAX_WIDTH);

    let inner = with_properties_title(&display_name, |title| {
        let block = dialog_block(title, Theme::warning_dialog_with_colors(colors));
        let inner = block.inner(area);
        f.render_widget(block, area);
        inner
    });

    // Minimum-size guard (see `render_error_dialog`): bail before building lines
    // when the bordered block leaves no inner room.
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Build the line buffer via a single `vec!` allocation instead of
    // allocating six independent `format!` strings per frame.
    let lines: Vec<Line> = vec![
        prop_line(PropField::Name, &display_name),
        prop_line(PropField::Type, &info.file_type),
        prop_line(PropField::Size, &info.size),
        prop_line(PropField::Modified, &info.mtime),
        prop_line(PropField::Permissions, &info.permissions),
        owner_line(&info.owner, &info.group),
        Line::from(""),
        Line::from(CLOSE_HINT_LABEL).style(Theme::info_with_colors(colors)),
    ];

    let paragraph = Paragraph::new(lines)
        // `trim: false` (unlike the error/progress dialogs above): these are
        // left-aligned `"Label: value"` rows, so leading whitespace must be
        // preserved to keep the columns aligned rather than trimmed away.
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::DEFAULT_COLORS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::borrow::Cow;

    #[test]
    fn percent_label_clamps_out_of_range() {
        assert_eq!(percent_label(0), "0%");
        assert_eq!(percent_label(100), "100%");
        // n > 100 must clamp to the last valid label, never index out of bounds.
        assert_eq!(percent_label(101), "100%");
        assert_eq!(percent_label(u16::MAX), "100%");
    }

    fn sample_info() -> PropertiesInfo<'static> {
        PropertiesInfo {
            name: Cow::Borrowed("file.txt"),
            size: Cow::Borrowed("1 KB"),
            mtime: Cow::Borrowed("2024-01-01 00:00"),
            permissions: Cow::Borrowed("rw-r--r--"),
            owner: Cow::Borrowed("user"),
            group: Cow::Borrowed("staff"),
            file_type: Cow::Borrowed("Regular File"),
        }
    }

    fn draw_into(render: impl FnOnce(&mut Frame)) {
        let mut terminal = Terminal::new(TestBackend::new(20, 20)).unwrap();
        terminal.draw(render).unwrap();
    }

    #[test]
    fn dialogs_survive_degenerate_areas() {
        let info = sample_info();
        // Zero-sized and sub-border areas must render without panicking.
        for area in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 4, 4),
        ] {
            draw_into(|f| render_error_dialog(f, area, "Err", "boom", &DEFAULT_COLORS));
            draw_into(|f| {
                // percent > 100 also exercises the `percent_label` clamp.
                render_progress_dialog(f, area, "Work", "copying", 150.0, true, &DEFAULT_COLORS);
            });
            draw_into(|f| render_properties_dialog(f, area, &info, &DEFAULT_COLORS));
        }
    }

    // The NaN→0 saturating cast (f32::NAN as u16) is the exact path under test.
    #[allow(clippy::cast_nan_to_int)]
    #[test]
    fn progress_dialog_nan_percent_does_not_panic() {
        // NaN input: `NaN.clamp(0.0, 100.0)` propagates NaN, `.round()` stays
        // NaN, and `NaN as u16` saturates to 0 (Rust 1.45+ well-defined cast).
        // The gauge must render "0%" without panicking on a normal-sized area.
        let area = Rect::new(0, 0, 20, 6);
        draw_into(|f| {
            render_progress_dialog(f, area, "Work", "copying", f32::NAN, false, &DEFAULT_COLORS);
        });
        // Verify the NaN → 0 cast path that `render_progress_dialog` relies on.
        assert_eq!(percent_label(f32::NAN as u16), "0%");
    }
}
