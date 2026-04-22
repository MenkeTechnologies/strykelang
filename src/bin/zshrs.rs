//! zshrs - The most powerful shell ever created
//!
//! A drop-in zsh replacement that combines:
//! - Full bash/zsh script compatibility  
//! - Fish-quality completions with SQLite indexing
//! - Native stryke parallel operations via @ prefix
//!
//! Copyright (C) 2026 MenkeTechnologies
//! License: GPL-2.0 (incorporates code from fish-shell)

use std::env;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use reedline::Color;
use reedline::{
    default_emacs_keybindings, Completer, DefaultHinter, Emacs, FileBackedHistory, Prompt,
    PromptHistorySearch, PromptHistorySearchStatus, Reedline, Signal, Span, Suggestion,
};

use stryke::shell_completion::CompletionEngine;
use stryke::shell_exec::ShellExecutor;
use stryke::shell_history::HistoryEngine;
use stryke::shell_zwc;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Handle --dump-zwc for debugging .zwc files
    if args.len() >= 3 && args[1] == "--dump-zwc" {
        if args.len() >= 4 {
            // Dump specific function
            if let Err(e) = shell_zwc::dump_zwc_function(&args[2], &args[3]) {
                eprintln!("zshrs: {}: {}", args[2], e);
                std::process::exit(1);
            }
        } else {
            // List all functions
            if let Err(e) = shell_zwc::dump_zwc_info(&args[2]) {
                eprintln!("zshrs: {}: {}", args[2], e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Handle -c 'command' syntax
    if args.len() >= 3 && args[1] == "-c" {
        let code = &args[2];

        // Check for @ stryke mode
        if code.starts_with('@') {
            let stryke_code = code.trim_start_matches('@').trim();
            match stryke::run(stryke_code) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("stryke error: {}", e);
                    std::process::exit(1);
                }
            }
            return;
        }

        let mut executor = ShellExecutor::new();
        let start = Instant::now();
        let result = executor.execute_script(code);
        let duration = start.elapsed().as_millis() as i64;

        // Track in history
        if let Some(ref engine) = executor.history {
            let cwd = std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            if let Ok(id) = engine.add(code, cwd.as_deref()) {
                let _ = engine.update_last(id, duration, executor.last_status);
            }
        }

        if let Err(e) = result {
            eprintln!("zshrs: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Handle script file argument
    if args.len() >= 2 && !args[1].starts_with('-') {
        let mut executor = ShellExecutor::new();
        match std::fs::read_to_string(&args[1]) {
            Ok(script) => {
                if let Err(e) = executor.execute_script(&script) {
                    eprintln!("zshrs: {}: {}", args[1], e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("zshrs: {}: {}", args[1], e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Check if stdin is a TTY
    if atty::is(atty::Stream::Stdin) {
        run_interactive();
    } else {
        run_non_interactive();
    }
}

fn run_non_interactive() {
    let mut executor = ShellExecutor::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() || line == "exit" || line == "logout" {
                    continue;
                }
                process_line(line, &mut executor);
            }
            Err(_) => break,
        }
    }
}

fn run_interactive() {
    // Set up signal handling
    let interrupted = Arc::new(AtomicBool::new(false));
    let i = interrupted.clone();
    ctrlc::set_handler(move || {
        i.store(true, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // Initialize completion engine
    let completion_engine = match CompletionEngine::new() {
        Ok(engine) => {
            // Index system commands on first run
            if engine.count().unwrap_or(0) == 0 {
                eprint!("Indexing completions... ");
                let cmd_count = engine.index_system_commands().unwrap_or(0);
                let builtin_count = engine.index_shell_builtins().unwrap_or(0);
                eprintln!("{} commands, {} builtins", cmd_count, builtin_count);
            }
            Some(engine)
        }
        Err(e) => {
            eprintln!("Warning: completion engine failed to initialize: {e}");
            None
        }
    };

    // Initialize SQLite history engine for frequency tracking
    let history_engine = match HistoryEngine::new() {
        Ok(engine) => {
            let count = engine.count().unwrap_or(0);
            if count > 0 {
                eprintln!("Loaded {} history entries", count);
            }
            Some(engine)
        }
        Err(e) => {
            eprintln!("Warning: history engine failed to initialize: {e}");
            None
        }
    };

    let line_editor = setup_editor(completion_engine);
    if line_editor.is_none() {
        eprintln!("Failed to initialize line editor");
        return;
    }
    let mut line_editor = line_editor.unwrap();

    println!("zshrs 0.1.0 - stryke-powered shell");
    println!("Type @ to enter stryke mode, exit to quit\n");

    let mut executor = ShellExecutor::new();

    loop {
        let prompt = ZshrsPrompt::new(&executor);
        match line_editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if line == "exit" || line == "logout" {
                    break;
                }

                let start = Instant::now();
                process_line(line, &mut executor);
                let duration = start.elapsed().as_millis() as i64;

                // Track in SQLite history with frequency
                if let Some(ref engine) = history_engine {
                    let cwd = std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string());
                    if let Ok(id) = engine.add(line, cwd.as_deref()) {
                        let _ = engine.update_last(id, duration, executor.last_status);
                    }
                }
            }
            Ok(Signal::CtrlD) => {
                // EOF - exit shell
                executor.run_trap("EXIT");
                println!();
                break;
            }
            Ok(Signal::CtrlC) => {
                // Interrupt - run INT trap if set, otherwise just print newline
                interrupted.store(false, Ordering::SeqCst);
                executor.run_trap("INT");
                println!();
                continue;
            }
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }
}

fn process_line(line: &str, executor: &mut ShellExecutor) {
    if line.starts_with('@') {
        // Stryke mode - execute as stryke code
        let code = line.trim_start_matches('@').trim();
        execute_stryke(code);
    } else {
        // Shell mode - use proper parser and executor
        if let Err(e) = executor.execute_script(line) {
            eprintln!("zshrs: {}", e);
        }
    }
}

fn setup_editor(completion_engine: Option<CompletionEngine>) -> Option<Reedline> {
    let history_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zshrs_history");

    let history = Box::new(FileBackedHistory::with_file(10000, history_path).ok()?);

    let edit_mode = Box::new(Emacs::new(default_emacs_keybindings()));

    let mut editor = Reedline::create()
        .with_history(history)
        .with_edit_mode(edit_mode)
        .with_hinter(Box::new(DefaultHinter::default()));

    if let Some(engine) = completion_engine {
        editor = editor.with_completer(Box::new(ZshrsCompleter::new(engine)));
    }

    Some(editor)
}

struct ZshrsCompleter {
    engine: CompletionEngine,
}

impl ZshrsCompleter {
    fn new(engine: CompletionEngine) -> Self {
        Self { engine }
    }
}

impl Completer for ZshrsCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        // Find the word being completed
        let line_to_pos = &line[..pos];
        let word_start = line_to_pos
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &line_to_pos[word_start..];

        // Skip stryke mode prefix
        let word = word.trim_start_matches('@');

        if word.is_empty() {
            return vec![];
        }

        // Query the completion engine
        match self.engine.search(word, 50) {
            Ok(completions) => completions
                .into_iter()
                .map(|c| Suggestion {
                    value: c.name.clone(),
                    description: c.description,
                    style: None,
                    extra: Some(vec![c.kind.as_str().to_string()]),
                    span: Span::new(word_start, pos),
                    append_whitespace: true,
                })
                .collect(),
            Err(_) => vec![],
        }
    }
}

fn execute_stryke(code: &str) {
    if code.is_empty() {
        return;
    }

    // Use the stryke interpreter directly
    match stryke::run(code) {
        Ok(_) => {}
        Err(e) => eprintln!("stryke error: {e}"),
    }
}

/// Custom prompt that supports PS1/PROMPT with zsh escape sequences
struct ZshrsPrompt {
    left_prompt: String,
    right_prompt: String,
}

impl ZshrsPrompt {
    fn new(executor: &ShellExecutor) -> Self {
        // Check for PS1 or PROMPT (zsh uses PROMPT, bash uses PS1)
        let prompt_str = executor
            .variables
            .get("PROMPT")
            .or_else(|| executor.variables.get("PS1"))
            .cloned()
            .or_else(|| env::var("PROMPT").ok())
            .or_else(|| env::var("PS1").ok())
            .unwrap_or_else(|| "%n@%m %1~ %# ".to_string());

        // Check for RPROMPT (right prompt, zsh feature)
        let rprompt_str = executor
            .variables
            .get("RPROMPT")
            .cloned()
            .or_else(|| env::var("RPROMPT").ok())
            .unwrap_or_default();

        let left_prompt = expand_prompt_escapes(&prompt_str, executor);
        let right_prompt = expand_prompt_escapes(&rprompt_str, executor);

        Self {
            left_prompt,
            right_prompt,
        }
    }
}

impl Prompt for ZshrsPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed(&self.left_prompt)
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed(&self.right_prompt)
    }

    fn render_prompt_indicator(
        &self,
        _edit_mode: reedline::PromptEditMode,
    ) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("> ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        std::borrow::Cow::Owned(format!(
            "({}reverse-i-search)`{}': ",
            prefix, history_search.term
        ))
    }

    fn get_prompt_color(&self) -> Color {
        Color::Green
    }

    fn get_indicator_color(&self) -> Color {
        Color::Cyan
    }

    fn get_prompt_right_color(&self) -> Color {
        Color::AnsiValue(5)
    }

    fn right_prompt_on_last_line(&self) -> bool {
        true
    }
}

fn expand_prompt_escapes(prompt: &str, executor: &ShellExecutor) -> String {
    let mut result = String::new();
    let mut chars = prompt.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('n') => {
                    // Username
                    result.push_str(&env::var("USER").unwrap_or_else(|_| "user".to_string()));
                }
                Some('m') => {
                    // Hostname (short)
                    result.push_str(
                        &hostname::get()
                            .map(|h| {
                                h.to_string_lossy()
                                    .split('.')
                                    .next()
                                    .unwrap_or("localhost")
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('M') => {
                    // Hostname (full)
                    result.push_str(
                        &hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('~') | Some('d') => {
                    // Current directory (~ for home)
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(home) = dirs::home_dir() {
                        let home_str = home.to_string_lossy();
                        if cwd.starts_with(home_str.as_ref()) {
                            result.push('~');
                            result.push_str(&cwd[home_str.len()..]);
                        } else {
                            result.push_str(&cwd);
                        }
                    } else {
                        result.push_str(&cwd);
                    }
                }
                Some('/') => {
                    // Current directory (full path)
                    result.push_str(
                        &env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "?".to_string()),
                    );
                }
                Some('1') | Some('c') | Some('C') => {
                    // Trailing component of current directory
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(name) = PathBuf::from(&cwd).file_name() {
                        result.push_str(&name.to_string_lossy());
                    } else {
                        result.push('/');
                    }
                }
                Some('#') | Some('%') => {
                    // # if root, % otherwise
                    let is_root = env::var("EUID")
                        .or_else(|_| env::var("UID"))
                        .map(|uid| uid == "0")
                        .unwrap_or(false);
                    if is_root {
                        result.push('#');
                    } else {
                        result.push('%');
                    }
                }
                Some('?') => {
                    // Exit status of last command
                    result.push_str(&executor.last_status.to_string());
                }
                Some('j') => {
                    // Number of jobs
                    result.push_str(&executor.jobs.count().to_string());
                }
                Some('T') => {
                    // Current time in 12-hour format
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%I:%M").to_string());
                }
                Some('t') | Some('@') => {
                    // Current time in 12-hour format with am/pm
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%I:%M %p").to_string());
                }
                Some('*') => {
                    // Current time in 24-hour format
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%H:%M").to_string());
                }
                Some('D') => {
                    // Date
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%Y-%m-%d").to_string());
                }
                Some('F') => {
                    // Bold (start)
                    result.push_str("\x1b[1m");
                }
                Some('f') => {
                    // Bold (end) / reset
                    result.push_str("\x1b[0m");
                }
                Some('B') => {
                    // Bold (start, alternative)
                    result.push_str("\x1b[1m");
                }
                Some('b') => {
                    // Bold (end, alternative)
                    result.push_str("\x1b[22m");
                }
                Some('{') => {
                    // Start of literal escape sequence (ignored)
                }
                Some('}') => {
                    // End of literal escape sequence (ignored)
                }
                Some('%') => {
                    result.push('%');
                }
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => {
                    result.push('%');
                }
            }
        } else if c == '\\' {
            // Bash-style escapes
            match chars.next() {
                Some('u') => {
                    result.push_str(&env::var("USER").unwrap_or_else(|_| "user".to_string()));
                }
                Some('h') => {
                    result.push_str(
                        &hostname::get()
                            .map(|h| {
                                h.to_string_lossy()
                                    .split('.')
                                    .next()
                                    .unwrap_or("localhost")
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('H') => {
                    result.push_str(
                        &hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('w') => {
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(home) = dirs::home_dir() {
                        let home_str = home.to_string_lossy();
                        if cwd.starts_with(home_str.as_ref()) {
                            result.push('~');
                            result.push_str(&cwd[home_str.len()..]);
                        } else {
                            result.push_str(&cwd);
                        }
                    } else {
                        result.push_str(&cwd);
                    }
                }
                Some('W') => {
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(name) = PathBuf::from(&cwd).file_name() {
                        result.push_str(&name.to_string_lossy());
                    } else {
                        result.push('/');
                    }
                }
                Some('$') => {
                    let is_root = env::var("EUID")
                        .or_else(|_| env::var("UID"))
                        .map(|uid| uid == "0")
                        .unwrap_or(false);
                    if is_root {
                        result.push('#');
                    } else {
                        result.push('$');
                    }
                }
                Some('n') => {
                    result.push('\n');
                }
                Some('r') => {
                    result.push('\r');
                }
                Some('\\') => {
                    result.push('\\');
                }
                Some('[') | Some(']') => {
                    // Non-printing character markers (ignored in output)
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => {
                    result.push('\\');
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
