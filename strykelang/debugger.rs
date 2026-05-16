//! Interactive debugger for stryke programs.
//!
//! Provides breakpoint-based debugging with single-stepping, variable inspection,
//! and call stack display. Two front-ends share this state machine:
//!
//! * **TTY/CLI** (default, via `-d`) — `prompt()` reads commands from stdin and
//!   writes to stderr. Same UX as `perl -d`.
//! * **DAP** (via `--dap`) — `prompt()` instead routes through
//!   [`crate::dap::DapShared`], emitting `stopped` events and condvar-waiting
//!   for resume. Stdin is owned by the DAP reader thread; the debugger never
//!   touches it in DAP mode.
//!
//! The split is per-instance, not compile-time: `Debugger::set_dap_backend`
//! configures the DAP backend. With no backend set, TTY behavior runs.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use crate::scope::Scope;
use crate::value::StrykeValue;

/// Debugger state, shared between VM and interpreter.
pub struct Debugger {
    /// Breakpoints by line number.
    breakpoints: HashSet<usize>,
    /// Breakpoints by subroutine name.
    sub_breakpoints: HashSet<String>,
    /// Single-step mode: stop at every statement/opcode.
    step_mode: bool,
    /// Step-over mode: stop at next statement at same or lower call depth.
    step_over_depth: Option<usize>,
    /// Step-out mode: stop when returning to this call depth.
    step_out_depth: Option<usize>,
    /// Current call depth for step-over/step-out.
    call_depth: usize,
    /// Last line we stopped at (avoid repeated stops on same line).
    last_stop_line: Option<usize>,
    /// Call depth at the last stop. Paired with [`Self::last_stop_line`] so the
    /// same-line guard treats a depth change (sub entered or returned) as
    /// forward progress — stepIn must fire when we enter a sub even if the
    /// first opcode inside reports the same source line as the call site.
    last_stop_depth: usize,
    /// Current source file name.
    pub file: String,
    /// Source lines (for display).
    source_lines: Vec<String>,
    /// Whether debugger is enabled.
    enabled: bool,
    /// Watch expressions (variable names to display on each stop).
    watches: Vec<String>,
    /// Command history.
    history: Vec<String>,
    /// Optional DAP backend. When `Some`, `prompt()` emits a `stopped` event
    /// and condvar-waits instead of reading from stdin.
    dap_backend: Option<DapBackendHandle>,
}

/// Opaque handle into [`crate::dap::DapShared`] + the shared breakpoint state.
/// Kept as `Arc<dyn Any>` so debugger.rs does not depend on dap.rs at compile
/// time (and tests work without DAP).
pub struct DapBackendHandle {
    pub shared: Arc<crate::dap::DapShared>,
    pub bp_state: Arc<Mutex<crate::dap::BreakpointState>>,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl Debugger {
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
            sub_breakpoints: HashSet::new(),
            step_mode: true,
            step_over_depth: None,
            step_out_depth: None,
            call_depth: 0,
            last_stop_line: None,
            last_stop_depth: 0,
            file: String::new(),
            source_lines: Vec::new(),
            enabled: true,
            watches: Vec::new(),
            history: Vec::new(),
            dap_backend: None,
        }
    }

    /// Add a line breakpoint programmatically (used by the DAP server before
    /// the VM starts). TTY users add via the `b N` command.
    pub fn add_breakpoint_line(&mut self, line: usize) {
        self.breakpoints.insert(line);
    }

    /// Add a function breakpoint programmatically.
    pub fn add_breakpoint_sub(&mut self, name: &str) {
        self.sub_breakpoints.insert(name.to_string());
    }

    /// Clear every line breakpoint (the DAP server re-sends the full set on
    /// every `setBreakpoints` request).
    pub fn clear_line_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Replace the entire line-breakpoint set in one shot.
    pub fn set_line_breakpoints(&mut self, lines: &[usize]) {
        self.breakpoints = lines.iter().copied().collect();
    }

    /// Toggle step-mode (controls whether `should_stop` returns true on every
    /// new line). The DAP server flips this in response to `next` / `stepIn`.
    pub fn set_step_mode(&mut self, on: bool) {
        self.step_mode = on;
    }

    /// Request a step-over from the next stop.
    pub fn request_step_over(&mut self) {
        self.step_over_depth = Some(self.call_depth);
    }

    /// Request a step-out from the next stop.
    pub fn request_step_out(&mut self) {
        self.step_out_depth = Some(self.call_depth);
    }

    /// Install a DAP backend. After this call, `prompt()` will route through
    /// the DAP server instead of TTY.
    pub fn set_dap_backend(
        &mut self,
        shared: Arc<crate::dap::DapShared>,
        bp_state: Arc<Mutex<crate::dap::BreakpointState>>,
    ) {
        self.dap_backend = Some(DapBackendHandle { shared, bp_state });
    }

    /// True when this debugger instance is wired to a DAP front-end.
    #[inline]
    pub fn is_dap(&self) -> bool {
        self.dap_backend.is_some()
    }

    /// Snapshot helper: current breakpoint lines, sorted.
    pub fn breakpoint_lines(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.breakpoints.iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// Build a [`crate::dap::PauseSnapshot`] for the current stop. Used only
    /// in DAP mode; harmless in TTY mode (returns an empty default).
    fn build_snapshot(
        &self,
        line: usize,
        scope: &Scope,
        call_stack: &[(String, usize)],
        reason: &str,
    ) -> crate::dap::PauseSnapshot {
        let mut frames: Vec<crate::dap::FrameSnap> = Vec::new();
        // Innermost (current) frame first
        frames.push(crate::dap::FrameSnap {
            name: "<current>".to_string(),
            file: self.file.clone(),
            line,
        });
        for (name, l) in call_stack.iter().rev() {
            frames.push(crate::dap::FrameSnap {
                name: name.clone(),
                file: self.file.clone(),
                line: *l,
            });
        }
        let mut var_ref_map = std::collections::HashMap::new();
        let locals = crate::dap::capture_locals_with_map(scope, &mut var_ref_map);
        crate::dap::PauseSnapshot {
            file: self.file.clone(),
            line,
            reason: reason.to_string(),
            frames,
            locals,
            globals: Vec::new(),
            var_ref_map,
        }
    }

    /// Load source for display in debugger.
    pub fn load_source(&mut self, source: &str) {
        self.source_lines = source.lines().map(String::from).collect();
    }

    /// Set source file name.
    pub fn set_file(&mut self, file: &str) {
        self.file = file.to_string();
    }

    /// Check if debugger should stop at this line.
    pub fn should_stop(&mut self, line: usize) -> bool {
        if !self.enabled {
            return false;
        }

        if std::env::var("STRYKE_DBG_TRACE").is_ok() {
            eprintln!("[ss] line={} bp_set={:?}", line, self.breakpoints.iter().collect::<Vec<_>>());
        }

        // Line 0 is the VM's "no source mapping" sentinel — synthetic teardown
        // ops, prelude bytecode, etc. Never user-visible. Skip silently or the
        // IDE pauses on an invalid frame (no Variables, no source highlight).
        if line == 0 {
            return false;
        }

        // Same-line guard: skip when we haven't made progress since the last
        // stop. Progress = the source line moved OR the call depth changed
        // (sub entered/returned). Without the depth half, stepIn would fire
        // on the next opcode of the call site (same line) instead of the
        // first opcode inside the sub.
        if self.last_stop_line == Some(line) && self.call_depth == self.last_stop_depth {
            return false;
        }

        // Check breakpoints
        if self.breakpoints.contains(&line) {
            return true;
        }

        // Check step mode
        if self.step_mode {
            return true;
        }

        // Check step-over (stop at same or lower depth)
        if let Some(depth) = self.step_over_depth {
            if self.call_depth <= depth {
                self.step_over_depth = None;
                return true;
            }
        }

        // Check step-out (stop when returning)
        if let Some(depth) = self.step_out_depth {
            if self.call_depth < depth {
                self.step_out_depth = None;
                return true;
            }
        }

        false
    }

    /// Check if we should stop at subroutine entry.
    pub fn should_stop_at_sub(&self, name: &str) -> bool {
        self.enabled && self.sub_breakpoints.contains(name)
    }

    /// Notify debugger of subroutine call.
    pub fn enter_sub(&mut self, _name: &str) {
        self.call_depth += 1;
    }

    /// Notify debugger of subroutine return.
    pub fn leave_sub(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    /// Interactive debugger prompt. Returns true to continue, false to quit.
    ///
    /// Two paths:
    /// * **DAP** — emit a `stopped` event with a snapshot, condvar-wait for
    ///   the next resume command, apply step-mode flags from the shared
    ///   breakpoint state, and return.
    /// * **TTY** — original `perl -d`-style REPL on stdin/stderr.
    pub fn prompt(
        &mut self,
        line: usize,
        scope: &Scope,
        call_stack: &[(String, usize)],
    ) -> DebugAction {
        self.last_stop_line = Some(line);
        self.last_stop_depth = self.call_depth;
        self.step_mode = false;

        // DAP front-end: route through the shared state, return early.
        if let Some(backend) = self.dap_backend.as_ref() {
            let reason = if self.breakpoints.contains(&line) {
                "breakpoint"
            } else {
                "step"
            };
            let snap = self.build_snapshot(line, scope, call_stack, reason);
            let shared = backend.shared.clone();
            let bp = backend.bp_state.clone();
            let action = shared.pause(snap);
            // Read any step-kind set by the DAP reader thread.
            if let Ok(mut g) = bp.lock() {
                if let Some(kind) = g.pending_step.take() {
                    match kind {
                        crate::dap::StepKind::Over => self.step_over_depth = Some(self.call_depth),
                        crate::dap::StepKind::Into => self.step_mode = true,
                        crate::dap::StepKind::Out => self.step_out_depth = Some(self.call_depth),
                    }
                }
                // Sync line breakpoints (client may have sent new ones while paused)
                if let Some(lines) = g.line_breakpoints.get(&self.file) {
                    let lines = lines.clone();
                    self.set_line_breakpoints(&lines);
                }
            }
            return action;
        }

        // Print location and source context
        self.print_location(line);
        self.print_watches(scope);

        loop {
            eprint!("  DB<{}> ", self.history.len() + 1);
            io::stderr().flush().ok();

            let mut input = String::new();
            if io::stdin().lock().read_line(&mut input).is_err() {
                return DebugAction::Quit;
            }
            let input = input.trim();

            if input.is_empty() {
                // Repeat last command or step
                if let Some(last) = self.history.last().cloned() {
                    return self.execute_command(&last, line, scope, call_stack);
                }
                self.step_mode = true;
                return DebugAction::Continue;
            }

            self.history.push(input.to_string());
            let action = self.execute_command(input, line, scope, call_stack);
            if !matches!(action, DebugAction::Prompt) {
                return action;
            }
        }
    }

    fn execute_command(
        &mut self,
        input: &str,
        line: usize,
        scope: &Scope,
        call_stack: &[(String, usize)],
    ) -> DebugAction {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd {
            // Step commands
            "s" | "step" | "n" | "next" => {
                self.step_mode = true;
                DebugAction::Continue
            }
            "o" | "over" => {
                self.step_over_depth = Some(self.call_depth);
                DebugAction::Continue
            }
            "out" | "finish" | "r" => {
                self.step_out_depth = Some(self.call_depth);
                DebugAction::Continue
            }
            "c" | "cont" | "continue" => {
                self.step_mode = false;
                DebugAction::Continue
            }

            // Breakpoints
            "b" | "break" => {
                if arg.is_empty() {
                    self.breakpoints.insert(line);
                    eprintln!("Breakpoint set at line {}", line);
                } else if let Ok(n) = arg.parse::<usize>() {
                    self.breakpoints.insert(n);
                    eprintln!("Breakpoint set at line {}", n);
                } else {
                    self.sub_breakpoints.insert(arg.to_string());
                    eprintln!("Breakpoint set at fn {}", arg);
                }
                DebugAction::Prompt
            }
            "B" | "delete" => {
                if arg.is_empty() || arg == "*" {
                    self.breakpoints.clear();
                    self.sub_breakpoints.clear();
                    eprintln!("All breakpoints deleted");
                } else if let Ok(n) = arg.parse::<usize>() {
                    self.breakpoints.remove(&n);
                    eprintln!("Breakpoint at line {} deleted", n);
                } else {
                    self.sub_breakpoints.remove(arg);
                    eprintln!("Breakpoint at fn {} deleted", arg);
                }
                DebugAction::Prompt
            }
            "L" | "breakpoints" => {
                if self.breakpoints.is_empty() && self.sub_breakpoints.is_empty() {
                    eprintln!("No breakpoints set");
                } else {
                    eprintln!("Breakpoints:");
                    for &bp in &self.breakpoints {
                        eprintln!("  line {}", bp);
                    }
                    for bp in &self.sub_breakpoints {
                        eprintln!("  fn {}", bp);
                    }
                }
                DebugAction::Prompt
            }

            // Inspection
            "p" | "print" | "x" => {
                if arg.is_empty() {
                    eprintln!("Usage: p <var> (e.g., p $x, p @arr, p %hash)");
                } else {
                    self.print_variable(arg, scope);
                }
                DebugAction::Prompt
            }
            "V" | "vars" => {
                self.print_all_vars(scope);
                DebugAction::Prompt
            }
            "w" | "watch" => {
                if arg.is_empty() {
                    if self.watches.is_empty() {
                        eprintln!("No watches set");
                    } else {
                        eprintln!("Watches: {}", self.watches.join(", "));
                    }
                } else {
                    self.watches.push(arg.to_string());
                    eprintln!("Watching: {}", arg);
                }
                DebugAction::Prompt
            }
            "W" => {
                if arg.is_empty() || arg == "*" {
                    self.watches.clear();
                    eprintln!("All watches cleared");
                } else {
                    self.watches.retain(|w| w != arg);
                    eprintln!("Watch {} removed", arg);
                }
                DebugAction::Prompt
            }

            // Stack
            "T" | "stack" | "bt" | "backtrace" => {
                self.print_stack(call_stack, line);
                DebugAction::Prompt
            }

            // Source listing
            "l" | "list" => {
                let target = if arg.is_empty() {
                    line
                } else {
                    arg.parse().unwrap_or(line)
                };
                self.list_source(target, 10);
                DebugAction::Prompt
            }
            "." => {
                self.print_location(line);
                DebugAction::Prompt
            }

            // Control
            "q" | "quit" | "exit" => DebugAction::Quit,
            "h" | "help" | "?" => {
                self.print_help();
                DebugAction::Prompt
            }
            "D" | "disable" => {
                self.enabled = false;
                eprintln!("Debugger disabled (use -d to re-enable on next run)");
                DebugAction::Continue
            }

            _ => {
                eprintln!("Unknown command: {}. Type 'h' for help.", cmd);
                DebugAction::Prompt
            }
        }
    }

    fn print_location(&self, line: usize) {
        let file_display = if self.file.is_empty() {
            "<eval>"
        } else {
            &self.file
        };
        eprintln!();
        eprintln!("{}:{}", file_display, line);

        // Print surrounding lines
        let start = line.saturating_sub(2);
        let end = (line + 2).min(self.source_lines.len());
        for i in start..end {
            let marker = if i + 1 == line { "==>" } else { "   " };
            if let Some(src) = self.source_lines.get(i) {
                eprintln!("{} {:4}:  {}", marker, i + 1, src);
            }
        }
    }

    fn print_watches(&self, scope: &Scope) {
        if self.watches.is_empty() {
            return;
        }
        eprintln!("Watches:");
        for w in &self.watches {
            eprint!("  {} = ", w);
            self.print_variable(w, scope);
        }
    }

    fn print_variable(&self, var: &str, scope: &Scope) {
        let var = var.trim();
        if let Some(name) = var.strip_prefix('$') {
            let val = scope.get_scalar(name);
            eprintln!("{}", format_value(&val));
        } else if let Some(name) = var.strip_prefix('@') {
            let val = scope.get_array(name);
            eprintln!(
                "({})",
                val.iter().map(format_value).collect::<Vec<_>>().join(", ")
            );
        } else if let Some(name) = var.strip_prefix('%') {
            let val = scope.get_hash(name);
            let pairs: Vec<String> = val
                .iter()
                .map(|(k, v)| format!("{} => {}", k, format_value(v)))
                .collect();
            eprintln!("({})", pairs.join(", "));
        } else {
            // Assume scalar
            let val = scope.get_scalar(var);
            eprintln!("{}", format_value(&val));
        }
    }

    fn print_all_vars(&self, scope: &Scope) {
        let vars = scope.all_scalar_names();
        if vars.is_empty() {
            eprintln!("No variables in scope");
            return;
        }
        eprintln!("Variables:");
        for name in vars {
            if name.starts_with('^') || name.starts_with('_') && name.len() > 2 {
                continue; // Skip special vars
            }
            let val = scope.get_scalar(&name);
            if !val.is_undef() {
                eprintln!("  ${} = {}", name, format_value(&val));
            }
        }
    }

    fn print_stack(&self, call_stack: &[(String, usize)], current_line: usize) {
        eprintln!("Call stack:");
        if call_stack.is_empty() {
            eprintln!("  #0  <main> at line {}", current_line);
        } else {
            for (i, (name, line)) in call_stack.iter().enumerate().rev() {
                eprintln!("  #{}  {} at line {}", call_stack.len() - i, name, line);
            }
            eprintln!("  #0  <current> at line {}", current_line);
        }
    }

    fn list_source(&self, center: usize, radius: usize) {
        let start = center.saturating_sub(radius);
        let end = (center + radius).min(self.source_lines.len());
        for i in start..end {
            let marker = if i + 1 == center { "==>" } else { "   " };
            let bp = if self.breakpoints.contains(&(i + 1)) {
                "b"
            } else {
                " "
            };
            if let Some(src) = self.source_lines.get(i) {
                eprintln!("{}{} {:4}:  {}", marker, bp, i + 1, src);
            }
        }
    }

    fn print_help(&self) {
        eprintln!(
            r#"
Debugger Commands:
  s, step, n, next    Step to next statement
  o, over             Step over (don't descend into subs)
  out, finish, r      Step out (run until sub returns)
  c, cont, continue   Continue execution

  b [line|sub]        Set breakpoint (current line if no arg)
  B [line|sub|*]      Delete breakpoint(s)
  L, breakpoints      List all breakpoints

  p, print, x <var>   Print variable ($x, @arr, %hash)
  V, vars             Print all variables in scope
  w <var>             Add watch expression
  W [var|*]           Remove watch expression(s)

  T, stack, bt        Print call stack backtrace
  l [line]            List source around line
  .                   Show current location

  q, quit, exit       Quit program
  h, help, ?          Show this help
  D, disable          Disable debugger (continue without stops)

  <Enter>             Repeat last command or step
"#
        );
    }
}

/// Action to take after debugger prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugAction {
    Continue,
    Quit,
    Prompt,
}

pub(crate) fn format_value(val: &StrykeValue) -> String {
    if val.is_undef() {
        "undef".to_string()
    } else if let Some(s) = val.as_str() {
        if s.parse::<f64>().is_ok() {
            s.to_string()
        } else {
            format!("\"{}\"", s.escape_default())
        }
    } else if let Some(n) = val.as_integer() {
        n.to_string()
    } else if let Some(f) = val.as_float() {
        f.to_string()
    } else if val.as_array_ref().is_some() || val.as_array_vec().is_some() {
        let list = val.to_list();
        let items: Vec<String> = list.iter().map(format_value).collect();
        format!("[{}]", items.join(", "))
    } else if val.as_hash_ref().is_some() {
        if let Some(map) = val.as_hash_map() {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} => {}", k, format_value(v)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        } else {
            "HASH(?)".to_string()
        }
    } else if val.as_code_ref().is_some() {
        "CODE(...)".to_string()
    } else {
        val.type_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debugger_new_defaults() {
        let dbg = Debugger::new();
        assert!(dbg.breakpoints.is_empty());
        assert!(dbg.sub_breakpoints.is_empty());
        assert!(dbg.step_mode);
        assert!(dbg.enabled);
        assert!(dbg.watches.is_empty());
        assert_eq!(dbg.call_depth, 0);
    }

    #[test]
    fn debugger_load_source_splits_lines() {
        let mut dbg = Debugger::new();
        dbg.load_source("line1\nline2\nline3");
        assert_eq!(dbg.source_lines.len(), 3);
        assert_eq!(dbg.source_lines[0], "line1");
        assert_eq!(dbg.source_lines[2], "line3");
    }

    #[test]
    fn debugger_set_file() {
        let mut dbg = Debugger::new();
        dbg.set_file("test.pl");
        assert_eq!(dbg.file, "test.pl");
    }

    #[test]
    fn debugger_should_stop_at_breakpoint() {
        let mut dbg = Debugger::new();
        dbg.step_mode = false;
        dbg.breakpoints.insert(10);
        assert!(dbg.should_stop(10));
        assert!(!dbg.should_stop(11));
    }

    #[test]
    fn debugger_should_stop_in_step_mode() {
        let mut dbg = Debugger::new();
        dbg.step_mode = true;
        assert!(dbg.should_stop(1));
        assert!(dbg.should_stop(999));
    }

    #[test]
    fn debugger_should_stop_disabled() {
        let mut dbg = Debugger::new();
        dbg.enabled = false;
        dbg.step_mode = true;
        assert!(!dbg.should_stop(1));
    }

    #[test]
    fn debugger_should_stop_at_sub() {
        let mut dbg = Debugger::new();
        dbg.sub_breakpoints.insert("foo".to_string());
        assert!(dbg.should_stop_at_sub("foo"));
        assert!(!dbg.should_stop_at_sub("bar"));
    }

    #[test]
    fn debugger_enter_leave_sub_tracks_depth() {
        let mut dbg = Debugger::new();
        assert_eq!(dbg.call_depth, 0);
        dbg.enter_sub("foo");
        assert_eq!(dbg.call_depth, 1);
        dbg.enter_sub("bar");
        assert_eq!(dbg.call_depth, 2);
        dbg.leave_sub();
        assert_eq!(dbg.call_depth, 1);
        dbg.leave_sub();
        assert_eq!(dbg.call_depth, 0);
        dbg.leave_sub();
        assert_eq!(dbg.call_depth, 0);
    }

    #[test]
    fn debugger_step_over_depth() {
        let mut dbg = Debugger::new();
        dbg.step_mode = false;
        dbg.enter_sub("outer");
        dbg.step_over_depth = Some(1);
        dbg.enter_sub("inner");
        assert!(!dbg.should_stop(5));
        dbg.leave_sub();
        assert!(dbg.should_stop(6));
        assert!(dbg.step_over_depth.is_none());
    }

    #[test]
    fn debugger_step_out_depth() {
        let mut dbg = Debugger::new();
        dbg.step_mode = false;
        dbg.enter_sub("outer");
        dbg.enter_sub("inner");
        dbg.step_out_depth = Some(2);
        assert!(!dbg.should_stop(5));
        dbg.leave_sub();
        assert!(dbg.should_stop(6));
        assert!(dbg.step_out_depth.is_none());
    }

    #[test]
    fn debugger_avoids_repeated_stops_on_same_line() {
        let mut dbg = Debugger::new();
        dbg.step_mode = false;
        dbg.breakpoints.insert(10);
        assert!(dbg.should_stop(10));
        dbg.last_stop_line = Some(10);
        assert!(!dbg.should_stop(10));
    }

    /// Regression for the "step-in fires on the same line" bug. The same-
    /// line guard must treat a depth change (sub entered or returned) as
    /// forward progress, otherwise step-in lands on the next opcode of the
    /// call site (which has the same source line as the call) and the
    /// user has to click step-in twice to actually enter the sub.
    #[test]
    fn same_line_guard_yields_to_depth_change_on_step_in() {
        let mut dbg = Debugger::new();
        // Stopped at line 10, depth 0 (caller's frame).
        dbg.last_stop_line = Some(10);
        dbg.last_stop_depth = 0;
        // step-in arms step_mode.
        dbg.step_mode = true;
        // Same line, same depth (still in caller) → skip.
        assert!(!dbg.should_stop(10));
        // Depth bumps (entered the sub) → fire even on same source line.
        dbg.enter_sub("callee");
        assert!(dbg.should_stop(10));
    }

    /// And the inverse — when call_depth shrinks past `step_out_depth`,
    /// step-out should fire even though we may land on the same line as
    /// the call site (when callee tail-returns at the call line).
    #[test]
    fn step_out_fires_when_returning_to_same_line() {
        let mut dbg = Debugger::new();
        // Inside callee at depth 1, stopped at line 5.
        dbg.enter_sub("callee");
        dbg.last_stop_line = Some(5);
        dbg.last_stop_depth = 1;
        dbg.step_out_depth = Some(1);
        // Still inside callee — don't fire.
        assert!(!dbg.should_stop(5));
        // Callee returned — depth drops.
        dbg.leave_sub();
        // Same source line as the call site but depth dropped → fire.
        assert!(dbg.should_stop(5));
    }

    /// Step-over requires the *exact* depth-aware guard the production
    /// debugger uses (`call_depth <= step_over_depth`). The earlier
    /// non-depth guard let step-over follow execution into UDFs because
    /// call_depth never moved.
    #[test]
    fn step_over_skips_into_nested_frame_and_resumes_after_return() {
        let mut dbg = Debugger::new();
        dbg.step_mode = false;
        // Step-over from the call site (depth 0) at line 10.
        dbg.step_over_depth = Some(0);
        dbg.enter_sub("callee");
        // Deeper than the request — skip every line inside the sub.
        assert!(!dbg.should_stop(20));
        assert!(!dbg.should_stop(21));
        // Sub returns → depth back to 0 → fire at the line after the
        // call.
        dbg.leave_sub();
        assert!(dbg.should_stop(11));
    }

    #[test]
    fn format_value_undef() {
        assert_eq!(format_value(&StrykeValue::UNDEF), "undef");
    }

    #[test]
    fn format_value_integer() {
        assert_eq!(format_value(&StrykeValue::integer(42)), "42");
        assert_eq!(format_value(&StrykeValue::integer(-100)), "-100");
    }

    #[test]
    fn format_value_float() {
        // Use a non-PI-approximation literal to dodge clippy::approx_constant.
        let f = format_value(&StrykeValue::float(2.71));
        assert!(f.starts_with("2.71"));
    }

    #[test]
    fn format_value_string() {
        assert_eq!(
            format_value(&StrykeValue::string("hello".into())),
            "\"hello\""
        );
    }

    #[test]
    fn format_value_numeric_string() {
        assert_eq!(format_value(&StrykeValue::string("42".into())), "42");
        assert_eq!(format_value(&StrykeValue::string("3.14".into())), "3.14");
    }

    #[test]
    fn format_value_array() {
        let arr = StrykeValue::array(vec![
            StrykeValue::integer(1),
            StrykeValue::integer(2),
            StrykeValue::integer(3),
        ]);
        assert_eq!(format_value(&arr), "[1, 2, 3]");
    }

    #[test]
    fn debug_action_eq() {
        assert_eq!(DebugAction::Continue, DebugAction::Continue);
        assert_ne!(DebugAction::Continue, DebugAction::Quit);
        assert_ne!(DebugAction::Quit, DebugAction::Prompt);
    }
}
