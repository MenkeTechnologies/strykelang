//! Wall-clock profiler for `fo --profile`.
//!
//! **Tree-walker**: per-statement line times and [`Profiler::enter_sub`] / [`Profiler::exit_sub`]
//! around subroutine bodies.
//!
//! **Bytecode VM**: per-opcode wall time is charged to that opcode's source line; `Call` / `Return`
//! add inclusive subroutine samples (Cranelift JIT is disabled while profiling).

use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

/// Line- and sub-level timings (nanoseconds).
pub struct Profiler {
    file: String,
    line_ns: HashMap<(String, usize), u64>,
    sub_stack: Vec<String>,
    /// Collapsed stacks `a;b;c` → total ns (flamegraph.pl folded input).
    folded_ns: HashMap<String, u64>,
    /// Per-subroutine name → inclusive time (ns).
    sub_inclusive_ns: HashMap<String, u64>,
}

impl Profiler {
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line_ns: HashMap::new(),
            sub_stack: Vec::new(),
            folded_ns: HashMap::new(),
            sub_inclusive_ns: HashMap::new(),
        }
    }

    pub fn on_line(&mut self, file: &str, line: usize, dt: Duration) {
        let ns = dt.as_nanos() as u64;
        *self.line_ns.entry((file.to_string(), line)).or_insert(0) += ns;
    }

    pub fn enter_sub(&mut self, name: &str) {
        self.sub_stack.push(name.to_string());
    }

    pub fn exit_sub(&mut self, dt: Duration) {
        let ns = dt.as_nanos() as u64;
        let Some(name) = self.sub_stack.pop() else {
            return;
        };
        *self.sub_inclusive_ns.entry(name.clone()).or_insert(0) += ns;
        let prefix = self.sub_stack.join(";");
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{};{}", prefix, name)
        };
        *self.folded_ns.entry(full).or_insert(0) += ns;
    }

    /// stderr: folded stacks (flamegraph.pl) + line totals + sub totals.
    pub fn print_report(&mut self) {
        // Incomplete enter/exit pairs (e.g. `die` before `return`) would confuse folded output.
        self.sub_stack.clear();

        eprintln!("# forge --profile: collapsed stacks (name stack → ns); feed to flamegraph.pl");
        let mut stacks: Vec<_> = self.folded_ns.iter().collect();
        stacks.sort_by(|a, b| b.1.cmp(a.1));
        for (k, ns) in stacks.iter() {
            eprintln!("{} {}", k, ns);
        }

        eprintln!("# forge --profile: lines (file:line → total ns)");
        let mut lines: Vec<_> = self.line_ns.iter().collect();
        lines.sort_by(|a, b| b.1.cmp(a.1));
        for ((f, ln), ns) in lines.iter() {
            eprintln!("{}:{} {}", f, ln, ns);
        }

        eprintln!("# forge --profile: subs (name → inclusive ns)");
        let mut subs: Vec<_> = self.sub_inclusive_ns.iter().collect();
        subs.sort_by(|a, b| b.1.cmp(a.1));
        for (name, ns) in subs {
            eprintln!("{} {}", name, ns);
        }
        eprintln!("# profile script: {}", self.file);
    }

    /// Render an SVG flamegraph to `writer` using the collected folded stacks.
    pub fn render_flame_svg<W: Write>(&mut self, writer: W) -> std::io::Result<()> {
        self.sub_stack.clear();

        let lines: Vec<String> = self
            .folded_ns
            .iter()
            .map(|(stack, ns)| format!("{} {}", stack, ns))
            .collect();
        let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();

        let mut opts = inferno::flamegraph::Options::default();
        opts.title = format!("forge --flame: {}", self.file);
        opts.count_name = "ns".to_string();
        opts.colors = inferno::flamegraph::color::Palette::Basic(
            inferno::flamegraph::color::BasicPalette::Hot,
        );
        inferno::flamegraph::from_lines(&mut opts, line_refs, writer)
    }

    /// Render a colored terminal flamegraph to stderr.
    ///
    /// Shows: (1) per-sub inclusive bars sorted hottest-first,
    /// (2) per-stack-frame bars with call depth indentation,
    /// (3) hottest source lines.
    pub fn render_flame_tty(&mut self) {
        self.sub_stack.clear();
        let total_ns = self.folded_ns.values().copied().max().unwrap_or(1);
        let term_width = term_width();
        // reserve columns: "100.0%  " (8) + name (dynamic) + " " + bar + " 999.9ms"
        let time_suffix_len = 10;
        let pct_prefix_len = 8;

        // ── header ──────────────────────────────────────────────────
        eprintln!("\x1b[1;97m── forge --flame: {} ──\x1b[0m", self.file);
        eprintln!();

        // ── subroutine inclusive time (flat) ─────────────────────────
        if !self.sub_inclusive_ns.is_empty() {
            eprintln!("\x1b[1;97m  Subroutines (inclusive)\x1b[0m");
            let mut subs: Vec<_> = self.sub_inclusive_ns.iter().collect();
            subs.sort_by(|a, b| b.1.cmp(a.1));
            let max_name = subs.iter().map(|(n, _)| n.len()).max().unwrap_or(4).min(40);
            let bar_budget =
                term_width.saturating_sub(pct_prefix_len + max_name + 2 + time_suffix_len);
            for (name, &ns) in &subs {
                let pct = ns as f64 / total_ns as f64 * 100.0;
                let bar_len = (ns as f64 / total_ns as f64 * bar_budget as f64) as usize;
                let color = heat_color(pct);
                let display_name = if name.len() > 40 {
                    format!("…{}", &name[name.len() - 39..])
                } else {
                    name.to_string()
                };
                eprintln!(
                    "  {:>5.1}%  {:<width$} {}{}\x1b[0m {}",
                    pct,
                    display_name,
                    color,
                    "█".repeat(bar_len.max(1)),
                    format_ns(ns),
                    width = max_name,
                );
            }
            eprintln!();
        }

        // ── call stacks (tree-style) ────────────────────────────────
        if !self.folded_ns.is_empty() {
            eprintln!("\x1b[1;97m  Call stacks\x1b[0m");
            let mut stacks: Vec<_> = self.folded_ns.iter().collect();
            stacks.sort_by(|a, b| b.1.cmp(a.1));
            let max_show = 20;
            for (stack, &ns) in stacks.iter().take(max_show) {
                let pct = ns as f64 / total_ns as f64 * 100.0;
                let depth = stack.matches(';').count();
                let leaf = stack.rsplit(';').next().unwrap_or(stack);
                let indent = "  ".repeat(depth);
                let display = format!("{}{}", indent, leaf);
                let name_width = display.len().min(50);
                let bar_budget =
                    term_width.saturating_sub(pct_prefix_len + name_width + 2 + time_suffix_len);
                let bar_len = (ns as f64 / total_ns as f64 * bar_budget as f64) as usize;
                let color = heat_color(pct);
                eprintln!(
                    "  {:>5.1}%  {:<width$} {}{}\x1b[0m {}",
                    pct,
                    display,
                    color,
                    "█".repeat(bar_len.max(1)),
                    format_ns(ns),
                    width = name_width,
                );
            }
            if stacks.len() > max_show {
                eprintln!("  … and {} more stacks", stacks.len() - max_show);
            }
            eprintln!();
        }

        // ── hottest source lines ────────────────────────────────────
        if !self.line_ns.is_empty() {
            eprintln!("\x1b[1;97m  Hot lines\x1b[0m");
            let mut lines: Vec<_> = self.line_ns.iter().collect();
            lines.sort_by(|a, b| b.1.cmp(a.1));
            let max_show = 10;
            let line_total: u64 = lines.iter().map(|(_, &ns)| ns).sum();
            for ((f, ln), &ns) in lines.iter().take(max_show) {
                let pct = ns as f64 / line_total as f64 * 100.0;
                let color = heat_color(pct);
                eprintln!(
                    "  {:>5.1}%  {}{}:{}\x1b[0m  {}",
                    pct,
                    color,
                    f,
                    ln,
                    format_ns(ns),
                );
            }
        }
        eprintln!();
    }
}

fn term_width() -> usize {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(2, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
            return ws.ws_col as usize;
        }
    }
    80
}

fn heat_color(pct: f64) -> &'static str {
    if pct >= 60.0 {
        "\x1b[1;91m" // bright red
    } else if pct >= 30.0 {
        "\x1b[1;93m" // bright yellow
    } else if pct >= 10.0 {
        "\x1b[33m" // yellow
    } else {
        "\x1b[32m" // green
    }
}

fn format_ns(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.1}s", ns as f64 / 1e9)
    } else if ns >= 1_000_000 {
        format!("{:.1}ms", ns as f64 / 1e6)
    } else if ns >= 1_000 {
        format!("{:.1}µs", ns as f64 / 1e3)
    } else {
        format!("{}ns", ns)
    }
}

#[cfg(test)]
impl Profiler {
    fn line_total_ns(&self, file: &str, line: usize) -> u64 {
        self.line_ns
            .get(&(file.to_string(), line))
            .copied()
            .unwrap_or(0)
    }

    fn folded_total_ns(&self, key: &str) -> u64 {
        self.folded_ns.get(key).copied().unwrap_or(0)
    }

    fn sub_inclusive_total_ns(&self, name: &str) -> u64 {
        self.sub_inclusive_ns.get(name).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn on_line_accumulates_per_file_line() {
        let mut p = Profiler::new("a.pl");
        p.on_line("a.pl", 2, Duration::from_nanos(100));
        p.on_line("a.pl", 2, Duration::from_nanos(50));
        assert_eq!(p.line_total_ns("a.pl", 2), 150);
    }

    #[test]
    fn exit_sub_nested_stack_folded_keys() {
        let mut p = Profiler::new("a.pl");
        p.enter_sub("outer");
        p.enter_sub("inner");
        p.exit_sub(Duration::from_nanos(7));
        assert_eq!(p.sub_inclusive_total_ns("inner"), 7);
        assert_eq!(p.folded_total_ns("outer;inner"), 7);
        p.exit_sub(Duration::from_nanos(11));
        assert_eq!(p.sub_inclusive_total_ns("outer"), 11);
        assert_eq!(p.folded_total_ns("outer"), 11);
    }

    #[test]
    fn exit_sub_without_matching_enter_is_silent() {
        let mut p = Profiler::new("a.pl");
        p.exit_sub(Duration::from_nanos(1));
        assert_eq!(p.sub_inclusive_total_ns("nope"), 0);
    }
}
