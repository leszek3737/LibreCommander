use std::path::PathBuf;
use std::time::SystemTime;

use super::text_input::TextInput;
use crate::ops::archive::ArchiveEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDetails {
    pub title: String,
    pub message: String,
    pub files: Option<Vec<String>>,
}

impl ConfirmDetails {
    pub fn simple(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: None,
        }
    }

    pub fn with_files(title: &str, message: &str, files: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: Some(files),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    CreateDirectory,
    Rename,
    Chmod,
    Filter,
    QuickCd,
    FindFile,
    ViewerSearch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopyMoveDetails {
    pub source: Vec<PathBuf>,
    pub dest: PathBuf,
    pub is_move: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertiesDetails {
    pub name: String,
    pub size: u64,
    pub mtime: SystemTime,
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverwriteConfirmDetails {
    pub conflicting: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveExtractDetails {
    pub source: PathBuf,
    pub entries: Vec<ArchiveEntry>,
    pub dest_input: TextInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveCreateDetails {
    pub sources: Vec<PathBuf>,
    pub dest_input: TextInput,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogKind {
    Confirm(ConfirmDetails),
    Input {
        prompt: String,
        action: InputAction,
    },
    Error(String),
    Help {
        message: String,
        scroll_offset: usize,
    },
    Progress {
        message: String,
        // Range: 0.0..=1.0; validated via clamp in DialogKind::progress() constructor.
        progress_fraction: f32,
        cancellable: bool,
    },
    // Boxed to keep the enum small — these variants carry large structs that would
    // inflate the discriminant size for all other variants (enum size optimization).
    CopyMove(Box<CopyMoveDetails>),
    Properties(Box<PropertiesDetails>),
    OverwriteConfirm(Box<OverwriteConfirmDetails>),
    ArchiveExtract(Box<ArchiveExtractDetails>),
    ArchiveCreate(Box<ArchiveCreateDetails>),
}

impl DialogKind {
    pub fn progress(message: String, fraction: f32, cancellable: bool) -> Self {
        Self::Progress {
            message,
            progress_fraction: fraction.clamp(0.0, 1.0),
            cancellable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    History,
    Hotlist,
    CompareMode,
    UserMenu,
    ArchiveMenu,
}
