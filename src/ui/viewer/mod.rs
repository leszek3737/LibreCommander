mod hex;
mod loader;
mod mime;
mod open;
mod render;
mod scroll;
mod search;
mod toggle;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

pub use loader::{ImagePreviewLoader, ViewerLoader};
pub use open::ViewerState;
pub use render::{
    render_hex_view_with_colors, render_image_view_with_colors, render_loading_with_colors,
    render_viewer_with_colors,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchLineMatch {
    pub(crate) line: usize,
    pub(crate) global_idx: usize,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}
