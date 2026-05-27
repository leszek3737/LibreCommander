use std::path::PathBuf;

use super::dialogs::{DialogKind, PickerKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Image,
}

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
}

impl CompareMode {
    pub const ALL: [Self; 3] = [Self::Quick, Self::Size, Self::Thorough];

    const _ASSERT_COMPLETE: () = assert!(matches!(
        Self::ALL,
        [Self::Quick, Self::Size, Self::Thorough]
    ));
}

impl PendingAction {
    pub fn set_overwrite(&mut self) {
        match self {
            Self::Copy { overwrite, .. } | Self::Move { overwrite, .. } => {
                *overwrite = true;
            }
            Self::Delete { .. } => {}
        }
    }
}
