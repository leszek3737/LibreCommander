mod app_state;
mod dialogs;
mod file_entry;
mod modes;
mod panel;
mod sorting;
mod text_input;

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests;

pub use app_state::AppState;
pub use dialogs::{ConfirmDetails, DialogKind, InputAction, PickerKind};
#[cfg(test)]
pub(crate) use file_entry::sanitize_for_display;
pub(crate) use file_entry::sanitize_name;
pub use file_entry::{
    FileCategory, FileEntry, FileEntryBuilder, FileSize, compute_category, format_permissions,
    format_size, format_time,
};
pub use modes::{AppMode, CompareMode, PendingAction, ViewMode};
pub use panel::{ActivePanel, PanelListing, PanelState};
pub use sorting::{ListingMode, SortMode, SortOptions};
pub use text_input::TextInput;
