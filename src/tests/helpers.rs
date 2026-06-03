use crate::input::mode_dispatch::handle_normal_mode;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use lc::app::dir_tree::TreeEntry;
use lc::app::job_runner::RunningJob;
use lc::app::types::AppState;
use lc::app::types::FileEntry;
use lc::ui::viewer;
use ratatui::layout::Size;
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;

pub const TERMINAL_HEIGHT: u16 = 24;
pub const TERMINAL_WIDTH: u16 = 80;
pub const VISIBLE_HEIGHT: usize = 20;

pub fn test_size() -> Size {
    Size::new(TERMINAL_WIDTH, TERMINAL_HEIGHT)
}

pub struct DispatchResult {
    pub handled: Result<bool, std::convert::Infallible>,
    pub viewer: Option<viewer::ViewerState>,
    pub job: Option<RunningJob>,
}

pub fn dispatch_test_event(
    state: &mut AppState,
    terminal: &mut Terminal<TestBackend>,
    event: &Event,
) -> DispatchResult {
    let mut viewer: Option<viewer::ViewerState> = None;
    let mut job: Option<RunningJob> = None;
    let mut size = test_size();
    let handled = super::super::dispatch_event(
        state,
        &mut viewer,
        &mut None,
        &mut None,
        &mut job,
        terminal,
        &mut size,
        event,
    );
    DispatchResult {
        handled,
        viewer,
        job,
    }
}

pub fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(TERMINAL_WIDTH, TERMINAL_HEIGHT)).unwrap()
}

pub struct TestEntry {
    pub name: String,
    pub path: Option<PathBuf>,
    pub size: u64,
    pub selected: bool,
}

impl TestEntry {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.is_empty(), "TestEntry name must not be empty");
        Self {
            name,
            path: None,
            size: 0,
            selected: false,
        }
    }

    pub fn path(mut self, p: impl Into<PathBuf>) -> Self {
        self.path = Some(p.into());
        self
    }

    pub fn size(mut self, s: u64) -> Self {
        self.size = s;
        self
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn build(self) -> FileEntry {
        let path = self
            .path
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/{}", self.name)));
        let cha = if self.size > 0 {
            crate::fs::cha::Cha::regular_file(self.size)
        } else {
            crate::fs::cha::Cha::dummy_dir()
        };
        FileEntry::builder()
            .name(&self.name)
            .path(path)
            .cha(cha)
            .selected(self.selected)
            .build()
    }
}

pub fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area();
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
    handle_normal_mode(
        state,
        &mut None,
        &mut None,
        key,
        modifiers,
        TERMINAL_HEIGHT,
        terminal,
    );
}

pub fn dummy_tree_entries(count: usize) -> Vec<TreeEntry> {
    (0..count)
        .map(|i| TreeEntry {
            path: PathBuf::from(format!("/tmp/{i}")),
            depth: 0,
            is_dir: false,
            expanded: false,
            name: format!("entry-{i}"),
            read_error: false,
        })
        .collect()
}
