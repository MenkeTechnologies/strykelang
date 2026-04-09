//! Progress reporting on stderr for parallel **`p*`** builtins when `progress => EXPR` is truthy
//! (`pmap`, `pgrep`, `pfor`, `preduce`, `pmap_chunked`, `psort`, `pcache`, `par_lines`, `glob_par`, …).
//!
//! Each completed work item redraws the bar on the **same** stderr line (`\r` + clear-to-EOL) so the
//! fill advances left-to-right interactively. Rayon workers call [`PmapProgress::tick`] concurrently;
//! a mutex serializes writes so lines do not interleave.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

/// Renders a `\r` progress line on stderr while parallel work runs.
pub(crate) struct PmapProgress {
    total: usize,
    done: AtomicUsize,
    /// Serializes stderr redraws — multiple rayon threads may call [`Self::tick`] at once.
    render: Mutex<()>,
    enabled: bool,
    /// When stderr is a TTY, use carriage-return + ANSI EL; otherwise print one line per tick.
    tty: bool,
}

impl PmapProgress {
    pub fn new(enabled: bool, total: usize) -> Self {
        let tty = io::stderr().is_terminal();
        Self {
            total,
            done: AtomicUsize::new(0),
            render: Mutex::new(()),
            enabled: enabled && total > 0,
            tty,
        }
    }

    #[inline]
    pub fn tick(&self) {
        if !self.enabled {
            return;
        }
        let d = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        let _guard = self.render.lock();
        // Flush stdout first so `say`/`print` lines finish before we `\r` on stderr (TTY ordering).
        let _ = io::stdout().flush();
        let mut stderr = io::stderr().lock();
        write_bar(&mut stderr, d, self.total, self.tty);
    }

    pub fn finish(&self) {
        if !self.enabled {
            return;
        }
        let _guard = self.render.lock();
        let _ = io::stdout().flush();
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
        stderr.flush().ok();
    }
}

fn write_bar(stderr: &mut dyn Write, done: usize, total: usize, tty: bool) {
    const W: usize = 48;
    let filled = (done * W) / total.max(1);
    let pct = (done * 100) / total.max(1);
    // Left-to-right fill: solid block then light shade (readable on light/dark backgrounds).
    let bar: String = (0..W).map(|i| if i < filled { '█' } else { '░' }).collect();
    if tty {
        // \r: same line; \x1b[K: erase to end of line (avoid leftover chars if bar shrinks).
        write!(
            stderr,
            "\r\x1b[K[parallel] [{}] {:3}% ({}/{})",
            bar, pct, done, total
        )
        .ok();
    } else {
        // Piped stderr: each completion is its own line so nothing is overwritten.
        writeln!(
            stderr,
            "[parallel] [{}] {:3}% ({}/{})",
            bar, pct, done, total
        )
        .ok();
    }
    stderr.flush().ok();
}

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
}
