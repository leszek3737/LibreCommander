use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDetails {
    pub title: String,
    pub message: String,
    pub files: Option<Vec<PathBuf>>,
}

impl ConfirmDetails {
    pub fn simple(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: None,
        }
    }

    pub fn with_files(title: &str, message: &str, files: Vec<PathBuf>) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files: if files.is_empty() { None } else { Some(files) },
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
        progress_fraction: f32,
        cancellable: bool,
    },
    CopyMove {
        source: Vec<PathBuf>,
        dest: PathBuf,
        is_move: bool,
    },
    Properties {
        name: String,
        size: u64,
        mtime: SystemTime,
        permissions: u32,
        owner: String,
        group: String,
        is_dir: bool,
        is_symlink: bool,
    },
    OverwriteConfirm {
        conflicting: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    History,
    Hotlist,
    CompareMode,
    UserMenu,
}
