//! zshrs - The most powerful shell ever created
//!
//! A drop-in zsh replacement that combines:
//! - Full bash/zsh script compatibility  
//! - Zsh-style completion menu with SQLite indexing
//! - Fish-style syntax highlighting and autosuggestions
//! - Native stryke parallel operations via @ prefix
//!
//! Copyright (C) 2026 MenkeTechnologies
//! License: GPL-2.0 (incorporates code from fish-shell)

use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use stryke::shell_exec::ShellExecutor;
use stryke::shell_history::HistoryEngine;

use compsys::{
    build_cache_from_fpath, cache::CompsysCache, compinit_lazy, generate_completions,
    get_system_fpath, zpwr_list_colors, MenuAction, MenuKeymap, MenuState,
};

use zsh::highlight_shell;

/// Print help message identical to zsh --help
fn print_help() {
    println!(
        r#"Usage: zsh [<options>] [<argument> ...]

Special options:
  --help     show this message, then exit
  --version  show zsh version number, then exit
  -b         end option processing, like --
  -c         take first argument as a command to execute
  -o OPTION  set an option by name (see below)

Normal options are named.  An option may be turned on by
`-o OPTION', `--OPTION', `+o no_OPTION' or `+-no-OPTION'.  An
option may be turned off by `-o no_OPTION', `--no-OPTION',
`+o OPTION' or `+-OPTION'.  Options are listed below only in
`--OPTION' or `--no-OPTION' form.
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--help") {
        print_help();
        return;
    }

    if args.iter().any(|a| a == "--version") {
        println!("zshrs {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Find -c flag and its argument (handles -f -c, -c, etc.)
    if let Some(c_pos) = args.iter().position(|a| a == "-c") {
        if c_pos + 1 < args.len() {
            let code = &args[c_pos + 1];
            if code.starts_with('@') {
                let stryke_code = code.trim_start_matches('@').trim();
                if let Err(e) = stryke::run(stryke_code) {
                    eprintln!("stryke error: {}", e);
                    std::process::exit(1);
                }
                return;
            }

            let mut executor = ShellExecutor::new();
            if let Err(e) = executor.execute_script(code) {
                eprintln!("zshrs: {}", e);
                std::process::exit(1);
            }
            return;
        }
    }

    // Handle script file argument (first non-flag arg)
    if let Some(script_arg) = args.iter().skip(1).find(|a| !a.starts_with('-')) {
        let mut executor = ShellExecutor::new();
        match std::fs::read_to_string(script_arg) {
            Ok(script) => {
                if let Err(e) = executor.execute_script(&script) {
                    eprintln!("zshrs: {}: {}", script_arg, e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("zshrs: {}: {}", script_arg, e);
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
    let mut lines = String::new();
    if stdin.lock().read_to_string(&mut lines).is_ok() {
        for line in lines.lines() {
            let line = line.trim();
            if line.is_empty() || line == "exit" || line == "logout" {
                continue;
            }
            process_line(line, &mut executor);
        }
    }
}

// =============================================================================
// Terminal handling (raw mode)
// =============================================================================

#[allow(dead_code)]
struct Terminal {
    orig: libc::termios,
}

#[allow(dead_code)]
impl Terminal {
    fn new() -> io::Result<Self> {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return Err(io::Error::last_os_error());
            }
            let orig = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO);
            t.c_iflag &= !(libc::IXON | libc::ICRNL);
            t.c_cc[libc::VMIN] = 1; // Block until at least 1 char
            t.c_cc[libc::VTIME] = 0; // No timeout
            if libc::tcsetattr(0, libc::TCSAFLUSH, &t) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { orig })
        }
    }

    fn restore(&self) {
        unsafe {
            libc::tcsetattr(0, libc::TCSAFLUSH, &self.orig);
        }
    }

    fn init_raw(&self) {
        unsafe {
            let mut t = self.orig;
            t.c_lflag &= !(libc::ICANON | libc::ECHO);
            t.c_iflag &= !(libc::IXON | libc::ICRNL);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSAFLUSH, &t);
        }
    }

    fn size(&self) -> (usize, usize) {
        unsafe {
            let mut w: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut w) == 0 {
                (w.ws_col as usize, w.ws_row as usize)
            } else {
                (80, 24)
            }
        }
    }

    fn clear_screen(&self) {
        print!("\x1b[2J\x1b[H");
    }

    fn clear_below(&self) {
        print!("\x1b[J");
    }

    fn goto(&self, row: usize) {
        print!("\x1b[{};1H", row + 1);
    }

    fn save_cursor(&self) {
        print!("\x1b7");
    }

    fn restore_cursor(&self) {
        print!("\x1b8");
    }

    fn flush(&self) {
        let _ = io::stdout().flush();
    }

    fn read(&self, buf: &mut [u8]) -> usize {
        io::stdin().read(buf).unwrap_or(0)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}

#[allow(dead_code)]
fn get_cursor_row() -> usize {
    // Try to query cursor position using ANSI escape
    print!("\x1b[6n");
    let _ = io::stdout().flush();
    let mut buf = [0u8; 32];
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut pos = 0;
    loop {
        if pos >= buf.len() {
            break;
        }
        if handle.read(&mut buf[pos..pos + 1]).unwrap_or(0) == 0 {
            break;
        }
        if buf[pos] == b'R' {
            break;
        }
        pos += 1;
    }
    // Parse response: ESC [ row ; col R
    let response = String::from_utf8_lossy(&buf[..pos]);
    if let Some(start) = response.find('[') {
        if let Some(semi) = response.find(';') {
            if let Ok(row) = response[start + 1..semi].parse::<usize>() {
                return row.saturating_sub(1);
            }
        }
    }
    0
}

// =============================================================================
// Line editor
// =============================================================================

struct Editor {
    line: String,
    cursor: usize,
    history: Vec<String>,
    history_pos: usize,
    saved_line: String,
}

impl Editor {
    fn new() -> Self {
        Self {
            line: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: 0,
            saved_line: String::new(),
        }
    }

    fn load_history(&mut self, path: &PathBuf) {
        if let Ok(content) = std::fs::read_to_string(path) {
            self.history = content.lines().map(|s| s.to_string()).collect();
            self.history_pos = self.history.len();
        }
    }

    fn add_history(&mut self, line: &str) {
        if !line.is_empty() && self.history.last().map(|l| l != line).unwrap_or(true) {
            self.history.push(line.to_string());
        }
        self.history_pos = self.history.len();
    }

    fn insert(&mut self, c: char) {
        self.line.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    #[allow(dead_code)]
    fn insert_str(&mut self, s: &str) {
        self.line.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.line[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor -= prev;
            self.line.remove(self.cursor);
        }
    }

    fn delete_word(&mut self) {
        while self.cursor > 0
            && self.line[..self.cursor]
                .chars()
                .last()
                .map(|c| c.is_whitespace())
                .unwrap_or(false)
        {
            self.backspace();
        }
        while self.cursor > 0
            && !self.line[..self.cursor]
                .chars()
                .last()
                .map(|c| c.is_whitespace())
                .unwrap_or(true)
        {
            self.backspace();
        }
    }

    fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= self.line[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
        }
    }

    fn right(&mut self) {
        if self.cursor < self.line.len() {
            self.cursor += self.line[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.line.len() {
            let next = self.line[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            if next > 0 {
                self.line.remove(self.cursor);
            }
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.line.len();
    }

    fn clear(&mut self) {
        self.line.clear();
        self.cursor = 0;
    }

    fn kill_line(&mut self) {
        self.line.truncate(self.cursor);
    }

    fn kill_line_backward(&mut self) {
        self.line = self.line[self.cursor..].to_string();
        self.cursor = 0;
    }

    fn history_prev(&mut self) {
        if self.history_pos > 0 {
            if self.history_pos == self.history.len() {
                self.saved_line = self.line.clone();
            }
            self.history_pos -= 1;
            self.line = self.history[self.history_pos].clone();
            self.cursor = self.line.len();
        }
    }

    fn history_next(&mut self) {
        if self.history_pos < self.history.len() {
            self.history_pos += 1;
            if self.history_pos == self.history.len() {
                self.line = self.saved_line.clone();
            } else {
                self.line = self.history[self.history_pos].clone();
            }
            self.cursor = self.line.len();
        }
    }

    fn current_word(&self) -> &str {
        let before = &self.line[..self.cursor];
        let start = before
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        &self.line[start..self.cursor]
    }

    fn replace_current_word(&mut self, new: &str) {
        let before = &self.line[..self.cursor];
        let start = before
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.cursor
            + self.line[self.cursor..]
                .find(char::is_whitespace)
                .unwrap_or(self.line.len() - self.cursor);
        self.line.replace_range(start..end, new);
        self.cursor = start + new.len();
    }

    fn display_highlighted(&self) -> String {
        let specs = highlight_shell(&self.line);
        let mut result = String::new();
        let mut last_end = 0;

        for (i, c) in self.line.char_indices() {
            if let Some(spec) = specs.get(i) {
                let color = role_to_ansi(spec.foreground);
                if !color.is_empty() {
                    result.push_str(&self.line[last_end..i]);
                    result.push_str(color);
                    result.push(c);
                    result.push_str("\x1b[0m");
                    last_end = i + c.len_utf8();
                }
            }
        }
        result.push_str(&self.line[last_end..]);

        // Add cursor
        if self.cursor < self.line.len() {
            let before = &self.line[..self.cursor];
            let at_cursor = self.line[self.cursor..].chars().next().unwrap_or(' ');
            let after_cursor = self.cursor + at_cursor.len_utf8();
            format!(
                "{}\x1b[7m{}\x1b[0m{}",
                highlight_shell_str(before),
                at_cursor,
                highlight_shell_str(&self.line[after_cursor..])
            )
        } else {
            format!("{}\x1b[7m \x1b[0m", highlight_shell_str(&self.line))
        }
    }
}

fn highlight_shell_str(s: &str) -> String {
    let specs = highlight_shell(s);
    let mut result = String::new();
    for (i, c) in s.char_indices() {
        if let Some(spec) = specs.get(i) {
            let color = role_to_ansi(spec.foreground);
            if !color.is_empty() {
                result.push_str(color);
                result.push(c);
                result.push_str("\x1b[0m");
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn role_to_ansi(role: zsh::HighlightRole) -> &'static str {
    use zsh::HighlightRole::*;
    match role {
        Command => "\x1b[1;32m",
        Keyword => "\x1b[1;34m",
        Statement => "\x1b[1;35m",
        Option => "\x1b[36m",
        Comment => "\x1b[90m",
        Error => "\x1b[1;31m",
        String | Quote => "\x1b[33m",
        Escape => "\x1b[1;33m",
        Operator => "\x1b[1;37m",
        Redirection => "\x1b[35m",
        Variable => "\x1b[1;36m",
        _ => "",
    }
}

// =============================================================================
// Interactive shell
// =============================================================================

fn run_interactive() {
    let term = match Terminal::new() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Terminal init failed: {}", e);
            return;
        }
    };

    let (tw, th) = term.size();

    // Initialize compsys cache
    let cache_path = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("zshrs/compsys.db");
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let cache = match CompsysCache::open(&cache_path) {
        Ok(mut c) => {
            let (valid, count) = compinit_lazy(&c);
            if !valid || count == 0 {
                eprint!("Building completion cache... ");
                let fpath = get_system_fpath();
                let _ = build_cache_from_fpath(&fpath, &mut c);
                eprintln!("done");
            }

            // Index executables if needed
            if !c.has_executables().unwrap_or(false) {
                eprint!("Indexing PATH... ");
                let path_var = env::var("PATH").unwrap_or_default();
                let mut executables = Vec::new();
                for dir in path_var.split(':') {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            if let Ok(ft) = entry.file_type() {
                                if ft.is_file() || ft.is_symlink() {
                                    if let Some(name) = entry.file_name().to_str() {
                                        executables.push((name.to_string(), dir.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = c.set_executables_bulk(&executables);
                eprintln!("{} commands", executables.len());
            }
            c
        }
        Err(e) => {
            eprintln!("Cache error: {}", e);
            return;
        }
    };

    // Initialize history engine
    let history_engine = HistoryEngine::new().ok();

    // Editor and menu
    let mut editor = Editor::new();
    let history_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zshrs_history");
    editor.load_history(&history_path);

    let mut menu = MenuState::new();
    menu.set_term_size(tw, th);
    menu.set_available_rows(th.saturating_sub(5));
    menu.set_show_headers(true);
    menu.set_group_colors(zpwr_list_colors());

    // Load list-separator from zstyle cache
    if let Ok(Some(entry)) = cache.lookup_zstyle(":completion:*", "list-separator") {
        if let Some(sep) = entry.values.first() {
            // Expand $ZPWR_CHAR_LOGO if present
            let expanded = if sep.starts_with('$') {
                env::var(sep.trim_start_matches('$')).unwrap_or_else(|_| " -- ".to_string())
            } else {
                sep.clone()
            };
            menu.set_list_separator(&expanded);
        }
    }

    let _keymap = MenuKeymap::new();
    let mut menu_active = false;
    let mut buf = [0u8; 32];

    let mut executor = ShellExecutor::new();

    // Source startup files
    let no_rcs = std::env::args().any(|a| a == "-f" || a == "--no-rcs");
    if !no_rcs {
        source_startup_files(&mut executor);
    }

    println!("zshrs {} - stryke-powered shell", env!("CARGO_PKG_VERSION"));
    println!("Type @ for stryke mode, Tab for completion, exit to quit\n");

    let mut menu_lines: usize = 0;

    loop {
        // If we showed menu lines before, move up and clear them
        if menu_lines > 0 {
            print!("\x1b[{}A", menu_lines); // move up
            print!("\x1b[J"); // clear from cursor down
            menu_lines = 0;
        }

        // Print prompt + input (stays on this line)
        print!(
            "\r\x1b[K{}{}",
            build_prompt(&executor),
            editor.display_highlighted()
        );

        // Show menu below if active
        if menu_active && menu.count() > 0 {
            let (tw, _) = term.size();
            let r = menu.render();

            // Print menu below prompt
            println!(); // move to next line
            println!("{}", "─".repeat(tw.min(80)));
            menu_lines = 2;

            for line in &r.lines {
                println!("{}", line.content);
                menu_lines += 1;
            }

            if let Some(status) = &r.status {
                println!("\x1b[36m{}\x1b[0m", status);
                menu_lines += 1;
            }

            print!("{}", "─".repeat(tw.min(80)));
            if let Some(c) = menu.selected() {
                let (sel, total, _) = menu.status_info();
                print!(" \x1b[7m {} \x1b[0m {}/{}", c.str_, sel, total);
            }
            menu_lines += 1;

            // Move cursor back up to prompt line
            print!("\x1b[{}A", menu_lines);
            print!(
                "\r\x1b[K{}{}",
                build_prompt(&executor),
                editor.display_highlighted()
            );
        }

        term.flush();

        // Read input (blocks until char available)
        let n = term.read(&mut buf);
        if n == 0 {
            // EOF
            break;
        }
        let input = &buf[..n];

        // Ctrl+C - cancel
        if input == [3] {
            if menu_active {
                menu_active = false;
            } else {
                print!("^C\r\n");
                editor.clear();
            }
            continue;
        }

        // Ctrl+D - EOF
        if input == [4] && editor.line.is_empty() {
            print!("\r\n");
            break;
        }

        // Tab - toggle/navigate menu
        if input == [9] {
            // Always refresh terminal size before completions
            let (tw, th) = term.size();
            menu.set_term_size(tw, th);
            menu.set_available_rows(th.saturating_sub(5));

            if !menu_active {
                // Generate completions and show menu
                let groups = generate_completions(&cache, &editor.line, editor.cursor);
                menu.set_prefix(editor.current_word());
                menu.set_completions(&groups);
                if menu.count() > 0 {
                    menu.start();
                    menu_active = true;
                }
            } else {
                // Navigate in menu
                menu.process_action(MenuAction::Next);
            }
            continue;
        }

        // Escape - cancel menu
        if input == [0x1b] && n == 1 {
            if menu_active {
                menu_active = false;
            }
            continue;
        }

        // Enter (CR or LF)
        if input == [13] || input == [10] {
            if menu_active && menu.count() > 0 {
                // Accept completion
                if let Some(c) = menu.selected() {
                    editor.replace_current_word(&c.str_.clone());
                }
                menu_active = false;
            } else if !editor.line.is_empty() {
                // Clear menu below prompt
                print!("\x1b[J");
                menu_active = false;
                menu_lines = 0;

                // Reprint prompt + command, then newline
                print!("\r\x1b[K{}{}\r\n", build_prompt(&executor), &editor.line);
                term.flush();
                term.restore();

                let line = editor.line.clone();
                editor.add_history(&line);

                let start = Instant::now();
                process_line(&line, &mut executor);
                let duration = start.elapsed().as_millis() as i64;

                // Track in history
                if let Some(ref engine) = history_engine {
                    let cwd = env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string());
                    if let Ok(id) = engine.add(&line, cwd.as_deref()) {
                        let _ = engine.update_last(id, duration, executor.last_status);
                    }
                }

                editor.clear();
                term.init_raw();
            } else {
                print!("\r\n");
            }
            continue;
        }

        // Menu navigation when active
        if menu_active {
            use compsys::menu::MenuMode;

            // Check if in search/interactive mode
            let mode = menu.get_mode();

            // Handle search mode input
            if mode == MenuMode::ForwardSearch || mode == MenuMode::BackwardSearch {
                match input {
                    // Escape - cancel search
                    &[0x1b] => {
                        menu.cancel_search();
                        continue;
                    }
                    // Enter - accept search position
                    &[13] | &[10] => {
                        menu.cancel_search();
                        continue;
                    }
                    // Backspace - remove last search char
                    &[127] | &[8] => {
                        menu.search_backspace();
                        continue;
                    }
                    // Ctrl+R - search again backward
                    &[0x12] => {
                        menu.search_again(true);
                        continue;
                    }
                    // Ctrl+S - search again forward
                    &[0x13] => {
                        menu.search_again(false);
                        continue;
                    }
                    // Regular chars - add to search
                    _ => {
                        if input[0] >= 32 && input[0] < 127 {
                            menu.search_input(input[0] as char);
                        }
                        continue;
                    }
                }
            }

            // Handle interactive mode input
            if mode == MenuMode::Interactive {
                match input {
                    // Escape - exit interactive mode
                    &[0x1b] => {
                        menu.toggle_interactive();
                        continue;
                    }
                    // Enter - accept current
                    &[13] | &[10] => {
                        menu.toggle_interactive();
                        // Accept will be handled by Enter handler
                    }
                    // Backspace - remove last char
                    &[127] | &[8] => {
                        menu.search_backspace();
                        continue;
                    }
                    // Arrow keys still navigate
                    &[0x1b, 0x5b, 0x41] => {
                        menu.process_action(MenuAction::Up);
                        continue;
                    }
                    &[0x1b, 0x5b, 0x42] => {
                        menu.process_action(MenuAction::Down);
                        continue;
                    }
                    &[0x1b, 0x5b, 0x43] => {
                        menu.process_action(MenuAction::Right);
                        continue;
                    }
                    &[0x1b, 0x5b, 0x44] => {
                        menu.process_action(MenuAction::Left);
                        continue;
                    }
                    // Regular chars - filter
                    _ => {
                        if input[0] >= 32 && input[0] < 127 {
                            menu.search_input(input[0] as char);
                        }
                        continue;
                    }
                }
            }

            let action = match input {
                // Arrow keys
                &[0x1b, 0x5b, 0x41] => Some(MenuAction::Up),
                &[0x1b, 0x5b, 0x42] => Some(MenuAction::Down),
                &[0x1b, 0x5b, 0x43] => Some(MenuAction::Right),
                &[0x1b, 0x5b, 0x44] => Some(MenuAction::Left),
                // Ctrl keys for navigation
                &[0x0e] => Some(MenuAction::Down),  // Ctrl+N
                &[0x10] => Some(MenuAction::Up),    // Ctrl+P
                &[0x06] => Some(MenuAction::Right), // Ctrl+F
                &[0x02] => Some(MenuAction::Left),  // Ctrl+B
                // Page up/down
                &[0x1b, 0x5b, 0x35, 0x7e] => Some(MenuAction::PageUp), // Page Up
                &[0x1b, 0x5b, 0x36, 0x7e] => Some(MenuAction::PageDown), // Page Down
                // Home/End
                &[0x1b, 0x5b, 0x48] | &[0x1b, 0x4f, 0x48] => Some(MenuAction::Beginning), // Home
                &[0x1b, 0x5b, 0x46] | &[0x1b, 0x4f, 0x46] => Some(MenuAction::End),       // End
                _ => None,
            };

            if let Some(act) = action {
                menu.process_action(act);
                continue;
            }

            // Special menu keys
            match input {
                // Ctrl+R - start backward search
                &[0x12] => {
                    menu.start_backward_search();
                    continue;
                }
                // Ctrl+S - start forward search
                &[0x13] => {
                    menu.start_forward_search();
                    continue;
                }
                // Ctrl+X Ctrl+I or just 'i' - toggle interactive mode (vi-insert)
                &[0x69] => {
                    menu.toggle_interactive();
                    continue;
                }
                // Ctrl+G or 'g' - beginning of list
                &[0x07] | &[0x67] => {
                    menu.navigate(compsys::menu::MenuMotion::First);
                    continue;
                }
                // 'G' - end of list
                &[0x47] => {
                    menu.navigate(compsys::menu::MenuMotion::Last);
                    continue;
                }
                // 'u' - undo
                &[0x75] => {
                    let _ = menu.pop_undo();
                    continue;
                }
                // Alt+> or '}' - next group (forward-blank-word)
                &[0x7d] => {
                    menu.navigate(compsys::menu::MenuMotion::ForwardBlankWord);
                    continue;
                }
                // Alt+< or '{' - prev group (backward-blank-word)
                &[0x7b] => {
                    menu.navigate(compsys::menu::MenuMotion::BackwardBlankWord);
                    continue;
                }
                // Ctrl+Space or Ctrl+@ - mark current (multi-select)
                &[0x00] => {
                    menu.mark_current();
                    continue;
                }
                // 'm' - mark and move next (accept-and-menu-complete)
                &[0x6d] => {
                    menu.mark_and_next();
                    continue;
                }
                // 'M' - clear all marks
                &[0x4d] => {
                    menu.clear_marks();
                    continue;
                }
                _ => {}
            }
        }

        // Normal editing
        match input {
            // Backspace
            &[127] | &[8] => {
                editor.backspace();
                if menu_active {
                    refresh_completions(&mut menu, &editor, &cache, &term);
                }
            }
            // Ctrl+W - delete word
            &[0x17] => {
                editor.delete_word();
                if menu_active {
                    refresh_completions(&mut menu, &editor, &cache, &term);
                }
            }
            // Ctrl+A - home
            &[0x01] => editor.home(),
            // Ctrl+E - end
            &[0x05] => editor.end(),
            // Ctrl+K - kill to end
            &[0x0b] => editor.kill_line(),
            // Ctrl+U - kill to start
            &[0x15] => editor.kill_line_backward(),
            // Arrow keys (not in menu)
            &[0x1b, 0x5b, 0x41] => editor.history_prev(), // Up
            &[0x1b, 0x5b, 0x42] => editor.history_next(), // Down
            &[0x1b, 0x5b, 0x43] => editor.right(),        // Right
            &[0x1b, 0x5b, 0x44] => editor.left(),         // Left
            // Delete
            &[0x1b, 0x5b, 0x33, 0x7e] => editor.delete(),
            // Home/End
            &[0x1b, 0x5b, 0x48] => editor.home(),
            &[0x1b, 0x5b, 0x46] => editor.end(),
            // Regular characters
            _ => {
                if input[0] >= 32 && input[0] < 127 {
                    for &b in input {
                        if (32..127).contains(&b) {
                            editor.insert(b as char);
                        }
                    }
                    if menu_active {
                        refresh_completions(&mut menu, &editor, &cache, &term);
                    }
                }
            }
        }
    }

    // Cleanup
    drop(term);

    // Save history
    if let Ok(mut f) = std::fs::File::create(&history_path) {
        for line in &editor.history {
            let _ = writeln!(f, "{}", line);
        }
    }
}

fn refresh_completions(
    menu: &mut MenuState,
    editor: &Editor,
    cache: &CompsysCache,
    term: &Terminal,
) {
    // Refresh terminal size
    let (tw, th) = term.size();
    menu.set_term_size(tw, th);
    menu.set_available_rows(th.saturating_sub(5));

    let groups = generate_completions(cache, &editor.line, editor.cursor);
    menu.set_prefix(editor.current_word());
    menu.set_completions(&groups);
    if menu.count() > 0 {
        menu.start();
    }
}

fn build_prompt(executor: &ShellExecutor) -> String {
    let prompt_str = executor
        .variables
        .get("PROMPT")
        .or_else(|| executor.variables.get("PS1"))
        .cloned()
        .or_else(|| env::var("PROMPT").ok())
        .or_else(|| env::var("PS1").ok())
        .unwrap_or_else(|| "%n@%m %1~ %# ".to_string());

    expand_prompt(&prompt_str, executor)
}

fn expand_prompt(prompt: &str, executor: &ShellExecutor) -> String {
    let mut result = String::new();
    let mut chars = prompt.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('n') => {
                    result.push_str(&env::var("USER").unwrap_or_else(|_| "user".to_string()));
                }
                Some('m') => {
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
                Some('~') | Some('d') => {
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
                Some('1') | Some('c') | Some('C') => {
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
                    let is_root = env::var("EUID")
                        .or_else(|_| env::var("UID"))
                        .map(|uid| uid == "0")
                        .unwrap_or(false);
                    result.push(if is_root { '#' } else { '%' });
                }
                Some('?') => {
                    result.push_str(&executor.last_status.to_string());
                }
                Some('F') => result.push_str("\x1b[1m"),
                Some('f') => result.push_str("\x1b[0m"),
                Some('B') => result.push_str("\x1b[1m"),
                Some('b') => result.push_str("\x1b[22m"),
                Some('{') | Some('}') => {}
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn process_line(line: &str, executor: &mut ShellExecutor) {
    if line.starts_with('@') {
        let code = line.trim_start_matches('@').trim();
        if !code.is_empty() {
            if let Err(e) = stryke::run(code) {
                eprintln!("stryke error: {}", e);
            }
        }
    } else if let Err(e) = executor.execute_script(line) {
        eprintln!("zshrs: {}", e);
    }
}

fn source_startup_files(executor: &mut ShellExecutor) {
    let zdotdir = env::var("ZDOTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

    // Source .zshenv
    source_file(executor, &zdotdir.join(".zshenv"));

    // Source .zshrc for interactive shell
    source_file(executor, &zdotdir.join(".zshrc"));
}

fn source_file(executor: &mut ShellExecutor, path: &PathBuf) {
    if !path.exists() {
        return;
    }

    if let Ok(contents) = std::fs::read_to_string(path) {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let _ = executor.execute_script(line);
        }
    }
}
