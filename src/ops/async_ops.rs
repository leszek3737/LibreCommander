use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn as_arc(&self) -> &Arc<AtomicBool> {
        &self.0
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct AsyncCopyState {
    pub total_files: usize,
    pub done_files: usize,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub current_file_bytes: u64,
    pub current_file_total: u64,
    pub started_at: Instant,
}

impl AsyncCopyState {
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        Self {
            total_files,
            done_files: 0,
            total_bytes,
            done_bytes: 0,
            current_file_bytes: 0,
            current_file_total: 0,
            started_at: Instant::now(),
        }
    }

    pub fn percent(&self) -> f32 {
        let percent = if self.total_bytes > 0 {
            self.done_bytes as f32 / self.total_bytes as f32
        } else if self.total_files > 0 {
            self.done_files as f32 / self.total_files as f32
        } else {
            0.0
        };

        percent.clamp(0.0, 1.0)
    }

    pub fn speed_bytes_per_sec(&self) -> f64 {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            0.0
        } else {
            (self.done_bytes as f64 / elapsed).max(0.0)
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        if self.total_bytes == 0 || self.done_bytes >= self.total_bytes {
            return None;
        }

        let speed = self.speed_bytes_per_sec();
        if speed <= 0.0 {
            return None;
        }

        let remaining_bytes = self.total_bytes.saturating_sub(self.done_bytes);
        Some(Duration::from_secs_f64(remaining_bytes as f64 / speed))
    }

    pub fn start_file(&mut self, file_total: u64) {
        self.current_file_bytes = 0;
        self.current_file_total = file_total;
    }

    pub fn add_bytes(&mut self, cumulative_for_current_file: u64) {
        let current = cumulative_for_current_file.min(self.current_file_total);
        let delta = current.saturating_sub(self.current_file_bytes);
        self.done_bytes = self.done_bytes.saturating_add(delta).min(self.total_bytes);
        self.current_file_bytes = current;
    }

    pub fn finish_file(&mut self) {
        if self.current_file_bytes < self.current_file_total {
            self.add_bytes(self.current_file_total);
        }

        self.done_files = self.done_files.saturating_add(1).min(self.total_files);
        self.current_file_bytes = 0;
        self.current_file_total = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_uses_bytes_when_total_bytes_known() {
        let mut state = AsyncCopyState::new(10, 100);

        state.start_file(100);
        state.add_bytes(25);

        assert_eq!(state.percent(), 0.25);
    }

    #[test]
    fn percent_uses_files_when_total_bytes_unknown() {
        let mut state = AsyncCopyState::new(4, 0);

        state.finish_file();

        assert_eq!(state.percent(), 0.25);
    }

    #[test]
    fn speed_is_non_negative() {
        let state = AsyncCopyState::new(1, 100);

        assert!(state.speed_bytes_per_sec() >= 0.0);
    }

    #[test]
    fn cumulative_add_bytes_does_not_double_count() {
        let mut state = AsyncCopyState::new(1, 100);

        state.start_file(100);
        state.add_bytes(10);
        state.add_bytes(25);
        state.add_bytes(25);

        assert_eq!(state.done_bytes, 25);
        assert_eq!(state.current_file_bytes, 25);
    }

    #[test]
    fn eta_is_none_when_speed_is_impossible() {
        let state = AsyncCopyState::new(1, 100);

        assert_eq!(state.eta(), None);
    }
}
