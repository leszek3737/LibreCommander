use crate::*;
use app::types::{FileEntry, PanelState};
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(80, 24)).unwrap()
}

pub struct TestEntry {
    pub name: String,
    pub path: Option<PathBuf>,
    pub size: u64,
    pub selected: bool,
}

impl TestEntry {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
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
        let mut cha = crate::fs::cha::Cha::dummy_dir();
        if self.size > 0 {
            cha.mode = crate::fs::cha::ChaMode::new(0o100644);
            cha.len = self.size;
            cha.mtime = Some(std::time::SystemTime::now());
            cha.btime = Some(UNIX_EPOCH);
        }
        FileEntry::builder()
            .name(&self.name)
            .path(path)
            .cha(cha)
            .selected(self.selected)
            .build()
    }
}

pub fn populate_panel(panel: &mut PanelState, entries: Vec<FileEntry>) {
    panel.set_entries(entries);
}

pub fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}
