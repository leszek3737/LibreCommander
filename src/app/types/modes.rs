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
//       Not refactoring now — ~38 call sites would need updating.
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
    pub fn set_overwrite(&mut self) -> bool {
        match self {
            Self::Copy { overwrite, .. } | Self::Move { overwrite, .. } => {
                *overwrite = true;
                true
            }
            Self::Delete { .. } | Self::ExtractArchive { .. } | Self::CreateArchive { .. } => false,
        }
    }
}
