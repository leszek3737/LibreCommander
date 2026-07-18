use std::borrow::Cow;

use lc::{app, ui};

use app::types::AppState;
use ui::dialogs;

// Dialog titles. Kept local to this module per the current wave's scope (no new
// shared title module yet).
const TITLE_INPUT: &str = "Input";
const TITLE_ERROR: &str = "Error";
const TITLE_HELP: &str = "Help";
const TITLE_PROGRESS: &str = "Progress";

pub(super) fn to_ui_dialog<'a>(
    dialog_kind: &'a app::types::DialogKind,
    state: &'a AppState,
) -> dialogs::DialogKind<'a> {
    match dialog_kind {
        app::types::DialogKind::Confirm(cd) => dialogs::DialogKind::Confirm {
            title: Cow::Borrowed(&cd.title),
            message: Cow::Borrowed(&cd.message),
            selection: state.input.dialog_selection,
            files: Cow::Borrowed(cd.files.as_deref().unwrap_or(&[])),
        },
        app::types::DialogKind::Input { prompt, .. } => dialogs::DialogKind::Input {
            title: Cow::Borrowed(TITLE_INPUT),
            prompt: Cow::Borrowed(prompt),
            value: Cow::Borrowed(state.input.dialog_input.text()),
            cursor_pos: state.input.dialog_input.cursor(),
        },
        app::types::DialogKind::Error(msg) => dialogs::DialogKind::Error {
            title: Cow::Borrowed(TITLE_ERROR),
            message: Cow::Borrowed(msg),
        },
        app::types::DialogKind::Help {
            message,
            scroll_offset,
        } => dialogs::DialogKind::Help {
            title: Cow::Borrowed(TITLE_HELP),
            message: Cow::Borrowed(message),
            scroll_offset: *scroll_offset,
        },
        app::types::DialogKind::Progress {
            message,
            progress_fraction,
            cancellable,
        } => dialogs::DialogKind::Progress {
            title: Cow::Borrowed(TITLE_PROGRESS),
            message: Cow::Borrowed(message),
            percent: progress_fraction.clamp(0.0, 1.0) * 100.0,
            cancellable: *cancellable,
        },
        app::types::DialogKind::Properties(details) => properties_to_ui_dialog(details),
        app::types::DialogKind::OverwriteConfirm(details) => {
            dialogs::DialogKind::OverwriteConfirm {
                selection: state.input.dialog_selection,
                files: Cow::Borrowed(&details.conflicting),
            }
        }
        app::types::DialogKind::ArchiveExtract(details) => {
            // Per-frame alloc. Not cacheable to Cow::Borrowed: composed from a
            // PathBuf display + entry count. A correct cache would need
            // persistent state keyed on the dialog data (forbidden this wave).
            let info = format!(
                "{}\n{} entries",
                details.source.display(),
                details.entries.len()
            );
            dialogs::DialogKind::ArchiveExtract {
                info: Cow::Owned(info),
                dest_value: Cow::Borrowed(details.dest_input.text()),
                dest_cursor: details.dest_input.cursor(),
                selection: state.input.dialog_selection,
            }
        }
        app::types::DialogKind::ArchiveCreate(details) => dialogs::DialogKind::ArchiveCreate {
            source_count: details.sources.len(),
            dest_value: Cow::Borrowed(details.dest_input.text()),
            dest_cursor: details.dest_input.cursor(),
            selection: state.input.dialog_selection,
        },
    }
}

fn properties_to_ui_dialog(details: &app::types::PropertiesDetails) -> dialogs::DialogKind<'_> {
    let file_type = details.kind.label();
    dialogs::DialogKind::Properties {
        info: dialogs::PropertiesInfo {
            name: Cow::Borrowed(details.name.as_str()),
            size: Cow::Borrowed(details.size_str.as_str()),
            mtime: Cow::Borrowed(details.mtime_str.as_str()),
            permissions: Cow::Borrowed(details.permissions_str.as_str()),
            owner: Cow::Borrowed(details.owner.as_str()),
            group: Cow::Borrowed(details.group.as_str()),
            file_type: Cow::Borrowed(file_type),
        },
    }
}
