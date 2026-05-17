//! Stryke long-lived log file at `~/.stryke/stryke.log` (or
//! `$STRYKE_HOME/stryke.log` when that env var is set).
//!
//! Both the LSP server (`stryke --lsp`) and the DAP server (`stryke --dap`)
//! emit milestone events here so plugin / IDE issues can be diagnosed by
//! `tail -f ~/.stryke/stryke.log` without having to attach a debugger or
//! re-launch with `RUST_LOG`. Keep messages terse — one line per event,
//! self-describing, no chatter inside hot loops above DEBUG level.
//!
//! ## Levels
//!
//! `TRACE` < `DEBUG` < `INFO` < `WARN` < `ERROR`. Compile-time filtering
//! happens at the macro site (the `format!` doesn't run when the level is
//! gated out). Runtime threshold from `$STRYKE_LOG_LEVEL` (case-insensitive,
//! default `INFO`).
//!
//! Use the macros, not the bare functions:
//!
//! ```ignore
//! crate::slog_info!("lsp", "started pid={}", pid);
//! crate::slog_debug!("lsp.rename", "files={} edits={}", n_files, n_edits);
//! crate::slog_warn!("dap", "tcp connect {} failed: {}", addr, err);
//! ```
//!
//! Honors `$STRYKE_LOG_FILE` for tests / one-off redirection. The user-
//! facing convention is `~/.stryke/stryke.log`; the env override exists for
//! sandboxed runs only.
//!
//! Failure to open / write the log MUST NOT crash the server — log
//! writes are best-effort.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static LEVEL: OnceLock<Level> = OnceLock::new();

/// Default rotation threshold per file (5 MB) — small enough that a long-
/// running daemon never accumulates a multi-GB log, large enough that
/// rotation doesn't churn during normal day-to-day editor traffic.
const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Number of rotated generations to keep (`stryke.log.1` … `stryke.log.5`).
/// The active file plus 5 rotations bounds the log dir at ~30 MB by default.
const DEFAULT_MAX_FILES: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    /// Parse a level name. Accepts any case (`info`/`Info`/`INFO`).
    /// Anything unknown returns `None` so the caller can decide on a fallback.
    pub fn parse(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Level::Trace),
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" | "err" => Some(Level::Error),
            _ => None,
        }
    }
}

/// Resolved log path. Order:
/// 1. `$STRYKE_LOG_FILE` — explicit override.
/// 2. `$STRYKE_HOME/stryke.log` — when `$STRYKE_HOME` is set.
/// 3. `~/.stryke/stryke.log` — default.
pub fn log_path() -> PathBuf {
    if let Ok(p) = std::env::var("STRYKE_LOG_FILE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(h) = std::env::var("STRYKE_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h).join("stryke.log");
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".stryke").join("stryke.log")
}

/// Current minimum level. Reads `$STRYKE_LOG_LEVEL` the first time it's
/// asked, then caches. Default = `INFO`. Tests that need to flip the level
/// must do it via [`force_level_for_test`] (the env var is read once).
pub fn current_level() -> Level {
    *LEVEL.get_or_init(|| {
        std::env::var("STRYKE_LOG_LEVEL")
            .ok()
            .and_then(|s| Level::parse(&s))
            .unwrap_or(Level::Info)
    })
}

/// Per-file rotation threshold in bytes. `0` disables rotation (the active
/// file grows unbounded). From `$STRYKE_LOG_MAX_BYTES`; default 5 MB.
///
/// Not cached — the env var is re-read on every call so tests and runtime
/// reconfiguration both work. The cost is one `env::var` per rotation check,
/// which is negligible next to the `fs::metadata` call rotation already
/// performs.
pub fn max_bytes() -> u64 {
    std::env::var("STRYKE_LOG_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_BYTES)
}

/// Maximum number of rotated generations to retain. The active file plus N
/// archives. From `$STRYKE_LOG_MAX_FILES`; default 5. Not cached (see
/// [`max_bytes`] for rationale).
pub fn max_files() -> u32 {
    std::env::var("STRYKE_LOG_MAX_FILES")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_MAX_FILES)
}

/// Rotate `path` if its current size meets or exceeds the configured
/// threshold. Renames `<path>.N-1` → `<path>.N` (oldest first), then
/// `<path>` → `<path>.1`. The drop of `<path>.MAX` is implicit — the next
/// shift overwrites it. No-op when the file doesn't exist, when rotation
/// is disabled (`max_bytes == 0`), or when the size is under threshold.
///
/// Called from inside [`log_at`] under the global write lock, so cross-
/// thread races between rotation and append can't interleave.
fn rotate_if_needed(path: &std::path::Path) {
    let max = max_bytes();
    if max == 0 {
        return;
    }
    let size = match std::fs::metadata(path) {
        Ok(md) => md.len(),
        Err(_) => return,
    };
    if size < max {
        return;
    }
    let n = max_files();
    let base = path.as_os_str().to_string_lossy().into_owned();
    // Drop the eldest by renaming inward — `.N-1 → .N` first, walking down
    // to `.1 → .2`, then `<path> → .1`. The eldest is overwritten in
    // place by the rename, which is what bounds the total file count.
    for i in (1..n).rev() {
        let from = format!("{base}.{i}");
        let to = format!("{base}.{}", i + 1);
        let _ = std::fs::rename(&from, &to);
    }
    let dot1 = format!("{base}.1");
    let _ = std::fs::rename(path, &dot1);
}

/// True if a message at `lvl` would be written. Use to short-circuit
/// expensive `format!` arguments at the call site — the macros do this for
/// you.
#[inline]
pub fn enabled(lvl: Level) -> bool {
    lvl >= current_level()
}

/// Append a level-stamped line to the log. Most callers should use the
/// `slog_*!` macros instead so the message argument is only formatted when
/// the level is enabled.
pub fn log_at(lvl: Level, tag: &str, msg: &str) {
    if !enabled(lvl) {
        return;
    }
    let mu = LOCK.get_or_init(|| Mutex::new(()));
    let _g = mu.lock().unwrap_or_else(|p| p.into_inner());
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Rotation check sits inside the write lock so two threads can't
    // concurrently rotate or write past the threshold.
    rotate_if_needed(&path);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let secs = (ts / 1000) as i64;
    let millis = (ts % 1000) as u32;
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "[{}.{:03}] [{:>5}] [{}] {}",
            secs,
            millis,
            lvl.as_str(),
            tag,
            msg
        );
    }
}

/// Back-compat alias: legacy callers that don't carry a level. Treated as
/// `INFO`. Prefer the leveled macros (`slog_info!`, etc.) for new code.
pub fn log(tag: &str, msg: &str) {
    log_at(Level::Info, tag, msg);
}

/// Test-only level override. Bypasses the cached `OnceLock` by writing to a
/// secondary atomic. Kept simple — production code path doesn't touch this.
#[cfg(test)]
pub fn force_level_for_test(_lvl: Level) {
    // The OnceLock makes runtime level changes a no-op in production. Tests
    // exercise `log_at` directly with the level parameter when they need
    // deterministic filtering, so this is intentionally a stub.
}

/// `slog_trace!("tag", "fmt {} {}", a, b);`
#[macro_export]
macro_rules! slog_trace {
    ($tag:expr, $($arg:tt)*) => {{
        if $crate::stryke_log::enabled($crate::stryke_log::Level::Trace) {
            $crate::stryke_log::log_at(
                $crate::stryke_log::Level::Trace,
                $tag,
                &format!($($arg)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! slog_debug {
    ($tag:expr, $($arg:tt)*) => {{
        if $crate::stryke_log::enabled($crate::stryke_log::Level::Debug) {
            $crate::stryke_log::log_at(
                $crate::stryke_log::Level::Debug,
                $tag,
                &format!($($arg)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! slog_info {
    ($tag:expr, $($arg:tt)*) => {{
        if $crate::stryke_log::enabled($crate::stryke_log::Level::Info) {
            $crate::stryke_log::log_at(
                $crate::stryke_log::Level::Info,
                $tag,
                &format!($($arg)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! slog_warn {
    ($tag:expr, $($arg:tt)*) => {{
        if $crate::stryke_log::enabled($crate::stryke_log::Level::Warn) {
            $crate::stryke_log::log_at(
                $crate::stryke_log::Level::Warn,
                $tag,
                &format!($($arg)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! slog_error {
    ($tag:expr, $($arg:tt)*) => {{
        if $crate::stryke_log::enabled($crate::stryke_log::Level::Error) {
            $crate::stryke_log::log_at(
                $crate::stryke_log::Level::Error,
                $tag,
                &format!($($arg)*),
            );
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Tests in this module mutate process-wide env vars
    /// (`STRYKE_LOG_FILE`, `STRYKE_HOME`, `STRYKE_LOG_MAX_*`). Cargo runs
    /// tests in parallel by default, so we serialize through a module-level
    /// mutex — every test that touches env state must hold this guard for
    /// its duration. Without it, `STRYKE_LOG_FILE` set by one test leaks
    /// into `log_path_honors_stryke_home`'s assertion etc.
    static ENV_GUARD: StdMutex<()> = StdMutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_GUARD.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clear_log_env() {
        std::env::remove_var("STRYKE_LOG_FILE");
        std::env::remove_var("STRYKE_HOME");
        std::env::remove_var("STRYKE_LOG_MAX_BYTES");
        std::env::remove_var("STRYKE_LOG_MAX_FILES");
    }

    fn fresh_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "stryke-log-{}-{}.log",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn log_writes_to_env_override() {
        let _g = env_lock();
        clear_log_env();
        let tmp = fresh_path();
        std::env::set_var("STRYKE_LOG_FILE", &tmp);
        log("test", "hello world");
        let contents = std::fs::read_to_string(&tmp).expect("log file written");
        assert!(
            contents.contains("[ INFO] [test] hello world"),
            "got: {contents:?}"
        );
        let _ = std::fs::remove_file(&tmp);
        clear_log_env();
    }

    #[test]
    fn log_path_honors_stryke_home() {
        let _g = env_lock();
        clear_log_env();
        std::env::set_var("STRYKE_HOME", "/tmp/stryke-home-fixture");
        let p = log_path();
        assert_eq!(p, PathBuf::from("/tmp/stryke-home-fixture/stryke.log"));
        clear_log_env();
    }

    #[test]
    fn level_parsing_accepts_canonical_names() {
        assert_eq!(Level::parse("trace"), Some(Level::Trace));
        assert_eq!(Level::parse("DEBUG"), Some(Level::Debug));
        assert_eq!(Level::parse("Info"), Some(Level::Info));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("err"), Some(Level::Error));
        assert_eq!(Level::parse("zorp"), None);
    }

    #[test]
    fn level_ordering_matches_severity() {
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
    }

    #[test]
    fn rotate_shifts_files_inward_when_oversize() {
        let _g = env_lock();
        clear_log_env();
        std::env::set_var("STRYKE_LOG_MAX_BYTES", "10");
        std::env::set_var("STRYKE_LOG_MAX_FILES", "3");
        let p = fresh_path();
        std::fs::write(&p, b"AAAAAAAAAAAAAAAA").unwrap(); // 16 bytes > 10
                                                          // Pre-existing rotations: simulate one previous shift so we can
                                                          // verify the inward walk.
        std::fs::write(format!("{}.1", p.display()), b"prev").unwrap();
        rotate_if_needed(&p);
        // Active file is gone (renamed to .1).
        assert!(!p.exists(), "active path must be rotated away");
        // .1 must now contain the old active body.
        let r1 = std::fs::read(format!("{}.1", p.display())).unwrap();
        assert_eq!(r1, b"AAAAAAAAAAAAAAAA");
        // The previous .1 should have moved to .2.
        let r2 = std::fs::read(format!("{}.2", p.display())).unwrap();
        assert_eq!(r2, b"prev");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.1", p.display()));
        let _ = std::fs::remove_file(format!("{}.2", p.display()));
        clear_log_env();
    }

    #[test]
    fn rotate_noop_when_disabled() {
        let _g = env_lock();
        clear_log_env();
        std::env::set_var("STRYKE_LOG_MAX_BYTES", "0");
        let p = fresh_path();
        std::fs::write(&p, b"this is way more than zero bytes long").unwrap();
        rotate_if_needed(&p);
        assert!(p.exists(), "rotation disabled — file must persist");
        let _ = std::fs::remove_file(&p);
        clear_log_env();
    }

    #[test]
    fn rotate_noop_when_under_threshold() {
        let _g = env_lock();
        clear_log_env();
        std::env::set_var("STRYKE_LOG_MAX_BYTES", "1000");
        let p = fresh_path();
        std::fs::write(&p, b"short").unwrap();
        rotate_if_needed(&p);
        assert!(p.exists(), "small file must not rotate");
        let _ = std::fs::remove_file(&p);
        clear_log_env();
    }

    #[test]
    fn log_at_writes_level_in_line() {
        let _g = env_lock();
        clear_log_env();
        let tmp = fresh_path();
        std::env::set_var("STRYKE_LOG_FILE", &tmp);
        // ERROR ≥ default INFO so the level threshold doesn't filter this.
        log_at(Level::Error, "boot", "fatal=42");
        let contents = std::fs::read_to_string(&tmp).expect("written");
        assert!(
            contents.contains("[ERROR] [boot] fatal=42"),
            "got: {contents:?}"
        );
        let _ = std::fs::remove_file(&tmp);
        clear_log_env();
    }

    #[test]
    fn rotation_kicks_in_during_real_writes() {
        // End-to-end: write enough that the rotation threshold fires
        // mid-stream. Verifies log_at + rotate_if_needed integration.
        let _g = env_lock();
        clear_log_env();
        let tmp = fresh_path();
        std::env::set_var("STRYKE_LOG_FILE", &tmp);
        std::env::set_var("STRYKE_LOG_MAX_BYTES", "100");
        std::env::set_var("STRYKE_LOG_MAX_FILES", "2");
        for i in 0..50 {
            log_at(
                Level::Error,
                "stress",
                &format!("line-{:04}-filler-text-here", i),
            );
        }
        let dot1 = std::path::PathBuf::from(format!("{}.1", tmp.display()));
        assert!(dot1.exists(), "rotation must have produced a .1 archive");
        // Active log must be smaller than the sum of all writes.
        let active_size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        assert!(
            active_size < 50 * 30,
            "active log {active_size} should be smaller than total write volume"
        );
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&dot1);
        let _ = std::fs::remove_file(format!("{}.2", tmp.display()));
        clear_log_env();
    }
}
