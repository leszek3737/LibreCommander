mod app_state;
mod dialogs;
mod file_entry;
mod modes;
mod panel;
mod sorting;
mod text_input;

#[cfg(test)]
pub(crate) mod test_helpers;

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests;

// --- Re-exports -----------------------------------------------------------
// Grouped by shape: data types (structs/enums) first, then free utility
// functions. WS-E debt: this is a flat ~30-symbol facade; a later pass could
// split it into per-concern submodule facades if the surface keeps growing.

// State containers & aggregates (AppState plus its extracted sub-states).
pub use app_state::{AppState, InputState, InteractionState, TreeState, UiState};

// Dialog data types.
pub use dialogs::{
    ArchiveCreateDetails, ArchiveExtractDetails, ConfirmDetails, DialogKind, FileKind, InputAction,
    OverwriteConfirmDetails, PickerKind, PropertiesDetails,
};

// File-entry data types.
pub use file_entry::{FileCategory, FileEntry};

// Modes & actions.
pub use modes::{AppMode, CompareMode, PendingAction, TransferAction, ViewMode};

// Panel data types. WS-E debt: `panel::ToggleResult` is intentionally NOT
// re-exported — `toggle_selection`'s result is ignored at the only caller, so no
// external code names the type yet. Re-export it once a caller consumes it.
pub use panel::{ActivePanel, ListingState, PanelListing, PanelState};

// Sorting data types. WS-E debt: `sorting::ParseSortError` is intentionally NOT
// re-exported — it is only produced by the in-crate `FromStr` impls and has no
// external consumer yet (config uses serde, not `FromStr`). Re-export when named
// outside `types`.
pub use sorting::{Direction, ListingMode, SortField, SortMode, SortOptions};

pub use text_input::TextInput;

// Utility functions (not data types). Kept in a separate group from the type
// re-exports above. The free `format_permissions` wrapper was removed (WS-B);
// callers use `FileEntry::display_permissions_raw` directly.
pub use file_entry::{compute_category, format_size, format_time};

// `sanitize_for_display` is only needed by test helpers.
#[cfg(test)]
pub(crate) use file_entry::sanitize_for_display;
