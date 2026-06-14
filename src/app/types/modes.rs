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

/// Copy/move payload shared by [`PendingAction::Copy`] and
/// [`PendingAction::Move`].
///
/// NOTE (overwrite unification): the `overwrite` flag is intentionally a bare
/// `bool` here and is mirrored in [`PendingAction::ExtractArchive`] /
/// [`PendingAction::CreateArchive`]. The duplication is centralized behind the
/// [`PendingAction::set_overwrite`] mutator and the [`PendingAction::overwrite`]
/// reader so call sites never poke individual variants. An `Overwrite` newtype
/// was considered but deferred: the flag is consumed as a plain `bool` by ~40
/// `ops::` functions (copy/move/archive), so a newtype would cascade `.0`
/// conversions far outside this module. Follow-up: introduce `Overwrite` once
/// the `ops` boundary takes it natively.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferAction {
    // NOTE (NonEmpty): `sources` must be non-empty for the action to be valid,
    // but is typed as `Vec<PathBuf>` because no `NonEmpty`/`nonempty` crate is
    // in the dependency tree. Follow-up: model emptiness out once a suitable
    // type is available (do not add a dependency just for this).
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
    // NOTE: `ALL` is hand-maintained and MUST stay in sync with the variants
    // above. The idiomatic fix is `#[derive(strum::EnumIter)]`, but `strum` is
    // not a dependency of this crate. Follow-up: switch to `EnumIter` if/when
    // `strum` is added (do not add the dependency solely for this).
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
    /// Marks the action as "overwrite existing targets".
    ///
    /// Unifies the duplicated `overwrite` flag across every variant in one place.
    /// In-place (`&mut self`) so callers can mutate the action directly inside
    /// `Option` without a `take()`/re-insert dance. `Delete` has no destination
    /// to overwrite, so it is a no-op.
    pub fn set_overwrite(&mut self) {
        match self {
            Self::Copy(t) | Self::Move(t) => t.overwrite = true,
            Self::Delete { .. } => {}
            Self::ExtractArchive { overwrite, .. } | Self::CreateArchive { overwrite, .. } => {
                *overwrite = true;
            }
        }
    }

    /// Unified reader for the (duplicated) `overwrite` flag. `Delete` never
    /// overwrites and always reports `false`.
    pub fn overwrite(&self) -> bool {
        match self {
            Self::Copy(t) | Self::Move(t) => t.overwrite,
            Self::Delete { .. } => false,
            Self::ExtractArchive { overwrite, .. } | Self::CreateArchive { overwrite, .. } => {
                *overwrite
            }
        }
    }
}
