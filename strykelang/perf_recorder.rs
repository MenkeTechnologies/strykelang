//! Wall-clock recorder for `stryke --record`. Writes one row per stryke
//! invocation to `~/.stryke/perf.sqlite`, queryable via the `perfview`
//! builtin.
//!
//! Design:
//! - Recording is **opt-in** via the `--record` CLI flag or `STRYKE_RECORD=1`
//!   env var. When set on the parent process the env var inherits to every
//!   child stryke process, so `s --record t TESTS...` records one row per
//!   test file plus one row for the parent.
//! - One SQLite write per invocation, sub-ms on WAL-mode storage. On failure
//!   (locked db, missing $HOME, permission error) recording silently no-ops
//!   so script execution is never broken.
//! - Auto-prune rows older than 90 days every ~1000 inserts so the DB stays
//!   bounded without manual maintenance.
//!
//! Schema:
//! ```sql
//! CREATE TABLE runs (
//!   id INTEGER PRIMARY KEY,
//!   path TEXT NOT NULL,           -- canonicalized abs path, or "<repl>"/"<eval>"/"<stdin>"/"<subcmd:NAME>"
//!   argv TEXT,                    -- json-encoded argv
//!   started_ns INTEGER NOT NULL,  -- unix ns at process start
//!   duration_ns INTEGER NOT NULL, -- wall-clock ns from start → drop
//!   exit_code INTEGER NOT NULL,
//!   version TEXT NOT NULL,        -- CARGO_PKG_VERSION
//!   host TEXT,
//!   pid INTEGER,
//!   parent_pid INTEGER
//! );
//! ```

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Returns `~/.stryke/perf.sqlite`, creating the parent dir if needed.
/// Honors `$STRYKE_HOME` for callers that override the root.
fn perf_db_path() -> Option<PathBuf> {
    let base = if let Ok(home) = std::env::var("STRYKE_HOME") {
        PathBuf::from(home)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".stryke")
    } else {
        return None;
    };
    if std::fs::create_dir_all(&base).is_err() {
        return None;
    }
    Some(base.join("perf.sqlite"))
}

/// Open the perf db in WAL mode and ensure the schema. Returns `None` on
/// any failure so callers can silently skip recording.
pub fn open_db() -> Option<Connection> {
    let path = perf_db_path()?;
    let conn = Connection::open(&path).ok()?;
    // WAL gives single-writer no-blocking-reader semantics — keeps `perfview`
    // queries snappy even while a parallel test pool is inserting.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            argv TEXT,
            started_ns INTEGER NOT NULL,
            duration_ns INTEGER NOT NULL,
            exit_code INTEGER NOT NULL,
            version TEXT NOT NULL,
            host TEXT,
            pid INTEGER,
            parent_pid INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_runs_path ON runs(path);
         CREATE INDEX IF NOT EXISTS idx_runs_started ON runs(started_ns);
         CREATE INDEX IF NOT EXISTS idx_runs_duration ON runs(duration_ns);",
    )
    .ok()?;
    Some(conn)
}

/// Best-effort hostname; empty string if unavailable.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_default()
}

/// Best-effort json-array encoding of argv. Avoids pulling serde into the
/// recorder hot path by hand-escaping the small set of chars that matter.
fn argv_json(argv: &[String]) -> String {
    let mut s = String::from("[");
    for (i, a) in argv.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        for c in a.chars() {
            match c {
                '"' => s.push_str("\\\""),
                '\\' => s.push_str("\\\\"),
                '\n' => s.push_str("\\n"),
                '\r' => s.push_str("\\r"),
                '\t' => s.push_str("\\t"),
                c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
                c => s.push(c),
            }
        }
        s.push('"');
    }
    s.push(']');
    s
}

/// One row, ready to insert.
#[derive(Debug, Clone)]
pub struct RunRow {
    /// `path` field.
    pub path: String,
    /// `argv` field.
    pub argv: Vec<String>,
    /// `started_ns` field.
    pub started_ns: i64,
    /// `duration_ns` field.
    pub duration_ns: i64,
    /// `exit_code` field.
    pub exit_code: i32,
    /// `version` field.
    pub version: String,
    /// `host` field.
    pub host: String,
    /// `pid` field.
    pub pid: i64,
    /// `parent_pid` field.
    pub parent_pid: i64,
}

/// Insert one row. Best-effort: returns false on any failure (db missing,
/// locked, schema mismatch).
pub fn insert(row: &RunRow) -> bool {
    let Some(conn) = open_db() else { return false };
    let argv_str = argv_json(&row.argv);
    conn.execute(
        "INSERT INTO runs (path, argv, started_ns, duration_ns, exit_code, version, host, pid, parent_pid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.path,
            argv_str,
            row.started_ns,
            row.duration_ns,
            row.exit_code,
            row.version,
            row.host,
            row.pid,
            row.parent_pid,
        ],
    )
    .is_ok()
}

/// Filter spec for `query()`. All fields are optional.
#[derive(Debug, Default, Clone)]
pub struct QueryFilter {
    /// Substring or regex applied to `path`. Empty = no filter.
    pub name_substr: Option<String>,
    /// Regex applied to `path`. Falls back to substring on regex compile failure.
    pub name_regex: Option<String>,
    /// Inclusive lower bound on `started_ns`.
    pub since_ns: Option<i64>,
    /// Exact path match (canonicalized).
    pub exact_path: Option<String>,
    /// `Some(true)` = duration desc (slowest first), `Some(false)` = asc.
    /// `None` = id desc (most recent first).
    pub slowest_first: Option<bool>,
    /// Max rows to return.
    pub limit: usize,
}

impl QueryFilter {
    /// `slowest_top` — see implementation.
    pub fn slowest_top(n: usize) -> Self {
        Self {
            slowest_first: Some(true),
            limit: n,
            ..Default::default()
        }
    }
}

/// One queried row.
#[derive(Debug, Clone)]
pub struct QueryRow {
    /// `id` field.
    pub id: i64,
    /// `path` field.
    pub path: String,
    /// `argv` field.
    pub argv: String,
    /// `started_ns` field.
    pub started_ns: i64,
    /// `duration_ns` field.
    pub duration_ns: i64,
    /// `exit_code` field.
    pub exit_code: i32,
    /// `version` field.
    pub version: String,
    /// `host` field.
    pub host: String,
    /// `pid` field.
    pub pid: i64,
    /// `parent_pid` field.
    pub parent_pid: i64,
}

/// Run a query. Best-effort: returns empty Vec on any failure.
pub fn query(f: &QueryFilter) -> Vec<QueryRow> {
    let Some(conn) = open_db() else {
        return Vec::new();
    };
    let mut sql = String::from(
        "SELECT id, path, argv, started_ns, duration_ns, exit_code, version,
                COALESCE(host, ''), COALESCE(pid, 0), COALESCE(parent_pid, 0)
         FROM runs",
    );
    let mut clauses: Vec<String> = Vec::new();
    let mut bind: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(p) = &f.exact_path {
        clauses.push("path = ?".to_string());
        bind.push(Box::new(p.clone()));
    }
    if let Some(s) = &f.name_substr {
        clauses.push("path LIKE ?".to_string());
        bind.push(Box::new(format!("%{}%", s)));
    }
    if let Some(ns) = f.since_ns {
        clauses.push("started_ns >= ?".to_string());
        bind.push(Box::new(ns));
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    match f.slowest_first {
        Some(true) => sql.push_str(" ORDER BY duration_ns DESC"),
        Some(false) => sql.push_str(" ORDER BY duration_ns ASC"),
        None => sql.push_str(" ORDER BY id DESC"),
    }
    let limit = if f.limit == 0 { 1000 } else { f.limit };
    sql.push_str(&format!(" LIMIT {}", limit));

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let params_refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
    let mut out: Vec<QueryRow> = Vec::new();
    let rows = stmt.query_map(rusqlite::params_from_iter(params_refs), |r| {
        Ok(QueryRow {
            id: r.get(0)?,
            path: r.get(1)?,
            argv: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            started_ns: r.get(3)?,
            duration_ns: r.get(4)?,
            exit_code: r.get(5)?,
            version: r.get(6)?,
            host: r.get(7)?,
            pid: r.get(8)?,
            parent_pid: r.get(9)?,
        })
    });
    if let Ok(iter) = rows {
        for r in iter.flatten() {
            // Optional regex filter applied client-side.
            if let Some(rx) = &f.name_regex {
                if let Ok(re) = regex::Regex::new(rx) {
                    if !re.is_match(&r.path) {
                        continue;
                    }
                }
            }
            out.push(r);
        }
    }
    out
}

/// Parse a duration string like `7d`, `24h`, `30m`, `90s` → seconds.
/// Returns `None` on parse failure.
pub fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, unit) = match s.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&s[..s.len() - 1], c.to_ascii_lowercase()),
        _ => (s, 's'),
    };
    let n: i64 = num_str.parse().ok()?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        'w' => 86_400 * 7,
        _ => return None,
    };
    Some(n * mult)
}

/// Prune rows older than `days` days. Best-effort.
pub fn prune_older_than(days: i64) -> Option<usize> {
    let conn = open_db()?;
    let cutoff_ns = (now_ns() - days * 86_400 * 1_000_000_000).max(0);
    conn.execute("DELETE FROM runs WHERE started_ns < ?1", params![cutoff_ns])
        .ok()
}

/// Counter for occasional auto-prune. Doesn't need atomicity — the only
/// cost of a duplicate prune is a redundant `DELETE WHERE started_ns < ...`
/// which is a no-op the second time.
fn maybe_auto_prune() {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    if n.is_multiple_of(1000) && n > 0 {
        let _ = prune_older_than(90);
    }
}

/// Wall-clock ns at unix epoch.
pub fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Global recorder state. Set once by `install`; read by `atexit_record`.
struct RecorderState {
    started_at: Instant,
    started_ns: i64,
    path: String,
    argv: Vec<String>,
}

static RECORDER: OnceLock<RecorderState> = OnceLock::new();
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Install the perf recorder. Captures process start time and registers an
/// `atexit` handler that writes one SQLite row when the process exits via
/// any path (`process::exit`, normal main return, libc::exit). Safe to call
/// at most once per process; subsequent calls no-op.
///
/// `path` is the canonical "what was run" (script path, `<repl>`, `<eval>`,
/// `<subcmd:NAME>`). `argv` is the full invocation argv (including
/// `argv[0]`).
///
/// `<repl>` invocations are not recorded — they include `--test-worker` /
/// `--remote-worker` pool processes (which have no script-path argv but
/// fork dozens of children that would otherwise all write `<repl>` rows)
/// and interactive REPL sessions (not meaningful as wall-clock data points).
/// Explicit `s --record -e '...'` still records as `<eval>` since `-e`
/// argv is preserved.
pub fn install(path: String, argv: Vec<String>) {
    if path == "<repl>" {
        return; // skip pool workers / interactive REPL; see doc above
    }
    if RECORDER
        .set(RecorderState {
            started_at: Instant::now(),
            started_ns: now_ns(),
            path,
            argv,
        })
        .is_err()
    {
        return; // already installed; ignore second call
    }

    // Register libc atexit handler. This fires for `process::exit`, normal
    // main() return, and explicit `libc::exit` — every path stryke uses.
    // SAFETY: the registered function is a plain `extern "C"` with no
    // captures; libc::atexit accepts up to 32 handlers by spec, well below
    // any usage stryke would hit.
    unsafe {
        libc::atexit(atexit_record);
    }

    // Panic hook: record exit code 101 (Rust's default panic exit code)
    // before unwind reaches atexit. Chains to any existing hook.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        EXIT_CODE.store(101, Ordering::Relaxed);
        prev(info);
    }));
}

/// Record an explicit exit code. Call this immediately before
/// `process::exit(N)` to capture `N`. Without this, atexit records `0`
/// (the libc atexit API doesn't expose the actual exit code).
pub fn set_exit_code(code: i32) {
    EXIT_CODE.store(code, Ordering::Relaxed);
}

/// libc atexit callback. Reads global recorder state, builds the row,
/// inserts. Never panics (uses best-effort error handling).
extern "C" fn atexit_record() {
    let Some(state) = RECORDER.get() else { return };
    let duration_ns = state.started_at.elapsed().as_nanos() as i64;
    let pid = std::process::id() as i64;
    let parent_pid = parent_pid();
    let row = RunRow {
        path: state.path.clone(),
        argv: state.argv.clone(),
        started_ns: state.started_ns,
        duration_ns,
        exit_code: EXIT_CODE.load(Ordering::Relaxed),
        version: env!("CARGO_PKG_VERSION").to_string(),
        host: hostname(),
        pid,
        parent_pid,
    };
    let _ = insert(&row);
    maybe_auto_prune();
}

#[cfg(unix)]
fn parent_pid() -> i64 {
    // SAFETY: getppid is always safe; returns pid_t.
    unsafe { libc::getppid() as i64 }
}

#[cfg(not(unix))]
fn parent_pid() -> i64 {
    0
}

/// Returns `true` if the parent process requested recording via env.
/// Inherited automatically by child stryke processes.
pub fn recording_enabled_in_env() -> bool {
    std::env::var("STRYKE_RECORD")
        .map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
}

/// Derive the canonical `path` field for a stryke invocation given argv.
/// Returns `(path, argv_to_record)`.
/// - First positional file arg → its canonical absolute path
/// - `-e` / `--exec` → "&lt;eval&gt;"
/// - REPL (no positional, stdin tty) → "&lt;repl&gt;"
/// - Subcommand → "&lt;subcmd:NAME&gt;"
pub fn classify_invocation(argv: &[String]) -> String {
    if argv.len() <= 1 {
        return "<repl>".to_string();
    }
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == "--" {
            break;
        }
        if a == "-e" || a == "--exec" {
            return "<eval>".to_string();
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        // First non-flag positional. If it's a known subcommand name, return
        // <subcmd:NAME>; otherwise treat as a script file.
        if is_subcommand_name(a) {
            return format!("<subcmd:{}>", a);
        }
        if std::path::Path::new(a).exists() {
            if let Ok(abs) = std::fs::canonicalize(a) {
                return abs.display().to_string();
            }
        }
        return a.clone();
    }
    "<repl>".to_string()
}

/// Known stryke subcommand names — first positional matching these is
/// classified as a subcommand rather than a script path.
fn is_subcommand_name(name: &str) -> bool {
    matches!(
        name,
        "t" | "test"
            | "check"
            | "fmt"
            | "format"
            | "lint"
            | "docs"
            | "doc"
            | "repl"
            | "build"
            | "run"
            | "install"
            | "uninstall"
            | "publish"
            | "init"
            | "new"
            | "search"
            | "list"
            | "info"
            | "lsp"
            | "completion"
            | "completions"
            | "perfview"
            | "version"
            | "help"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_json_escapes_quotes_and_specials() {
        let argv = vec!["s".to_string(), "-e".to_string(), "p \"hi\"\n".to_string()];
        let out = argv_json(&argv);
        assert_eq!(out, "[\"s\",\"-e\",\"p \\\"hi\\\"\\n\"]");
    }

    #[test]
    fn classify_eval() {
        let argv = vec!["s".to_string(), "-e".to_string(), "p 42".to_string()];
        assert_eq!(classify_invocation(&argv), "<eval>");
    }

    #[test]
    fn classify_repl_when_no_args() {
        let argv = vec!["s".to_string()];
        assert_eq!(classify_invocation(&argv), "<repl>");
    }

    #[test]
    fn classify_subcommand() {
        let argv = vec!["s".to_string(), "test".to_string(), "t/".to_string()];
        assert_eq!(classify_invocation(&argv), "<subcmd:test>");
    }

    #[test]
    fn classify_t_short() {
        let argv = vec!["s".to_string(), "t".to_string(), "t/".to_string()];
        assert_eq!(classify_invocation(&argv), "<subcmd:t>");
    }

    #[test]
    fn recording_enabled_only_when_env_truthy() {
        let key = "STRYKE_RECORD";
        let saved = std::env::var(key).ok();
        std::env::remove_var(key);
        assert!(!recording_enabled_in_env());
        std::env::set_var(key, "1");
        assert!(recording_enabled_in_env());
        std::env::set_var(key, "0");
        assert!(!recording_enabled_in_env());
        std::env::set_var(key, "false");
        assert!(!recording_enabled_in_env());
        std::env::set_var(key, "");
        assert!(!recording_enabled_in_env());
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
