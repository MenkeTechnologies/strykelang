//! Interactive debugger for stryke programs.
//!
//! Provides breakpoint-based debugging with single-stepping, variable inspection,
//! and call stack display. Activated via `-d` flag.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};

use crate::scope::Scope;
use crate::value::PerlValue;

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
            file: String::new(),
            source_lines: Vec::new(),
            enabled: true,
            watches: Vec::new(),
            history: Vec::new(),
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

        // Avoid stopping on the same line repeatedly (unless stepping)
        if !self.step_mode && self.last_stop_line == Some(line) {
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
    pub fn prompt(
        &mut self,
        line: usize,
        scope: &Scope,
        call_stack: &[(String, usize)],
    ) -> DebugAction {
        self.last_stop_line = Some(line);
        self.step_mode = false;

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

fn format_value(val: &PerlValue) -> String {
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

    #[test]
    fn format_value_undef() {
        assert_eq!(format_value(&PerlValue::UNDEF), "undef");
    }

    #[test]
    fn format_value_integer() {
        assert_eq!(format_value(&PerlValue::integer(42)), "42");
        assert_eq!(format_value(&PerlValue::integer(-100)), "-100");
    }

    #[test]
    fn format_value_float() {
        // Use a non-PI-approximation literal to dodge clippy::approx_constant.
        let f = format_value(&PerlValue::float(2.71));
        assert!(f.starts_with("2.71"));
    }

    #[test]
    fn format_value_string() {
        assert_eq!(
            format_value(&PerlValue::string("hello".into())),
            "\"hello\""
        );
    }

    #[test]
    fn format_value_numeric_string() {
        assert_eq!(format_value(&PerlValue::string("42".into())), "42");
        assert_eq!(format_value(&PerlValue::string("3.14".into())), "3.14");
    }

    #[test]
    fn format_value_array() {
        let arr = PerlValue::array(vec![
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(3),
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
