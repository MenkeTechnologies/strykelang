//! Interactive REPL for `stryke` — utop-style line editor backed by `reedline`.
//!
//! Layout per turn:
//!
//! ```text
//! ─( HH:MM:SS )──< command N >─────────────────────────────{ stryke 0.11.5 }─
//! stryke❯ <buffer>
//!         abs           accumulate    acos          all           any   …
//! ```
//!
//! * Top "modeline" is rendered as part of `Prompt::render_prompt_left` so it
//!   repaints with the buffer (no scroll-off, no flicker).
//! * Tab pops a `ColumnarMenu` of suggestions sourced from
//!   `crate::lsp::builtin_completion_words` plus the live interpreter
//!   binding/sub names — the same wordlist the LSP serves.
//! * History is `~/.stryke/history` via `FileBackedHistory`.
//! * `$obj->method` completion uses the running interpreter's blessed-scalar
//!   snapshot (same code path the old rustyline driver used).
//!
//! Reedline does not include a file-path completer; bare-path completion is
//! intentionally dropped — the LSP word list covers the high-value surface
//! and matches utop's UX (commands, not paths).

use std::borrow::Cow;
use std::process;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use nu_ansi_term::{Color as NuColor, Style};
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, Completer, EditMode, Emacs, FileBackedHistory, KeyCode, KeyModifiers,
    Keybindings, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion, Vi,
};

use crate::cli::Cli;
use crate::error::ErrorKind;
use crate::lsp::builtin_completion_words;
use crate::token::KEYWORDS;
use crate::vm_helper::{repl_arrow_method_completions, ReplCompletionSnapshot, VMHelper};

/// Builtin names not yet captured in `lsp_completion_words.txt`.
const EXTRA_KEYWORDS: &[&str] = &["deque", "heap", "ppool", "barrier", "bench", "spawn"];

const STRYKE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn stryke_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".stryke"))
        .unwrap_or_else(|| std::path::PathBuf::from(".stryke"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn history_path() -> std::path::PathBuf {
    stryke_dir().join("history")
}

fn config_path() -> std::path::PathBuf {
    stryke_dir().join("config.toml")
}

/// Contents of the auto-seeded `~/.stryke/config.toml`. Every setting is
/// commented out so the seeded file documents the schema without changing
/// behavior — uncomment + edit a line to override the in-code default.
const DEFAULT_CONFIG_TOML: &str = r#"# stryke runtime config — auto-generated on first launch.
# Lines starting with `#` are comments. Uncomment + edit a line to
# override the in-code default. Delete this file and stryke will
# regenerate it on the next run.

[repl]
# Edit mode for the interactive REPL. Defaults to emacs.
#
#   "emacs" — Ctrl-A/Ctrl-E/Ctrl-K/etc., readline-style (default)
#   "vi"    — modal editing; Esc → normal mode, i/a → insert,
#             h/j/k/l navigation, dd/cc/yy/x, /-search, etc.
#
# Tab + Shift+Tab cycle the completion menu in either mode.
# Override per-session with `STRYKE_REPL_MODE=vi stryke`.
# mode = "emacs"
"#;

/// First-run seed: write `~/.stryke/config.toml` if it does not exist.
/// Safe to call on every binary launch — no-op when the file is already
/// there (and silent if the home directory is read-only). Honors
/// `STRYKE_NO_CONFIG=1` for CI / sandbox environments that should not
/// touch the user's home dir.
pub fn ensure_default_config_seeded() {
    if std::env::var_os("STRYKE_NO_CONFIG").is_some() {
        return;
    }
    let path = config_path();
    if path.exists() {
        return;
    }
    // `stryke_dir()` already created the directory; ignore write failures
    // (read-only homes, sandboxed PATH probes, parallel workers racing on
    // the same path — losing the race just leaves the existing file).
    let _ = std::fs::write(&path, DEFAULT_CONFIG_TOML);
}

/// REPL edit-mode selector. `Emacs` is the default; `Vi` enables reedline's
/// two-mode insert/normal keybinding set with the standard `Esc` toggle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ReplMode {
    Emacs,
    Vi,
}

/// Resolve the REPL edit mode in this precedence:
/// 1. `STRYKE_REPL_MODE=emacs|vi` env var (overrides everything; handy in
///    tests / dotfile bootstrap before the config file exists).
/// 2. `~/.stryke/config.toml` `[repl] mode = "vi"`.
/// 3. Default `Emacs`.
fn resolve_repl_mode() -> ReplMode {
    if let Some(env) = std::env::var_os("STRYKE_REPL_MODE") {
        let s = env.to_string_lossy().to_ascii_lowercase();
        if s == "vi" || s == "vim" {
            return ReplMode::Vi;
        }
        if s == "emacs" {
            return ReplMode::Emacs;
        }
    }
    let raw = match std::fs::read_to_string(config_path()) {
        Ok(s) => s,
        Err(_) => return ReplMode::Emacs,
    };
    let parsed: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return ReplMode::Emacs,
    };
    let mode = parsed
        .get("repl")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("emacs");
    match mode.to_ascii_lowercase().as_str() {
        "vi" | "vim" => ReplMode::Vi,
        _ => ReplMode::Emacs,
    }
}

/// Apply the completion-menu Tab / Shift+Tab bindings to a keybinding set
/// — shared so the bindings live on the emacs map AND the vi insert map.
fn install_menu_bindings(keybindings: &mut Keybindings) {
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
}

fn build_static_completions() -> Vec<String> {
    let mut v: Vec<String> = KEYWORDS
        .iter()
        .chain(EXTRA_KEYWORDS.iter())
        .map(|s| (*s).to_string())
        .collect();
    v.extend(builtin_completion_words().iter().cloned());
    v.sort();
    v.dedup();
    v
}

/// Byte index `start` and the incomplete word before cursor (for prefix matching).
/// Word boundaries include whitespace and punctuation; if the tail contains `$`, `@`, or `%`,
/// the start snaps to that sigil so variables complete as `$name`, `@name`, `%name`.
fn completion_word_start(line: &str, pos: usize) -> (usize, &str) {
    let pos = pos.min(line.len());
    let before = line.get(..pos).unwrap_or("");
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| {
            c.is_whitespace()
                || matches!(
                    *c,
                    '(' | ')' | ',' | ';' | '[' | ']' | '{' | '}' | '|' | '=' | '&' | '+'
                )
        })
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let mut word_start = start;
    let tail = line.get(word_start..pos).unwrap_or("");
    if let Some(rel) = tail.find(['$', '@', '%']) {
        word_start += rel;
    }
    (word_start, line.get(word_start..pos).unwrap_or(""))
}

struct StrykeCompleter {
    static_words: Vec<String>,
    dynamic: Arc<Mutex<Vec<String>>>,
    snapshot: Arc<Mutex<ReplCompletionSnapshot>>,
}

impl StrykeCompleter {
    fn build_word_suggestions(&self, prefix: &str, span: Span) -> Vec<Suggestion> {
        let dyn_list = match self.dynamic.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out: Vec<Suggestion> = Vec::new();
        for w in self.static_words.iter().chain(dyn_list.iter()) {
            if !w.starts_with(prefix) {
                continue;
            }
            if !seen.insert(w.clone()) {
                continue;
            }
            out.push(Suggestion {
                value: w.clone(),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: false,
                display_override: None,
                match_indices: None,
            });
        }
        out.sort_by(|a, b| a.value.cmp(&b.value));
        out
    }
}

impl Completer for StrykeCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        // 1. `$obj->method` arrow-method completion
        if let Ok(g) = self.snapshot.lock() {
            if let Some((start, methods)) = repl_arrow_method_completions(&g, line, pos) {
                let span = Span::new(start, pos);
                let mut out: Vec<Suggestion> = methods
                    .into_iter()
                    .map(|m| Suggestion {
                        value: m,
                        description: None,
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: false,
                        display_override: None,
                        match_indices: None,
                    })
                    .collect();
                out.sort_by(|a, b| a.value.cmp(&b.value));
                return out;
            }
        }

        // 2. word completion (handles sigil-prefixed and bare names)
        let (start, prefix) = completion_word_start(line, pos);
        let span = Span::new(start, pos);
        self.build_word_suggestions(prefix, span)
    }
}

struct StrykePrompt {
    cmd_count: Arc<Mutex<u64>>,
}

fn now_hms() -> String {
    // Local time via `libc::localtime_r` — no chrono / time crate. Reads
    // `/etc/localtime` (or `TZ` env), works on macOS aarch64 + Linux. On
    // failure or invalid epoch, falls back to UTC modulo math so the
    // status bar always shows something.
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    if ok {
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    } else {
        let s = (secs as u64) % 86_400;
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

fn term_cols() -> usize {
    use std::os::unix::io::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let fd = std::io::stdout().as_raw_fd();
    let cols = if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
        ws.ws_col as usize
    } else {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(80)
    };
    cols.max(40)
}

/// Read one line from stdin with a plain prompt, for when reedline cannot
/// paint — see the `plain_input` fallback in [`run`].
///
/// The signal shape is reedline's so the caller's `match` is unchanged: EOF is
/// `CtrlD`, which is what the REPL already treats as "leave".
///
/// The fallback is entered because the terminal answered crossterm's
/// cursor-position query (`ESC [ 6 n`) too late, and those answers do still
/// arrive: on a cooked tty they land in the line being read, ahead of what the
/// user types (`^[[48;1R^[[48;1Rp 42`). They are stripped here so they never
/// reach the parser, and the second value reports that one was seen — proof the
/// terminal answers after all, so the caller can give the editor another try.
fn read_plain_line(cmd_count: u64) -> (Signal, bool) {
    use std::io::{BufRead, Write};

    print!("stryke[{}]> ", cmd_count);
    let _ = std::io::stdout().flush();

    let carried =
        std::mem::take(&mut *TYPED_DURING_DRAIN.lock().unwrap_or_else(|e| e.into_inner()));
    let mut line = String::from_utf8_lossy(&carried).replace('\r', "\n");
    if let Some(nl) = line.find('\n') {
        // The user already pressed Enter while the drain was waiting.
        let rest = line.split_off(nl + 1);
        *TYPED_DURING_DRAIN.lock().unwrap_or_else(|e| e.into_inner()) = rest.into_bytes();
        print!("{line}");
        let (clean, saw_reply) = strip_cursor_reports(line.trim_end_matches('\n'));
        return (Signal::Success(clean), saw_reply);
    }
    print!("{line}");
    let _ = std::io::stdout().flush();
    match std::io::stdin().lock().read_line(&mut line) {
        // 0 bytes is end of input, not an empty line.
        Ok(0) => (Signal::CtrlD, false),
        Ok(_) => {
            let (clean, saw_reply) = strip_cursor_reports(line.trim_end_matches(['\n', '\r']));
            (Signal::Success(clean), saw_reply)
        }
        Err(_) => (Signal::CtrlD, false),
    }
}

/// Remove every cursor-position report — `ESC [ <row> ; <col> R`, the answer to
/// `ESC [ 6 n` — from `s`, and say whether there was one. Anything else that
/// starts with `ESC [` is left alone.
fn strip_cursor_reports(s: &str) -> (String, bool) {
    let (rest, count) = split_cursor_reports(s.as_bytes());
    // Only whole ASCII sequences were removed, so the rest is still UTF-8.
    (String::from_utf8(rest).unwrap_or_default(), count > 0)
}

/// `b` without its cursor-position reports, and how many there were.
fn split_cursor_reports(b: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(b.len());
    let mut count = 0;
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = cursor_report_end(&b[i..]) {
            count += 1;
            i += end;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    (out, count)
}

/// Keystrokes typed while [`drain_cursor_reports`] was waiting; the plain-line
/// reader puts them back in front of the next line so none are lost.
static TYPED_DURING_DRAIN: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Wait up to `budget` for `expected` late cursor-position reports to arrive on
/// `fd` and swallow them without echo, so they never show up as `^[[43;1R` at
/// the prompt. Returns how many arrived. Anything else the user typed meanwhile
/// is kept in [`TYPED_DURING_DRAIN`]. The tty's modes are restored on return.
///
/// Called when the editor gave up on the terminal: every query it sent is still
/// owed an answer, and a terminal that is merely slow delivers them a few
/// seconds later onto a cooked tty, which echoes them.
#[cfg(unix)]
fn drain_cursor_reports(fd: i32, expected: usize, budget: std::time::Duration) -> usize {
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
        return 0;
    }
    let mut raw = saved;
    raw.c_lflag &= !(libc::ECHO | libc::ICANON);
    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 1; // 100 ms per read
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return 0;
    }

    let deadline = std::time::Instant::now() + budget;
    let mut got = Vec::new();
    let mut seen = 0;
    let mut chunk = [0u8; 256];
    while seen < expected && std::time::Instant::now() < deadline {
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            got.extend_from_slice(&chunk[..n as usize]);
            seen = split_cursor_reports(&got).1;
        } else if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            break;
        }
    }
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };

    let (typed, _) = split_cursor_reports(&got);
    if let Ok(mut t) = TYPED_DURING_DRAIN.lock() {
        t.extend_from_slice(&typed);
    }
    seen
}

/// Length of the cursor-position report at the start of `b`, if there is one.
fn cursor_report_end(b: &[u8]) -> Option<usize> {
    let rest = b.strip_prefix(b"\x1b[")?;
    let row = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    let rest = rest[row..].strip_prefix(b";")?;
    let col = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    if row == 0 || col == 0 || rest.get(col) != Some(&b'R') {
        return None;
    }
    Some(2 + row + 1 + col + 1)
}

fn render_status_bar(cmd_count: u64) -> String {
    let cols = term_cols();
    let dim = NuColor::DarkGray;
    let accent = NuColor::Cyan;
    let label = NuColor::LightYellow;

    let left = format!(" {} ", now_hms());
    let mid = format!(" command {} ", cmd_count);
    let right = format!(" stryke {} ", STRYKE_VERSION);

    // Plain-text widths for layout math (segments themselves contain no ANSI yet).
    // `frame_chars` = display width of every literal frame char emitted below
    // (`─(`, `)──<`, `>`, `{`, `}─`). Off-by-N here pushes the right segment
    // onto a new line — bug observed at v0.11.6 when this was hand-counted as 4.
    // `chars().count()` isn't `const fn`, so this is a `let` (runs once per repaint).
    let frame_chars = "─()──<>{}─".chars().count();
    let visible = left.chars().count() + mid.chars().count() + right.chars().count() + frame_chars;
    let dashes = cols.saturating_sub(visible);
    // Need at least 1 dash on each side for the frame look; if the terminal
    // is genuinely too narrow, drop the right segment entirely instead of
    // wrapping (one line, no overflow — utop does the same).
    if dashes < 2 {
        return format!(
            "{lp}{l}{rp}{ml}{m}{mr}",
            lp = Style::new().fg(dim).paint("─("),
            l = Style::new().fg(accent).paint(left),
            rp = Style::new().fg(dim).paint(")"),
            ml = Style::new().fg(dim).paint("──<"),
            m = Style::new().fg(label).bold().paint(mid),
            mr = Style::new().fg(dim).paint(">"),
        );
    }
    let left_dash = dashes / 2;
    let right_dash = dashes - left_dash;

    let bar_l = "─".repeat(left_dash);
    let bar_r = "─".repeat(right_dash);

    format!(
        "{lp}{l}{rp}{ml}{m}{mr}{bar}{rl}{r}{rr}",
        lp = Style::new().fg(dim).paint("─("),
        l = Style::new().fg(accent).paint(left),
        rp = Style::new().fg(dim).paint(")"),
        ml = Style::new().fg(dim).paint("──<"),
        m = Style::new().fg(label).bold().paint(mid),
        mr = Style::new().fg(dim).paint(">"),
        bar = Style::new().fg(dim).paint(format!("{}{}", bar_l, bar_r)),
        rl = Style::new().fg(dim).paint("{"),
        r = Style::new().fg(NuColor::Magenta).paint(right),
        rr = Style::new().fg(dim).paint("}─"),
    )
}

impl Prompt for StrykePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let count = self.cmd_count.lock().map(|g| *g).unwrap_or(0);
        let bar = render_status_bar(count);
        let prompt = Style::new()
            .fg(NuColor::Cyan)
            .bold()
            .paint("stryke")
            .to_string();
        Cow::Owned(format!("{}\n{}", bar, prompt))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        let s = Style::new()
            .fg(NuColor::LightCyan)
            .bold()
            .paint("❯ ")
            .to_string();
        Cow::Owned(s)
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        let s = Style::new()
            .fg(NuColor::DarkGray)
            .paint("····❯ ")
            .to_string();
        Cow::Owned(s)
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

/// Visible (printable) width of a string that may contain ANSI CSI escape
/// sequences. Counts every char outside the `ESC[...m` codes. Used by the
/// banner box renderer so colored content pads to the right border
/// regardless of how many invisible color toggles it carries.
/// Thin wrapper around `crate::banner::print_banner` kept for backwards
/// compatibility with existing `repl::print_cyberpunk_banner` callers
/// (REPL startup, `stryke --help`). The actual rendering lives in the
/// library so the `banner()` builtin can share the same source.
pub fn print_cyberpunk_banner() {
    crate::banner::print_banner(true);
}
/// Editor attempts before the REPL drops to plain lines. Each failed attempt
/// waits crossterm's 2 s for the cursor report, and a report that is late for
/// one query answers a later one, so a terminal up to ~2 s × this late recovers.
const EDITOR_ATTEMPTS: usize = 6;

/// Seconds to wait for the reports still owed when the editor gives up.
const DRAIN_BUDGET_SECS: u64 = 8;

/// `run` — see implementation.
pub fn run(cli: &Cli) {
    let mut interp = VMHelper::new();
    crate::cli::configure_interpreter(cli, &mut interp, "repl");

    // Show the same cyberpunk banner that `stryke --help` displays, so a
    // fresh REPL session looks like the rest of the CLI surface. Followed
    // by a single hint line so newcomers know how to leave the REPL.
    print_cyberpunk_banner();
    println!();
    println!("\x1b[2m  type `exit` or Ctrl-D to leave the REPL — Tab for completion\x1b[0m");
    println!();

    let prelude = crate::cli::module_prelude(cli);
    let static_words = build_static_completions();
    let dynamic = Arc::new(Mutex::new(interp.repl_completion_names()));
    let snapshot = Arc::new(Mutex::new(interp.repl_completion_snapshot()));
    let cmd_count = Arc::new(Mutex::new(0u64));

    let completer = StrykeCompleter {
        static_words,
        dynamic: Arc::clone(&dynamic),
        snapshot: Arc::clone(&snapshot),
    };

    let menu = ColumnarMenu::default()
        .with_name("completion_menu")
        .with_columns(4)
        .with_column_padding(2);

    // Mode (emacs/vi) comes from `~/.stryke/config.toml` `[repl] mode = ...`
    // or `STRYKE_REPL_MODE=vi` (env override). Menu navigation bindings
    // attach to the active insert-mode keymap so completion behaves the same
    // in either edit mode. Vi normal-mode keys (`h`/`j`/`k`/`l`, `dd`, etc.)
    // come from reedline's `default_vi_normal_keybindings()` and stay
    // untouched — only the insert map gains the menu shortcuts.
    let edit_mode: Box<dyn EditMode> = match resolve_repl_mode() {
        ReplMode::Emacs => {
            let mut kb = default_emacs_keybindings();
            install_menu_bindings(&mut kb);
            Box::new(Emacs::new(kb))
        }
        ReplMode::Vi => {
            let mut insert_kb = default_vi_insert_keybindings();
            install_menu_bindings(&mut insert_kb);
            let normal_kb = default_vi_normal_keybindings();
            Box::new(Vi::new(insert_kb, normal_kb))
        }
    };

    let history = match FileBackedHistory::with_file(5_000, history_path()) {
        Ok(h) => Box::new(h) as Box<dyn reedline::History>,
        Err(e) => {
            eprintln!("repl: history unavailable: {}", e);
            Box::new(FileBackedHistory::new(5_000).unwrap_or_else(|_| {
                eprintln!("repl: cannot create in-memory history");
                process::exit(1);
            })) as Box<dyn reedline::History>
        }
    };

    // `with_partial_completions(true)` made every Tab re-run
    // `can_partially_complete`, which re-inserted the longest common prefix
    // and re-ran the completer on each MenuNext — pinning the cursor near
    // the top of the menu. Disabled so Tab is a pure "next suggestion" hop.
    let mut line_editor = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(edit_mode)
        .with_history(history);

    let prompt = StrykePrompt {
        cmd_count: Arc::clone(&cmd_count),
    };

    // reedline draws through crossterm, and crossterm asks the terminal where
    // the cursor is (`ESC [ 6 n`) before it paints the first prompt. A terminal
    // that does not answer inside its window — one whose reply was consumed by
    // something else reading the tty, a multiplexer under load, an emulator
    // that does not implement the report — makes `read_line` fail with
    //
    //     The cursor position could not be read within a normal duration
    //
    // That used to end the REPL on the spot: banner, one error line, back to
    // the shell. A query the terminal was slow to answer is not a reason to
    // refuse to run a language.
    //
    // So a failure is retried up to `EDITOR_ATTEMPTS` times. Each retry sends a
    // fresh query, and an answer that was late for an earlier query satisfies
    // the one in flight, so a terminal that is merely slow recovers on its own.
    //
    // If every attempt fails, the answers still owed are not lost, only late:
    // left alone they reach the cooked tty and echo as `^[[43;1R`.
    // `drain_cursor_reports` swallows them silently; if any arrive the terminal
    // does answer and the editor gets one more try. Otherwise the REPL drops to
    // reading plain lines from stdin. No completion, no history keys, no menus,
    // but every other thing the REPL does still works, which is the difference
    // between a degraded prompt and no prompt at all. `read_plain_line` still
    // strips any report that straggles in later, and seeing one sends the REPL
    // back to the editor.
    let mut editor_failures = 0usize;
    let mut plain_input = false;
    let mut drained = false;

    loop {
        // Refresh `%main::` / `%Pkg::` so each prompt sees the current symbol
        // table (subs / `our` declarations added on the prior line).
        interp.refresh_package_stashes();

        if let Ok(mut g) = dynamic.lock() {
            *g = interp.repl_completion_names();
        }
        if let Ok(mut s) = snapshot.lock() {
            *s = interp.repl_completion_snapshot();
        }

        let sig = if plain_input {
            let (sig, terminal_answered) =
                read_plain_line(cmd_count.lock().map(|g| *g).unwrap_or(0));
            // A late cursor report in the line means the terminal does answer,
            // only slower than crossterm waits: try the editor again next prompt.
            if terminal_answered {
                plain_input = false;
                editor_failures = 0;
            }
            sig
        } else {
            match line_editor.read_line(&prompt) {
                Ok(s) => {
                    editor_failures = 0;
                    s
                }
                Err(e) => {
                    editor_failures += 1;
                    if editor_failures < EDITOR_ATTEMPTS {
                        continue;
                    }
                    // Every failed attempt left one query unanswered. Swallow
                    // the answers silently; if any come, the terminal works and
                    // is only slow, so the editor gets one more try.
                    #[cfg(unix)]
                    if !drained {
                        drained = true;
                        let arrived = drain_cursor_reports(
                            0,
                            editor_failures,
                            std::time::Duration::from_secs(DRAIN_BUDGET_SECS),
                        );
                        if arrived > 0 {
                            editor_failures = 0;
                            continue;
                        }
                    }
                    eprintln!("repl: {}", e);
                    eprintln!("repl: line editor unavailable — reading plain lines instead");
                    plain_input = true;
                    continue;
                }
            }
        };

        match sig {
            Signal::Success(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let low = trimmed.to_lowercase();
                if low == "exit" || low == "quit" {
                    break;
                }

                if let Ok(mut g) = cmd_count.lock() {
                    *g += 1;
                }

                let full = format!("{}{}", prelude, trimmed);
                let program = match crate::parse(&full) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("{}", e);
                        continue;
                    }
                };

                match interp.execute(&program) {
                    Ok(v) => {
                        if !v.is_undef() {
                            println!("{}", v);
                        }
                    }
                    Err(e) => match e.kind {
                        ErrorKind::Exit(code) => process::exit(code),
                        ErrorKind::Die => {
                            eprint!("{}", e);
                        }
                        _ => eprintln!("{}", e),
                    },
                }
            }
            Signal::CtrlC => {
                continue;
            }
            Signal::CtrlD => break,
            _ => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn late_cursor_reports_are_stripped_from_a_plain_line() {
        // What a cooked tty delivers after three timed-out queries and `p 42`.
        let (line, saw) = strip_cursor_reports("\x1b[48;1R\x1b[48;1R\x1b[48;1Rp 42");
        assert_eq!(line, "p 42");
        assert!(saw);
    }

    #[test]
    fn other_escapes_and_plain_text_are_left_alone() {
        for s in [
            "p 42",
            "\x1b[31mred",
            "\x1b[48R",
            "\x1b[;1R",
            "\x1b[48;R",
            "say \"\x1b[\"",
        ] {
            assert_eq!(strip_cursor_reports(s), (s.to_string(), false), "{s:?}");
        }
        // A report in the middle still goes; the text around it stays put.
        assert_eq!(
            strip_cursor_reports("a\x1b[1;80Rb"),
            ("ab".to_string(), true)
        );
    }

    #[test]
    fn split_counts_every_report_and_keeps_the_rest() {
        let (rest, n) = split_cursor_reports(b"a\x1b[43;1Rb\x1b[43;1R\x1b[3");
        assert_eq!((rest.as_slice(), n), (&b"ab\x1b[3"[..], 2));
    }

    /// A pty master/slave pair, so the drain can be driven without a terminal.
    #[cfg(unix)]
    fn open_pty() -> (i32, i32) {
        unsafe {
            let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(master >= 0 && libc::grantpt(master) == 0 && libc::unlockpt(master) == 0);
            let name = libc::ptsname(master);
            assert!(!name.is_null());
            let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
            assert!(slave >= 0);
            (master, slave)
        }
    }

    #[cfg(unix)]
    #[test]
    fn drain_swallows_late_reports_without_echo_and_restores_the_tty() {
        let (master, slave) = open_pty();
        let before = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            libc::tcgetattr(slave, &mut t);
            t
        };
        assert_ne!(before.c_lflag & libc::ECHO, 0, "pty starts cooked");

        // Two owed reports, with a keystroke typed in between, arriving after
        // the drain has started waiting — as late answers do.
        let writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let input = b"\x1b[43;1Rx\x1b[43;1R";
            unsafe { libc::write(master, input.as_ptr().cast(), input.len()) };
        });

        TYPED_DURING_DRAIN.lock().unwrap().clear();
        let seen = drain_cursor_reports(slave, 2, std::time::Duration::from_secs(5));
        writer.join().unwrap();
        assert_eq!(seen, 2);
        assert_eq!(TYPED_DURING_DRAIN.lock().unwrap().as_slice(), b"x");

        // Echo would have written the bytes back to the master side.
        let mut echoed = [0u8; 64];
        let flags = unsafe { libc::fcntl(master, libc::F_GETFL) };
        unsafe { libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        let n = unsafe { libc::read(master, echoed.as_mut_ptr().cast(), echoed.len()) };
        assert!(n <= 0, "drain echoed {n} bytes back to the terminal");

        let after = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            libc::tcgetattr(slave, &mut t);
            t
        };
        // PENDIN is set by the kernel itself after a tcsetattr with input queued.
        let user_flags = !libc::PENDIN;
        assert_eq!(
            after.c_lflag & user_flags,
            before.c_lflag & user_flags,
            "termios not restored"
        );
        unsafe {
            libc::close(slave);
            libc::close(master);
        }
        TYPED_DURING_DRAIN.lock().unwrap().clear();
    }

    #[cfg(unix)]
    #[test]
    fn drain_gives_up_at_the_budget_when_no_report_comes() {
        let (master, slave) = open_pty();
        let start = std::time::Instant::now();
        let seen = drain_cursor_reports(slave, 1, std::time::Duration::from_millis(300));
        assert_eq!(seen, 0);
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        unsafe {
            libc::close(slave);
            libc::close(master);
        }
    }

    #[test]
    fn arrow_method_completion_uses_blessed_class_and_subs() {
        let state = ReplCompletionSnapshot {
            subs: vec!["Pkg::foo".to_string()],
            blessed_scalars: HashMap::from([("o".to_string(), "Pkg".to_string())]),
            ..Default::default()
        };
        let line = "$o->f";
        let (start, methods) =
            repl_arrow_method_completions(&state, line, line.len()).expect("arrow context");
        assert_eq!(start, 4);
        assert!(methods.iter().any(|m| m == "foo"));
    }

    #[test]
    fn completion_word_at_cursor_includes_sigil() {
        let s = "print $foo";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(st, 6);
        assert_eq!(pre, "$foo");
    }

    #[test]
    fn completion_start_of_word_after_space_before_sigil() {
        let s = "my $x";
        let (st, pre) = completion_word_start(s, 3);
        assert_eq!(st, 3);
        assert_eq!(pre, "");
    }

    #[test]
    fn static_completions_include_lsp_words() {
        let v = build_static_completions();
        assert!(v.iter().any(|w| w == "abs"));
        assert!(v.iter().any(|w| w == "uniq"));
        assert!(v.iter().any(|w| w == "sha256"));
        assert!(v.iter().any(|w| w == "base64_encode"));
    }

    #[test]
    fn static_completions_include_dispatch_aliases() {
        // Regression guard for `pin` / `faf` (aliases of `fire_and_forget`).
        // Source: lsp_completion_words.txt regenerated from runtime `%all`,
        // which exports every callable spelling from BUILTIN_ARMS. If this
        // test ever fails, the txt drifted from %all — regenerate it.
        let v = build_static_completions();
        assert!(v.iter().any(|w| w == "pin"), "pin missing from completion");
        assert!(v.iter().any(|w| w == "faf"), "faf missing from completion");
        assert!(
            v.iter().any(|w| w == "fire_and_forget"),
            "fire_and_forget missing from completion"
        );
    }
}
