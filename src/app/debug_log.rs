use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, TryLockError};

use chrono::Local;

const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;

/// Simple file-based debug logger for runtime diagnostics during TUI operation.
/// Writes to a single log file, thread-safe via Mutex.
/// Log location: follows XDG_CACHE_HOME or falls back to ~/.cache/lc/debug.log
///
/// Usage: `debug_log!("message: {}", value)` — same syntax as eprintln!
///
/// For pre-TUI and post-TUI output, use eprintln!/println! with #[allow] instead.
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

#[cfg(test)]
static TEST_CACHE_HOME: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

fn log_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(cache_dir) = TEST_CACHE_HOME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        return cache_dir.join("lc").join("debug.log");
    }

    super::paths::cache_home(&super::paths::ProcessEnv)
        .map(|dir| dir.join("debug.log"))
        .unwrap_or_else(|| std::env::temp_dir().join("lc_debug.log"))
}

static CHECK_COUNTER: AtomicU32 = AtomicU32::new(0);
const CHECK_INTERVAL: u32 = 256;

fn ensure_log_file() -> std::io::Result<std::fs::File> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new().create(true).append(true).open(&path)
}

/// Fallback stderr write — used only when the debug logger itself fails.
/// This module is the last-resort logger, so stderr is the only remaining channel.
fn stderr_fallback(msg: &str) {
    let _ = std::io::stderr().write_all(msg.as_bytes());
    let _ = std::io::stderr().write_all(b"\n");
    let _ = std::io::stderr().flush();
}

pub fn log(args: std::fmt::Arguments<'_>) {
    let mut guard = match LOG_FILE.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(err)) => err.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    let freshly_opened = guard.is_none();
    if freshly_opened {
        match ensure_log_file() {
            Ok(file) => *guard = Some(file),
            Err(e) => {
                stderr_fallback(&format!("[lc:debug_log:open_error] {e}"));
                return;
            }
        }
    }
    let need_size_check = freshly_opened
        || CHECK_COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(CHECK_INTERVAL);
    if need_size_check
        && guard
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() > MAX_LOG_SIZE)
    {
        *guard = None;
        let path = log_path();
        let _ = std::fs::File::create(&path).and_then(|f| f.sync_all());
        match ensure_log_file() {
            Ok(f) => *guard = Some(f),
            Err(e) => {
                stderr_fallback(&format!("[lc:debug_log:open_error] {e}"));
                return;
            }
        }
    }
    if let Some(file) = guard.as_mut() {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        if let Err(e) = writeln!(file, "[{timestamp}] {args}") {
            stderr_fallback(&format!("[lc:debug_log:write_error] {e}"));
        }
    }
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::app::debug_log::log(format_args!($($arg)*))
    };
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Read;

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct TestCacheHome {
        _dir: tempfile::TempDir,
    }

    impl TestCacheHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create temporary cache directory");
            *TEST_CACHE_HOME.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(dir.path().to_owned());
            Self { _dir: dir }
        }
    }

    impl Drop for TestCacheHome {
        fn drop(&mut self) {
            *TEST_CACHE_HOME.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn reset_for_test() {
        let mut guard = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
        CHECK_COUNTER.store(0, Ordering::SeqCst);
    }

    #[test]
    fn log_writes_to_file() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _cache_home = TestCacheHome::new();
        let path = log_path();
        reset_for_test();
        let _ = std::fs::remove_file(&path);

        log(format_args!("test message"));

        let mut file = std::fs::File::open(&path).expect("open debug log from test cache");
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .expect("read debug log contents");
        assert!(contents.contains("test message"));
        assert!(contents.starts_with('['));

        let _ = std::fs::remove_file(&path);
        reset_for_test();
    }

    #[test]
    fn log_returns_when_mutex_contended() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        reset_for_test();
        let _guard = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());

        log(format_args!("dropped message"));
    }

    #[test]
    fn log_truncates_oversized_file() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _cache_home = TestCacheHome::new();
        let path = log_path();
        reset_for_test();
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        {
            let mut f = std::fs::File::create(&path).expect("create oversized log");
            std::io::Write::write_all(&mut f, &vec![b'X'; (MAX_LOG_SIZE + 1) as usize])
                .expect("write oversized log");
        }

        let mut truncated = false;
        for attempt in 0..20 {
            reset_for_test();
            log(format_args!("attempt {attempt}"));
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len > 0 && len < MAX_LOG_SIZE {
                let mut contents = String::new();
                let _ = std::fs::File::open(&path)
                    .expect("open log")
                    .read_to_string(&mut contents);
                assert!(contents.contains("attempt"));
                truncated = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let _ = std::fs::remove_file(&path);
        reset_for_test();
        assert!(
            truncated,
            "log() never acquired mutex to truncate oversized file after 20 retries"
        );
    }
}
