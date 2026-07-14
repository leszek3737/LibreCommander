mod hex;
mod loader;
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

/// A single logical search match — one entry per query occurrence.
///
/// This is the unified match representation: navigation (`next`/`prev`/current),
/// the match count, and scroll-to-match all index into the `search_matches`
/// list. Offsets are byte offsets in a single coordinate space:
/// - text view: bytes into the match's starting line ([`ViewerState::get_line`]);
/// - hex view: column bytes into the rendered hex line.
///
/// A logical match that spans several rendered lines (only possible in hex view)
/// is stored here once, anchored at its first line, and is expanded into one
/// [`SearchLineMatch`] per line for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SearchMatch {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A per-rendered-line segment of a [`SearchMatch`].
///
/// `global_idx` back-references the owning logical match in `search_matches`,
/// so the renderer can tell whether a segment belongs to the current match
/// (for the distinct "current" highlight). All offsets are byte offsets in the
/// same coordinate space as [`SearchMatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchLineMatch {
    pub(crate) line: usize,
    pub(crate) global_idx: usize,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}
