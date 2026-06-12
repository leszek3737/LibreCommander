use std::path::PathBuf;

use super::dialogs::{DialogKind, PickerKind};
use crate::ops::archive::ArchiveFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareMode {
    #[default]
    Quick,
    Size,
    Thorough,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Viewing,
    CommandLine,
    Dialog(DialogKind),
    Search,
    Menu,
    ListPicker(PickerKind),
    DirectoryTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Text,
    Hex,
    Image,
}

// TODO: Copy and Move share identical fields (sources, dest, overwrite).
//       Extract into a shared struct (e.g. TransferAction) to reduce duplication.
//       Plan: introduce `struct TransferAction { sources: Vec<PathBuf>, dest: PathBuf, overwrite: bool }`,
//       replace both variants with `Copy(TransferAction)` / `Move(TransferAction)`.
//       Impact: ~38 call sites (construction, match arms, field access) across ops/, input/, app/.
//       Low risk but high churn — batch with other PendingAction changes.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    Copy {
        sources: Vec<PathBuf>,
        dest: PathBuf,
        overwrite: bool,
    },
    Move {
        sources: Vec<PathBuf>,
        dest: PathBuf,
        overwrite: bool,
    },
    Delete {
        paths: Vec<PathBuf>,
    },
    ExtractArchive {
        source: PathBuf,
        dest: PathBuf,
    },
    CreateArchive {
        sources: Vec<PathBuf>,
        dest: PathBuf,
        format: ArchiveFormat,
    },
}

impl CompareMode {
    pub const ALL: [Self; 3] = [Self::Quick, Self::Size, Self::Thorough];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Quick",
            Self::Size => "Size",
            Self::Thorough => "Thorough",
        }
    }
}

impl PendingAction {
    pub fn set_overwrite(&mut self) {
        match self {
            Self::Copy { overwrite, .. } | Self::Move { overwrite, .. } => {
                *overwrite = true;
            }
            Self::Delete { .. } | Self::ExtractArchive { .. } | Self::CreateArchive { .. } => {}
        }
    }
}
