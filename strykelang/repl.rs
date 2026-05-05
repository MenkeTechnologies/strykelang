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
//!   `stryke::lsp::builtin_completion_words` plus the live interpreter
//!   binding/sub names — the same wordlist the LSP serves.
//! * History is `~/.stryke_history` via `FileBackedHistory`.
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
    default_emacs_keybindings, ColumnarMenu, Completer, Emacs, FileBackedHistory, KeyCode,
    KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion,
};

use crate::Cli;
use stryke::error::ErrorKind;
use stryke::vm_helper::{repl_arrow_method_completions, VMHelper, ReplCompletionSnapshot};
use stryke::lsp::builtin_completion_words;
use stryke::token::KEYWORDS;

/// Builtin names not yet captured in `lsp_completion_words.txt`.
const EXTRA_KEYWORDS: &[&str] = &["deque", "heap", "ppool", "barrier", "bench", "spawn"];

const STRYKE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn history_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".stryke_history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".stryke_history"))
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

pub fn run(cli: &Cli) {
    let mut interp = VMHelper::new();
    crate::configure_interpreter(cli, &mut interp, "repl");

    let prelude = crate::module_prelude(cli);
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

    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );

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

    let mut line_editor = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(Box::new(Emacs::new(keybindings)))
        .with_history(history)
        .with_partial_completions(true);

    let prompt = StrykePrompt {
        cmd_count: Arc::clone(&cmd_count),
    };

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

        let sig = match line_editor.read_line(&prompt) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("repl: {}", e);
                break;
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
                let program = match stryke::parse(&full) {
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
