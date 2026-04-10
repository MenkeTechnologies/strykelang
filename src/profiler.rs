//! Wall-clock profiler for `pe --profile`.
//!
//! **Tree-walker**: per-statement line times and [`Profiler::enter_sub`] / [`Profiler::exit_sub`]
//! around subroutine bodies.
//!
//! **Bytecode VM**: per-opcode wall time is charged to that opcode's source line; `Call` / `Return`
//! add inclusive subroutine samples (Cranelift JIT is disabled while profiling).

use std::collections::HashMap;
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

        eprintln!("# perlrs --profile: collapsed stacks (name stack → ns); feed to flamegraph.pl");
        let mut stacks: Vec<_> = self.folded_ns.iter().collect();
        stacks.sort_by(|a, b| b.1.cmp(a.1));
        for (k, ns) in stacks.iter() {
            eprintln!("{} {}", k, ns);
        }

        eprintln!("# perlrs --profile: lines (file:line → total ns)");
        let mut lines: Vec<_> = self.line_ns.iter().collect();
        lines.sort_by(|a, b| b.1.cmp(a.1));
        for ((f, ln), ns) in lines.iter() {
            eprintln!("{}:{} {}", f, ln, ns);
        }

        eprintln!("# perlrs --profile: subs (name → inclusive ns)");
        let mut subs: Vec<_> = self.sub_inclusive_ns.iter().collect();
        subs.sort_by(|a, b| b.1.cmp(a.1));
        for (name, ns) in subs {
            eprintln!("{} {}", name, ns);
        }
        eprintln!("# profile script: {}", self.file);
    }
}
