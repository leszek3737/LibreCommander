use std::cell::RefCell;
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
/// For pre-TUI and post-TUI output, use eprintln!/println! with `#[allow]` instead.
///
/// **Blocking behavior:** The internal mutex uses a blocking `std::sync::Mutex`.
/// On a stalled filesystem (network mount, writeback pressure) the lock
/// acquisition and subsequent I/O will block the calling thread until complete.
///
/// Acceptable because: debug_log is a best-effort diagnostic aid, not on the
/// hot rendering path. The only non-test callers are event-loop housekeeping
/// (watcher sync, job runner callbacks) which already tolerate occasional
/// stalls. If this becomes a problem, switch to `parking_lot::Mutex` (no
/// poisoning overhead) or a `mpsc` logger thread — but the added complexity
/// is not justified yet.
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

/// Flush the `BufWriter` every N entries instead of after each write, so the
/// buffer can actually batch syscalls. Kept small to bound how many entries a
/// crash can lose, while still amortizing the vast majority of writes.
const FLUSH_INTERVAL: u32 = 16;

/// Shared tag for failures to open/reopen the log file. Deduplicated so both
/// the initial-open and post-truncate-reopen paths report consistently.
const OPEN_ERROR_TAG: &str = "open_error";

thread_local! {
    /// Caches the formatted timestamp for the current whole second. The log
    /// timestamp has 1s resolution, so reformat only when the second changes.
    /// Thread-local avoids an extra lock: `log()` already serializes on the
    /// `LOG_FILE` mutex, so cross-thread sharing buys nothing here.
    static TS_CACHE: RefCell<(i64, String)> = const { RefCell::new((i64::MIN, String::new())) };
}

fn ensure_log_file() -> std::io::Result<BufWriter<std::fs::File>> {
    let path = log_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        // Double-noise note: when mkdir fails, the open() below almost always
        // fails too, emitting a second `open_error` line on stderr. We keep
        // both deliberately — the mkdir error names the more specific cause
        // (e.g. EACCES on the parent), while open_error confirms the file is
        // unusable.
        report_error("mkdir_error", &e);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map(BufWriter::new)
}

/// Report an internal error to stderr with a consistent `[lc:debug_log:<tag>]` prefix.
fn report_error(tag: &str, error: impl std::fmt::Display) {
    stderr_fallback(&format!("[lc:debug_log:{tag}] {error}"));
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
                report_error(OPEN_ERROR_TAG, &e);
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
                report_error(OPEN_ERROR_TAG, &e);
                return;
            }
        }
    }
    if let Some(bw) = guard.as_mut() {
        let now = Local::now();
        let secs = now.timestamp();
        TS_CACHE.with_borrow_mut(|(cached_secs, cached_str)| {
            if *cached_secs != secs {
                use std::fmt::Write as _;
                *cached_secs = secs;
                cached_str.clear();
                let _ = write!(cached_str, "{}", now.format("%Y-%m-%d %H:%M:%S"));
            }
            if let Err(e) = writeln!(bw, "[{cached_str}] {args}") {
                report_error("write_error", &e);
            }
        });
        // Batch flush: let the BufWriter coalesce writes and flush only every
        // FLUSH_INTERVAL entries (and right after open). This is the main win —
        // the previous per-entry flush defeated the BufWriter entirely.
        if freshly_opened || count.is_multiple_of(FLUSH_INTERVAL) {
            let _ = bw.flush();
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

        // The truncation guard only fires on a *freshly opened* file (or every
        // CHECK_INTERVAL writes), so each attempt re-creates the oversized file
        // and re-opens via `reset_for_test()` to genuinely exercise truncation.
        // Creating it once before the loop would only test the first iteration:
        // after any `log()` truncates, the file is small and later attempts just
        // append, no longer exercising the truncation path.
        //
        // Cross-test interference is expected and the retry loop MUST NOT be
        // collapsed into a single attempt. Production `log()` relies on
        // PROCESS-GLOBAL statics: `CHECK_COUNTER` (AtomicU32), `LOG_FILE` (Mutex)
        // and the `TEST_CACHE_HOME` redirect. `TEST_MUTEX` only serializes the two
        // debug_log tests against each other; it does NOT exclude the hundreds of
        // other tests on parallel threads that invoke the production
        // `debug_log!`/`log()` macro. Once this test installs its tempdir into the
        // process-global `TEST_CACHE_HOME`, those concurrent `log()` calls bump the
        // shared `CHECK_COUNTER` and write to this test's tempdir, perturbing the
        // size dance below. The retry loop tolerates that until one cycle wins.
        //
        // TODO(tech-debt): the principled fix is a per-test redirect gate that
        // production `log()` consults first, isolating these globals; tracked as
        // follow-up. Until then the test is empirically — not provably — robust.

        // Empirically sufficient under N-way parallel `cargo test`; ~250ms worst
        // case (RETRY_BUDGET iterations * the 5ms back-off below).
        const RETRY_BUDGET: usize = 50;
        let mut truncated = false;
        for attempt in 0..RETRY_BUDGET {
            reset_for_test();
            std::fs::write(&path, vec![b'X'; (MAX_LOG_SIZE_BYTES + 1) as usize])
                .expect("write oversized log");
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
            "log() never truncated the oversized file within {RETRY_BUDGET} retries"
        );
    }
}
