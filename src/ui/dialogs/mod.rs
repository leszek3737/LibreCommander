use std::borrow::Cow;

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Clear},
};

use super::theme::{ColorPalette, Theme};

mod archive;
mod confirm;
mod help;
mod input;
mod layout;
mod list_picker;
mod simple;
mod text;

pub use archive::{render_archive_create_dialog, render_archive_extract_dialog};
pub use confirm::{render_confirm_dialog, render_overwrite_dialog};
pub use help::render_help_dialog;
pub use input::render_input_dialog;
pub use layout::{
    HelpGeometry, centered_rect, help_dialog_geometry, help_message_width, help_visible_height,
    input_dialog_rect,
};
pub use list_picker::{render_list_picker, render_list_picker_with_colors};
pub use simple::{render_error_dialog, render_progress_dialog, render_properties_dialog};
pub use text::wrapped_line_count;

use layout::{DIALOG_HEIGHT_PERCENT, DIALOG_WIDTH_PERCENT};

#[derive(Debug, Clone)]
pub struct PropertiesInfo<'a> {
    pub name: Cow<'a, str>,
    pub size: Cow<'a, str>,
    pub mtime: Cow<'a, str>,
    pub permissions: Cow<'a, str>,
    pub owner: Cow<'a, str>,
    pub group: Cow<'a, str>,
    pub file_type: Cow<'a, str>,
}

#[derive(Debug, Clone)]
pub enum DialogKind<'a> {
    Confirm {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        selection: usize,
        files: Cow<'a, [String]>,
    },
    Input {
        title: Cow<'a, str>,
        prompt: Cow<'a, str>,
        value: Cow<'a, str>,
        cursor_pos: usize,
    },
    Error {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
    },
    Help {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        scroll_offset: usize,
    },
    Progress {
        title: Cow<'a, str>,
        message: Cow<'a, str>,
        percent: f32,
        cancellable: bool,
    },
    Properties {
        info: PropertiesInfo<'a>,
    },
    OverwriteConfirm {
        selection: usize,
        files: Cow<'a, [String]>,
    },
    ArchiveExtract {
        info: Cow<'a, str>,
        dest_value: Cow<'a, str>,
        dest_cursor: usize,
        selection: usize,
    },
    ArchiveCreate {
        source_count: usize,
        dest_value: Cow<'a, str>,
        dest_cursor: usize,
        selection: usize,
    },
}

pub fn render_dialog(f: &mut Frame, dialog: &DialogKind<'_>) {
    render_dialog_with_colors(f, dialog, &ColorPalette::default());
}

pub fn render_dialog_with_colors(f: &mut Frame, dialog: &DialogKind<'_>, colors: &ColorPalette) {
    if matches!(dialog, DialogKind::OverwriteConfirm { files, .. } if files.is_empty()) {
        return;
    }

    let rect = f.area();
    let dialog_area = centered_rect(DIALOG_WIDTH_PERCENT, DIALOG_HEIGHT_PERCENT, rect);

    f.render_widget(Clear, dialog_area);
    let bg_block = Block::default().style(Theme::dialog_with_colors(colors));
    f.render_widget(bg_block, dialog_area);

    dispatch_dialog_render(f, dialog, dialog_area, colors);
}

fn dispatch_dialog_render(
    f: &mut Frame,
    dialog: &DialogKind<'_>,
    area: Rect,
    colors: &ColorPalette,
) {
    match dialog {
        DialogKind::Confirm {
            title,
            message,
            selection,
            files,
        } => render_confirm_dialog(
            f,
            area,
            title.as_ref(),
            message.as_ref(),
            *selection,
            files,
            colors,
        ),
        DialogKind::Input {
            title,
            prompt,
            value,
            cursor_pos,
        } => render_input_dialog(
            f,
            area,
            title.as_ref(),
            prompt.as_ref(),
            value.as_ref(),
            *cursor_pos,
            colors,
        ),
        DialogKind::Error { title, message } => {
            render_error_dialog(f, area, title.as_ref(), message.as_ref(), colors);
        }
        DialogKind::Help {
            title,
            message,
            scroll_offset,
        } => render_help_dialog(
            f,
            area,
            title.as_ref(),
            message.as_ref(),
            *scroll_offset,
            colors,
        ),
        DialogKind::Progress {
            title,
            message,
            percent,
            cancellable,
        } => render_progress_dialog(
            f,
            area,
            title.as_ref(),
            message.as_ref(),
            *percent,
            *cancellable,
            colors,
        ),
        DialogKind::Properties { info } => {
            render_properties_dialog(f, area, info, colors);
        }
        DialogKind::OverwriteConfirm { selection, files } => {
            render_overwrite_dialog(f, area, *selection, files, colors);
        }
        DialogKind::ArchiveExtract {
            info,
            dest_value,
            dest_cursor,
            selection,
        } => render_archive_extract_dialog(
            f,
            area,
            info.as_ref(),
            dest_value.as_ref(),
            *dest_cursor,
            *selection,
            colors,
        ),
        DialogKind::ArchiveCreate {
            source_count,
            dest_value,
            dest_cursor,
            selection,
        } => render_archive_create_dialog(
            f,
            area,
            *source_count,
            dest_value.as_ref(),
            *dest_cursor,
            *selection,
            colors,
        ),
    }
}

#[cfg(test)]
mod tests;
