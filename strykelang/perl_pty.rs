//! PTY-driven interactive automation — Phases 1–4 of the Tcl/Expect-style
//! feature. Builtins are bare-name and take the handle hashref as first
//! argument; method-form `$h->expect(...)` works through the thin
//! `PtyHandle` class shipped via [`PTY_HANDLE_CLASS_STK`].
//!
//! Builtins:
//!   * `pty_spawn(cmd_line)` / `pty_spawn(cmd, arg, arg, ...)`  → handle
//!   * `pty_send($h, "text")`                                   → bytes written
//!   * `pty_read($h, timeout_secs)`                              → string or undef
//!   * `pty_expect($h, qr/.../, timeout_secs?)`                 → matched text or undef
//!   * `pty_expect_table($h, [+{re=>qr/../, do=>sub{}}, ...], timeout_secs?)`
//!     → return value of the matched branch's `do` sub, or undef on timeout
//!   * `pty_close($h)`                                          → exit status
//!   * `pty_eof($h)`  / `pty_alive($h)`                          → 0/1
//!   * `pty_buffer($h)`                                         → unconsumed buffer
//!   * `pty_interact($h)`                                       → handoff to user
//!
//! Cross-platform: Unix only for v0. Windows would need ConPTY which is
//! its own ~5-day project.

use crate::error::StrykeError;
use crate::value::StrykeValue;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, StrykeError>;

// ── Registry ──────────────────────────────────────────────────────────
//
// Each handle is referenced from stryke as a hashref carrying
// `__pty_id__`. The registry maps that id → live PtyHandle. We hand the
// whole handle out behind an `Arc<Mutex>` so multiple actions on the
// same handle from concurrent green-threads serialize cleanly.

struct PtyHandle {
    master_fd: OwnedFd,
    pid: nix::unistd::Pid,
    #[allow(dead_code)]
    cmd: String,
    /// Bytes read from the master that haven't been consumed by an
    /// `expect` match yet. `expect` scans this for the regex; `read`
    /// drains it; `send` does not touch it.
    buffer: Vec<u8>,
    closed: bool,
    exit_status: Option<i32>,
}

static REGISTRY: OnceLock<Mutex<IndexMap<u64, Arc<Mutex<PtyHandle>>>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Mutex<IndexMap<u64, Arc<Mutex<PtyHandle>>>> {
    REGISTRY.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn lookup(handle: &StrykeValue, line: usize) -> Result<Arc<Mutex<PtyHandle>>> {
    let map = handle
        .as_hash_map()
        .or_else(|| handle.as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| StrykeError::runtime("pty: handle must be a hashref", line))?;
    let id = map
        .get("__pty_id__")
        .map(|v| v.to_int() as u64)
        .ok_or_else(|| StrykeError::runtime("pty: hashref missing `__pty_id__`", line))?;
    registry().lock().get(&id).cloned().ok_or_else(|| {
        StrykeError::runtime(format!("pty: handle id {} not found (closed?)", id), line)
    })
}

fn make_handle_value(id: u64, cmd: &str, pid: i32) -> StrykeValue {
    let mut m = IndexMap::new();
    m.insert("__pty_id__".to_string(), StrykeValue::integer(id as i64));
    m.insert("__pty__".to_string(), StrykeValue::integer(1));
    m.insert("cmd".to_string(), StrykeValue::string(cmd.to_string()));
    m.insert("pid".to_string(), StrykeValue::integer(pid as i64));
    StrykeValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
}

// ── pty_spawn ────────────────────────────────────────────────────────

pub(crate) fn pty_spawn(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime(
            "pty_spawn: usage: pty_spawn(\"cmd ...\") or pty_spawn(\"cmd\", arg, arg, ...)",
            line,
        ));
    }

    // Two argument shapes:
    //   pty_spawn("ssh user@host")          → split by whitespace
    //   pty_spawn("ssh", "user@host", ...) → use args verbatim
    let (cmd_path, argv): (String, Vec<String>) = if args.len() == 1 {
        let line_str = args[0].to_string();
        let parts: Vec<String> = shell_split(&line_str);
        if parts.is_empty() {
            return Err(StrykeError::runtime("pty_spawn: empty command", line));
        }
        let cmd = parts[0].clone();
        (cmd, parts)
    } else {
        let cmd = args[0].to_string();
        let argv: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        (cmd, argv)
    };

    let openpty = nix::pty::openpty(None, None)
        .map_err(|e| StrykeError::runtime(format!("pty_spawn: openpty: {}", e), line))?;

    // SAFETY: fork() in Rust must be careful — between fork and execvp
    // we must not call any function that takes a lock or allocates
    // beyond what we pre-built. `setsid`, `ioctl(TIOCSCTTY)`, `dup2`
    // and `execvp` are all signal-safe.
    let result = unsafe { nix::unistd::fork() }
        .map_err(|e| StrykeError::runtime(format!("pty_spawn: fork: {}", e), line))?;

    match result {
        nix::unistd::ForkResult::Child => {
            // We're in the child. The slave fd from openpty becomes our
            // controlling tty; the master is what the parent reads/writes.
            let slave_fd = openpty.slave.as_raw_fd();
            // Start a new session so we can claim the PTY as ctty.
            let _ = nix::unistd::setsid();
            unsafe {
                // Drop master fd in child.
                libc::close(openpty.master.as_raw_fd());
                // Make the slave our controlling terminal.
                libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);
                // Re-wire stdio.
                libc::dup2(slave_fd, libc::STDIN_FILENO);
                libc::dup2(slave_fd, libc::STDOUT_FILENO);
                libc::dup2(slave_fd, libc::STDERR_FILENO);
                if slave_fd > libc::STDERR_FILENO {
                    libc::close(slave_fd);
                }
            }
            // Build argv as a Vec<CString> for execvp.
            use std::ffi::CString;
            let cs_argv: Vec<CString> = argv
                .iter()
                .map(|s| CString::new(s.clone()).unwrap_or_else(|_| CString::new("").unwrap()))
                .collect();
            let cs_cmd = CString::new(cmd_path).unwrap_or_else(|_| CString::new("").unwrap());
            let _ = nix::unistd::execvp(&cs_cmd, &cs_argv);
            // execvp only returns on failure.
            unsafe {
                libc::write(
                    libc::STDERR_FILENO,
                    b"pty_spawn: execvp failed\n".as_ptr() as *const _,
                    25,
                );
                libc::_exit(127);
            }
        }
        nix::unistd::ForkResult::Parent { child } => {
            // Parent — close slave, set master non-blocking, register.
            drop(openpty.slave);
            set_nonblocking(openpty.master.as_raw_fd())?;
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let cmd_str = argv.join(" ");
            let handle = PtyHandle {
                master_fd: openpty.master,
                pid: child,
                cmd: cmd_str.clone(),
                buffer: Vec::new(),
                closed: false,
                exit_status: None,
            };
            registry().lock().insert(id, Arc::new(Mutex::new(handle)));
            Ok(make_handle_value(id, &cmd_str, child.as_raw()))
        }
    }
}

fn set_nonblocking(fd: RawFd) -> Result<()> {
    // nix 0.31 wants `AsFd`; we have a raw fd we don't own here, so go
    // through libc directly to avoid borrowing semantics.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(StrykeError::runtime(
            format!("pty_spawn: fcntl get: {}", std::io::Error::last_os_error()),
            0,
        ));
    }
    let r = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if r < 0 {
        return Err(StrykeError::runtime(
            format!("pty_spawn: fcntl set: {}", std::io::Error::last_os_error()),
            0,
        ));
    }
    Ok(())
}

// Minimal shell-style splitter: whitespace, plus simple double/single
// quotes. Good enough for `ssh user@host`-shaped commands; users with
// complex shell metas should pass argv as a flat list.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            esc = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            other => cur.push(other),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ── pty_send ──────────────────────────────────────────────────────────

pub(crate) fn pty_send(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_send: handle required", line))?,
        line,
    )?;
    let payload = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let g = h.lock();
    if g.closed {
        return Err(StrykeError::runtime("pty_send: handle is closed", line));
    }
    let fd = g.master_fd.as_raw_fd();
    let mut written = 0;
    let bytes = payload.as_bytes();
    while written < bytes.len() {
        let n = unsafe {
            libc::write(
                fd,
                bytes.as_ptr().add(written) as *const _,
                bytes.len() - written,
            )
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EAGAIN)
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
            {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(StrykeError::runtime(
                format!("pty_send: write: {}", err),
                line,
            ));
        }
        if n == 0 {
            break;
        }
        written += n as usize;
    }
    Ok(StrykeValue::integer(written as i64))
}

// ── pty_read ──────────────────────────────────────────────────────────
//
// One-shot read with timeout. Returns whatever bytes are available
// (decoded as UTF-8 lossy) plus what was already buffered. Empty
// string on timeout. Undef on EOF.

pub(crate) fn pty_read(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_read: handle required", line))?,
        line,
    )?;
    let timeout_secs = args.get(1).map(|v| v.to_int()).unwrap_or(5);
    let timeout = Duration::from_millis((timeout_secs.max(0) as u64) * 1000);

    let mut g = h.lock();
    if g.closed {
        return Ok(StrykeValue::UNDEF);
    }
    let fd = g.master_fd.as_raw_fd();

    // Drain whatever the kernel has now without waiting.
    drain_into_buffer(fd, &mut g.buffer);

    // If nothing buffered, wait up to `timeout` for one read.
    if g.buffer.is_empty() && timeout > Duration::ZERO {
        match wait_readable(fd, timeout) {
            ReadyResult::Ready => {
                drain_into_buffer(fd, &mut g.buffer);
            }
            ReadyResult::Timeout => {}
            ReadyResult::Eof => {
                g.closed = true;
                return Ok(StrykeValue::UNDEF);
            }
        }
    }

    let bytes = std::mem::take(&mut g.buffer);
    Ok(StrykeValue::string(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

#[derive(Debug)]
#[allow(dead_code)]
enum ReadyResult {
    Ready,
    Timeout,
    Eof,
}

/// `select()` on a single fd with timeout; on ready, do *not* read —
/// the caller decides how to drain. Returns Eof if `read()` would
/// indicate the child has closed its tty.
fn wait_readable(fd: RawFd, timeout: Duration) -> ReadyResult {
    let mut tv = libc::timeval {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_usec: (timeout.subsec_micros() as libc::suseconds_t),
    };
    let mut rfds: libc::fd_set = unsafe { std::mem::zeroed() };
    unsafe {
        libc::FD_ZERO(&mut rfds);
        libc::FD_SET(fd, &mut rfds);
    }
    let n = unsafe {
        libc::select(
            fd + 1,
            &mut rfds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut tv,
        )
    };
    if n < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            return ReadyResult::Timeout;
        }
        return ReadyResult::Timeout;
    }
    if n == 0 {
        return ReadyResult::Timeout;
    }
    ReadyResult::Ready
}

/// Read available bytes into `buffer`. Non-blocking: stops at EAGAIN.
/// Returns `true` if any bytes were appended.
fn drain_into_buffer(fd: RawFd, buffer: &mut Vec<u8>) -> bool {
    let mut tmp = [0u8; 4096];
    let mut got = false;
    loop {
        let n = unsafe { libc::read(fd, tmp.as_mut_ptr() as *mut _, tmp.len()) };
        if n > 0 {
            buffer.extend_from_slice(&tmp[..n as usize]);
            got = true;
            continue;
        }
        if n == 0 {
            // EOF — child closed pty.
            return got;
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(e) if e == libc::EAGAIN || e == libc::EWOULDBLOCK => return got,
            Some(libc::EINTR) => continue,
            // EIO is what Linux returns when the slave side closes —
            // treat as EOF.
            Some(libc::EIO) => return got,
            _ => return got,
        }
    }
}

// ── pty_expect (single pattern + table form) ──────────────────────────

pub(crate) fn pty_expect(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h_v = args
        .first()
        .ok_or_else(|| StrykeError::runtime("pty_expect: handle required", line))?;
    let h = lookup(h_v, line)?;
    let pattern_v = args
        .get(1)
        .ok_or_else(|| StrykeError::runtime("pty_expect: pattern required", line))?;
    let timeout_secs = args.get(2).map(|v| v.to_int()).unwrap_or(30);
    let re = compile_pattern(pattern_v, line)?;

    expect_one(
        &h,
        &re,
        Duration::from_millis((timeout_secs.max(0) as u64) * 1000),
        line,
    )
}

pub(crate) fn pty_expect_table(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    // args = ($h, [+{re => qr/.../, do => sub{}}, ...], $timeout?)
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_expect_table: handle required", line))?,
        line,
    )?;
    let table_v = args
        .get(1)
        .ok_or_else(|| StrykeError::runtime("pty_expect_table: branch list required", line))?;
    let timeout_secs = args.get(2).map(|v| v.to_int()).unwrap_or(30);
    let timeout = Duration::from_millis((timeout_secs.max(0) as u64) * 1000);

    let entries: Vec<StrykeValue> = table_v
        .as_array_ref()
        .map(|a| a.read().clone())
        .unwrap_or_else(|| table_v.clone().to_list());

    let mut compiled: Vec<(regex::bytes::Regex, StrykeValue)> = Vec::new();
    for entry in entries {
        let map = entry
            .as_hash_map()
            .or_else(|| entry.as_hash_ref().map(|h| h.read().clone()))
            .ok_or_else(|| {
                StrykeError::runtime(
                    "pty_expect_table: each branch must be a hashref { re => qr/../, do => sub{} }",
                    line,
                )
            })?;
        let re_pat = map.get("re").cloned().unwrap_or(StrykeValue::UNDEF);
        let action = map.get("do").cloned().unwrap_or(StrykeValue::UNDEF);
        let re = compile_pattern(&re_pat, line)?;
        compiled.push((re, action));
    }

    let started = Instant::now();
    loop {
        let mut g = h.lock();
        if g.closed {
            return Ok(StrykeValue::UNDEF);
        }
        // Try every branch against the buffer in order; first match wins.
        let mut hit: Option<(usize, usize, StrykeValue)> = None;
        for (re, action) in &compiled {
            if let Some(m) = re.find(&g.buffer) {
                hit = Some((m.start(), m.end(), action.clone()));
                break;
            }
        }
        if let Some((start, end, action)) = hit {
            let matched_bytes = g.buffer[start..end].to_vec();
            let matched = String::from_utf8_lossy(&matched_bytes).into_owned();
            g.buffer.drain(..end);
            drop(g);
            {
                // Return the action coderef + the matched text. The
                // caller (interpreter glue) invokes the action; but
                // since we can't call into the interp from a free
                // builtin, we package both into a hashref the caller
                // unpacks. The wrapper class chooses whether to call
                // it or just return the text.
                let mut result = IndexMap::new();
                result.insert("matched".into(), StrykeValue::string(matched));
                result.insert("action".into(), action);
                return Ok(StrykeValue::hash_ref(Arc::new(parking_lot::RwLock::new(
                    result,
                ))));
            }
        }
        // No branch matched — read more.
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(StrykeValue::UNDEF);
        }
        let remaining = timeout - elapsed;
        let fd = g.master_fd.as_raw_fd();
        drop(g);
        match wait_readable(fd, remaining.min(Duration::from_millis(500))) {
            ReadyResult::Ready => {
                let mut g = h.lock();
                let any = drain_into_buffer(fd, &mut g.buffer);
                if !any {
                    g.closed = true;
                    return Ok(StrykeValue::UNDEF);
                }
            }
            ReadyResult::Timeout => continue,
            ReadyResult::Eof => {
                let mut g = h.lock();
                g.closed = true;
                return Ok(StrykeValue::UNDEF);
            }
        }
    }
}

fn expect_one(
    h: &Arc<Mutex<PtyHandle>>,
    re: &regex::bytes::Regex,
    timeout: Duration,
    line: usize,
) -> Result<StrykeValue> {
    let _ = line;
    let started = Instant::now();
    loop {
        let mut g = h.lock();
        if g.closed && g.buffer.is_empty() {
            return Ok(StrykeValue::UNDEF);
        }
        let hit_range: Option<(usize, usize)> = re.find(&g.buffer).map(|m| (m.start(), m.end()));
        if let Some((start, end)) = hit_range {
            let bytes = g.buffer[start..end].to_vec();
            g.buffer.drain(..end);
            return Ok(StrykeValue::string(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(StrykeValue::UNDEF);
        }
        let remaining = timeout - elapsed;
        let fd = g.master_fd.as_raw_fd();
        drop(g);
        match wait_readable(fd, remaining.min(Duration::from_millis(500))) {
            ReadyResult::Ready => {
                let mut g = h.lock();
                let any = drain_into_buffer(fd, &mut g.buffer);
                if !any && g.buffer.is_empty() {
                    g.closed = true;
                    return Ok(StrykeValue::UNDEF);
                }
            }
            ReadyResult::Timeout => continue,
            ReadyResult::Eof => {
                let mut g = h.lock();
                g.closed = true;
                return Ok(StrykeValue::UNDEF);
            }
        }
    }
}

fn compile_pattern(v: &StrykeValue, line: usize) -> Result<regex::bytes::Regex> {
    let pat = v.to_string();
    // Stryke `qr/.../` typically stringifies to `(?^:...)` Perl-style.
    // The `regex` crate doesn't grok that prefix — strip it.
    let stripped = if let Some(inner) = pat.strip_prefix("(?^").and_then(|s| {
        // optional flags before colon: `(?^u:...)` etc.
        let close = s.find(')')?;
        let body_start = s.find(':')?;
        if body_start < close {
            Some(&s[body_start + 1..s.len() - 1])
        } else {
            None
        }
    }) {
        inner.to_string()
    } else {
        pat
    };
    regex::bytes::Regex::new(&stripped)
        .map_err(|e| StrykeError::runtime(format!("pty: bad regex `{}`: {}", stripped, e), line))
}

// ── pty_buffer / pty_alive / pty_eof ─────────────────────────────────

pub(crate) fn pty_buffer(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_buffer: handle required", line))?,
        line,
    )?;
    let g = h.lock();
    Ok(StrykeValue::string(
        String::from_utf8_lossy(&g.buffer).into_owned(),
    ))
}

pub(crate) fn pty_alive(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_alive: handle required", line))?,
        line,
    )?;
    let g = h.lock();
    if g.closed {
        return Ok(StrykeValue::integer(0));
    }
    // Non-blocking waitpid to see if child is still running.
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    match waitpid(g.pid, Some(WaitPidFlag::WNOHANG)) {
        Ok(WaitStatus::StillAlive) => Ok(StrykeValue::integer(1)),
        Ok(_) => Ok(StrykeValue::integer(0)),
        Err(_) => Ok(StrykeValue::integer(0)),
    }
}

pub(crate) fn pty_eof(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_eof: handle required", line))?,
        line,
    )?;
    let g = h.lock();
    Ok(StrykeValue::integer(if g.closed && g.buffer.is_empty() {
        1
    } else {
        0
    }))
}

// ── pty_close ─────────────────────────────────────────────────────────

pub(crate) fn pty_close(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h_v = args
        .first()
        .ok_or_else(|| StrykeError::runtime("pty_close: handle required", line))?;
    let h = lookup(h_v, line)?;
    let id = handle_id(h_v).unwrap_or(0);

    let pid;
    let already_closed;
    {
        let mut g = h.lock();
        already_closed = g.closed;
        pid = g.pid;
        g.closed = true;
        g.buffer.clear();
    }

    if !already_closed {
        // SIGTERM first, give it 200ms, then SIGKILL.
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::SIGTERM);
        std::thread::sleep(Duration::from_millis(200));
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        let exit = match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {
                let _ = nix::sys::signal::kill(pid, nix::sys::signal::SIGKILL);
                let _ = waitpid(pid, None);
                -9
            }
            Ok(WaitStatus::Exited(_, code)) => code,
            Ok(WaitStatus::Signaled(_, sig, _)) => -(sig as i32),
            _ => 0,
        };
        h.lock().exit_status = Some(exit);
    }
    if id != 0 {
        registry().lock().shift_remove(&id);
    }
    let exit = h.lock().exit_status.unwrap_or(0);
    Ok(StrykeValue::integer(exit as i64))
}

fn handle_id(v: &StrykeValue) -> Option<u64> {
    let map = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))?;
    map.get("__pty_id__").map(|v| v.to_int() as u64)
}

// ── pty_interact ──────────────────────────────────────────────────────
//
// Hand control to the user. Forwards stdin → pty master and master →
// stdout in raw mode until EOF on either side or the user hits Ctrl-]
// (the standard expect interact escape).

pub(crate) fn pty_interact(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| StrykeError::runtime("pty_interact: handle required", line))?,
        line,
    )?;

    let stdin_fd = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let master_fd = h.lock().master_fd.as_raw_fd();

    // Save tty mode + go raw if stdin is a tty.
    use nix::sys::termios::{tcgetattr, tcsetattr, SetArg};
    let saved = if unsafe { libc::isatty(stdin_fd) } != 0 {
        let cur = tcgetattr(unsafe { std::os::fd::BorrowedFd::borrow_raw(stdin_fd) }).ok();
        if let Some(t) = cur.clone() {
            let mut raw = t.clone();
            nix::sys::termios::cfmakeraw(&mut raw);
            let _ = tcsetattr(
                unsafe { std::os::fd::BorrowedFd::borrow_raw(stdin_fd) },
                SetArg::TCSANOW,
                &raw,
            );
        }
        cur
    } else {
        None
    };
    // Drain anything the master already has so the user sees the
    // current prompt before they start typing.
    {
        let mut g = h.lock();
        if !g.buffer.is_empty() {
            let bytes = std::mem::take(&mut g.buffer);
            unsafe {
                libc::write(stdout_fd, bytes.as_ptr() as *const _, bytes.len());
            }
        }
    }

    // Loop forwarding both directions.
    let mut buf = [0u8; 4096];
    'outer: loop {
        let mut rfds: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut rfds);
            libc::FD_SET(stdin_fd, &mut rfds);
            libc::FD_SET(master_fd, &mut rfds);
        }
        let max = stdin_fd.max(master_fd) + 1;
        let n = unsafe {
            libc::select(
                max,
                &mut rfds,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break 'outer;
        }
        // Stdin → master.
        if unsafe { libc::FD_ISSET(stdin_fd, &rfds) } {
            let r = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if r <= 0 {
                break 'outer;
            }
            // Ctrl-] (0x1d) is the escape — mirror Tcl Expect.
            if buf[..r as usize].contains(&0x1d) {
                break 'outer;
            }
            unsafe {
                libc::write(master_fd, buf.as_ptr() as *const _, r as usize);
            }
        }
        // Master → stdout.
        if unsafe { libc::FD_ISSET(master_fd, &rfds) } {
            let r = unsafe { libc::read(master_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if r <= 0 {
                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(e) if e == libc::EAGAIN || e == libc::EWOULDBLOCK => continue,
                    _ => break 'outer,
                }
            }
            unsafe {
                libc::write(stdout_fd, buf.as_ptr() as *const _, r as usize);
            }
        }
    }
    // Restore tty.
    if let Some(saved) = saved {
        let _ = tcsetattr(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(stdin_fd) },
            SetArg::TCSANOW,
            &saved,
        );
    }
    Ok(StrykeValue::UNDEF)
}

// ── ANSI strip ────────────────────────────────────────────────────────
//
// `pty_strip_ansi($text)` removes CSI/OSC/ESC sequences so callers can
// match against rendered text. This is a small VT100/xterm subset —
// enough for SSH banners, prompts, and progress bars; no full terminal
// emulator. Pure logic, no `mut self`, no allocation beyond the result.
pub(crate) fn pty_strip_ansi(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let s = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| StrykeError::runtime("pty_strip_ansi: text required", line))?;
    Ok(StrykeValue::string(strip_ansi(&s)))
}

fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    // CSI: ESC [ ... <0x40-0x7E>
                    i += 2;
                    while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                }
                b']' => {
                    // OSC: ESC ] ... BEL or ESC \
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'(' | b')' | b'*' | b'+' => {
                    // Character set selection: ESC ( <X>
                    i += 3.min(bytes.len() - i);
                }
                b'P' | b'^' | b'_' => {
                    // DCS / PM / APC — terminated by ST (ESC \) or BEL.
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Other 2-byte ESC sequences (RIS, IND, etc.)
                    i += 2;
                }
            }
        } else if bytes[i] == 0x07 || bytes[i] == 0x08 {
            // Strip BEL and BS — they show up in expect buffers as cruft.
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── pty_after_eof ─────────────────────────────────────────────────────
//
// Async EOF reaper: spawn a background thread that polls the handle's
// closed flag, and when it transitions to true, fires a marker so the
// caller can react. Since stryke closures can't be safely invoked from
// a free Rust thread (closures capture the interpreter context), we
// take a stryke fn name as a string and the user runs the dispatch
// loop themselves. The runtime side just records the event.

static EOF_EVENTS: std::sync::OnceLock<parking_lot::Mutex<Vec<EofEvent>>> =
    std::sync::OnceLock::new();

#[derive(Clone)]
struct EofEvent {
    handle_id: u64,
    callback: String,
    fired: bool,
}

fn eof_events() -> &'static parking_lot::Mutex<Vec<EofEvent>> {
    EOF_EVENTS.get_or_init(|| parking_lot::Mutex::new(Vec::new()))
}

/// `pty_after_eof($h, "callback_name")` — register a callback name to
/// fire when this PTY reaches EOF. Spawns a background watcher thread
/// that polls the handle's closed flag every 100ms. Returns 1.
///
/// To dispatch fired events from your script, call `pty_pending_events()`
/// in your main loop — it returns an arrayref of `+{handle_id, callback}`
/// pairs since the last call. This avoids the bidirectional callback
/// dance and matches stryke's main-loop model.
pub(crate) fn pty_after_eof(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let h_v = args
        .first()
        .ok_or_else(|| StrykeError::runtime("pty_after_eof: handle required", line))?;
    let id = handle_id(h_v)
        .ok_or_else(|| StrykeError::runtime("pty_after_eof: hashref missing __pty_id__", line))?;
    let callback = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "<anonymous>".to_string());

    eof_events().lock().push(EofEvent {
        handle_id: id,
        callback: callback.clone(),
        fired: false,
    });

    // Spawn a watcher thread that flips `fired` once the PTY closes.
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let still_open = registry()
            .lock()
            .get(&id)
            .map(|h| {
                let g = h.lock();
                !g.closed
            })
            .unwrap_or(false);
        if !still_open {
            let mut events = eof_events().lock();
            for e in events.iter_mut() {
                if e.handle_id == id && e.callback == callback && !e.fired {
                    e.fired = true;
                }
            }
            break;
        }
    });

    Ok(StrykeValue::integer(1))
}

/// `pty_pending_events()` → arrayref of `+{handle_id, callback}` for any
/// `pty_after_eof` registrations that have fired since the last call.
/// Drains the fired list. Pair with a main-loop poller:
///
/// ```stryke
/// while (1) {
///     for my $e (@{ pty_pending_events() }) {
///         my $cb = $main::dispatch->{ $e->{callback} };
///         $cb->($e) if $cb;
///     }
///     sleep 1;
/// }
/// ```
pub(crate) fn pty_pending_events(_args: &[StrykeValue], _line: usize) -> Result<StrykeValue> {
    let mut events = eof_events().lock();
    let mut out: Vec<StrykeValue> = Vec::new();
    let mut keep: Vec<EofEvent> = Vec::with_capacity(events.len());
    for e in events.drain(..) {
        if e.fired {
            let mut h = indexmap::IndexMap::new();
            h.insert(
                "handle_id".to_string(),
                StrykeValue::integer(e.handle_id as i64),
            );
            h.insert(
                "callback".to_string(),
                StrykeValue::string(e.callback.clone()),
            );
            out.push(StrykeValue::hash_ref(std::sync::Arc::new(
                parking_lot::RwLock::new(h),
            )));
        } else {
            keep.push(e);
        }
    }
    *events = keep;
    Ok(StrykeValue::array_ref(std::sync::Arc::new(
        parking_lot::RwLock::new(out),
    )))
}

// ── PtyHandle wrapper class ───────────────────────────────────────────
//
// Drop-in stryke source the user can `require` to get the method-form
// `$h->expect(...)`. Apps that prefer the bare-builtin form skip this.
/// `PTY_HANDLE_CLASS_STK` constant.

pub const PTY_HANDLE_CLASS_STK: &str = include_str!("perl_pty_class.stk");

// ── Public API ────────────────────────────────────────────────────────
/// `close_all` — see implementation.

#[allow(dead_code)]
pub fn close_all() {
    // Test-utility: nuke every live PTY. Not wired to a builtin.
    let ids: Vec<u64> = registry().lock().keys().copied().collect();
    for id in ids {
        let h = registry().lock().get(&id).cloned();
        if let Some(h) = h {
            let pid = { h.lock().pid };
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::SIGKILL);
            registry().lock().shift_remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── shell_split ─────────────────────────────────────────────────────

    #[test]
    fn shell_split_basic_whitespace_tokens() {
        assert_eq!(shell_split("ssh user@host"), vec!["ssh", "user@host"]);
    }

    #[test]
    fn shell_split_preserves_double_quoted_spaces() {
        assert_eq!(shell_split("cmd \"a b\" c"), vec!["cmd", "a b", "c"]);
    }

    #[test]
    fn shell_split_single_quoted_keeps_backslash_literal() {
        // Inside single quotes, `\` is literal — quote != Some('\'') gate.
        assert_eq!(shell_split("'a\\b' c"), vec!["a\\b", "c"]);
    }

    #[test]
    fn shell_split_backslash_outside_quotes_escapes_next_char() {
        assert_eq!(shell_split("a\\ b"), vec!["a b"]);
    }

    #[test]
    fn shell_split_empty_or_whitespace_only_yields_no_tokens() {
        assert!(shell_split("").is_empty());
        assert!(shell_split("   \t  ").is_empty());
    }

    #[test]
    fn shell_split_unterminated_quote_collects_remainder() {
        // No closing `"` means the rest of the input is one token (cur).
        assert_eq!(shell_split("a \"hello"), vec!["a", "hello"]);
    }

    // ─── strip_ansi ──────────────────────────────────────────────────────

    #[test]
    fn strip_ansi_removes_csi_color_sequence() {
        // ESC[31m red ESC[0m
        let s = "\x1b[31mred\x1b[0m";
        assert_eq!(strip_ansi(s), "red");
    }

    #[test]
    fn strip_ansi_removes_bel_and_backspace() {
        // Both 0x07 (BEL) and 0x08 (BS) are dropped per source comment.
        assert_eq!(strip_ansi("a\x07b\x08c"), "abc");
    }

    #[test]
    fn strip_ansi_passes_through_plain_ascii() {
        assert_eq!(strip_ansi("plain text 123"), "plain text 123");
    }

    #[test]
    fn strip_ansi_handles_osc_terminated_by_bel() {
        // ESC ] ... BEL — common for terminal title sequences.
        let s = "before\x1b]0;title\x07after";
        assert_eq!(strip_ansi(s), "beforeafter");
    }

    #[test]
    fn strip_ansi_handles_osc_terminated_by_st() {
        // ESC ] ... ESC \ (ST = String Terminator)
        let s = "x\x1b]0;ttl\x1b\\y";
        assert_eq!(strip_ansi(s), "xy");
    }

    #[test]
    fn strip_ansi_handles_multi_param_csi() {
        // ESC [ 1 ; 31 ; 42 m
        let s = "\x1b[1;31;42mhi";
        assert_eq!(strip_ansi(s), "hi");
    }

    #[test]
    fn strip_ansi_empty_input_empty_output() {
        assert_eq!(strip_ansi(""), "");
    }

    // ─── make_handle_value ───────────────────────────────────────────────

    #[test]
    fn make_handle_value_populates_required_keys() {
        let v = make_handle_value(42, "echo hi", 12345);
        let h = v.as_hash_ref().expect("hashref");
        let g = h.read();
        assert_eq!(g.get("__pty_id__").unwrap().to_int(), 42);
        assert_eq!(g.get("__pty__").unwrap().to_int(), 1);
        assert_eq!(g.get("cmd").unwrap().to_string(), "echo hi");
        assert_eq!(g.get("pid").unwrap().to_int(), 12345);
    }

    // ─── handle_id ───────────────────────────────────────────────────────

    #[test]
    fn handle_id_extracts_from_pty_handle_hashref() {
        let v = make_handle_value(99, "cmd", 7);
        assert_eq!(handle_id(&v), Some(99));
    }

    #[test]
    fn handle_id_non_hash_returns_none() {
        assert_eq!(handle_id(&StrykeValue::integer(123)), None);
    }

    #[test]
    fn handle_id_hash_without_marker_returns_none() {
        // A plain hashref without `__pty_id__` key → None.
        let mut m = IndexMap::new();
        m.insert("other".to_string(), StrykeValue::integer(1));
        let v = StrykeValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)));
        assert_eq!(handle_id(&v), None);
    }
}
