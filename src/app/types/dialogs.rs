use std::path::PathBuf;
use std::time::SystemTime;

use super::text_input::TextInput;
use crate::ops::archive::ArchiveEntry;

// NOTE (Message newtype / Cow): the message-carrying `String` fields below
// (`ConfirmDetails::{title,message}`, `DialogKind::{Error, Help.message,
// Input.prompt, Progress.message}`) were considered for a `Message` newtype and
// for `Cow<'static, str>`. Both are deferred: these strings are read as `&str`
// by the render layer (`render_dialog_map`) and constructed at ~60 call sites
// across `input::` and the tests, so either change cascades widely without
// adding real invariant safety here. Follow-up: introduce `Message` (and/or
// `Cow`) when the render boundary is reworked.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDetails {
    pub title: String,
    pub message: String,
    pub files: Option<Vec<String>>,
}

impl ConfirmDetails {
    /// Shared constructor; `simple` and `with_files` differ only in `files`.
    fn build(title: &str, message: &str, files: Option<Vec<String>>) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            files,
        }
    }

    pub fn simple(title: &str, message: &str) -> Self {
        Self::build(title, message, None)
    }

    pub fn with_files(title: &str, message: &str, files: Vec<String>) -> Self {
        Self::build(title, message, Some(files))
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

/// The kind of filesystem object a [`PropertiesDetails`] describes. Replaces the
/// former `is_dir: bool` / `is_symlink: bool` flag pair, which could encode the
/// nonsensical "both true / which wins?" states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

impl FileKind {
    /// Build from the legacy `(is_dir, is_symlink)` metadata flags. Symlinks
    /// take precedence over directories, matching the previous render logic.
    pub fn from_metadata_flags(is_dir: bool, is_symlink: bool) -> Self {
        if is_symlink {
            Self::Symlink
        } else if is_dir {
            Self::Directory
        } else {
            Self::File
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Directory => "Directory",
            Self::Symlink => "Symlink",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertiesDetails {
    pub name: String,
    pub size: u64,
    pub mtime: SystemTime,
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub kind: FileKind,
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
    Properties(Box<PropertiesDetails>),
    OverwriteConfirm(Box<OverwriteConfirmDetails>),
    ArchiveExtract(Box<ArchiveExtractDetails>),
    ArchiveCreate(Box<ArchiveCreateDetails>),
}

impl DialogKind {
    pub fn progress(message: String, fraction: f32, cancellable: bool) -> Self {
        Self::Progress {
            message,
            // Intentional fixup: out-of-range inputs (e.g. a computed 1.5 or a
            // negative fraction) are silently snapped into 0.0..=1.0 rather than
            // rejected. Progress is cosmetic, so clamping is preferred over an
            // error path or a debug assertion here.
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
