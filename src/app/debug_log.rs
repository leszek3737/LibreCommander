use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use chrono::Local;

const MIB: u64 = 1024 * 1024;
const MAX_LOG_SIZE_BYTES: u64 = 10 * MIB;
/// Re-check on-disk size every N writes so a long session cannot grow forever.
const SIZE_CHECK_INTERVAL: u32 = 256;

/// File-based debug logger for TUI runtime diagnostics.
/// Location: XDG_CACHE_HOME/lc/debug.log (or ~/.cache/lc/debug.log).
/// Usage: `debug_log!("message: {}", value)`.
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static WRITE_COUNT: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
static TEST_CACHE_HOME: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

fn log_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(cache_dir) = lock_recover(&TEST_CACHE_HOME).as_ref() {
        return cache_dir.join("lc").join("debug.log");
    }

    super::paths::cache_home(&super::paths::ProcessEnv)
        .map(|dir| dir.join("debug.log"))
        .unwrap_or_else(|| std::env::temp_dir().join("lc_debug.log"))
}

fn report_error(tag: &str, error: impl std::fmt::Display) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "[lc:debug_log:{tag}] {error}");
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| {
        report_error(
            "mutex_poison",
            "recovering from poisoned mutex — another thread panicked while holding the lock",
        );
        e.into_inner()
    })
}

fn open_log() -> std::io::Result<std::fs::File> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Truncate when oversized so a stuck session cannot fill the disk.
    if std::fs::metadata(&path)
        .map(|m| m.len() > MAX_LOG_SIZE_BYTES)
        .unwrap_or(false)
    {
        OpenOptions::new().write(true).truncate(true).open(&path)
    } else {
        OpenOptions::new().create(true).append(true).open(&path)
    }
}

fn reopen_if_needed(guard: &mut Option<std::fs::File>) -> bool {
    let oversized = guard
        .as_ref()
        .and_then(|f| f.metadata().ok())
        .is_some_and(|m| m.len() > MAX_LOG_SIZE_BYTES);
    if !oversized {
        return true;
    }
    *guard = None;
    match open_log() {
        Ok(file) => {
            *guard = Some(file);
            true
        }
        Err(e) => {
            report_error("open_error", &e);
            false
        }
    }
}

#[inline(never)]
pub fn log(args: std::fmt::Arguments<'_>) {
    let mut guard = lock_recover(&LOG_FILE);
    if guard.is_none() {
        match open_log() {
            Ok(file) => *guard = Some(file),
            Err(e) => {
                report_error("open_error", &e);
                return;
            }
        }
    }
    let count = WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
    if count.is_multiple_of(SIZE_CHECK_INTERVAL) && !reopen_if_needed(&mut guard) {
        return;
    }
    if let Some(file) = guard.as_mut() {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        if let Err(e) = writeln!(file, "[{stamp}] {args}") {
            report_error("write_error", &e);
            *guard = None;
            return;
        }
        let _ = file.flush();
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
            *lock_recover(&TEST_CACHE_HOME) = Some(dir.path().to_owned());
            Self { _dir: dir }
        }
    }

    impl Drop for TestCacheHome {
        fn drop(&mut self) {
            *lock_recover(&TEST_CACHE_HOME) = None;
        }
    }

    fn reset_for_test() {
        *lock_recover(&LOG_FILE) = None;
        WRITE_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn log_writes_to_file() {
        let _guard = lock_recover(&TEST_MUTEX);
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
    fn log_truncates_oversized_file() {
        let _guard = lock_recover(&TEST_MUTEX);
        let _cache_home = TestCacheHome::new();
        let path = log_path();
        // Concurrent tests may poke process-global LOG_FILE / WRITE_COUNT while
        // TEST_CACHE_HOME redirects here — retry until truncate wins.
        const RETRY_BUDGET: usize = 50;
        let mut truncated = false;
        for attempt in 0..RETRY_BUDGET {
            reset_for_test();
            let _ = std::fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&path, vec![b'X'; (MAX_LOG_SIZE_BYTES + 1) as usize])
                .expect("write oversized log");
            log(format_args!("after truncate {attempt}"));
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len > 0 && len < MAX_LOG_SIZE_BYTES {
                let mut contents = String::new();
                let _ = std::fs::File::open(&path)
                    .expect("open log")
                    .read_to_string(&mut contents);
                assert!(contents.contains("after truncate"));
                truncated = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let _ = std::fs::remove_file(&path);
        reset_for_test();
        assert!(
            truncated,
            "log() never truncated the oversized file within {RETRY_BUDGET} retries"
        );
    }
}
