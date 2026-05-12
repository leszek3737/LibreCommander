use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, TryLockError};

use chrono::Local;

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

fn ensure_log_file() -> Option<std::fs::File> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

pub fn log(args: std::fmt::Arguments<'_>) {
    let mut guard = match LOG_FILE.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(err)) => err.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    if guard.is_none() {
        *guard = ensure_log_file();
    }
    if let Some(file) = guard.as_mut() {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{timestamp}] {args}");
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
}
