use crate::input::mode_dispatch::handle_normal_mode;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use lc::app::dir_tree::TreeEntry;
use lc::app::job_runner::RunningJob;
use lc::app::types::AppState;
use lc::ui::viewer;
use ratatui::layout::Size;
use ratatui::{Terminal, backend::TestBackend};

pub const TERMINAL_HEIGHT: u16 = 24;
pub const TERMINAL_WIDTH: u16 = 80;
pub const VISIBLE_HEIGHT: usize = 20;

pub fn test_size() -> Size {
    Size::new(TERMINAL_WIDTH, TERMINAL_HEIGHT)
}

/// Everything `dispatch_event` may have produced as a side effect, captured for
/// inspection. The struct is constructed *only* by [`dispatch_test_event`], so
/// fields can be added freely: every call-site destructures with a `..` rest
/// pattern (`DispatchResult { handled, .. }`) and stays source-compatible.
///
/// `viewer_loader` is the background viewer-loader handle that `dispatch_event`
/// leaves in the `EventContext`. Exposing it lets a test assert that an action
/// *did* (or did not) kick off a viewer load:
/// `dispatch_test_event_exposes_viewer_loader_on_f3` drives F3 on a file and
/// asserts `viewer_loader` is populated. Previously it was a hardcoded throwaway
/// inside the harness and thus unobservable.
pub struct DispatchResult {
    pub handled: Result<bool, std::convert::Infallible>,
    pub viewer: Option<viewer::ViewerState>,
    pub viewer_loader: Option<viewer::ViewerLoader>,
    pub job: Option<RunningJob>,
}

pub fn dispatch_test_event(
    state: &mut AppState,
    terminal: &mut Terminal<TestBackend>,
    event: &Event,
) -> DispatchResult {
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut viewer_loader: Option<viewer::ViewerLoader> = None;
    let mut image_preview_loader: Option<viewer::ImagePreviewLoader> = None;
    let mut job: Option<RunningJob> = None;
    let handled = {
        let mut ctx = crate::input::EventContext {
            state,
            viewer_state: &mut viewer,
            viewer_loader: &mut viewer_loader,
            image_preview_loader: &mut image_preview_loader,
            running_job: &mut job,
            term_size: test_size(),
        };
        super::super::dispatch_event(&mut ctx, terminal, event)
    };
    // `image_preview_loader` is bound only to satisfy `EventContext`'s `&mut`
    // field; it is intentionally not exposed on `DispatchResult` because no test
    // reads it (and any loader it captured is dropped here, cancelling its worker).
    DispatchResult {
        handled,
        viewer,
        viewer_loader,
        job,
    }
}

pub fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(TERMINAL_WIDTH, TERMINAL_HEIGHT)).unwrap()
}

// `TestEntry` is defined under `src/app/types/` because it builds a
// `crate::app::types::FileEntry` via `FileEntry::new` / `TestEntry` —
// machinery only reachable from inside the `lc` library crate under `cfg(test)`.
// This integration suite is a *separate* test crate, so it cannot name that
// module directly. The `#[path]` attribute mounts the very same source file into
// this crate's module tree, re-using one `TestEntry` builder across both worlds
// instead of duplicating it (which would drift out of sync with `FileEntry`).
#[path = "../app/types/test_helpers.rs"]
mod test_helpers;

pub use test_helpers::TestEntry;

pub fn test_path(name: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    std::path::PathBuf::from("/lc-test-fixtures").join(name)
}

pub fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area();
    // Pre-size for the worst case: every cell contributes one char (`width`
    // per row) plus one `'\n'` separator per row — hence the `+ 1`. (Strictly
    // there are only `height - 1` separators since the last row has no trailing
    // newline, but rounding up by one row avoids a reallocation and keeps the
    // arithmetic obvious. Wide glyphs may push past this, but it is just a hint.)
    let mut result = String::with_capacity((area.width as usize + 1) * area.height as usize);
    for y in 0..area.height {
        if y > 0 {
            result.push('\n');
        }
        for x in 0..area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                result.push_str(cell.symbol());
            }
        }
    }
    result
}

pub fn dispatch_key(
    state: &mut AppState,
    key: KeyCode,
    modifiers: KeyModifiers,
    terminal: &mut Terminal<TestBackend>,
) {
    let mut viewer_state = None;
    let mut viewer_loader = None;
    let mut image_preview_loader = None;
    let mut running_job = None;
    let mut ctx = crate::input::EventContext {
        state,
        viewer_state: &mut viewer_state,
        viewer_loader: &mut viewer_loader,
        image_preview_loader: &mut image_preview_loader,
        running_job: &mut running_job,
        term_size: test_size(),
    };
    handle_normal_mode(&mut ctx, key, modifiers, terminal);
}

/// Drive `handle_dialog` against a freshly built `EventContext`. The viewer and
/// running-job handles are local throwaways for the all-`None` call-sites; tests
/// that need to inspect the job after the call should build the context inline.
pub fn dialog_key(state: &mut AppState, key: KeyCode, size: Size) {
    let mut viewer_state = None;
    let mut viewer_loader = None;
    let mut image_preview_loader = None;
    let mut running_job = None;
    let mut ctx = crate::input::EventContext {
        state,
        viewer_state: &mut viewer_state,
        viewer_loader: &mut viewer_loader,
        image_preview_loader: &mut image_preview_loader,
        running_job: &mut running_job,
        term_size: size,
    };
    crate::input::dialogs::handle_dialog(&mut ctx, key, crossterm::event::KeyModifiers::NONE);
}

pub fn dummy_tree_entries(count: usize) -> Vec<TreeEntry> {
    (0..count)
        .map(|i| {
            let name = format!("entry-{i}");
            let name_width = unicode_width::UnicodeWidthStr::width(name.as_str());
            TreeEntry {
                path: std::env::temp_dir().join(format!("{i}")),
                depth: 0,
                is_dir: false,
                expanded: false,
                name,
                name_width,
                read_error: false,
            }
        })
        .collect()
}
