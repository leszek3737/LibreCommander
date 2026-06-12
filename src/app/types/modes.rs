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

#[derive(Debug, Clone, PartialEq)]
pub struct TransferAction {
    pub sources: Vec<PathBuf>,
    pub dest: PathBuf,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    Copy(TransferAction),
    Move(TransferAction),
    Delete {
        paths: Vec<PathBuf>,
    },
    ExtractArchive {
        source: PathBuf,
        dest: PathBuf,
        overwrite: bool,
    },
    CreateArchive {
        sources: Vec<PathBuf>,
        dest: PathBuf,
        format: ArchiveFormat,
        overwrite: bool,
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
            Self::Copy(t) | Self::Move(t) => {
                t.overwrite = true;
            }
            Self::Delete { .. } => {}
            Self::ExtractArchive { overwrite, .. } | Self::CreateArchive { overwrite, .. } => {
                *overwrite = true;
            }
        }
    }
}
