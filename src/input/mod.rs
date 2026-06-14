pub(crate) mod command_line;
pub(crate) mod dialogs;
pub(crate) mod directory_tree;
pub(crate) mod menu_actions;
pub(crate) mod mode_dispatch;
pub(crate) mod mouse;
pub(crate) mod normal;
pub(crate) mod pickers;

use ratatui::layout::Size;

use lc::app::job_runner::RunningJob;
use lc::app::types::AppState;
use lc::ui::viewer;

/// Aggregates the long-lived, per-frame mutable state that the event-dispatch
/// layer threads through the key/mouse handlers. Replaces the 7-8 argument
/// parameter lists that `dispatch_event` and friends used to carry by hand.
///
/// Deliberately *not* generic over the terminal backend: the `Terminal<B>` is
/// passed as a separate argument only to the handlers that actually need it
/// (normal/menu mode spawning an external editor), so the backend type never
/// leaks into this struct and infects the whole call graph with a `B` parameter.
/// Per-event data (key code/modifiers, mouse event) is likewise kept out of the
/// struct and passed separately.
pub(crate) struct EventContext<'a> {
    pub state: &'a mut AppState,
    pub viewer_state: &'a mut Option<viewer::ViewerState>,
    pub viewer_loader: &'a mut Option<viewer::ViewerLoader>,
    pub image_preview_loader: &'a mut Option<viewer::ImagePreviewLoader>,
    pub running_job: &'a mut Option<RunningJob>,
    pub term_size: Size,
}
