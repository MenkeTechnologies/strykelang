//! Progress reporting for parallel builtins when `progress => EXPR` is truthy.
//!
//! ## `PmapProgress` — aggregate bar for `pmap`, `pgrep`, `pfor`, `preduce`, …
//!
//! A single updating line (`\r` + clear-to-EOL) fills left to right with a spinner and elapsed
//! time, like `brew install` / `cargo build`.  A background ticker redraws every 80 ms.
//!
//! ## `FanProgress` — per-worker bars for `fan` / `fan_cap`
//!
//! Each worker gets its own line with a **pv-style sweep animation** while running, snapping to a
//! full bar on completion.  A background ticker redraws all lines every 80 ms so the sweep is
//! always visually moving left→right.
//!
//! ## Stream selection
//!
//! If **stderr** is a TTY, progress goes to stderr; else if **stdout** is a TTY, progress goes to
//! stdout.  `FORGE_PROGRESS_PLAIN=1` forces one line per tick (CI/logs).
//! `FORGE_PROGRESS_FULLSCREEN=1` opts in to the alternate-screen mode (PmapProgress only).

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const TICK_INTERVAL_MS: u64 = 80;

#[derive(Clone, Copy, Debug)]
enum ProgressStream {
    Stderr,
    Stdout,
}

// ─── helpers shared by both progress types ───────────────────────────────────

fn detect_stream() -> (ProgressStream, bool) {
    let stderr_tty = io::stderr().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    if stderr_tty {
        (ProgressStream::Stderr, true)
    } else if stdout_tty {
        (ProgressStream::Stdout, true)
    } else {
        (ProgressStream::Stderr, false)
    }
}

fn flush_other(stream: ProgressStream) {
    match stream {
        ProgressStream::Stderr => {
            let _ = io::stdout().flush();
        }
        ProgressStream::Stdout => {
            let _ = io::stderr().flush();
        }
    }
}

fn env_force_plain_lines() -> bool {
    match std::env::var("FORGE_PROGRESS_PLAIN") {
        Ok(s) if s == "0" || s.eq_ignore_ascii_case("false") => false,
        Ok(s) if s.is_empty() => false,
        Ok(_) => true,
        Err(_) => false,
    }
}

fn parse_fullscreen_var(v: &str) -> bool {
    v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
}

fn env_fullscreen_mode() -> bool {
    std::env::var("FORGE_PROGRESS_FULLSCREEN")
        .map(|v| parse_fullscreen_var(&v))
        .unwrap_or(false)
}

fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80)
        .clamp(40, 200)
}

fn terminal_height() -> usize {
    std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24)
        .max(1)
}

fn format_elapsed(start: Instant) -> String {
    let secs = start.elapsed().as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

fn format_elapsed_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("0.{}s", ms / 100)
    } else if ms < 60_000 {
        format!("{}.{}s", ms / 1000, (ms % 1000) / 100)
    } else {
        let secs = ms / 1000;
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// PmapProgress — aggregate single-bar progress (pmap, pgrep, pfor, …)
// ═════════════════════════════════════════════════════════════════════════════

struct TickerShared {
    total: usize,
    done: AtomicUsize,
    render: Mutex<()>,
    stream: ProgressStream,
    tty: bool,
    fullscreen: bool,
    force_plain_lines: bool,
    alt_active: AtomicBool,
    stop: AtomicBool,
    spinner_idx: AtomicUsize,
    start: Instant,
}

impl TickerShared {
    fn redraw(&self) {
        let d = self.done.load(Ordering::Relaxed);
        let _guard = self.render.lock();
        flush_other(self.stream);
        match self.stream {
            ProgressStream::Stderr => {
                let mut w = io::stderr().lock();
                self.draw_on_writer(&mut w, d);
            }
            ProgressStream::Stdout => {
                let mut w = io::stdout().lock();
                self.draw_on_writer(&mut w, d);
            }
        }
    }

    fn draw_on_writer(&self, w: &mut dyn Write, d: usize) {
        if !self.tty || self.force_plain_lines {
            return;
        }
        if self.fullscreen {
            if !self.alt_active.load(Ordering::SeqCst) {
                write!(w, "\x1b[?1049h\x1b[?25l").ok();
                self.alt_active.store(true, Ordering::SeqCst);
            }
            let si = self.spinner_idx.fetch_add(1, Ordering::Relaxed);
            write_fullscreen_frame(w, d, self.total, SPINNER[si % SPINNER.len()], self.start);
        } else {
            let si = self.spinner_idx.fetch_add(1, Ordering::Relaxed);
            write_line_mode_bar(w, d, self.total, SPINNER[si % SPINNER.len()], self.start);
        }
    }
}

pub(crate) struct PmapProgress {
    enabled: bool,
    shared: Arc<TickerShared>,
    ticker_handle: Option<std::thread::JoinHandle<()>>,
    finished: AtomicBool,
}

impl PmapProgress {
    pub fn new(enabled: bool, total: usize) -> Self {
        let (stream, tty) = detect_stream();
        let force_plain_lines = env_force_plain_lines();
        let fullscreen = tty && !force_plain_lines && env_fullscreen_mode();
        let enabled = enabled && total > 0;

        let shared = Arc::new(TickerShared {
            total,
            done: AtomicUsize::new(0),
            render: Mutex::new(()),
            stream,
            tty,
            fullscreen,
            force_plain_lines,
            alt_active: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            spinner_idx: AtomicUsize::new(0),
            start: Instant::now(),
        });

        let ticker_handle = if enabled && tty && !force_plain_lines {
            shared.redraw();
            let s = Arc::clone(&shared);
            Some(std::thread::spawn(move || {
                while !s.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(TICK_INTERVAL_MS));
                    if s.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    s.redraw();
                }
            }))
        } else {
            None
        };

        Self {
            enabled,
            shared,
            ticker_handle,
            finished: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn tick(&self) {
        if !self.enabled {
            return;
        }
        let d = self.shared.done.fetch_add(1, Ordering::Relaxed) + 1;
        if !self.shared.tty || self.shared.force_plain_lines {
            let _guard = self.shared.render.lock();
            flush_other(self.shared.stream);
            match self.shared.stream {
                ProgressStream::Stderr => {
                    let mut w = io::stderr().lock();
                    write_piped_lines(&mut w, d, self.shared.total);
                }
                ProgressStream::Stdout => {
                    let mut w = io::stdout().lock();
                    write_piped_lines(&mut w, d, self.shared.total);
                }
            }
        }
    }

    pub fn finish(&self) {
        if !self.enabled {
            return;
        }
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        let _guard = self.shared.render.lock();
        flush_other(self.shared.stream);
        match self.shared.stream {
            ProgressStream::Stderr => {
                let mut w = io::stderr().lock();
                self.finish_on_writer(&mut w);
            }
            ProgressStream::Stdout => {
                let mut w = io::stdout().lock();
                self.finish_on_writer(&mut w);
            }
        }
    }

    fn finish_on_writer(&self, w: &mut dyn Write) {
        let d = self.shared.done.load(Ordering::Relaxed);
        if self.shared.tty && self.shared.alt_active.load(Ordering::SeqCst) {
            let si = self.shared.spinner_idx.load(Ordering::Relaxed);
            write_fullscreen_frame(
                w,
                d,
                self.shared.total,
                SPINNER[si % SPINNER.len()],
                self.shared.start,
            );
            writeln!(w, "\x1b[?25h\x1b[?1049l").ok();
        } else if self.shared.tty && !self.shared.force_plain_lines {
            write_line_mode_bar(w, d, self.shared.total, '✔', self.shared.start);
            writeln!(w).ok();
        } else {
            writeln!(w).ok();
        }
        w.flush().ok();
    }
}

impl Drop for PmapProgress {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.ticker_handle.take() {
            let _ = h.join();
        }
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.shared.tty && self.shared.alt_active.load(Ordering::SeqCst) {
            let _ = match self.shared.stream {
                ProgressStream::Stderr => writeln!(io::stderr(), "\x1b[?25h\x1b[?1049l"),
                ProgressStream::Stdout => writeln!(io::stdout(), "\x1b[?25h\x1b[?1049l"),
            };
        }
    }
}

// ── PmapProgress rendering ──────────────────────────────────────────────────

fn write_fullscreen_frame(
    w: &mut dyn Write,
    done: usize,
    total: usize,
    spinner: char,
    start: Instant,
) {
    let cols = terminal_width();
    let rows = terminal_height();
    let bar_w = cols.saturating_sub(4).clamp(16, 96);
    let filled = (done * bar_w) / total.max(1);
    let pct = (done * 100) / total.max(1);
    let bar: String = (0..bar_w)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();

    write!(w, "\x1b[2J\x1b[H").ok();
    let pad_top = (rows / 2).saturating_sub(3);
    for _ in 0..pad_top {
        writeln!(w).ok();
    }
    let inner = bar_w + 14;
    let pad_l = (cols.saturating_sub(inner)) / 2;
    let pad = " ".repeat(pad_l);
    writeln!(w, "{}{} parallel", pad, spinner).ok();
    writeln!(w).ok();
    writeln!(w, "{}[{}]", pad, bar).ok();
    writeln!(
        w,
        "{}  {:3}%     {}/{}  {}",
        pad,
        pct,
        done,
        total,
        format_elapsed(start)
    )
    .ok();
    w.flush().ok();
}

fn write_line_mode_bar(
    w: &mut dyn Write,
    done: usize,
    total: usize,
    spinner: char,
    start: Instant,
) {
    const BAR_W: usize = 48;
    let filled = (done * BAR_W) / total.max(1);
    let pct = (done * 100) / total.max(1);
    let bar: String = (0..BAR_W)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    write!(
        w,
        "\r\x1b[K{} [parallel] [{}] {:3}% ({}/{}) {}",
        spinner,
        bar,
        pct,
        done,
        total,
        format_elapsed(start)
    )
    .ok();
    w.flush().ok();
}

fn write_piped_lines(w: &mut dyn Write, done: usize, total: usize) {
    const BAR_W: usize = 48;
    let filled = (done * BAR_W) / total.max(1);
    let pct = (done * 100) / total.max(1);
    let bar: String = (0..BAR_W)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    writeln!(w, "[parallel] [{}] {:3}% ({}/{})", bar, pct, done, total).ok();
    w.flush().ok();
}

// ═════════════════════════════════════════════════════════════════════════════
// FanProgress — per-worker animated bars for fan / fan_cap
// ═════════════════════════════════════════════════════════════════════════════

const WORKER_PENDING: u8 = 0;
const WORKER_RUNNING: u8 = 1;
const WORKER_DONE: u8 = 2;

/// Per-worker slot; atomics avoid locking on the hot path.
struct WorkerSlot {
    /// 0 = pending, 1 = running, 2 = done.
    state: AtomicU8,
    /// Milliseconds since `FanShared::start` when this worker began.
    started_ms: AtomicU64,
    /// Total elapsed ms for this worker (set on completion).
    elapsed_ms: AtomicU64,
}

struct FanShared {
    total: usize,
    stream: ProgressStream,
    tty: bool,
    force_plain_lines: bool,
    start: Instant,
    workers: Vec<WorkerSlot>,
    render: Mutex<()>,
    stop: AtomicBool,
    spinner_idx: AtomicUsize,
    /// Counts completed workers (for plain-lines fallback).
    done_count: AtomicUsize,
    /// True after the first frame has been drawn (so subsequent draws know to cursor-up).
    drawn_once: AtomicBool,
}

impl FanShared {
    fn redraw(&self) {
        let _guard = self.render.lock();
        flush_other(self.stream);
        match self.stream {
            ProgressStream::Stderr => {
                let mut w = io::stderr().lock();
                self.draw_workers(&mut w);
            }
            ProgressStream::Stdout => {
                let mut w = io::stdout().lock();
                self.draw_workers(&mut w);
            }
        }
    }

    fn draw_workers(&self, w: &mut dyn Write) {
        if !self.tty || self.force_plain_lines {
            return;
        }
        let n = self.total;
        let now_ms = self.start.elapsed().as_millis() as u64;
        let si = self.spinner_idx.fetch_add(1, Ordering::Relaxed);
        let spinner = SPINNER[si % SPINNER.len()];
        let bar_w = (terminal_width().saturating_sub(22)).clamp(16, 40);
        let idx_w = digit_count(n.saturating_sub(1));

        if self.drawn_once.swap(true, Ordering::SeqCst) {
            // Move cursor up to overwrite previous frame.
            write!(w, "\x1b[{}A", n).ok();
        } else {
            // First frame: hide cursor.
            write!(w, "\x1b[?25l").ok();
        }

        for i in 0..n {
            let slot = &self.workers[i];
            let state = slot.state.load(Ordering::Relaxed);
            write!(w, "\r\x1b[K").ok();
            match state {
                WORKER_RUNNING => {
                    let started = slot.started_ms.load(Ordering::Relaxed);
                    let elapsed = now_ms.saturating_sub(started);
                    let bar = render_asymptotic_fill(bar_w, elapsed);
                    writeln!(
                        w,
                        "{} worker {:>width$}  [{}]  {}",
                        spinner,
                        i,
                        bar,
                        format_elapsed_ms(elapsed),
                        width = idx_w,
                    )
                    .ok();
                }
                WORKER_DONE => {
                    let elapsed = slot.elapsed_ms.load(Ordering::Relaxed);
                    writeln!(
                        w,
                        "✔ worker {:>width$}  [{}]  {}",
                        i,
                        "█".repeat(bar_w),
                        format_elapsed_ms(elapsed),
                        width = idx_w,
                    )
                    .ok();
                }
                _ => {
                    // Pending.
                    writeln!(
                        w,
                        "  worker {:>width$}  [{}]  waiting",
                        i,
                        "░".repeat(bar_w),
                        width = idx_w,
                    )
                    .ok();
                }
            }
        }
        w.flush().ok();
    }
}

/// Per-worker animated progress bars for `fan` / `fan_cap`.
pub(crate) struct FanProgress {
    enabled: bool,
    shared: Arc<FanShared>,
    ticker_handle: Option<std::thread::JoinHandle<()>>,
    finished: AtomicBool,
}

impl FanProgress {
    pub fn new(enabled: bool, total: usize) -> Self {
        let (stream, tty) = detect_stream();
        let force_plain_lines = env_force_plain_lines();
        let enabled = enabled && total > 0;

        let workers: Vec<WorkerSlot> = (0..total)
            .map(|_| WorkerSlot {
                state: AtomicU8::new(WORKER_PENDING),
                started_ms: AtomicU64::new(0),
                elapsed_ms: AtomicU64::new(0),
            })
            .collect();

        let shared = Arc::new(FanShared {
            total,
            stream,
            tty,
            force_plain_lines,
            start: Instant::now(),
            workers,
            render: Mutex::new(()),
            stop: AtomicBool::new(false),
            spinner_idx: AtomicUsize::new(0),
            done_count: AtomicUsize::new(0),
            drawn_once: AtomicBool::new(false),
        });

        let ticker_handle = if enabled && tty && !force_plain_lines {
            // Draw the initial frame (all workers "waiting").
            shared.redraw();
            let s = Arc::clone(&shared);
            Some(std::thread::spawn(move || {
                while !s.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(TICK_INTERVAL_MS));
                    if s.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    s.redraw();
                }
            }))
        } else {
            None
        };

        Self {
            enabled,
            shared,
            ticker_handle,
            finished: AtomicBool::new(false),
        }
    }

    /// Mark worker `i` as running (call before the block executes).
    #[inline]
    pub fn start_worker(&self, i: usize) {
        if !self.enabled || i >= self.shared.total {
            return;
        }
        let slot = &self.shared.workers[i];
        let now_ms = self.shared.start.elapsed().as_millis() as u64;
        slot.started_ms.store(now_ms, Ordering::Relaxed);
        slot.state.store(WORKER_RUNNING, Ordering::Relaxed);
    }

    /// Mark worker `i` as done (call after the block executes).
    #[inline]
    pub fn finish_worker(&self, i: usize) {
        if !self.enabled || i >= self.shared.total {
            return;
        }
        let slot = &self.shared.workers[i];
        let started = slot.started_ms.load(Ordering::Relaxed);
        let now_ms = self.shared.start.elapsed().as_millis() as u64;
        slot.elapsed_ms
            .store(now_ms.saturating_sub(started), Ordering::Relaxed);
        slot.state.store(WORKER_DONE, Ordering::Relaxed);

        let d = self.shared.done_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Plain-lines fallback (no ticker thread running).
        if !self.shared.tty || self.shared.force_plain_lines {
            let _guard = self.shared.render.lock();
            flush_other(self.shared.stream);
            match self.shared.stream {
                ProgressStream::Stderr => {
                    let mut w = io::stderr().lock();
                    write_piped_lines(&mut w, d, self.shared.total);
                }
                ProgressStream::Stdout => {
                    let mut w = io::stdout().lock();
                    write_piped_lines(&mut w, d, self.shared.total);
                }
            }
        }
    }

    /// Finalize the display: draw the last frame, show cursor.
    pub fn finish(&self) {
        if !self.enabled {
            return;
        }
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        // Draw the final frame so all bars show ✔.
        self.shared.redraw();
        // Show cursor.
        if self.shared.tty && !self.shared.force_plain_lines {
            let _guard = self.shared.render.lock();
            match self.shared.stream {
                ProgressStream::Stderr => {
                    write!(io::stderr(), "\x1b[?25h").ok();
                    let _ = io::stderr().flush();
                }
                ProgressStream::Stdout => {
                    write!(io::stdout(), "\x1b[?25h").ok();
                    let _ = io::stdout().flush();
                }
            }
        }
    }
}

impl Drop for FanProgress {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.ticker_handle.take() {
            let _ = h.join();
        }
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        // Restore cursor visibility.
        if self.shared.tty
            && !self.shared.force_plain_lines
            && self.shared.drawn_once.load(Ordering::SeqCst)
        {
            let _ = match self.shared.stream {
                ProgressStream::Stderr => write!(io::stderr(), "\x1b[?25h"),
                ProgressStream::Stdout => write!(io::stdout(), "\x1b[?25h"),
            };
        }
    }
}

// ── FanProgress rendering helpers ───────────────────────────────────────────

/// Asymptotic fill: bar fills quickly at first then slows, never reaching 100%
/// until the worker actually finishes.  Uses `1 - e^(-t/τ)` with τ = 8 seconds
/// so the bar is ~63% at 8 s, ~86% at 16 s, ~95% at 24 s, always creeping forward.
fn render_asymptotic_fill(bar_w: usize, elapsed_ms: u64) -> String {
    const TAU_MS: f64 = 8000.0;
    // Cap at 95% so the bar never looks "done" until finish_worker is called.
    let frac = (1.0 - (-(elapsed_ms as f64) / TAU_MS).exp()).min(0.95);
    let filled = (frac * bar_w as f64) as usize;

    (0..bar_w)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect()
}

fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    ((n as f64).log10().floor() as usize) + 1
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_progress_tick_is_noop() {
        let p = PmapProgress::new(false, 10);
        for _ in 0..5 {
            p.tick();
        }
        p.finish();
    }

    #[test]
    fn zero_total_disables_progress() {
        let p = PmapProgress::new(true, 0);
        p.tick();
        p.finish();
    }

    #[test]
    fn parse_fullscreen_var_accepts_truthy_tokens() {
        assert!(parse_fullscreen_var("1"));
        assert!(parse_fullscreen_var("true"));
        assert!(!parse_fullscreen_var("0"));
        assert!(!parse_fullscreen_var(""));
    }

    #[test]
    fn fan_progress_disabled_is_noop() {
        let p = FanProgress::new(false, 4);
        p.start_worker(0);
        p.finish_worker(0);
        p.finish();
    }

    #[test]
    fn fan_progress_zero_total_is_noop() {
        let p = FanProgress::new(true, 0);
        p.finish();
    }

    #[test]
    fn asymptotic_fill_length_and_monotonic() {
        let mut prev_filled = 0;
        for ms in (0..30_000).step_by(100) {
            let bar = render_asymptotic_fill(30, ms);
            assert_eq!(bar.chars().count(), 30);
            let filled = bar.chars().filter(|&c| c == '█').count();
            assert!(filled >= prev_filled, "bar must never shrink");
            assert!(filled < 30, "bar must not reach 100% before finish");
            prev_filled = filled;
        }
    }

    #[test]
    fn digit_count_works() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(1), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
    }
}
