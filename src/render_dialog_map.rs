use chrono::TimeZone;
use std::borrow::Cow;

use lc::{app, ui};

use app::types::AppState;
use ui::dialogs;

pub(super) fn to_ui_dialog<'a>(
    dialog_kind: &'a app::types::DialogKind,
    state: &'a AppState,
) -> dialogs::DialogKind<'a> {
    match dialog_kind {
        app::types::DialogKind::Confirm(cd) => dialogs::DialogKind::Confirm {
            title: Cow::Borrowed(&cd.title),
            message: Cow::Borrowed(&cd.message),
            selection: state.dialog_selection,
            files: Cow::Borrowed(cd.files.as_deref().unwrap_or(&[])),
        },
        app::types::DialogKind::Input { prompt, .. } => dialogs::DialogKind::Input {
            title: Cow::Borrowed("Input"),
            prompt: Cow::Borrowed(prompt),
            value: Cow::Borrowed(state.dialog_input.text()),
            cursor_pos: state.dialog_input.cursor(),
        },
        app::types::DialogKind::Error(msg) => dialogs::DialogKind::Error {
            title: Cow::Borrowed("Error"),
            message: Cow::Borrowed(msg),
        },
        app::types::DialogKind::Help {
            message,
            scroll_offset,
        } => dialogs::DialogKind::Help {
            title: Cow::Borrowed("Help"),
            message: Cow::Borrowed(message),
            scroll_offset: *scroll_offset,
        },
        app::types::DialogKind::Progress {
            message,
            progress_fraction,
            cancellable,
        } => dialogs::DialogKind::Progress {
            title: Cow::Borrowed("Progress"),
            message: Cow::Borrowed(message),
            percent: progress_fraction.clamp(0.0, 1.0) * 100.0,
            cancellable: *cancellable,
        },
        app::types::DialogKind::CopyMove(details) => {
            let action = if details.is_move { "Move" } else { "Copy" };
            // Per-frame alloc; low cost for short strings.
            let msg = format!(
                "{} {} item(s)\nfrom: {}\n  to: {}",
                action,
                details.source.len(),
                details
                    .source
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                details.dest.display(),
            );
            dialogs::DialogKind::Confirm {
                title: Cow::Borrowed(if details.is_move {
                    "Move Confirm"
                } else {
                    "Copy Confirm"
                }),
                message: Cow::Owned(msg),
                selection: state.dialog_selection,
                files: Cow::Borrowed(&details.source_display),
            }
        }
        app::types::DialogKind::Properties(details) => properties_to_ui_dialog(details),
        app::types::DialogKind::OverwriteConfirm(details) => {
            dialogs::DialogKind::OverwriteConfirm {
                selection: state.dialog_selection,
                files: Cow::Borrowed(&details.conflicting),
            }
        }
        app::types::DialogKind::ArchiveExtract(details) => {
            // Per-frame alloc; low cost for short strings.
            let info = format!(
                "{}\n{} entries",
                details.source.display(),
                details.entries.len()
            );
            dialogs::DialogKind::ArchiveExtract {
                info: Cow::Owned(info),
                dest_value: Cow::Borrowed(details.dest_input.text()),
                dest_cursor: details.dest_input.cursor(),
                selection: state.dialog_selection,
            }
        }
        app::types::DialogKind::ArchiveCreate(details) => dialogs::DialogKind::ArchiveCreate {
            source_count: details.sources.len(),
            dest_value: Cow::Borrowed(details.dest_input.text()),
            dest_cursor: details.dest_input.cursor(),
            selection: state.dialog_selection,
        },
    }
}

fn properties_to_ui_dialog(details: &app::types::PropertiesDetails) -> dialogs::DialogKind<'_> {
    let file_type = if details.is_symlink {
        "Symlink"
    } else if details.is_dir {
        "Directory"
    } else {
        "File"
    };
    let mtime_str = if let Ok(duration) = details.mtime.duration_since(std::time::UNIX_EPOCH) {
        chrono::Local
            .timestamp_opt(i64::try_from(duration.as_secs()).unwrap_or(i64::MAX), 0)
            .single()
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.into())
            .format("%Y-%m-%d %H:%M:%S")
            // Per-frame alloc; low cost for short strings.
            .to_string()
    } else {
        "Unknown".to_string()
    };
    dialogs::DialogKind::Properties {
        info: dialogs::PropertiesInfo {
            name: Cow::Borrowed(details.name.as_str()),
            size: Cow::Owned(app::types::FileEntry::format_size(details.size)), // Per-frame alloc; low cost for short strings.
            mtime: Cow::Owned(mtime_str),
            permissions: Cow::Owned(app::types::FileEntry::display_permissions_raw(
                details.permissions,
            )), // Per-frame alloc; low cost for short strings.
            owner: Cow::Borrowed(details.owner.as_str()),
            group: Cow::Borrowed(details.group.as_str()),
            file_type: Cow::Borrowed(file_type),
        },
    }
}
