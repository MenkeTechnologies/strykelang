//! Shell command executor for zshrs
//!
//! Executes the parsed shell AST.

use crate::shell_history::HistoryEngine;
use crate::shell_jobs::{continue_job, wait_for_child, wait_for_job, JobState, JobTable};
use crate::shell_parse::*;
use crate::shell_zwc::ZwcFile;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct ShellExecutor {
    pub functions: HashMap<String, ShellCommand>,
    pub aliases: HashMap<String, String>,
    pub last_status: i32,
    pub variables: HashMap<String, String>,
    pub arrays: HashMap<String, Vec<String>>,
    pub jobs: JobTable,
    pub fpath: Vec<PathBuf>,
    pub zwc_cache: HashMap<PathBuf, ZwcFile>,
    pub positional_params: Vec<String>,
    pub history: Option<HistoryEngine>,
    process_sub_counter: u32,
    pub traps: HashMap<String, String>,
    pub options: HashMap<String, bool>,
}

impl ShellExecutor {
    pub fn new() -> Self {
        // Initialize fpath from FPATH env var or use defaults
        let fpath = env::var("FPATH")
            .unwrap_or_default()
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();

        let history = HistoryEngine::new().ok();

        Self {
            functions: HashMap::new(),
            aliases: HashMap::new(),
            last_status: 0,
            variables: HashMap::new(),
            arrays: HashMap::new(),
            jobs: JobTable::new(),
            fpath,
            zwc_cache: HashMap::new(),
            positional_params: Vec::new(),
            history,
            process_sub_counter: 0,
            traps: HashMap::new(),
            options: HashMap::new(),
        }
    }

    /// Try to load a function from ZWC files in fpath
    pub fn autoload_function(&mut self, name: &str) -> Option<ShellCommand> {
        // First check if already loaded
        if let Some(func) = self.functions.get(name) {
            return Some(func.clone());
        }

        // Search fpath for the function
        for dir in &self.fpath.clone() {
            // Try directory.zwc first
            let zwc_path = dir.with_extension("zwc");
            if zwc_path.exists() {
                if let Some(func) = self.load_function_from_zwc(&zwc_path, name) {
                    return Some(func);
                }
            }

            // Try individual function.zwc
            let func_zwc = dir.join(format!("{}.zwc", name));
            if func_zwc.exists() {
                if let Some(func) = self.load_function_from_zwc(&func_zwc, name) {
                    return Some(func);
                }
            }

            // Look for directory/*.zwc files containing this function
            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |e| e == "zwc") {
                            if let Some(func) = self.load_function_from_zwc(&path, name) {
                                return Some(func);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Load a specific function from a ZWC file
    fn load_function_from_zwc(&mut self, path: &Path, name: &str) -> Option<ShellCommand> {
        // Check cache
        let zwc = if let Some(cached) = self.zwc_cache.get(path) {
            cached
        } else {
            // Load and cache the ZWC file
            let zwc = ZwcFile::load(path).ok()?;
            self.zwc_cache.insert(path.to_path_buf(), zwc);
            self.zwc_cache.get(path)?
        };

        // Find the function
        let func = zwc.get_function(name)?;
        let decoded = zwc.decode_function(func)?;

        // Convert to shell command and cache
        let shell_func = decoded.to_shell_function()?;

        // Register the function
        if let ShellCommand::FunctionDef(fname, body) = &shell_func {
            self.functions.insert(fname.clone(), (**body).clone());
        }

        Some(shell_func)
    }

    /// Add a directory to fpath
    pub fn add_fpath(&mut self, path: PathBuf) {
        if !self.fpath.contains(&path) {
            self.fpath.insert(0, path);
        }
    }

    pub fn execute_script(&mut self, script: &str) -> Result<i32, String> {
        // Expand history references before parsing
        let expanded = self.expand_history(script);

        let mut parser = ShellParser::new(&expanded);
        let commands = parser.parse_script()?;

        for cmd in commands {
            self.execute_command(&cmd)?;
        }

        Ok(self.last_status)
    }

    /// Expand history references: !!, !n, !-n, !string, !?string?
    fn expand_history(&self, input: &str) -> String {
        let Some(ref engine) = self.history else {
            return input.to_string();
        };

        if !input.contains('!') {
            return input.to_string();
        }

        let history_count = engine.count().unwrap_or(0) as usize;
        if history_count == 0 {
            return input.to_string();
        }

        let mut result = String::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        let mut in_brace = 0; // Track ${...} nesting

        while i < chars.len() {
            // Track brace depth to avoid expanding ! inside ${...}
            if i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '{' {
                in_brace += 1;
                result.push(chars[i]);
                i += 1;
                result.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '}' && in_brace > 0 {
                in_brace -= 1;
                result.push(chars[i]);
                i += 1;
                continue;
            }

            if chars[i] == '!' && in_brace == 0 {
                if i + 1 < chars.len() {
                    match chars[i + 1] {
                        '!' => {
                            // !! - previous command
                            if let Ok(Some(entry)) = engine.get_by_offset(0) {
                                result.push_str(&entry.command);
                            }
                            i += 2;
                            continue;
                        }
                        '-' => {
                            // !-n - nth previous command
                            i += 2;
                            let start = i;
                            while i < chars.len() && chars[i].is_ascii_digit() {
                                i += 1;
                            }
                            if i > start {
                                let n: usize = chars[start..i]
                                    .iter()
                                    .collect::<String>()
                                    .parse()
                                    .unwrap_or(0);
                                if n > 0 {
                                    if let Ok(Some(entry)) = engine.get_by_offset(n - 1) {
                                        result.push_str(&entry.command);
                                    }
                                }
                            }
                            continue;
                        }
                        '?' => {
                            // !?string? - contains search
                            i += 2;
                            let start = i;
                            while i < chars.len() && chars[i] != '?' {
                                i += 1;
                            }
                            let search: String = chars[start..i].iter().collect();
                            if i < chars.len() && chars[i] == '?' {
                                i += 1;
                            }
                            if let Ok(entries) = engine.search(&search, 1) {
                                if let Some(entry) = entries.first() {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        c if c.is_ascii_digit() => {
                            // !n - command at position n
                            i += 1;
                            let start = i;
                            while i < chars.len() && chars[i].is_ascii_digit() {
                                i += 1;
                            }
                            let n: i64 = chars[start..i]
                                .iter()
                                .collect::<String>()
                                .parse()
                                .unwrap_or(0);
                            if n > 0 {
                                if let Ok(Some(entry)) = engine.get_by_number(n) {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        c if c.is_alphabetic() || c == '_' || c == '/' || c == '.' => {
                            // !string - prefix search
                            i += 1;
                            let start = i;
                            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '!' {
                                i += 1;
                            }
                            let prefix: String = chars[start..i].iter().collect();
                            if let Ok(entries) = engine.search_prefix(&prefix, 1) {
                                if let Some(entry) = entries.first() {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            result.push(chars[i]);
            i += 1;
        }

        result
    }

    pub fn execute_command(&mut self, cmd: &ShellCommand) -> Result<i32, String> {
        match cmd {
            ShellCommand::Simple(simple) => self.execute_simple(simple),
            ShellCommand::Pipeline(cmds, negated) => {
                let status = self.execute_pipeline(cmds)?;
                if *negated {
                    self.last_status = if status == 0 { 1 } else { 0 };
                } else {
                    self.last_status = status;
                }
                Ok(self.last_status)
            }
            ShellCommand::List(items) => self.execute_list(items),
            ShellCommand::Compound(compound) => self.execute_compound(compound),
            ShellCommand::FunctionDef(name, body) => {
                self.functions.insert(name.clone(), (**body).clone());
                self.last_status = 0;
                Ok(0)
            }
        }
    }

    fn execute_simple(&mut self, cmd: &SimpleCommand) -> Result<i32, String> {
        // Handle assignments
        for (var, val) in &cmd.assignments {
            match val {
                ShellWord::ArrayLiteral(elements) => {
                    // Array assignment: arr=(a b c)
                    let array: Vec<String> = elements.iter().map(|e| self.expand_word(e)).collect();
                    self.arrays.insert(var.clone(), array);
                }
                _ => {
                    let expanded = self.expand_word(val);
                    if cmd.words.is_empty() {
                        // Just assignment, set in environment
                        env::set_var(var, &expanded);
                    }
                    self.variables.insert(var.clone(), expanded);
                }
            }
        }

        if cmd.words.is_empty() {
            self.last_status = 0;
            return Ok(0);
        }

        let words: Vec<String> = cmd
            .words
            .iter()
            .flat_map(|w| self.expand_word_glob(w))
            .collect();
        if words.is_empty() {
            self.last_status = 0;
            return Ok(0);
        }
        let cmd_name = &words[0];
        let args = &words[1..];

        // Check for shell builtins
        let status = match cmd_name.as_str() {
            "cd" => self.builtin_cd(args),
            "pwd" => self.builtin_pwd(&cmd.redirects),
            "echo" => self.builtin_echo(args, &cmd.redirects),
            "export" => self.builtin_export(args),
            "unset" => self.builtin_unset(args),
            "source" | "." => self.builtin_source(args),
            "exit" => self.builtin_exit(args),
            "return" => self.builtin_return(args),
            "true" => 0,
            "false" => 1,
            ":" => 0,
            "test" | "[" => self.builtin_test(args),
            "local" => self.builtin_local(args),
            "declare" | "typeset" => self.builtin_declare(args),
            "read" => self.builtin_read(args),
            "shift" => self.builtin_shift(args),
            "eval" => self.builtin_eval(args),
            "jobs" => self.builtin_jobs(args),
            "fg" => self.builtin_fg(args),
            "bg" => self.builtin_bg(args),
            "kill" => self.builtin_kill(args),
            "disown" => self.builtin_disown(args),
            "wait" => self.builtin_wait(args),
            "autoload" => self.builtin_autoload(args),
            "history" => self.builtin_history(args),
            "fc" => self.builtin_fc(args),
            "trap" => self.builtin_trap(args),
            "alias" => self.builtin_alias(args),
            "unalias" => self.builtin_unalias(args),
            "set" => self.builtin_set(args),
            "shopt" => self.builtin_shopt(args),
            "getopts" => self.builtin_getopts(args),
            "type" => self.builtin_type(args),
            "hash" => self.builtin_hash(args),
            "command" => self.builtin_command(args, &cmd.redirects),
            "builtin" => self.builtin_builtin(args, &cmd.redirects),
            "let" => self.builtin_let(args),
            _ => {
                // Check for function
                if let Some(func) = self.functions.get(cmd_name).cloned() {
                    return self.call_function(&func, args);
                }

                // Try autoloading from ZWC
                if self.autoload_function(cmd_name).is_some() {
                    if let Some(func) = self.functions.get(cmd_name).cloned() {
                        return self.call_function(&func, args);
                    }
                }

                // External command
                self.execute_external(cmd_name, args, &cmd.redirects)?
            }
        };

        self.last_status = status;
        Ok(status)
    }

    /// Call a function with positional parameters
    fn call_function(&mut self, func: &ShellCommand, args: &[String]) -> Result<i32, String> {
        // Save current positional params
        let saved_params = std::mem::take(&mut self.positional_params);

        // Set new positional params
        self.positional_params = args.to_vec();

        // Execute the function
        let result = self.execute_command(func);

        // Restore positional params
        self.positional_params = saved_params;

        result
    }

    fn execute_external(
        &mut self,
        cmd: &str,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<i32, String> {
        self.execute_external_bg(cmd, args, redirects, false)
    }

    fn execute_external_bg(
        &mut self,
        cmd: &str,
        args: &[String],
        redirects: &[Redirect],
        background: bool,
    ) -> Result<i32, String> {
        let mut command = Command::new(cmd);
        command.args(args);

        // Apply redirections
        for redir in redirects {
            let target = self.expand_word(&redir.target);
            match redir.op {
                RedirectOp::Read => match File::open(&target) {
                    Ok(f) => {
                        command.stdin(Stdio::from(f));
                    }
                    Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                },
                RedirectOp::Write => match File::create(&target) {
                    Ok(f) => {
                        command.stdout(Stdio::from(f));
                    }
                    Err(e) => return Err(format!("Cannot create {}: {}", target, e)),
                },
                RedirectOp::Append => {
                    match OpenOptions::new().create(true).append(true).open(&target) {
                        Ok(f) => {
                            command.stdout(Stdio::from(f));
                        }
                        Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                    }
                }
                RedirectOp::WriteBoth => match File::create(&target) {
                    Ok(f) => {
                        let f2 = f
                            .try_clone()
                            .map_err(|e| format!("Cannot clone fd: {}", e))?;
                        command.stdout(Stdio::from(f));
                        command.stderr(Stdio::from(f2));
                    }
                    Err(e) => return Err(format!("Cannot create {}: {}", target, e)),
                },
                RedirectOp::AppendBoth => {
                    match OpenOptions::new().create(true).append(true).open(&target) {
                        Ok(f) => {
                            let f2 = f
                                .try_clone()
                                .map_err(|e| format!("Cannot clone fd: {}", e))?;
                            command.stdout(Stdio::from(f));
                            command.stderr(Stdio::from(f2));
                        }
                        Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                    }
                }
                RedirectOp::HereDoc => {
                    // Here-document - provide content as stdin
                    if let Some(ref content) = redir.heredoc_content {
                        // Expand variables in content (unless delimiter was quoted)
                        let expanded = self.expand_string(content);
                        command.stdin(Stdio::piped());
                        // Store the content to write after spawn
                        // For now, create a temp file
                        use std::io::Write;
                        let mut temp_file = tempfile::NamedTempFile::new()
                            .map_err(|e| format!("Cannot create temp file: {}", e))?;
                        temp_file
                            .write_all(expanded.as_bytes())
                            .map_err(|e| format!("Cannot write to temp file: {}", e))?;
                        let temp_path = temp_file.into_temp_path();
                        let f = File::open(&temp_path)
                            .map_err(|e| format!("Cannot open temp file: {}", e))?;
                        command.stdin(Stdio::from(f));
                    }
                }
                RedirectOp::HereString => {
                    // Here-string - provide target as stdin
                    use std::io::Write;
                    let content = format!("{}\n", target);
                    let mut temp_file = tempfile::NamedTempFile::new()
                        .map_err(|e| format!("Cannot create temp file: {}", e))?;
                    temp_file
                        .write_all(content.as_bytes())
                        .map_err(|e| format!("Cannot write to temp file: {}", e))?;
                    let temp_path = temp_file.into_temp_path();
                    let f = File::open(&temp_path)
                        .map_err(|e| format!("Cannot open temp file: {}", e))?;
                    command.stdin(Stdio::from(f));
                }
                _ => {
                    // Other redirections handled simply
                }
            }
        }

        if background {
            match command.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    let cmd_str = format!("{} {}", cmd, args.join(" "));
                    let job_id = self.jobs.add_job(child, cmd_str, JobState::Running);
                    println!("[{}] {}", job_id, pid);
                    Ok(0)
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        eprintln!("zshrs: command not found: {}", cmd);
                        Ok(127)
                    } else {
                        Err(format!("zshrs: {}: {}", cmd, e))
                    }
                }
            }
        } else {
            match command.status() {
                Ok(status) => Ok(status.code().unwrap_or(1)),
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        eprintln!("zshrs: command not found: {}", cmd);
                        Ok(127)
                    } else {
                        Err(format!("zshrs: {}: {}", cmd, e))
                    }
                }
            }
        }
    }

    fn execute_pipeline(&mut self, cmds: &[ShellCommand]) -> Result<i32, String> {
        if cmds.len() == 1 {
            return self.execute_command(&cmds[0]);
        }

        let mut children: Vec<Child> = Vec::new();
        let mut prev_stdout: Option<std::process::ChildStdout> = None;

        for (i, cmd) in cmds.iter().enumerate() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if words.is_empty() {
                    continue;
                }

                let mut command = Command::new(&words[0]);
                command.args(&words[1..]);

                if let Some(stdout) = prev_stdout.take() {
                    command.stdin(Stdio::from(stdout));
                }

                if i < cmds.len() - 1 {
                    command.stdout(Stdio::piped());
                }

                match command.spawn() {
                    Ok(mut child) => {
                        prev_stdout = child.stdout.take();
                        children.push(child);
                    }
                    Err(e) => {
                        eprintln!("zshrs: {}: {}", words[0], e);
                        return Ok(127);
                    }
                }
            }
        }

        // Wait for all children
        let mut last_status = 0;
        for mut child in children {
            if let Ok(status) = child.wait() {
                last_status = status.code().unwrap_or(1);
            }
        }

        Ok(last_status)
    }

    fn execute_list(&mut self, items: &[(ShellCommand, ListOp)]) -> Result<i32, String> {
        for (cmd, op) in items {
            // Check if this command should run in background
            let background = matches!(op, ListOp::Amp);

            let status = if background {
                self.execute_command_bg(cmd)?
            } else {
                self.execute_command(cmd)?
            };

            match op {
                ListOp::And => {
                    if status != 0 {
                        return Ok(status);
                    }
                }
                ListOp::Or => {
                    if status == 0 {
                        return Ok(0);
                    }
                }
                ListOp::Amp => {
                    // Already backgrounded above, continue
                }
                ListOp::Semi | ListOp::Newline => {
                    // Sequential, continue
                }
            }
        }

        Ok(self.last_status)
    }

    fn execute_command_bg(&mut self, cmd: &ShellCommand) -> Result<i32, String> {
        // For simple commands, run in background
        if let ShellCommand::Simple(simple) = cmd {
            if simple.words.is_empty() {
                return Ok(0);
            }
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_name = &words[0];
            let args: Vec<String> = words[1..].to_vec();
            return self.execute_external_bg(cmd_name, &args, &simple.redirects, true);
        }
        // For complex commands, just execute normally (could fork in future)
        self.execute_command(cmd)
    }

    fn execute_compound(&mut self, compound: &CompoundCommand) -> Result<i32, String> {
        match compound {
            CompoundCommand::BraceGroup(cmds) | CompoundCommand::Subshell(cmds) => {
                for cmd in cmds {
                    self.execute_command(cmd)?;
                }
                Ok(self.last_status)
            }

            CompoundCommand::If {
                conditions,
                else_part,
            } => {
                for (cond, body) in conditions {
                    // Execute condition
                    for cmd in cond {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status == 0 {
                        // Condition true, execute body
                        for cmd in body {
                            self.execute_command(cmd)?;
                        }
                        return Ok(self.last_status);
                    }
                }

                // All conditions false, execute else
                if let Some(else_cmds) = else_part {
                    for cmd in else_cmds {
                        self.execute_command(cmd)?;
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::For { var, words, body } => {
                let items: Vec<String> = if let Some(words) = words {
                    words
                        .iter()
                        .flat_map(|w| self.expand_word_split(w))
                        .collect()
                } else {
                    // Iterate over positional parameters
                    self.positional_params.clone()
                };

                for item in items {
                    env::set_var(var, &item);
                    self.variables.insert(var.clone(), item);

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::ForArith {
                init,
                cond,
                step,
                body,
            } => {
                // C-style for loop: for ((init; cond; step))
                // Execute init expression (use evaluate_arithmetic_expr for assignment support)
                if !init.is_empty() {
                    self.evaluate_arithmetic_expr(init);
                }

                // Loop while condition is true
                loop {
                    // Evaluate condition (use eval_arith_expr for comparison result)
                    if !cond.is_empty() {
                        let cond_result = self.eval_arith_expr(cond);
                        if cond_result == 0 {
                            break;
                        }
                    }

                    // Execute body
                    for cmd in body {
                        self.execute_command(cmd)?;
                    }

                    // Execute step (use evaluate_arithmetic_expr for assignment support like i++)
                    if !step.is_empty() {
                        self.evaluate_arithmetic_expr(step);
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::While { condition, body } => {
                loop {
                    for cmd in condition {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status != 0 {
                        break;
                    }

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Until { condition, body } => {
                loop {
                    for cmd in condition {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status == 0 {
                        break;
                    }

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Case { word, cases } => {
                let value = self.expand_word(word);

                for (patterns, body, term) in cases {
                    for pattern in patterns {
                        let pat = self.expand_word(pattern);
                        if self.matches_pattern(&value, &pat) {
                            for cmd in body {
                                self.execute_command(cmd)?;
                            }

                            match term {
                                CaseTerminator::Break => return Ok(self.last_status),
                                CaseTerminator::Fallthrough => {
                                    // Continue to next case body
                                }
                                CaseTerminator::Continue => {
                                    // Continue pattern matching
                                    break;
                                }
                            }
                        }
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::Select { var, words, body } => {
                // Simplified: just use first word
                if let Some(words) = words {
                    if let Some(first) = words.first() {
                        let val = self.expand_word(first);
                        env::set_var(var, &val);
                        for cmd in body {
                            self.execute_command(cmd)?;
                        }
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Cond(expr) => {
                let result = self.eval_cond_expr(expr);
                self.last_status = if result { 0 } else { 1 };
                Ok(self.last_status)
            }

            CompoundCommand::Arith(expr) => {
                // Evaluate arithmetic expression and set variables
                let result = self.evaluate_arithmetic_expr(expr);
                // (( )) returns 0 if result is non-zero, 1 if result is zero
                self.last_status = if result != 0 { 0 } else { 1 };
                Ok(self.last_status)
            }

            CompoundCommand::Coproc { name, body } => {
                // Create pipes for stdin and stdout
                let (stdin_read, stdin_write) =
                    os_pipe::pipe().map_err(|e| format!("Cannot create pipe: {}", e))?;
                let (stdout_read, stdout_write) =
                    os_pipe::pipe().map_err(|e| format!("Cannot create pipe: {}", e))?;

                // Get the command to run
                let cmd_str = match body.as_ref() {
                    ShellCommand::Simple(simple) => simple
                        .words
                        .iter()
                        .map(|w| self.expand_word(w))
                        .collect::<Vec<_>>()
                        .join(" "),
                    ShellCommand::Compound(CompoundCommand::BraceGroup(cmds)) => {
                        // Just run as a subshell with the commands
                        // For simplicity, we'll use bash -c
                        "bash -c 'true'".to_string()
                    }
                    _ => "true".to_string(),
                };

                // Fork and run the command in background with redirected stdin/stdout
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if parts.is_empty() {
                    return Ok(0);
                }

                let mut command = Command::new(parts[0]);
                if parts.len() > 1 {
                    command.args(&parts[1..]);
                }

                use std::os::unix::io::{FromRawFd, IntoRawFd};

                command.stdin(unsafe { Stdio::from_raw_fd(stdin_read.into_raw_fd()) });
                command.stdout(unsafe { Stdio::from_raw_fd(stdout_write.into_raw_fd()) });

                match command.spawn() {
                    Ok(child) => {
                        let pid = child.id();
                        let coproc_name = name.clone().unwrap_or_else(|| "COPROC".to_string());

                        // Store file descriptors in environment-like variables
                        // COPROC[0] = read from coproc (stdout_read)
                        // COPROC[1] = write to coproc (stdin_write)
                        let read_fd = stdout_read.into_raw_fd();
                        let write_fd = stdin_write.into_raw_fd();

                        self.arrays.insert(
                            coproc_name.clone(),
                            vec![read_fd.to_string(), write_fd.to_string()],
                        );

                        // Also store PID
                        self.variables
                            .insert(format!("{}_PID", coproc_name), pid.to_string());

                        let cmd_str_clone = cmd_str.clone();
                        self.jobs.add_job(child, cmd_str_clone, JobState::Running);

                        Ok(0)
                    }
                    Err(e) => {
                        if e.kind() == io::ErrorKind::NotFound {
                            eprintln!("zshrs: command not found: {}", parts[0]);
                            Ok(127)
                        } else {
                            Err(format!("zshrs: coproc: {}: {}", parts[0], e))
                        }
                    }
                }
            }
        }
    }

    /// Expand a word with brace and glob expansion (for command arguments)
    fn expand_word_glob(&mut self, word: &ShellWord) -> Vec<String> {
        match word {
            ShellWord::SingleQuoted(s) => vec![s.clone()],
            ShellWord::DoubleQuoted(parts) => {
                // Double quotes prevent glob and brace expansion
                vec![parts.iter().map(|p| self.expand_word(p)).collect()]
            }
            _ => {
                let expanded = self.expand_word(word);

                // First do brace expansion
                let brace_expanded = self.expand_braces(&expanded);

                // Then glob expansion on each result
                brace_expanded
                    .into_iter()
                    .flat_map(|s| {
                        if s.contains('*')
                            || s.contains('?')
                            || s.contains('[')
                            || self.has_extglob_pattern(&s)
                        {
                            self.expand_glob(&s)
                        } else {
                            vec![s]
                        }
                    })
                    .collect()
            }
        }
    }

    /// Expand brace patterns like {a,b,c} and {1..10}
    fn expand_braces(&self, s: &str) -> Vec<String> {
        // Find a brace pattern
        let mut depth = 0;
        let mut brace_start = None;

        for (i, c) in s.char_indices() {
            match c {
                '{' => {
                    if depth == 0 {
                        brace_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start) = brace_start {
                            let prefix = &s[..start];
                            let content = &s[start + 1..i];
                            let suffix = &s[i + 1..];

                            // Check if this is a sequence {a..b} or a list {a,b,c}
                            let expansions = if content.contains("..") {
                                self.expand_brace_sequence(content)
                            } else if content.contains(',') {
                                self.expand_brace_list(content)
                            } else {
                                // Not a valid brace expansion, return as-is
                                return vec![s.to_string()];
                            };

                            // Combine prefix, expansions, and suffix
                            let mut results = Vec::new();
                            for exp in expansions {
                                let combined = format!("{}{}{}", prefix, exp, suffix);
                                // Recursively expand any remaining braces
                                results.extend(self.expand_braces(&combined));
                            }
                            return results;
                        }
                    }
                }
                _ => {}
            }
        }

        // No brace expansion found
        vec![s.to_string()]
    }

    /// Expand comma-separated brace list like {a,b,c}
    fn expand_brace_list(&self, content: &str) -> Vec<String> {
        // Split by comma, but respect nested braces
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for c in content.chars() {
            match c {
                '{' => {
                    depth += 1;
                    current.push(c);
                }
                '}' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    parts.push(current.clone());
                    current.clear();
                }
                _ => current.push(c),
            }
        }
        parts.push(current);

        parts
    }

    /// Expand sequence brace pattern like {1..10} or {a..z}
    fn expand_brace_sequence(&self, content: &str) -> Vec<String> {
        let parts: Vec<&str> = content.splitn(3, "..").collect();
        if parts.len() < 2 {
            return vec![content.to_string()];
        }

        let start = parts[0];
        let end = parts[1];
        let step: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

        // Try numeric sequence
        if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
            let mut results = Vec::new();
            if start_num <= end_num {
                let mut i = start_num;
                while i <= end_num {
                    results.push(i.to_string());
                    i += step;
                }
            } else {
                let mut i = start_num;
                while i >= end_num {
                    results.push(i.to_string());
                    i -= step;
                }
            }
            return results;
        }

        // Try character sequence
        if start.len() == 1 && end.len() == 1 {
            let start_char = start.chars().next().unwrap();
            let end_char = end.chars().next().unwrap();
            let mut results = Vec::new();

            if start_char <= end_char {
                let mut c = start_char;
                while c <= end_char {
                    results.push(c.to_string());
                    c = (c as u8 + step as u8) as char;
                    if c as u8 > end_char as u8 {
                        break;
                    }
                }
            } else {
                let mut c = start_char;
                while c >= end_char {
                    results.push(c.to_string());
                    if (c as u8) < step as u8 {
                        break;
                    }
                    c = (c as u8 - step as u8) as char;
                }
            }
            return results;
        }

        vec![content.to_string()]
    }

    /// Expand glob pattern to matching files
    fn expand_glob(&self, pattern: &str) -> Vec<String> {
        // Check for extended glob patterns: ?(pat), *(pat), +(pat), @(pat), !(pat)
        if self.has_extglob_pattern(pattern) {
            return self.expand_extglob(pattern);
        }

        let options = glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: true,
        };

        match glob::glob_with(pattern, options) {
            Ok(paths) => {
                let expanded: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                if expanded.is_empty() {
                    // No matches - return the pattern as-is
                    vec![pattern.to_string()]
                } else {
                    expanded
                }
            }
            Err(_) => vec![pattern.to_string()],
        }
    }

    /// Check if pattern contains extended glob syntax
    fn has_extglob_pattern(&self, pattern: &str) -> bool {
        let chars: Vec<char> = pattern.chars().collect();
        for i in 0..chars.len().saturating_sub(1) {
            if (chars[i] == '?'
                || chars[i] == '*'
                || chars[i] == '+'
                || chars[i] == '@'
                || chars[i] == '!')
                && chars[i + 1] == '('
            {
                return true;
            }
        }
        false
    }

    /// Convert extended glob pattern to regex
    fn extglob_to_regex(&self, pattern: &str) -> String {
        let mut regex = String::from("^");
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            // Check for extglob patterns
            if i + 1 < chars.len() && chars[i + 1] == '(' {
                match c {
                    '?' => {
                        // ?(pattern) - zero or one occurrence
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})?", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '*' => {
                        // *(pattern) - zero or more occurrences
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})*", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '+' => {
                        // +(pattern) - one or more occurrences
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})+", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '@' => {
                        // @(pattern) - exactly one occurrence
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '!' => {
                        // !(pattern) - handled specially in expand_extglob
                        // Just skip this extglob for regex, will do manual filtering
                        let (_, end) = self.extract_extglob_inner(&chars, i + 2);
                        regex.push_str(".*"); // Match anything, we filter later
                        i = end + 1;
                        continue;
                    }
                    _ => {}
                }
            }

            // Handle regular glob characters
            match c {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                '.' => regex.push_str("\\."),
                '[' => {
                    regex.push('[');
                    i += 1;
                    while i < chars.len() && chars[i] != ']' {
                        if chars[i] == '!' && regex.ends_with('[') {
                            regex.push('^');
                        } else {
                            regex.push(chars[i]);
                        }
                        i += 1;
                    }
                    regex.push(']');
                }
                '^' | '$' | '(' | ')' | '{' | '}' | '|' | '\\' => {
                    regex.push('\\');
                    regex.push(c);
                }
                _ => regex.push(c),
            }
            i += 1;
        }

        regex.push('$');
        regex
    }

    /// Extract the inner part of an extglob pattern (until closing paren)
    fn extract_extglob_inner(&self, chars: &[char], start: usize) -> (String, usize) {
        let mut inner = String::new();
        let mut depth = 1;
        let mut i = start;

        while i < chars.len() && depth > 0 {
            if chars[i] == '(' {
                depth += 1;
            } else if chars[i] == ')' {
                depth -= 1;
                if depth == 0 {
                    return (inner, i);
                }
            }
            inner.push(chars[i]);
            i += 1;
        }

        (inner, i)
    }

    /// Convert the inner part of extglob (handles | for alternation)
    fn extglob_inner_to_regex(&self, inner: &str) -> String {
        // Split by | and convert each alternative
        let alternatives: Vec<String> = inner
            .split('|')
            .map(|alt| {
                let mut result = String::new();
                for c in alt.chars() {
                    match c {
                        '*' => result.push_str(".*"),
                        '?' => result.push('.'),
                        '.' => result.push_str("\\."),
                        '^' | '$' | '(' | ')' | '{' | '}' | '\\' => {
                            result.push('\\');
                            result.push(c);
                        }
                        _ => result.push(c),
                    }
                }
                result
            })
            .collect();

        alternatives.join("|")
    }

    /// Expand extended glob pattern
    fn expand_extglob(&self, pattern: &str) -> Vec<String> {
        // Determine directory to search
        let (search_dir, file_pattern) = if let Some(last_slash) = pattern.rfind('/') {
            (&pattern[..last_slash], &pattern[last_slash + 1..])
        } else {
            (".", pattern)
        };

        // Check for !(pattern) - negative matching
        if let Some((neg_pat, suffix)) = self.extract_neg_extglob(file_pattern) {
            return self.expand_neg_extglob(search_dir, &neg_pat, &suffix, pattern);
        }

        // Convert file pattern to regex for positive extglob
        let regex_str = self.extglob_to_regex(file_pattern);

        let re = match regex::Regex::new(&regex_str) {
            Ok(r) => r,
            Err(_) => return vec![pattern.to_string()],
        };

        let mut results = Vec::new();

        if let Ok(entries) = std::fs::read_dir(search_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files unless pattern starts with .
                if name.starts_with('.') && !file_pattern.starts_with('.') {
                    continue;
                }

                if re.is_match(&name) {
                    let full_path = if search_dir == "." {
                        name
                    } else {
                        format!("{}/{}", search_dir, name)
                    };
                    results.push(full_path);
                }
            }
        }

        if results.is_empty() {
            vec![pattern.to_string()]
        } else {
            results.sort();
            results
        }
    }

    /// Handle !(pattern) negative extglob expansion
    fn expand_neg_extglob(
        &self,
        search_dir: &str,
        neg_pat: &str,
        suffix: &str,
        original_pattern: &str,
    ) -> Vec<String> {
        let mut results = Vec::new();

        if let Ok(entries) = std::fs::read_dir(search_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files
                if name.starts_with('.') {
                    continue;
                }

                // File must end with suffix
                if !name.ends_with(suffix) {
                    continue;
                }

                let basename = &name[..name.len() - suffix.len()];
                // Check if basename matches any negated alternative
                let alts: Vec<&str> = neg_pat.split('|').collect();
                let matches_neg = alts.iter().any(|alt| {
                    if alt.contains('*') || alt.contains('?') {
                        let alt_re = self.extglob_inner_to_regex(alt);
                        if let Ok(r) = regex::Regex::new(&format!("^{}$", alt_re)) {
                            r.is_match(basename)
                        } else {
                            *alt == basename
                        }
                    } else {
                        *alt == basename
                    }
                });

                if !matches_neg {
                    let full_path = if search_dir == "." {
                        name
                    } else {
                        format!("{}/{}", search_dir, name)
                    };
                    results.push(full_path);
                }
            }
        }

        if results.is_empty() {
            vec![original_pattern.to_string()]
        } else {
            results.sort();
            results
        }
    }

    /// Extract !(pattern) info from file pattern, returns (inner_pattern, suffix)
    fn extract_neg_extglob(&self, pattern: &str) -> Option<(String, String)> {
        let chars: Vec<char> = pattern.chars().collect();
        if chars.len() >= 3 && chars[0] == '!' && chars[1] == '(' {
            let mut depth = 1;
            let mut i = 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '(' {
                    depth += 1;
                } else if chars[i] == ')' {
                    depth -= 1;
                }
                i += 1;
            }
            if depth == 0 {
                let inner: String = chars[2..i - 1].iter().collect();
                let suffix: String = chars[i..].iter().collect();
                return Some((inner, suffix));
            }
        }
        None
    }

    /// Expand a word with word splitting (for contexts like `for x in $words`)
    fn expand_word_split(&mut self, word: &ShellWord) -> Vec<String> {
        match word {
            ShellWord::Literal(s) => {
                // Check if this is an array expansion ${arr[@]}
                self.expand_string_split(s)
            }
            ShellWord::SingleQuoted(s) => vec![s.clone()],
            ShellWord::DoubleQuoted(parts) => {
                // Double quotes prevent word splitting
                vec![parts.iter().map(|p| self.expand_word(p)).collect()]
            }
            ShellWord::Variable(name) => {
                let val = env::var(name).unwrap_or_default();
                self.split_words(&val)
            }
            ShellWord::VariableBraced(name, modifier) => {
                let val = env::var(name).ok();
                let expanded = self.apply_var_modifier(val, modifier.as_deref());
                self.split_words(&expanded)
            }
            ShellWord::ArrayVar(name, index) => {
                let idx_str = self.expand_word(index);
                if idx_str == "@" || idx_str == "*" {
                    // ${arr[@]} returns each element as separate word
                    self.arrays.get(name).cloned().unwrap_or_default()
                } else {
                    vec![self.expand_array_access(name, index)]
                }
            }
            ShellWord::Glob(pattern) => match glob::glob(pattern) {
                Ok(paths) => {
                    let expanded: Vec<String> = paths
                        .filter_map(|p| p.ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    if expanded.is_empty() {
                        vec![pattern.clone()]
                    } else {
                        expanded
                    }
                }
                Err(_) => vec![pattern.clone()],
            },
            _ => vec![self.expand_word(word)],
        }
    }

    /// Expand string with word splitting - returns Vec for array expansions
    fn expand_string_split(&mut self, s: &str) -> Vec<String> {
        let mut results: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut brace_content = String::new();
                    let mut depth = 1;
                    while let Some(ch) = chars.next() {
                        if ch == '{' {
                            depth += 1;
                            brace_content.push(ch);
                        } else if ch == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            brace_content.push(ch);
                        } else {
                            brace_content.push(ch);
                        }
                    }

                    // Check if this is an array expansion ${arr[@]} or ${arr[*]}
                    if let Some(bracket_start) = brace_content.find('[') {
                        let var_name = &brace_content[..bracket_start];
                        let bracket_content = &brace_content[bracket_start + 1..];
                        if let Some(bracket_end) = bracket_content.find(']') {
                            let index = &bracket_content[..bracket_end];
                            if (index == "@" || index == "*")
                                && bracket_end + 1 == bracket_content.len()
                            {
                                // This is ${arr[@]} - expand to separate elements
                                if !current.is_empty() {
                                    results.push(current.clone());
                                    current.clear();
                                }
                                if let Some(arr) = self.arrays.get(var_name) {
                                    results.extend(arr.clone());
                                }
                                continue;
                            }
                        }
                    }

                    // Not an array expansion, use normal expansion
                    current.push_str(&self.expand_braced_variable(&brace_content));
                } else {
                    // Simple variable like $var
                    let mut var_name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            var_name.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    let val = self.get_variable(&var_name);
                    // Split this variable's value
                    if !current.is_empty() {
                        results.push(current.clone());
                        current.clear();
                    }
                    results.extend(self.split_words(&val));
                }
            } else {
                current.push(c);
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        if results.is_empty() {
            results.push(String::new());
        }

        results
    }

    /// Split a string into words based on IFS
    fn split_words(&self, s: &str) -> Vec<String> {
        let ifs = self
            .variables
            .get("IFS")
            .cloned()
            .or_else(|| env::var("IFS").ok())
            .unwrap_or_else(|| " \t\n".to_string());

        if ifs.is_empty() {
            return vec![s.to_string()];
        }

        s.split(|c: char| ifs.contains(c))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn expand_word(&mut self, word: &ShellWord) -> String {
        match word {
            ShellWord::Literal(s) => {
                let expanded = self.expand_string(s);
                // Don't glob-expand here, that's done in expand_word_glob
                expanded
            }
            ShellWord::SingleQuoted(s) => s.clone(),
            ShellWord::DoubleQuoted(parts) => parts.iter().map(|p| self.expand_word(p)).collect(),
            ShellWord::Variable(name) => env::var(name).unwrap_or_default(),
            ShellWord::VariableBraced(name, modifier) => {
                let val = env::var(name).ok();
                self.apply_var_modifier(val, modifier.as_deref())
            }
            ShellWord::Tilde(user) => {
                if let Some(u) = user {
                    // ~user expansion (simplified)
                    format!("/home/{}", u)
                } else {
                    dirs::home_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "~".to_string())
                }
            }
            ShellWord::Glob(pattern) => {
                // Expand glob
                match glob::glob(pattern) {
                    Ok(paths) => {
                        let expanded: Vec<String> = paths
                            .filter_map(|p| p.ok())
                            .map(|p| p.to_string_lossy().to_string())
                            .collect();
                        if expanded.is_empty() {
                            pattern.clone()
                        } else {
                            expanded.join(" ")
                        }
                    }
                    Err(_) => pattern.clone(),
                }
            }
            ShellWord::Concat(parts) => parts.iter().map(|p| self.expand_word(p)).collect(),
            ShellWord::CommandSub(cmd) => self.execute_command_substitution(cmd),
            ShellWord::ProcessSubIn(cmd) => self.execute_process_sub_in(cmd),
            ShellWord::ProcessSubOut(cmd) => self.execute_process_sub_out(cmd),
            ShellWord::ArithSub(expr) => self.evaluate_arithmetic(expr),
            ShellWord::ArrayVar(name, index) => self.expand_array_access(name, index),
            ShellWord::ArrayLiteral(elements) => elements
                .iter()
                .map(|e| self.expand_word(e))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    fn expand_braced_variable(&mut self, content: &str) -> String {
        // Handle ${#arr[@]} - array length
        if content.starts_with('#') {
            let rest = &content[1..];
            if let Some(bracket_start) = rest.find('[') {
                let var_name = &rest[..bracket_start];
                let bracket_content = &rest[bracket_start + 1..];
                if let Some(bracket_end) = bracket_content.find(']') {
                    let index = &bracket_content[..bracket_end];
                    if index == "@" || index == "*" {
                        // ${#arr[@]} - return array length
                        return self
                            .arrays
                            .get(var_name)
                            .map(|arr| arr.len().to_string())
                            .unwrap_or_else(|| "0".to_string());
                    }
                }
            }
            // ${#var} - string length
            let val = self.get_variable(rest);
            return val.len().to_string();
        }

        // Handle ${arr[idx]}
        if let Some(bracket_start) = content.find('[') {
            let var_name = &content[..bracket_start];
            let bracket_content = &content[bracket_start + 1..];
            if let Some(bracket_end) = bracket_content.find(']') {
                let index = &bracket_content[..bracket_end];

                if index == "@" || index == "*" {
                    // ${arr[@]} - return all elements
                    return self
                        .arrays
                        .get(var_name)
                        .map(|arr| arr.join(" "))
                        .unwrap_or_default();
                } else if let Ok(idx) = index.parse::<i64>() {
                    // ${arr[1]} - return element at index (1-indexed in zsh)
                    return self
                        .arrays
                        .get(var_name)
                        .and_then(|arr| {
                            let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                            arr.get(actual_idx).cloned()
                        })
                        .unwrap_or_default();
                } else {
                    // Index is a variable, expand it first
                    let expanded_idx = self.get_variable(index);
                    if let Ok(idx) = expanded_idx.parse::<i64>() {
                        return self
                            .arrays
                            .get(var_name)
                            .and_then(|arr| {
                                let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                                arr.get(actual_idx).cloned()
                            })
                            .unwrap_or_default();
                    }
                }
            }
        }

        // Handle ${var:-default}, ${var:=default}, ${var:?error}, ${var:+alternate}
        if let Some(colon_pos) = content.find(':') {
            let var_name = &content[..colon_pos];
            let rest = &content[colon_pos + 1..];
            let val = self.get_variable(var_name);
            let val_opt = if val.is_empty() {
                None
            } else {
                Some(val.clone())
            };

            if rest.starts_with('-') {
                // ${var:-default}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => self.expand_string(&rest[1..]),
                };
            } else if rest.starts_with('=') {
                // ${var:=default}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        let default = self.expand_string(&rest[1..]);
                        self.variables.insert(var_name.to_string(), default.clone());
                        default
                    }
                };
            } else if rest.starts_with('?') {
                // ${var:?error}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        let msg = self.expand_string(&rest[1..]);
                        eprintln!("zshrs: {}: {}", var_name, msg);
                        String::new()
                    }
                };
            } else if rest.starts_with('+') {
                // ${var:+alternate}
                return match val_opt {
                    Some(v) if !v.is_empty() => self.expand_string(&rest[1..]),
                    _ => String::new(),
                };
            } else if rest
                .chars()
                .next()
                .map(|c| c.is_ascii_digit() || c == '-')
                .unwrap_or(false)
            {
                // ${var:offset} or ${var:offset:length}
                let parts: Vec<&str> = rest.splitn(2, ':').collect();
                let offset: i64 = parts[0].parse().unwrap_or(0);
                let length: Option<usize> = parts.get(1).and_then(|s| s.parse().ok());

                let start = if offset < 0 {
                    (val.len() as i64 + offset).max(0) as usize
                } else {
                    (offset as usize).min(val.len())
                };

                return if let Some(len) = length {
                    val.chars().skip(start).take(len).collect()
                } else {
                    val.chars().skip(start).collect()
                };
            }
        }

        // Handle ${var/pattern/replacement} and ${var//pattern/replacement}
        // Only if the part before / is a valid variable name
        if let Some(slash_pos) = content.find('/') {
            let var_name = &content[..slash_pos];
            // Variable names must start with letter/underscore and contain only alnum/_
            if !var_name.is_empty()
                && var_name
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic() || c == '_')
                    .unwrap_or(false)
                && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                let rest = &content[slash_pos + 1..];
                let val = self.get_variable(var_name);

                let replace_all = rest.starts_with('/');
                let rest = if replace_all { &rest[1..] } else { rest };

                let parts: Vec<&str> = rest.splitn(2, '/').collect();
                let pattern = parts.get(0).unwrap_or(&"");
                let replacement = parts.get(1).unwrap_or(&"");

                return if replace_all {
                    val.replace(pattern, replacement)
                } else {
                    val.replacen(pattern, replacement, 1)
                };
            }
        }

        // Handle ${var#pattern} and ${var##pattern} - remove prefix
        // But only if the # is not at the start (which would be length)
        if let Some(hash_pos) = content.find('#') {
            if hash_pos > 0 {
                let var_name = &content[..hash_pos];
                // Make sure var_name looks like a valid variable name
                if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let rest = &content[hash_pos + 1..];
                    let val = self.get_variable(var_name);

                    let long = rest.starts_with('#');
                    let pattern = if long { &rest[1..] } else { rest };

                    // Convert shell glob pattern to regex-style for matching prefixes
                    // Escape special regex chars, then convert glob wildcards
                    let pattern_regex = regex::escape(pattern)
                        .replace(r"\*", ".*")
                        .replace(r"\?", ".");

                    if let Ok(re) = regex::Regex::new(&format!("^{}", pattern_regex)) {
                        if long {
                            // Remove longest prefix match - find all matches and use the longest
                            let mut longest_end = 0;
                            for m in re.find_iter(&val) {
                                if m.end() > longest_end {
                                    longest_end = m.end();
                                }
                            }
                            if longest_end > 0 {
                                return val[longest_end..].to_string();
                            }
                        } else {
                            // Remove shortest prefix match
                            if let Some(m) = re.find(&val) {
                                return val[m.end()..].to_string();
                            }
                        }
                    }
                    return val;
                }
            }
        }

        // Handle ${var%pattern} and ${var%%pattern} - remove suffix
        if let Some(pct_pos) = content.find('%') {
            if pct_pos > 0 {
                let var_name = &content[..pct_pos];
                if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let rest = &content[pct_pos + 1..];
                    let val = self.get_variable(var_name);

                    let long = rest.starts_with('%');
                    let pattern = if long { &rest[1..] } else { rest };

                    // Convert shell glob pattern to regex-style for matching suffixes
                    // Escape special regex chars, then convert glob wildcards
                    let pattern_regex = regex::escape(pattern)
                        .replace(r"\*", ".*")
                        .replace(r"\?", ".");

                    if let Ok(re) = regex::Regex::new(&format!("{}$", pattern_regex)) {
                        if long {
                            // Remove longest suffix match - find earliest start
                            let mut earliest_start = val.len();
                            for m in re.find_iter(&val) {
                                if m.start() < earliest_start {
                                    earliest_start = m.start();
                                }
                            }
                            if earliest_start < val.len() {
                                return val[..earliest_start].to_string();
                            }
                        } else {
                            // Remove shortest suffix match
                            if let Some(m) = re.find(&val) {
                                return val[..m.start()].to_string();
                            }
                        }
                    }
                    return val;
                }
            }
        }

        // Handle ${var^} and ${var^^} - uppercase
        if let Some(caret_pos) = content.find('^') {
            let var_name = &content[..caret_pos];
            let val = self.get_variable(var_name);
            let all = content[caret_pos + 1..].starts_with('^');

            return if all {
                val.to_uppercase()
            } else {
                let mut chars = val.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            };
        }

        // Handle ${var,} and ${var,,} - lowercase
        if let Some(comma_pos) = content.find(',') {
            let var_name = &content[..comma_pos];
            let val = self.get_variable(var_name);
            let all = content[comma_pos + 1..].starts_with(',');

            return if all {
                val.to_lowercase()
            } else {
                let mut chars = val.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            };
        }

        // Handle ${!prefix*} and ${!prefix@} - expand to variable names with prefix
        if content.starts_with('!') {
            let rest = &content[1..];
            if rest.ends_with('*') || rest.ends_with('@') {
                let prefix = &rest[..rest.len() - 1];
                let mut matches: Vec<String> = self
                    .variables
                    .keys()
                    .filter(|k| k.starts_with(prefix))
                    .cloned()
                    .collect();
                // Also check arrays
                for k in self.arrays.keys() {
                    if k.starts_with(prefix) && !matches.contains(k) {
                        matches.push(k.clone());
                    }
                }
                matches.sort();
                return matches.join(" ");
            }

            // ${!var} - indirect expansion
            let var_name = self.get_variable(rest);
            return self.get_variable(&var_name);
        }

        // Default: just get the variable
        self.get_variable(content)
    }

    fn expand_array_access(&mut self, name: &str, index: &ShellWord) -> String {
        let idx_str = self.expand_word(index);

        if idx_str == "@" || idx_str == "*" {
            // Return all elements
            if let Some(arr) = self.arrays.get(name) {
                arr.join(" ")
            } else {
                String::new()
            }
        } else if let Ok(idx) = idx_str.parse::<i64>() {
            // Return element at index (zsh is 1-indexed by default)
            if let Some(arr) = self.arrays.get(name) {
                let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                arr.get(actual_idx).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }

    fn expand_string(&mut self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                if chars.peek() == Some(&'(') {
                    chars.next(); // consume '('

                    // Check for $(( )) arithmetic
                    if chars.peek() == Some(&'(') {
                        chars.next(); // consume second '('
                        let expr = Self::collect_until_double_paren(&mut chars);
                        result.push_str(&self.evaluate_arithmetic(&expr));
                    } else {
                        // Command substitution $(...)
                        let cmd_str = Self::collect_until_paren(&mut chars);
                        result.push_str(&self.run_command_substitution(&cmd_str));
                    }
                } else if chars.peek() == Some(&'{') {
                    chars.next();
                    // Collect the full braced expression including brackets
                    let mut brace_content = String::new();
                    let mut depth = 1;
                    while let Some(c) = chars.next() {
                        if c == '{' {
                            depth += 1;
                            brace_content.push(c);
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            brace_content.push(c);
                        } else {
                            brace_content.push(c);
                        }
                    }
                    result.push_str(&self.expand_braced_variable(&brace_content));
                } else {
                    let mut var_name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_alphanumeric()
                            || c == '_'
                            || c == '@'
                            || c == '*'
                            || c == '#'
                            || c == '?'
                        {
                            var_name.push(chars.next().unwrap());
                            // Handle single-char special vars
                            if matches!(
                                var_name.as_str(),
                                "@" | "*"
                                    | "#"
                                    | "?"
                                    | "0"
                                    | "1"
                                    | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                            ) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    result.push_str(&self.get_variable(&var_name));
                }
            } else if c == '`' {
                // Backtick command substitution
                let cmd_str: String = chars.by_ref().take_while(|&c| c != '`').collect();
                result.push_str(&self.run_command_substitution(&cmd_str));
            } else if c == '<' && chars.peek() == Some(&'(') {
                // Process substitution <(cmd)
                chars.next(); // consume '('
                let cmd_str = Self::collect_until_paren(&mut chars);
                result.push_str(&self.run_process_sub_in(&cmd_str));
            } else if c == '>' && chars.peek() == Some(&'(') {
                // Process substitution >(cmd)
                chars.next(); // consume '('
                let cmd_str = Self::collect_until_paren(&mut chars);
                result.push_str(&self.run_process_sub_out(&cmd_str));
            } else if c == '~' && result.is_empty() {
                if let Some(home) = dirs::home_dir() {
                    result.push_str(&home.to_string_lossy());
                } else {
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn collect_until_paren(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut depth = 1;

        while let Some(c) = chars.next() {
            if c == '(' {
                depth += 1;
                result.push(c);
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                result.push(c);
            } else {
                result.push(c);
            }
        }

        result
    }

    fn collect_until_double_paren(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut depth = 1;

        while let Some(c) = chars.next() {
            if c == '(' {
                if chars.peek() == Some(&'(') {
                    chars.next();
                    depth += 1;
                    result.push_str("((");
                } else {
                    result.push(c);
                }
            } else if c == ')' {
                if chars.peek() == Some(&')') {
                    chars.next();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    result.push_str("))");
                } else {
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn run_process_sub_in(&mut self, cmd_str: &str) -> String {
        use std::fs;
        use std::process::Stdio;

        // Parse the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        // Create a unique FIFO in temp directory
        let fifo_path = format!("/tmp/zshrs_psub_{}", std::process::id());
        let fifo_counter = self.process_sub_counter;
        self.process_sub_counter += 1;
        let fifo_path = format!("{}_{}", fifo_path, fifo_counter);

        // Remove if exists, then create FIFO
        let _ = fs::remove_file(&fifo_path);
        if let Err(_) = nix::unistd::mkfifo(fifo_path.as_str(), nix::sys::stat::Mode::S_IRWXU) {
            return String::new();
        }

        // Spawn command that writes to the FIFO
        let fifo_clone = fifo_path.clone();
        if let Some(cmd) = commands.first() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if !words.is_empty() {
                    let cmd_name = words[0].clone();
                    let args: Vec<String> = words[1..].to_vec();

                    std::thread::spawn(move || {
                        // Open FIFO for writing (will block until reader connects)
                        if let Ok(fifo) = fs::OpenOptions::new().write(true).open(&fifo_clone) {
                            let _ = Command::new(&cmd_name)
                                .args(&args)
                                .stdout(fifo)
                                .stderr(Stdio::inherit())
                                .status();
                        }
                        // Clean up FIFO after command completes
                        let _ = fs::remove_file(&fifo_clone);
                    });
                }
            }
        }

        fifo_path
    }

    fn run_process_sub_out(&mut self, cmd_str: &str) -> String {
        use std::fs;
        use std::process::Stdio;

        // Parse the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        // Create a unique FIFO in temp directory
        let fifo_path = format!("/tmp/zshrs_psub_{}", std::process::id());
        let fifo_counter = self.process_sub_counter;
        self.process_sub_counter += 1;
        let fifo_path = format!("{}_{}", fifo_path, fifo_counter);

        // Remove if exists, then create FIFO
        let _ = fs::remove_file(&fifo_path);
        if let Err(_) = nix::unistd::mkfifo(fifo_path.as_str(), nix::sys::stat::Mode::S_IRWXU) {
            return String::new();
        }

        // Spawn command that reads from the FIFO
        let fifo_clone = fifo_path.clone();
        if let Some(cmd) = commands.first() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if !words.is_empty() {
                    let cmd_name = words[0].clone();
                    let args: Vec<String> = words[1..].to_vec();

                    std::thread::spawn(move || {
                        // Open FIFO for reading (will block until writer connects)
                        if let Ok(fifo) = fs::File::open(&fifo_clone) {
                            let _ = Command::new(&cmd_name)
                                .args(&args)
                                .stdin(fifo)
                                .stdout(Stdio::inherit())
                                .stderr(Stdio::inherit())
                                .status();
                        }
                        // Clean up FIFO after command completes
                        let _ = fs::remove_file(&fifo_clone);
                    });
                }
            }
        }

        fifo_path
    }

    fn run_command_substitution(&mut self, cmd_str: &str) -> String {
        use std::process::Stdio;

        // Parse and execute the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        if commands.is_empty() {
            return String::new();
        }

        // For simple external commands, capture output directly
        if let ShellCommand::Simple(simple) = &commands[0] {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            if words.is_empty() {
                return String::new();
            }

            let cmd_name = &words[0];
            let args = &words[1..];

            // Handle echo specially
            if cmd_name == "echo" {
                return args.join(" ");
            }

            if cmd_name == "pwd" {
                return env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
            }

            // External command
            let output = Command::new(cmd_name)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output();

            match output {
                Ok(output) => String::from_utf8_lossy(&output.stdout)
                    .trim_end_matches('\n')
                    .to_string(),
                Err(_) => String::new(),
            }
        } else {
            String::new()
        }
    }

    /// Process substitution <(cmd) - returns FIFO path
    fn execute_process_sub_in(&mut self, cmd: &ShellCommand) -> String {
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_str = words.join(" ");
            self.run_process_sub_in(&cmd_str)
        } else {
            String::new()
        }
    }

    /// Process substitution >(cmd) - returns FIFO path
    fn execute_process_sub_out(&mut self, cmd: &ShellCommand) -> String {
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_str = words.join(" ");
            self.run_process_sub_out(&cmd_str)
        } else {
            String::new()
        }
    }

    fn get_variable(&self, name: &str) -> String {
        // Handle special parameters
        match name {
            "@" | "*" => self.positional_params.join(" "),
            "#" => self.positional_params.len().to_string(),
            "?" => self.last_status.to_string(),
            "0" => env::args().next().unwrap_or_default(),
            n if n.chars().all(|c| c.is_ascii_digit()) => {
                let idx: usize = n.parse().unwrap_or(0);
                if idx == 0 {
                    env::args().next().unwrap_or_default()
                } else {
                    self.positional_params
                        .get(idx - 1)
                        .cloned()
                        .unwrap_or_default()
                }
            }
            _ => {
                // Check local variables first, then env
                self.variables
                    .get(name)
                    .cloned()
                    .or_else(|| env::var(name).ok())
                    .unwrap_or_default()
            }
        }
    }

    fn apply_var_modifier(
        &mut self,
        val: Option<String>,
        modifier: Option<&VarModifier>,
    ) -> String {
        match modifier {
            None => val.unwrap_or_default(),

            // ${var:-word} - use default value
            Some(VarModifier::Default(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => self.expand_word(word),
            },

            // ${var:=word} - assign default value
            Some(VarModifier::DefaultAssign(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => self.expand_word(word),
            },

            // ${var:?word} - error if null or unset
            Some(VarModifier::Error(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => {
                    let msg = self.expand_word(word);
                    eprintln!("zshrs: {}", msg);
                    String::new()
                }
            },

            // ${var:+word} - use alternate value
            Some(VarModifier::Alternate(word)) => match &val {
                Some(v) if !v.is_empty() => self.expand_word(word),
                _ => String::new(),
            },

            // ${#var} - string length
            Some(VarModifier::Length) => val
                .map(|v| v.len().to_string())
                .unwrap_or_else(|| "0".to_string()),

            // ${var:offset} or ${var:offset:length} - substring
            Some(VarModifier::Substring(offset, length)) => {
                let v = val.unwrap_or_default();
                let start = if *offset < 0 {
                    (v.len() as i64 + offset).max(0) as usize
                } else {
                    (*offset as usize).min(v.len())
                };

                if let Some(len) = length {
                    let len = (*len as usize).min(v.len().saturating_sub(start));
                    v.chars().skip(start).take(len).collect()
                } else {
                    v.chars().skip(start).collect()
                }
            }

            // ${var#pattern} - remove shortest prefix
            Some(VarModifier::RemovePrefix(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                if v.starts_with(&pat) {
                    v[pat.len()..].to_string()
                } else {
                    v
                }
            }

            // ${var##pattern} - remove longest prefix
            Some(VarModifier::RemovePrefixLong(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                // For glob patterns, find longest match from start
                if let Ok(glob) = glob::Pattern::new(&pat) {
                    for i in (0..=v.len()).rev() {
                        if glob.matches(&v[..i]) {
                            return v[i..].to_string();
                        }
                    }
                }
                v
            }

            // ${var%pattern} - remove shortest suffix
            Some(VarModifier::RemoveSuffix(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                if v.ends_with(&pat) {
                    v[..v.len() - pat.len()].to_string()
                } else {
                    v
                }
            }

            // ${var%%pattern} - remove longest suffix
            Some(VarModifier::RemoveSuffixLong(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                // For glob patterns, find longest match from end
                if let Ok(glob) = glob::Pattern::new(&pat) {
                    for i in 0..=v.len() {
                        if glob.matches(&v[i..]) {
                            return v[..i].to_string();
                        }
                    }
                }
                v
            }

            // ${var/pattern/replacement} - replace first match
            Some(VarModifier::Replace(pattern, replacement)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                let repl = self.expand_word(replacement);
                v.replacen(&pat, &repl, 1)
            }

            // ${var//pattern/replacement} - replace all matches
            Some(VarModifier::ReplaceAll(pattern, replacement)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                let repl = self.expand_word(replacement);
                v.replace(&pat, &repl)
            }

            // ${var^} or ${var^^} - uppercase
            Some(VarModifier::Upper) => val.map(|v| v.to_uppercase()).unwrap_or_default(),

            // ${var,} or ${var,,} - lowercase
            Some(VarModifier::Lower) => val.map(|v| v.to_lowercase()).unwrap_or_default(),

            // Array-related modifiers are handled elsewhere
            Some(VarModifier::ArrayLength)
            | Some(VarModifier::ArrayIndex(_))
            | Some(VarModifier::ArrayAll) => val.unwrap_or_default(),
        }
    }

    /// Execute a command and capture its output (command substitution)
    fn execute_command_substitution(&mut self, cmd: &ShellCommand) -> String {
        match self.execute_command_capture(cmd) {
            Ok(output) => output.trim_end_matches('\n').to_string(),
            Err(_) => String::new(),
        }
    }

    /// Execute a command and capture its stdout
    fn execute_command_capture(&mut self, cmd: &ShellCommand) -> Result<String, String> {
        // For simple commands, we can use Command directly
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            if words.is_empty() {
                return Ok(String::new());
            }

            let cmd_name = &words[0];
            let args = &words[1..];

            // Handle some builtins that can return values
            match cmd_name.as_str() {
                "echo" => {
                    let output = args.join(" ");
                    return Ok(format!("{}\n", output));
                }
                "printf" => {
                    if !args.is_empty() {
                        // Simple printf - just format string with args
                        let format = &args[0];
                        let result = if args.len() > 1 {
                            // Very basic: just handle %s
                            let mut out = format.clone();
                            for (i, arg) in args[1..].iter().enumerate() {
                                out = out.replacen("%s", arg, 1);
                                out = out.replacen(&format!("${}", i + 1), arg, 1);
                            }
                            out
                        } else {
                            format.clone()
                        };
                        return Ok(result);
                    }
                    return Ok(String::new());
                }
                "pwd" => {
                    return Ok(env::current_dir()
                        .map(|p| format!("{}\n", p.display()))
                        .unwrap_or_default());
                }
                _ => {}
            }

            // External command - capture its output
            let output = Command::new(cmd_name)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output();

            match output {
                Ok(output) => {
                    self.last_status = output.status.code().unwrap_or(1);
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                }
                Err(e) => {
                    self.last_status = 127;
                    Err(format!("{}: {}", cmd_name, e))
                }
            }
        } else if let ShellCommand::Pipeline(cmds, _negated) = cmd {
            // For pipelines, execute and capture output of the last command
            // This is simplified - proper implementation would pipe between all
            if let Some(last) = cmds.last() {
                return self.execute_command_capture(last);
            }
            Ok(String::new())
        } else {
            // For compound commands, execute them and return empty
            // (complex case - could be expanded later)
            let _ = self.execute_command(cmd);
            Ok(String::new())
        }
    }

    /// Evaluate arithmetic expression
    fn evaluate_arithmetic(&mut self, expr: &str) -> String {
        // Simple arithmetic evaluator
        let expr = self.expand_string(expr);

        // Handle simple cases
        let result = self.eval_arith_expr(&expr);
        result.to_string()
    }

    fn eval_arith_expr(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Handle assignment: var=expr (must check before other operators)
        // But not ==, !=, <=, >=
        if let Some(eq_pos) = expr.find('=') {
            let chars: Vec<char> = expr.chars().collect();
            let is_comparison = (eq_pos > 0
                && (chars[eq_pos - 1] == '!'
                    || chars[eq_pos - 1] == '<'
                    || chars[eq_pos - 1] == '>'
                    || chars[eq_pos - 1] == '='))
                || chars.get(eq_pos + 1) == Some(&'=');

            if !is_comparison {
                let var_part = &expr[..eq_pos];
                // Check if the left side is a simple variable name (no operators)
                if !var_part.contains(|c: char| "+-*/%<>!&|()".contains(c)) {
                    let var_name = var_part.trim();
                    if !var_name.is_empty()
                        && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        let value_expr = &expr[eq_pos + 1..];
                        let value = self.eval_arith_expr(value_expr);
                        self.variables
                            .insert(var_name.to_string(), value.to_string());
                        env::set_var(var_name, value.to_string());
                        return value;
                    }
                }
            }
        }

        // Handle post-increment/decrement: var++ var--
        if expr.ends_with("++") {
            let var_name = expr[..expr.len() - 2].trim();
            let current = self.get_variable(var_name).parse::<i64>().unwrap_or(0);
            let new_val = current + 1;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return current; // Post-increment returns old value
        }
        if expr.ends_with("--") {
            let var_name = expr[..expr.len() - 2].trim();
            let current = self.get_variable(var_name).parse::<i64>().unwrap_or(0);
            let new_val = current - 1;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return current; // Post-decrement returns old value
        }

        // Handle pre-increment/decrement: ++var --var
        if expr.starts_with("++") {
            let var_name = expr[2..].trim();
            let current = self.get_variable(var_name).parse::<i64>().unwrap_or(0);
            let new_val = current + 1;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return new_val; // Pre-increment returns new value
        }
        if expr.starts_with("--") {
            let var_name = expr[2..].trim();
            let current = self.get_variable(var_name).parse::<i64>().unwrap_or(0);
            let new_val = current - 1;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return new_val; // Pre-decrement returns new value
        }

        // Handle parentheses
        if expr.starts_with('(') && expr.ends_with(')') {
            return self.eval_arith_expr(&expr[1..expr.len() - 1]);
        }

        // Handle binary operators (lowest to highest precedence)
        let mut depth = 0;
        let chars: Vec<char> = expr.chars().collect();

        // First, scan for comparison operators (<, >, <=, >=, ==, !=)
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '<' if depth == 0 => {
                    if chars.get(i + 1) == Some(&'=') {
                        // <=
                        let left = &expr[..i];
                        let right = &expr[i + 2..];
                        let l = self.eval_arith_expr(left);
                        let r = self.eval_arith_expr(right);
                        return if l <= r { 1 } else { 0 };
                    } else if chars.get(i + 1) != Some(&'<') {
                        // < (not <<)
                        let left = &expr[..i];
                        let right = &expr[i + 1..];
                        let l = self.eval_arith_expr(left);
                        let r = self.eval_arith_expr(right);
                        return if l < r { 1 } else { 0 };
                    }
                }
                '>' if depth == 0 => {
                    if chars.get(i + 1) == Some(&'=') {
                        // >=
                        let left = &expr[..i];
                        let right = &expr[i + 2..];
                        let l = self.eval_arith_expr(left);
                        let r = self.eval_arith_expr(right);
                        return if l >= r { 1 } else { 0 };
                    } else if chars.get(i + 1) != Some(&'>') {
                        // > (not >>)
                        let left = &expr[..i];
                        let right = &expr[i + 1..];
                        let l = self.eval_arith_expr(left);
                        let r = self.eval_arith_expr(right);
                        return if l > r { 1 } else { 0 };
                    }
                }
                '=' if depth == 0 && chars.get(i + 1) == Some(&'=') => {
                    // ==
                    let left = &expr[..i];
                    let right = &expr[i + 2..];
                    let l = self.eval_arith_expr(left);
                    let r = self.eval_arith_expr(right);
                    return if l == r { 1 } else { 0 };
                }
                '!' if depth == 0 && chars.get(i + 1) == Some(&'=') => {
                    // !=
                    let left = &expr[..i];
                    let right = &expr[i + 2..];
                    let l = self.eval_arith_expr(left);
                    let r = self.eval_arith_expr(right);
                    return if l != r { 1 } else { 0 };
                }
                _ => {}
            }
        }

        depth = 0;
        // Look for + and - (lowest arithmetic precedence, left-to-right)

        // Scan right-to-left for + and - (to get left-associativity)
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    let left = &expr[..i];
                    let right = &expr[i + 1..];
                    let l = self.eval_arith_expr(left);
                    let r = self.eval_arith_expr(right);
                    return if chars[i] == '+' { l + r } else { l - r };
                }
                _ => {}
            }
        }

        // Scan for ** first (needs to be before * check)
        // Find rightmost ** that's not inside parentheses
        depth = 0;
        let mut last_exp_pos = None;
        for i in 0..chars.len().saturating_sub(1) {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 && chars.get(i + 1) == Some(&'*') => {
                    last_exp_pos = Some(i);
                }
                _ => {}
            }
        }
        if let Some(pos) = last_exp_pos {
            let left = &expr[..pos];
            let right = &expr[pos + 2..];
            let l = self.eval_arith_expr(left);
            let r = self.eval_arith_expr(right);
            return l.pow(r as u32);
        }

        // Scan for * and / (but not **)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0
                    && chars.get(i + 1) != Some(&'*')
                    && (i == 0 || chars[i - 1] != '*') =>
                {
                    let left = &expr[..i];
                    let right = &expr[i + 1..];
                    let l = self.eval_arith_expr(left);
                    let r = self.eval_arith_expr(right);
                    return l * r;
                }
                '/' | '%' if depth == 0 => {
                    let left = &expr[..i];
                    let right = &expr[i + 1..];
                    let l = self.eval_arith_expr(left);
                    let r = self.eval_arith_expr(right);
                    return match chars[i] {
                        '/' => {
                            if r != 0 {
                                l / r
                            } else {
                                0
                            }
                        }
                        '%' => {
                            if r != 0 {
                                l % r
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                }
                _ => {}
            }
        }

        // Try to parse as number
        if let Ok(n) = expr.parse::<i64>() {
            return n;
        }

        // Try as variable
        self.get_variable(expr).parse().unwrap_or(0)
    }

    fn matches_pattern(&self, value: &str, pattern: &str) -> bool {
        // Simple glob matching
        if pattern == "*" {
            return true;
        }
        if pattern.contains('*') || pattern.contains('?') {
            // Use glob matching
            glob::Pattern::new(pattern)
                .map(|p| p.matches(value))
                .unwrap_or(false)
        } else {
            value == pattern
        }
    }

    fn eval_cond_expr(&mut self, expr: &CondExpr) -> bool {
        match expr {
            CondExpr::FileExists(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileRegular(w) => std::path::Path::new(&self.expand_word(w)).is_file(),
            CondExpr::FileDirectory(w) => std::path::Path::new(&self.expand_word(w)).is_dir(),
            CondExpr::FileSymlink(w) => std::path::Path::new(&self.expand_word(w)).is_symlink(),
            CondExpr::FileReadable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileWritable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileExecutable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileNonEmpty(w) => std::fs::metadata(&self.expand_word(w))
                .map(|m| m.len() > 0)
                .unwrap_or(false),
            CondExpr::StringEmpty(w) => self.expand_word(w).is_empty(),
            CondExpr::StringNonEmpty(w) => !self.expand_word(w).is_empty(),
            CondExpr::StringEqual(a, b) => self.expand_word(a) == self.expand_word(b),
            CondExpr::StringNotEqual(a, b) => self.expand_word(a) != self.expand_word(b),
            CondExpr::StringMatch(a, b) => {
                let val = self.expand_word(a);
                let pattern = self.expand_word(b);
                regex::Regex::new(&pattern)
                    .map(|re| re.is_match(&val))
                    .unwrap_or(false)
            }
            CondExpr::StringLess(a, b) => self.expand_word(a) < self.expand_word(b),
            CondExpr::StringGreater(a, b) => self.expand_word(a) > self.expand_word(b),
            CondExpr::NumEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val == b_val
            }
            CondExpr::NumNotEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val != b_val
            }
            CondExpr::NumLess(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val < b_val
            }
            CondExpr::NumLessEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val <= b_val
            }
            CondExpr::NumGreater(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val > b_val
            }
            CondExpr::NumGreaterEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val >= b_val
            }
            CondExpr::Not(inner) => !self.eval_cond_expr(inner),
            CondExpr::And(a, b) => self.eval_cond_expr(a) && self.eval_cond_expr(b),
            CondExpr::Or(a, b) => self.eval_cond_expr(a) || self.eval_cond_expr(b),
        }
    }

    // Builtins

    fn builtin_cd(&mut self, args: &[String]) -> i32 {
        let path = args.first().map(|s| s.as_str()).unwrap_or("~");
        let path = if path == "~" || path == "" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else if path.starts_with("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(&path[2..])
        } else if path == "-" {
            if let Ok(oldpwd) = env::var("OLDPWD") {
                PathBuf::from(oldpwd)
            } else {
                eprintln!("cd: OLDPWD not set");
                return 1;
            }
        } else {
            PathBuf::from(path)
        };

        if let Ok(cwd) = env::current_dir() {
            env::set_var("OLDPWD", cwd);
        }

        match env::set_current_dir(&path) {
            Ok(_) => {
                if let Ok(cwd) = env::current_dir() {
                    env::set_var("PWD", cwd);
                }
                0
            }
            Err(e) => {
                eprintln!("cd: {}: {}", path.display(), e);
                1
            }
        }
    }

    fn builtin_pwd(&mut self, _redirects: &[Redirect]) -> i32 {
        match env::current_dir() {
            Ok(path) => {
                println!("{}", path.display());
                0
            }
            Err(e) => {
                eprintln!("pwd: {}", e);
                1
            }
        }
    }

    fn builtin_echo(&mut self, args: &[String], _redirects: &[Redirect]) -> i32 {
        let mut newline = true;
        let mut interpret_escapes = false;
        let mut start = 0;

        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                "-n" => {
                    newline = false;
                    start = i + 1;
                }
                "-e" => {
                    interpret_escapes = true;
                    start = i + 1;
                }
                "-E" => {
                    interpret_escapes = false;
                    start = i + 1;
                }
                _ => break,
            }
        }

        let output = args[start..].join(" ");
        if interpret_escapes {
            print!("{}", output.replace("\\n", "\n").replace("\\t", "\t"));
        } else {
            print!("{}", output);
        }

        if newline {
            println!();
        }
        0
    }

    fn builtin_export(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                env::set_var(key, value);
            }
        }
        0
    }

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        for arg in args {
            env::remove_var(arg);
            self.variables.remove(arg);
        }
        0
    }

    fn builtin_source(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("source: filename argument required");
            return 1;
        }

        let path = &args[0];
        match std::fs::read_to_string(path) {
            Ok(content) => match self.execute_script(&content) {
                Ok(status) => status,
                Err(e) => {
                    eprintln!("source: {}: {}", path, e);
                    1
                }
            },
            Err(e) => {
                eprintln!("source: {}: {}", path, e);
                1
            }
        }
    }

    fn builtin_exit(&mut self, args: &[String]) -> i32 {
        let code = args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(self.last_status);
        std::process::exit(code);
    }

    fn builtin_return(&mut self, args: &[String]) -> i32 {
        args.first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(self.last_status)
    }

    fn builtin_test(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }

        // Simple test implementation
        let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        match args.as_slice() {
            ["-z", s] => {
                if s.is_empty() {
                    0
                } else {
                    1
                }
            }
            ["-n", s] => {
                if !s.is_empty() {
                    0
                } else {
                    1
                }
            }
            ["-e", path] => {
                if std::path::Path::new(path).exists() {
                    0
                } else {
                    1
                }
            }
            ["-f", path] => {
                if std::path::Path::new(path).is_file() {
                    0
                } else {
                    1
                }
            }
            ["-d", path] => {
                if std::path::Path::new(path).is_dir() {
                    0
                } else {
                    1
                }
            }
            ["-r", path] => {
                if std::path::Path::new(path).exists() {
                    0
                } else {
                    1
                }
            }
            ["-w", path] => {
                if std::path::Path::new(path).exists() {
                    0
                } else {
                    1
                }
            }
            ["-x", path] => {
                if std::path::Path::new(path).exists() {
                    0
                } else {
                    1
                }
            }
            [a, "=", b] | [a, "==", b] => {
                if a == b {
                    0
                } else {
                    1
                }
            }
            [a, "!=", b] => {
                if a != b {
                    0
                } else {
                    1
                }
            }
            [a, "-eq", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a == b {
                    0
                } else {
                    1
                }
            }
            [a, "-ne", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a != b {
                    0
                } else {
                    1
                }
            }
            [a, "-lt", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a < b {
                    0
                } else {
                    1
                }
            }
            [a, "-le", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a <= b {
                    0
                } else {
                    1
                }
            }
            [a, "-gt", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a > b {
                    0
                } else {
                    1
                }
            }
            [a, "-ge", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a >= b {
                    0
                } else {
                    1
                }
            }
            [s] => {
                if !s.is_empty() {
                    0
                } else {
                    1
                }
            }
            _ => 1,
        }
    }

    fn builtin_local(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                self.variables.insert(key.to_string(), value.to_string());
            } else {
                self.variables.insert(arg.clone(), String::new());
            }
        }
        0
    }

    fn builtin_declare(&mut self, args: &[String]) -> i32 {
        self.builtin_local(args)
    }

    fn builtin_read(&mut self, args: &[String]) -> i32 {
        let var = args.first().map(|s| s.as_str()).unwrap_or("REPLY");
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let input = input.trim_end();
                env::set_var(var, input);
                self.variables.insert(var.to_string(), input.to_string());
                0
            }
            Err(_) => 1,
        }
    }

    fn builtin_shift(&mut self, _args: &[String]) -> i32 {
        // Simplified: no-op for now
        0
    }

    fn builtin_eval(&mut self, args: &[String]) -> i32 {
        let code = args.join(" ");
        match self.execute_script(&code) {
            Ok(status) => status,
            Err(e) => {
                eprintln!("eval: {}", e);
                1
            }
        }
    }

    fn builtin_autoload(&mut self, args: &[String]) -> i32 {
        // Parse options: -U (no alias expansion), -z (zsh style), -X (execute immediately)
        let mut functions = Vec::new();
        let mut fpath_dirs = Vec::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') {
                // Skip options for now, just parse them
                match arg.as_str() {
                    "-U" | "-z" | "-k" => {}
                    "-X" | "-XU" | "-XUz" => {}
                    s if s.starts_with("-d") => {
                        // -d dir: add directory to search
                        if s.len() > 2 {
                            fpath_dirs.push(PathBuf::from(&s[2..]));
                        } else if i + 1 < args.len() {
                            i += 1;
                            fpath_dirs.push(PathBuf::from(&args[i]));
                        }
                    }
                    _ => {}
                }
            } else {
                functions.push(arg.clone());
            }
            i += 1;
        }

        // Add any specified fpath directories
        for dir in fpath_dirs {
            self.add_fpath(dir);
        }

        // Mark functions for autoload (stub functions that will load on first use)
        for func_name in &functions {
            // Create a stub function that will be replaced when called
            let stub = ShellCommand::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![ShellWord::Literal(":".to_string())],
                redirects: vec![],
            });

            // Try to actually load the function from ZWC
            if self.autoload_function(func_name).is_some() {
                // Already loaded
                continue;
            }

            // Otherwise register stub
            self.functions.insert(func_name.clone(), stub);
        }

        0
    }

    fn builtin_jobs(&mut self, _args: &[String]) -> i32 {
        // Reap finished jobs first
        for job in self.jobs.reap_finished() {
            println!("[{}]  Done                    {}", job.id, job.command);
        }

        // List remaining jobs
        for job in self.jobs.list() {
            let marker = if job.is_current { "+" } else { "-" };
            let state = match job.state {
                JobState::Running => "Running",
                JobState::Stopped => "Stopped",
                JobState::Done => "Done",
            };
            println!(
                "[{}]{} {}                    {}",
                job.id, marker, state, job.command
            );
        }
        0
    }

    fn builtin_fg(&mut self, args: &[String]) -> i32 {
        let job_id = if let Some(arg) = args.first() {
            // Parse %N or just N
            let s = arg.trim_start_matches('%');
            match s.parse::<usize>() {
                Ok(id) => Some(id),
                Err(_) => {
                    eprintln!("fg: {}: no such job", arg);
                    return 1;
                }
            }
        } else {
            self.jobs.current().map(|j| j.id)
        };

        let Some(id) = job_id else {
            eprintln!("fg: no current job");
            return 1;
        };

        let Some(job) = self.jobs.get(id) else {
            eprintln!("fg: %{}: no such job", id);
            return 1;
        };

        let pid = job.pid;
        let cmd = job.command.clone();
        println!("{}", cmd);

        // Continue the job
        if let Err(e) = continue_job(pid) {
            eprintln!("fg: {}", e);
            return 1;
        }

        // Wait for it
        match wait_for_job(pid) {
            Ok(status) => {
                self.jobs.remove(id);
                status
            }
            Err(e) => {
                eprintln!("fg: {}", e);
                1
            }
        }
    }

    fn builtin_bg(&mut self, args: &[String]) -> i32 {
        let job_id = if let Some(arg) = args.first() {
            let s = arg.trim_start_matches('%');
            match s.parse::<usize>() {
                Ok(id) => Some(id),
                Err(_) => {
                    eprintln!("bg: {}: no such job", arg);
                    return 1;
                }
            }
        } else {
            self.jobs.current().map(|j| j.id)
        };

        let Some(id) = job_id else {
            eprintln!("bg: no current job");
            return 1;
        };

        let Some(job) = self.jobs.get_mut(id) else {
            eprintln!("bg: %{}: no such job", id);
            return 1;
        };

        let pid = job.pid;
        let cmd = job.command.clone();

        if let Err(e) = continue_job(pid) {
            eprintln!("bg: {}", e);
            return 1;
        }

        job.state = JobState::Running;
        println!("[{}] {} &", id, cmd);
        0
    }

    fn builtin_kill(&mut self, args: &[String]) -> i32 {
        use crate::shell_jobs::send_signal;
        use nix::sys::signal::Signal;

        if args.is_empty() {
            eprintln!("kill: usage: kill [-signal] pid ...");
            return 1;
        }

        let mut sig = Signal::SIGTERM;
        let mut start = 0;

        // Check for signal specification
        if let Some(first) = args.first() {
            if first.starts_with('-') {
                let sig_str = first.trim_start_matches('-');
                sig = match sig_str.to_uppercase().as_str() {
                    "1" | "HUP" | "SIGHUP" => Signal::SIGHUP,
                    "2" | "INT" | "SIGINT" => Signal::SIGINT,
                    "3" | "QUIT" | "SIGQUIT" => Signal::SIGQUIT,
                    "9" | "KILL" | "SIGKILL" => Signal::SIGKILL,
                    "15" | "TERM" | "SIGTERM" => Signal::SIGTERM,
                    "18" | "CONT" | "SIGCONT" => Signal::SIGCONT,
                    "19" | "STOP" | "SIGSTOP" => Signal::SIGSTOP,
                    _ => {
                        eprintln!("kill: {}: invalid signal", first);
                        return 1;
                    }
                };
                start = 1;
            }
        }

        let mut status = 0;
        for arg in &args[start..] {
            // Handle %job syntax
            if arg.starts_with('%') {
                let id: usize = match arg[1..].parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("kill: {}: no such job", arg);
                        status = 1;
                        continue;
                    }
                };
                if let Some(job) = self.jobs.get(id) {
                    if let Err(e) = send_signal(job.pid, sig) {
                        eprintln!("kill: {}", e);
                        status = 1;
                    }
                } else {
                    eprintln!("kill: {}: no such job", arg);
                    status = 1;
                }
            } else {
                // Direct PID
                let pid: u32 = match arg.parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("kill: {}: invalid pid", arg);
                        status = 1;
                        continue;
                    }
                };
                if let Err(e) = send_signal(pid, sig) {
                    eprintln!("kill: {}", e);
                    status = 1;
                }
            }
        }
        status
    }

    fn builtin_disown(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Disown current job
            if let Some(job) = self.jobs.current() {
                let id = job.id;
                self.jobs.remove(id);
            }
            return 0;
        }

        for arg in args {
            let s = arg.trim_start_matches('%');
            if let Ok(id) = s.parse::<usize>() {
                self.jobs.remove(id);
            } else {
                eprintln!("disown: {}: no such job", arg);
            }
        }
        0
    }

    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Wait for all jobs
            let ids: Vec<usize> = self.jobs.list().iter().map(|j| j.id).collect();
            for id in ids {
                if let Some(mut job) = self.jobs.remove(id) {
                    if let Some(ref mut child) = job.child {
                        let _ = wait_for_child(child);
                    }
                }
            }
            return 0;
        }

        let mut status = 0;
        for arg in args {
            if arg.starts_with('%') {
                let id: usize = match arg[1..].parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("wait: {}: no such job", arg);
                        status = 127;
                        continue;
                    }
                };
                if let Some(mut job) = self.jobs.remove(id) {
                    if let Some(ref mut child) = job.child {
                        match wait_for_child(child) {
                            Ok(s) => status = s,
                            Err(e) => {
                                eprintln!("wait: {}", e);
                                status = 127;
                            }
                        }
                    }
                } else {
                    eprintln!("wait: {}: no such job", arg);
                    status = 127;
                }
            } else {
                let pid: u32 = match arg.parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("wait: {}: invalid pid", arg);
                        status = 127;
                        continue;
                    }
                };
                match wait_for_job(pid) {
                    Ok(s) => status = s,
                    Err(e) => {
                        eprintln!("wait: {}", e);
                        status = 127;
                    }
                }
            }
        }
        status
    }
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_echo() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("true").unwrap();
        assert_eq!(status, 0);
    }

    #[test]
    fn test_if_true() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("if true; then true; fi").unwrap();
        assert_eq!(status, 0);
    }

    #[test]
    fn test_if_false() {
        let mut exec = ShellExecutor::new();
        let status = exec
            .execute_script("if false; then true; else false; fi")
            .unwrap();
        assert_eq!(status, 1);
    }

    #[test]
    fn test_for_loop() {
        let mut exec = ShellExecutor::new();
        exec.execute_script("for i in a b c; do true; done")
            .unwrap();
        assert_eq!(exec.last_status, 0);
    }

    #[test]
    fn test_and_list() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("true && true").unwrap();
        assert_eq!(status, 0);

        let status = exec.execute_script("true && false").unwrap();
        assert_eq!(status, 1);
    }

    #[test]
    fn test_or_list() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("false || true").unwrap();
        assert_eq!(status, 0);
    }
}

impl ShellExecutor {
    fn builtin_history(&self, args: &[String]) -> i32 {
        let Some(ref engine) = self.history else {
            eprintln!("history: history engine not available");
            return 1;
        };

        // Parse options
        let mut count = 20usize;
        let mut show_all = false;
        let mut search_query = None;

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-c" | "--clear" => {
                    // Clear history - need mutable access
                    eprintln!("history: clear not supported in this mode");
                    return 1;
                }
                "-a" | "--all" => show_all = true,
                "-n" => {
                    if i + 1 < args.len() {
                        i += 1;
                        count = args[i].parse().unwrap_or(20);
                    }
                }
                s if s.starts_with('-') && s[1..].chars().all(|c| c.is_ascii_digit()) => {
                    count = s[1..].parse().unwrap_or(20);
                }
                s if s.chars().all(|c| c.is_ascii_digit()) => {
                    count = s.parse().unwrap_or(20);
                }
                s => {
                    search_query = Some(s.to_string());
                }
            }
            i += 1;
        }

        if show_all {
            count = 10000;
        }

        let entries = if let Some(ref q) = search_query {
            engine.search(q, count)
        } else {
            engine.recent(count)
        };

        match entries {
            Ok(entries) => {
                // Print in chronological order (reverse the results since recent() is newest-first)
                for entry in entries.into_iter().rev() {
                    println!("{:>6}  {}", entry.id, entry.command);
                }
                0
            }
            Err(e) => {
                eprintln!("history: {}", e);
                1
            }
        }
    }

    fn builtin_fc(&mut self, args: &[String]) -> i32 {
        let Some(ref engine) = self.history else {
            eprintln!("fc: history engine not available");
            return 1;
        };

        // fc -l: list
        // fc -e -: re-execute last command
        // fc -s old=new: substitute and execute

        if args.is_empty() || args[0] == "-l" {
            // List mode
            let count = if args.len() > 1 {
                args[1].parse().unwrap_or(16)
            } else {
                16
            };

            match engine.recent(count) {
                Ok(entries) => {
                    for entry in entries.into_iter().rev() {
                        println!("{:>6}  {}", entry.id, entry.command);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0] == "-e" && args.get(1).map(|s| s.as_str()) == Some("-") {
            // Re-execute last command
            match engine.get_by_offset(0) {
                Ok(Some(entry)) => {
                    println!("{}", entry.command);
                    self.execute_script(&entry.command).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: no command to re-execute");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0] == "-s" {
            // Substitution mode: fc -s old=new or fc -s pattern
            let substitution = args.get(1);

            match engine.get_by_offset(0) {
                Ok(Some(entry)) => {
                    let mut cmd = entry.command.clone();

                    if let Some(sub) = substitution {
                        if let Some((old, new)) = sub.split_once('=') {
                            cmd = cmd.replace(old, new);
                        }
                    }

                    println!("{}", cmd);
                    self.execute_script(&cmd).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: no command to re-execute");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0].starts_with('-') && args[0].chars().skip(1).all(|c| c.is_ascii_digit()) {
            // fc -N: re-execute Nth previous command
            let n: usize = args[0][1..].parse().unwrap_or(1);
            match engine.get_by_offset(n - 1) {
                Ok(Some(entry)) => {
                    println!("{}", entry.command);
                    self.execute_script(&entry.command).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: event not found");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else {
            // Try to find command by prefix
            let prefix = &args[0];
            match engine.search_prefix(prefix, 1) {
                Ok(entries) if !entries.is_empty() => {
                    println!("{}", entries[0].command);
                    self.execute_script(&entries[0].command).unwrap_or(1)
                }
                Ok(_) => {
                    eprintln!("fc: event not found: {}", prefix);
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        }
    }

    fn builtin_trap(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all traps
            for (sig, action) in &self.traps {
                println!("trap -- '{}' {}", action, sig);
            }
            return 0;
        }

        // trap -l: list signal names
        if args.len() == 1 && args[0] == "-l" {
            let signals = [
                "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV",
                "USR2", "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN",
                "TTOU", "URG", "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO", "PWR", "SYS",
            ];
            for (i, sig) in signals.iter().enumerate() {
                print!("{:2}) SIG{:<8}", i + 1, sig);
                if (i + 1) % 5 == 0 {
                    println!();
                }
            }
            println!();
            return 0;
        }

        // trap -p [sigspec...]: print trap commands
        if args.len() >= 1 && args[0] == "-p" {
            let signals = if args.len() > 1 {
                &args[1..]
            } else {
                &[] as &[String]
            };
            if signals.is_empty() {
                for (sig, action) in &self.traps {
                    println!("trap -- '{}' {}", action, sig);
                }
            } else {
                for sig in signals {
                    if let Some(action) = self.traps.get(sig) {
                        println!("trap -- '{}' {}", action, sig);
                    }
                }
            }
            return 0;
        }

        // trap '' signal: reset to default
        // trap action signal...: set trap
        // trap signal: print current action for signal
        if args.len() == 1 {
            // Print trap for this signal
            let sig = &args[0];
            if let Some(action) = self.traps.get(sig) {
                println!("trap -- '{}' {}", action, sig);
            }
            return 0;
        }

        let action = &args[0];
        let signals = &args[1..];

        for sig in signals {
            let sig_upper = sig.to_uppercase();
            let sig_name = if sig_upper.starts_with("SIG") {
                sig_upper[3..].to_string()
            } else {
                sig_upper.clone()
            };

            if action.is_empty() || action == "-" {
                // Reset to default
                self.traps.remove(&sig_name);
            } else {
                self.traps.insert(sig_name, action.clone());
            }
        }

        0
    }

    /// Execute trap handlers for a signal
    pub fn run_trap(&mut self, signal: &str) {
        if let Some(action) = self.traps.get(signal).cloned() {
            let _ = self.execute_script(&action);
        }
    }

    fn builtin_alias(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all aliases
            for (name, value) in &self.aliases {
                println!("alias {}='{}'", name, value);
            }
            return 0;
        }

        for arg in args {
            if let Some(eq_pos) = arg.find('=') {
                // Define alias: name=value
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                self.aliases.insert(name.to_string(), value.to_string());
            } else {
                // Print alias
                if let Some(value) = self.aliases.get(arg) {
                    println!("alias {}='{}'", arg, value);
                } else {
                    eprintln!("zshrs: alias: {}: not found", arg);
                    return 1;
                }
            }
        }
        0
    }

    fn builtin_unalias(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("zshrs: unalias: usage: unalias [-a] name [name ...]");
            return 1;
        }

        if args.len() == 1 && args[0] == "-a" {
            // Remove all aliases
            self.aliases.clear();
            return 0;
        }

        for name in args {
            if name == "-a" {
                continue;
            }
            if self.aliases.remove(name).is_none() {
                eprintln!("zshrs: unalias: {}: not found", name);
                return 1;
            }
        }
        0
    }

    fn builtin_set(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all variables
            for (k, v) in &self.variables {
                println!("{}={}", k, v);
            }
            return 0;
        }

        // Handle set options
        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-e" => self.options.insert("errexit".to_string(), true),
                "+e" => self.options.insert("errexit".to_string(), false),
                "-x" => self.options.insert("xtrace".to_string(), true),
                "+x" => self.options.insert("xtrace".to_string(), false),
                "-u" => self.options.insert("nounset".to_string(), true),
                "+u" => self.options.insert("nounset".to_string(), false),
                "-o" => {
                    if let Some(opt) = iter.next() {
                        self.options.insert(opt.clone(), true);
                    }
                    continue;
                }
                "+o" => {
                    if let Some(opt) = iter.next() {
                        self.options.insert(opt.clone(), false);
                    }
                    continue;
                }
                "--" => {
                    // Set positional parameters
                    self.positional_params = iter.cloned().collect();
                    break;
                }
                _ => {
                    // Unknown option or positional params
                    if arg.starts_with('-') || arg.starts_with('+') {
                        eprintln!("zshrs: set: {}: invalid option", arg);
                        return 1;
                    }
                    // Treat remaining as positional params
                    self.positional_params =
                        std::iter::once(arg.clone()).chain(iter.cloned()).collect();
                    break;
                }
            };
        }
        0
    }

    fn builtin_shopt(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all shell options
            for (opt, val) in &self.options {
                println!("shopt {} {}", if *val { "-s" } else { "-u" }, opt);
            }
            return 0;
        }

        let mut set = None;
        let mut opts = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-s" => set = Some(true),
                "-u" => set = Some(false),
                "-p" => {
                    // Print option status
                    for opt in &opts {
                        let val = self.options.get(opt).copied().unwrap_or(false);
                        println!("shopt {} {}", if val { "-s" } else { "-u" }, opt);
                    }
                    return 0;
                }
                _ => opts.push(arg.clone()),
            }
        }

        if let Some(enable) = set {
            for opt in &opts {
                self.options.insert(opt.clone(), enable);
            }
        } else {
            // Query options
            for opt in &opts {
                let val = self.options.get(opt).copied().unwrap_or(false);
                println!("shopt {} {}", if val { "-s" } else { "-u" }, opt);
            }
        }
        0
    }

    fn builtin_getopts(&mut self, args: &[String]) -> i32 {
        if args.len() < 2 {
            eprintln!("zshrs: getopts: usage: getopts optstring name [arg ...]");
            return 1;
        }

        let optstring = &args[0];
        let varname = &args[1];
        let opt_args: Vec<&str> = if args.len() > 2 {
            args[2..].iter().map(|s| s.as_str()).collect()
        } else {
            self.positional_params.iter().map(|s| s.as_str()).collect()
        };

        // Get current OPTIND
        let optind: usize = self
            .variables
            .get("OPTIND")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        if optind > opt_args.len() {
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        let current_arg = opt_args[optind - 1];

        if !current_arg.starts_with('-') || current_arg == "-" {
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        if current_arg == "--" {
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        // Get current option position within the argument
        let optpos: usize = self
            .variables
            .get("_OPTPOS")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let opt_char = current_arg.chars().nth(optpos);

        if let Some(c) = opt_char {
            // Look up option in optstring
            let opt_idx = optstring.find(c);

            match opt_idx {
                Some(idx) => {
                    // Check if option takes an argument
                    let takes_arg = optstring.chars().nth(idx + 1) == Some(':');

                    if takes_arg {
                        // Get argument
                        let arg = if optpos + 1 < current_arg.len() {
                            // Argument is rest of current arg
                            current_arg[optpos + 1..].to_string()
                        } else if optind < opt_args.len() {
                            // Argument is next arg
                            self.variables
                                .insert("OPTIND".to_string(), (optind + 2).to_string());
                            self.variables.remove("_OPTPOS");
                            opt_args[optind].to_string()
                        } else {
                            // Missing argument
                            self.variables.insert(varname.to_string(), "?".to_string());
                            if !optstring.starts_with(':') {
                                eprintln!("zshrs: getopts: option requires an argument -- {}", c);
                            }
                            self.variables.insert("OPTARG".to_string(), c.to_string());
                            return 1;
                        };

                        self.variables.insert("OPTARG".to_string(), arg);
                        self.variables
                            .insert("OPTIND".to_string(), (optind + 1).to_string());
                        self.variables.remove("_OPTPOS");
                    } else {
                        // No argument needed
                        if optpos + 1 < current_arg.len() {
                            // More options in this arg
                            self.variables
                                .insert("_OPTPOS".to_string(), (optpos + 1).to_string());
                        } else {
                            // Move to next arg
                            self.variables
                                .insert("OPTIND".to_string(), (optind + 1).to_string());
                            self.variables.remove("_OPTPOS");
                        }
                    }

                    self.variables.insert(varname.to_string(), c.to_string());
                    0
                }
                None => {
                    // Unknown option
                    if !optstring.starts_with(':') {
                        eprintln!("zshrs: getopts: illegal option -- {}", c);
                    }
                    self.variables.insert(varname.to_string(), "?".to_string());
                    self.variables.insert("OPTARG".to_string(), c.to_string());

                    // Advance to next option/arg
                    if optpos + 1 < current_arg.len() {
                        self.variables
                            .insert("_OPTPOS".to_string(), (optpos + 1).to_string());
                    } else {
                        self.variables
                            .insert("OPTIND".to_string(), (optind + 1).to_string());
                        self.variables.remove("_OPTPOS");
                    }
                    0
                }
            }
        } else {
            // No more options in current arg
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.remove("_OPTPOS");
            self.variables.insert(varname.to_string(), "?".to_string());
            1
        }
    }

    fn builtin_type(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 0;
        }

        let mut status = 0;
        for name in args {
            // Check for alias
            if self.aliases.contains_key(name) {
                println!(
                    "{} is aliased to `{}'",
                    name,
                    self.aliases.get(name).unwrap()
                );
                continue;
            }

            // Check for function
            if self.functions.contains_key(name) {
                println!("{} is a shell function", name);
                continue;
            }

            // Check for builtin
            let builtins = [
                "cd", "pwd", "echo", "export", "unset", "source", "exit", "return", "true",
                "false", ":", "test", "[", "local", "declare", "typeset", "read", "shift", "eval",
                "jobs", "fg", "bg", "kill", "disown", "wait", "autoload", "history", "fc", "trap",
                "alias", "unalias", "set", "shopt", "getopts", "type", "hash", "command",
                "builtin", "let",
            ];
            if builtins.contains(&name.as_str()) {
                println!("{} is a shell builtin", name);
                continue;
            }

            // Check for external command
            if let Ok(path) = std::process::Command::new("which").arg(name).output() {
                if path.status.success() {
                    let path_str = String::from_utf8_lossy(&path.stdout);
                    println!("{} is {}", name, path_str.trim());
                    continue;
                }
            }

            eprintln!("zshrs: type: {}: not found", name);
            status = 1;
        }
        status
    }

    fn builtin_hash(&mut self, args: &[String]) -> i32 {
        // Simplified hash - just report if command exists
        if args.is_empty() {
            return 0;
        }

        for name in args {
            if let Ok(output) = std::process::Command::new("which").arg(name).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout);
                    println!("{}={}", name, path.trim());
                } else {
                    eprintln!("zshrs: hash: {}: not found", name);
                    return 1;
                }
            }
        }
        0
    }

    fn builtin_command(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        // Run command, bypassing functions and aliases
        if args.is_empty() {
            return 0;
        }

        let cmd = &args[0];
        let cmd_args = &args[1..];

        // Execute as external command
        self.execute_external(cmd, cmd_args, redirects)
            .unwrap_or(127)
    }

    fn builtin_builtin(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        // Run builtin, bypassing functions and aliases
        if args.is_empty() {
            return 0;
        }

        let cmd = &args[0];
        let cmd_args = &args[1..];

        match cmd.as_str() {
            "cd" => self.builtin_cd(cmd_args),
            "pwd" => self.builtin_pwd(redirects),
            "echo" => self.builtin_echo(cmd_args, redirects),
            "export" => self.builtin_export(cmd_args),
            "unset" => self.builtin_unset(cmd_args),
            "exit" => self.builtin_exit(cmd_args),
            "return" => self.builtin_return(cmd_args),
            "true" => 0,
            "false" => 1,
            ":" => 0,
            "test" | "[" => self.builtin_test(cmd_args),
            "local" => self.builtin_local(cmd_args),
            "declare" | "typeset" => self.builtin_declare(cmd_args),
            "read" => self.builtin_read(cmd_args),
            "shift" => self.builtin_shift(cmd_args),
            "eval" => self.builtin_eval(cmd_args),
            "alias" => self.builtin_alias(cmd_args),
            "unalias" => self.builtin_unalias(cmd_args),
            "set" => self.builtin_set(cmd_args),
            "shopt" => self.builtin_shopt(cmd_args),
            "getopts" => self.builtin_getopts(cmd_args),
            "type" => self.builtin_type(cmd_args),
            "hash" => self.builtin_hash(cmd_args),
            _ => {
                eprintln!("zshrs: builtin: {}: not a shell builtin", cmd);
                1
            }
        }
    }

    fn builtin_let(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }

        let mut result = 0i64;
        for expr in args {
            result = self.evaluate_arithmetic_expr(expr);
        }

        // let returns 1 if last expression evaluates to 0, 0 otherwise
        if result == 0 {
            1
        } else {
            0
        }
    }

    fn evaluate_arithmetic_expr(&mut self, expr: &str) -> i64 {
        // Handle simple assignment like x=5
        // But not if it's a comparison like == or !=
        if let Some(eq_pos) = expr.find('=') {
            // Check it's not part of ==, !=, <=, >=
            let chars: Vec<char> = expr.chars().collect();
            let is_comparison = (eq_pos > 0
                && (chars[eq_pos - 1] == '!'
                    || chars[eq_pos - 1] == '<'
                    || chars[eq_pos - 1] == '>'
                    || chars[eq_pos - 1] == '='))
                || chars.get(eq_pos + 1) == Some(&'=');

            if !is_comparison && !expr[..eq_pos].contains(|c| "+-*/%<>!&|".contains(c)) {
                let var_name = expr[..eq_pos].trim();
                if !var_name.is_empty() {
                    let value_expr = &expr[eq_pos + 1..];
                    let value = self.evaluate_arithmetic_expr(value_expr);
                    self.variables
                        .insert(var_name.to_string(), value.to_string());
                    env::set_var(var_name, value.to_string());
                    return value;
                }
            }
        }

        // Use the existing arithmetic evaluator
        let result_str = self.evaluate_arithmetic(expr);
        result_str.parse().unwrap_or(0)
    }
}
