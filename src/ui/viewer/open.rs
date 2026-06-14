use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::ViewMode;

use super::hex::HEX_BYTES_PER_LINE;
use super::mime::{is_image_mime, should_open_as_text};
use super::scroll::line_number_column_width;

pub(crate) struct ViewerRenderCache {
    pub(crate) visual_heights: RefCell<Vec<usize>>,
    pub(crate) visual_offsets: RefCell<Vec<usize>>,
    pub(crate) cached_content_width: RefCell<usize>,
    pub(crate) cached_line_num_col_width: Cell<usize>,
    pub(crate) cached_image_size: Option<(u16, u16)>,
    pub(crate) cached_image_text: Option<ratatui::text::Text<'static>>,
}

impl ViewerRenderCache {
    fn new() -> Self {
        Self {
            visual_heights: RefCell::new(Vec::new()),
            visual_offsets: RefCell::new(Vec::new()),
            cached_content_width: RefCell::new(0),
            cached_line_num_col_width: Cell::new(0),
            cached_image_size: None,
            cached_image_text: None,
        }
    }
}

pub struct ViewerState {
    pub file_path: PathBuf,
    pub(crate) line_offsets: Vec<usize>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub line_count: usize,
    pub search_query: Option<String>,
    pub search_matches: Vec<super::SearchMatch>,
    pub(crate) search_matches_by_line: Vec<super::SearchLineMatch>,
    pub current_match: Option<usize>,
    pub wrap_lines: bool,
    pub show_line_numbers: bool,
    pub view_mode: ViewMode,
    pub(crate) raw_bytes: Vec<u8>,
    pub(crate) max_line_width: usize,
    pub detected_mime: Option<String>,
    pub file_size: usize,
    pub has_invalid_utf8: bool,
    pub(crate) originally_binary: bool,
    pub(crate) file_truncated: bool,
    pub(crate) render_cache: ViewerRenderCache,
}

/// Parameters for [`ViewerState::build`]. Groups the per-constructor inputs so
/// the shared assembler stays under the argument-count lint without an
/// `#[allow]`.
struct ViewerInit {
    path: PathBuf,
    raw_bytes: Vec<u8>,
    file_size: usize,
    view_mode: ViewMode,
    detected_mime: Option<String>,
    file_truncated: bool,
    wrap_lines: bool,
    show_line_numbers: bool,
    has_invalid_utf8: bool,
    originally_binary: bool,
}

impl ViewerState {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_cancel(path, None)
    }

    pub(crate) fn compute_line_offsets(bytes: &[u8]) -> Vec<usize> {
        if bytes.is_empty() {
            return vec![0];
        }
        let mut offsets = vec![0];
        offsets.extend(
            memchr::memchr_iter(b'\n', bytes)
                .filter(|&pos| pos + 1 < bytes.len())
                .map(|pos| pos + 1),
        );
        offsets
    }

    fn line_end_excluding_newline(bytes: &[u8], end: usize) -> usize {
        if end > 0 && bytes[end - 1] == b'\n' {
            end - 1
        } else {
            end
        }
    }

    /// Display width of a raw line slice, computed without allocating.
    ///
    /// Valid UTF-8 is measured directly. Invalid UTF-8 is measured the way it
    /// will be displayed: `String::from_utf8_lossy` substitutes exactly one
    /// `U+FFFD` (display width 1) per maximal ill-formed subsequence, which is
    /// precisely the unit [`str::utf8_chunks`] yields, so we add 1 per invalid
    /// chunk instead of materializing the lossy `String`.
    pub(crate) fn line_display_width(bytes: &[u8]) -> usize {
        use unicode_width::UnicodeWidthStr;
        match std::str::from_utf8(bytes) {
            Ok(s) => UnicodeWidthStr::width(s),
            Err(_) => {
                let mut w = 0;
                for chunk in bytes.utf8_chunks() {
                    w += UnicodeWidthStr::width(chunk.valid());
                    if !chunk.invalid().is_empty() {
                        w += 1;
                    }
                }
                w
            }
        }
    }

    fn line_bytes_in<'a>(raw_bytes: &'a [u8], line_offsets: &[usize], idx: usize) -> &'a [u8] {
        let Some(&start) = line_offsets.get(idx) else {
            return b"";
        };
        let end = line_offsets
            .get(idx + 1)
            .copied()
            .unwrap_or(raw_bytes.len());
        let line_end = Self::line_end_excluding_newline(raw_bytes, end);
        if line_end <= start {
            return b"";
        }
        &raw_bytes[start..line_end]
    }

    /// Display width of logical line `idx`, computed straight from the raw
    /// bytes. Used by the wrap-layout hot path so it never has to decode (and,
    /// for invalid UTF-8, allocate) a `String` just to measure a line.
    pub(crate) fn line_width(&self, idx: usize) -> usize {
        Self::line_display_width(Self::line_bytes_in(
            &self.raw_bytes,
            &self.line_offsets,
            idx,
        ))
    }

    pub(crate) fn compute_max_line_width(line_offsets: &[usize], raw_bytes: &[u8]) -> usize {
        (0..line_offsets.len())
            .map(|i| Self::line_display_width(Self::line_bytes_in(raw_bytes, line_offsets, i)))
            .max()
            .unwrap_or(0)
    }

    /// Decodes logical line `idx` for display.
    ///
    /// Valid UTF-8 lines are returned borrowed (zero-copy); invalid UTF-8 lines
    /// are decoded lossily into an owned `String`. The per-frame wrap-layout
    /// pass measures widths via [`Self::line_width`] (no decode), so the only
    /// repeated `Cow::Owned` allocations are for the handful of lines actually
    /// visible on screen.
    pub fn get_line(&self, idx: usize) -> Cow<'_, str> {
        if self.raw_bytes.is_empty() && idx == 0 {
            return Cow::Borrowed("[Empty file]");
        }
        let bytes = Self::line_bytes_in(&self.raw_bytes, &self.line_offsets, idx);
        if bytes.is_empty() {
            return Cow::Borrowed("");
        }
        match std::str::from_utf8(bytes) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(String::from_utf8_lossy(bytes).into_owned()),
        }
    }

    /// Computes the text-mode line metrics shared by every constructor and by
    /// the hex→text toggle: byte offsets of each logical line, the line count
    /// (an empty file still reports one line for the `[Empty file]`
    /// placeholder), and the widest line's display width.
    pub(crate) fn compute_text_metrics(raw_bytes: &[u8]) -> (Vec<usize>, usize, usize) {
        let line_offsets = Self::compute_line_offsets(raw_bytes);
        let line_count = if raw_bytes.is_empty() {
            1
        } else {
            line_offsets.len()
        };
        let max_line_width = if raw_bytes.is_empty() {
            0
        } else {
            Self::compute_max_line_width(&line_offsets, raw_bytes)
        };
        (line_offsets, line_count, max_line_width)
    }

    /// Shared field assembly for every constructor. Computes line metrics from
    /// `view_mode` (text vs hex/image) and fills the render cache, so the two
    /// public entry points only differ in the flags they pass in.
    fn build(init: ViewerInit) -> Self {
        let ViewerInit {
            path,
            raw_bytes,
            file_size,
            view_mode,
            detected_mime,
            file_truncated,
            wrap_lines,
            show_line_numbers,
            has_invalid_utf8,
            originally_binary,
        } = init;

        let (line_offsets, line_count, max_line_width) = if matches!(view_mode, ViewMode::Text) {
            Self::compute_text_metrics(&raw_bytes)
        } else {
            let line_count = if raw_bytes.is_empty() {
                1
            } else {
                raw_bytes.len().div_ceil(HEX_BYTES_PER_LINE)
            };
            (Vec::new(), line_count, 0)
        };

        let render_cache = ViewerRenderCache::new();
        render_cache
            .cached_line_num_col_width
            .set(line_number_column_width(line_count));

        ViewerState {
            file_path: path,
            line_offsets,
            scroll_offset: 0,
            horizontal_offset: 0,
            line_count,
            search_query: None,
            search_matches: Vec::new(),
            search_matches_by_line: Vec::new(),
            current_match: None,
            wrap_lines,
            show_line_numbers,
            view_mode,
            raw_bytes,
            max_line_width,
            detected_mime,
            file_size,
            has_invalid_utf8,
            originally_binary,
            file_truncated,
            render_cache,
        }
    }

    fn new_normal(
        path: &Path,
        raw_bytes: Vec<u8>,
        file_size: usize,
        view_mode: ViewMode,
        detected_mime: Option<String>,
        file_truncated: bool,
    ) -> Self {
        let has_invalid_utf8 = !raw_bytes.is_empty() && std::str::from_utf8(&raw_bytes).is_err();
        let originally_binary = !matches!(view_mode, ViewMode::Text);
        Self::build(ViewerInit {
            path: path.to_path_buf(),
            raw_bytes,
            file_size,
            view_mode,
            detected_mime,
            file_truncated,
            wrap_lines: true,
            show_line_numbers: false,
            has_invalid_utf8,
            originally_binary,
        })
    }

    fn new_text_listing(
        path: &Path,
        raw_bytes: Vec<u8>,
        file_size: usize,
        wrap_lines: bool,
        show_line_numbers: bool,
    ) -> Self {
        Self::build(ViewerInit {
            path: path.to_path_buf(),
            raw_bytes,
            file_size,
            view_mode: ViewMode::Text,
            detected_mime: Some("text/plain".to_string()),
            file_truncated: false,
            wrap_lines,
            show_line_numbers,
            // Archive listings are generated text we produce ourselves, so they
            // are always valid UTF-8 — never flag the invalid-UTF-8 warning.
            has_invalid_utf8: false,
            originally_binary: false,
        })
    }

    pub(crate) fn open_with_cancel(path: &Path, cancel: Option<&AtomicBool>) -> io::Result<Self> {
        const MAX_VIEW_SIZE: usize = 100 * 1024 * 1024;
        const READ_CHUNK: usize = 64 * 1024;

        let meta = fs::metadata(path)?;
        if meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("cannot open directory as viewer file: {}", path.display()),
            ));
        }
        let file_size = usize::try_from(meta.len()).unwrap_or(usize::MAX);

        if crate::app::file_type::is_archive(&path.to_string_lossy())
            && let Some(state) = Self::open_as_archive_listing(path, file_size)
        {
            return Ok(state);
        }

        let file = fs::File::open(path)?;
        let mut raw_bytes = Vec::with_capacity(file_size.min(MAX_VIEW_SIZE + 1));
        let mut reader = file.take((MAX_VIEW_SIZE + 1) as u64);
        let mut buf = [0u8; READ_CHUNK];
        loop {
            if let Some(c) = cancel
                && c.load(Ordering::Relaxed)
            {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            raw_bytes.extend_from_slice(&buf[..n]);
        }
        let file_truncated = raw_bytes.len() > MAX_VIEW_SIZE;
        if file_truncated {
            raw_bytes.truncate(MAX_VIEW_SIZE);
        }
        let mime =
            crate::app::mime::detect_mime_from_bytes(path, &raw_bytes[..raw_bytes.len().min(8192)]);

        let open_as_text = should_open_as_text(path, mime.as_deref(), &raw_bytes);
        let is_image = is_image_mime(mime.as_deref());
        let view_mode = if is_image {
            ViewMode::Image
        } else if open_as_text {
            ViewMode::Text
        } else {
            ViewMode::Hex
        };

        Ok(Self::new_normal(
            path,
            raw_bytes,
            file_size,
            view_mode,
            mime,
            file_truncated,
        ))
    }

    pub fn open_background(path: PathBuf) -> super::loader::ViewerLoader {
        super::loader::ViewerLoader::start(path)
    }

    fn open_as_archive_listing(path: &Path, file_size: usize) -> Option<Self> {
        let text = Self::format_archive_listing(path).ok()?;
        Some(Self::new_text_listing(
            path,
            text.into_bytes(),
            file_size,
            false,
            true,
        ))
    }

    fn format_archive_listing(path: &Path) -> io::Result<String> {
        use crate::ops::archive::list::list_archive;
        use std::fmt::Write;

        let entries = list_archive(path).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to list archive {}: {e}", path.display()),
            )
        })?;
        let mut out = String::new();

        let mut write_section = || -> Result<(), std::fmt::Error> {
            writeln!(out, "Archive: {}", path.display())?;
            writeln!(out, "Entries: {}", entries.len())?;
            writeln!(out)?;
            writeln!(out, "  {:<8} {:<20} Name", "Size", "Modified")?;
            writeln!(out, "  {:<8} {:<20} ----", "----", "--------")?;
            for entry in &entries {
                let size = if entry.is_dir {
                    "<DIR>".to_string()
                } else {
                    crate::app::types::format_size(entry.size)
                };
                let mtime = entry
                    .modified
                    .map(crate::app::types::format_time)
                    .unwrap_or_default();
                let name = if entry.is_dir {
                    format!("{}/", entry.name)
                } else {
                    entry.name.to_string()
                };
                writeln!(out, "  {size:<8} {mtime:<20} {name}")?;
            }
            Ok(())
        };

        // Writing into a `String` only fails if a `Display` impl errors, which
        // none of these do; surface it as I/O data corruption rather than
        // discarding the cause.
        write_section().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(out)
    }

    #[must_use]
    pub fn is_hex_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Hex)
    }

    #[must_use]
    pub fn is_image_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Image)
    }

    pub fn image_content_size(area_width: u16, area_height: u16) -> (u16, u16) {
        (area_width, area_height.saturating_sub(3))
    }
}
