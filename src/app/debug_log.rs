use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use chrono::Local;

const MIB: u64 = 1024 * 1024;
const MAX_LOG_SIZE_BYTES: u64 = 10 * MIB;

/// Simple file-based debug logger for runtime diagnostics during TUI operation.
/// Writes to a single log file, thread-safe via Mutex.
/// Log location: follows XDG_CACHE_HOME or falls back to ~/.cache/lc/debug.log
///
/// Usage: `debug_log!("message: {}", value)` — same syntax as eprintln!
///
/// For pre-TUI and post-TUI output, use eprintln!/println! with #[allow] instead.
///
/// **Blocking behavior:** The internal mutex uses a blocking lock. On a stalled
/// filesystem (network mount, writeback pressure) this will block the calling
/// thread until the lock is released. Do not call `debug_log!` from paths where
/// filesystem latency is expected to be high.
static LOG_FILE: Mutex<Option<BufWriter<std::fs::File>>> = Mutex::new(None);

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

static CHECK_COUNTER: AtomicU32 = AtomicU32::new(0);
const CHECK_INTERVAL: u32 = 256;

fn ensure_log_file() -> std::io::Result<BufWriter<std::fs::File>> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map(BufWriter::new)
}

/// Fallback stderr write — used only when the debug logger itself fails.
/// This module is the last-resort logger, so stderr is the only remaining channel.
fn stderr_fallback(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(msg.as_bytes());
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| {
        stderr_fallback("[lc:debug_log:mutex_poison] recovering from poisoned mutex — another thread panicked while holding the lock");
        e.into_inner()
    })
}

#[inline(never)]
pub fn log(args: std::fmt::Arguments<'_>) {
    let mut guard: MutexGuard<'_, Option<BufWriter<std::fs::File>>> = lock_recover(&LOG_FILE);
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
    let count = CHECK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let need_size_check = freshly_opened || count.is_multiple_of(CHECK_INTERVAL);
    if need_size_check
        && guard
            .as_ref()
            .and_then(|f| f.get_ref().metadata().ok())
            .is_some_and(|m| m.len() > MAX_LOG_SIZE_BYTES)
    {
        if let Some(bw) = guard.as_mut() {
            let _ = bw.flush();
        }
        *guard = None;
        let path = log_path();
        match OpenOptions::new().write(true).truncate(true).open(&path) {
            Ok(f) => *guard = Some(BufWriter::new(f)),
            Err(e) => {
                stderr_fallback(&format!("[lc:debug_log:open_error] {e}"));
                return;
            }
        }
    }
    if let Some(bw) = guard.as_mut() {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        if let Err(e) = writeln!(bw, "[{timestamp}] {args}") {
            stderr_fallback(&format!("[lc:debug_log:write_error] {e}"));
        }
        let _ = bw.flush();
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
        let mut guard = lock_recover(&LOG_FILE);
        *guard = None;
        CHECK_COUNTER.store(0, Ordering::SeqCst);
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
        reset_for_test();
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        {
            let mut f = std::fs::File::create(&path).expect("create oversized log");
            std::io::Write::write_all(&mut f, &vec![b'X'; (MAX_LOG_SIZE_BYTES + 1) as usize])
                .expect("write oversized log");
        }

        let mut truncated = false;
        for attempt in 0..20 {
            reset_for_test();
            log(format_args!("attempt {attempt}"));
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len > 0 && len < MAX_LOG_SIZE_BYTES {
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
