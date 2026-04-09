//! Progress reporting on stderr for parallel **`p*`** builtins when `progress => EXPR` is truthy
//! (`pmap`, `pgrep`, `pfor`, `preduce`, `pmap_chunked`, `psort`, `pcache`, `par_lines`, …).

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

/// Renders a `\r` progress line on stderr while parallel work runs.
pub(crate) struct PmapProgress {
    total: usize,
    done: AtomicUsize,
    last_pct: Mutex<u8>,
    enabled: bool,
}

impl PmapProgress {
    pub fn new(enabled: bool, total: usize) -> Self {
        Self {
            total,
            done: AtomicUsize::new(0),
            last_pct: Mutex::new(0),
            enabled: enabled && total > 0,
        }
    }

    #[inline]
    pub fn tick(&self) {
        if !self.enabled {
            return;
        }
        let d = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        let pct = ((d * 100) / self.total.max(1)).min(100) as u8;
        let mut prev = self.last_pct.lock();
        if pct > *prev {
            *prev = pct;
            let mut stderr = io::stderr().lock();
            write_bar(&mut stderr, d, self.total);
        }
    }

    pub fn finish(&self) {
        if !self.enabled {
            return;
        }
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
    }
}

fn write_bar(stderr: &mut dyn Write, done: usize, total: usize) {
    const W: usize = 40;
    let filled = (done * W) / total.max(1);
    let pct = (done * 100) / total.max(1);
    let bar: String = (0..W).map(|i| if i < filled { '#' } else { '-' }).collect();
    write!(
        stderr,
        "\r[parallel] [{}] {:3}% ({}/{})",
        bar, pct, done, total
    )
    .ok();
    stderr.flush().ok();
}
