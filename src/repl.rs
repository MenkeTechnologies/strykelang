//! Interactive REPL for the `pe` binary (readline, history, tab-completion).

use std::process;
use std::sync::{Arc, Mutex};

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};

use crate::Cli;
use perlrs::error::ErrorKind;
use perlrs::interpreter::{repl_arrow_method_completions, Interpreter, ReplCompletionSnapshot};
use perlrs::token::KEYWORDS;

/// Extra builtin names not listed in [`perlrs::token::KEYWORDS`].
const EXTRA_KEYWORDS: &[&str] = &["deque", "heap", "ppool", "barrier", "bench", "spawn"];

fn history_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".perlrs_history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".perlrs_history"))
}

fn build_static_completions() -> Vec<String> {
    let mut v: Vec<String> = KEYWORDS
        .iter()
        .chain(EXTRA_KEYWORDS.iter())
        .map(|s| (*s).to_string())
        .collect();
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

struct ReplHelper {
    static_words: Vec<String>,
    dynamic: Arc<Mutex<Vec<String>>>,
    snapshot: Arc<Mutex<ReplCompletionSnapshot>>,
    file: FilenameCompleter,
}

impl ReplHelper {
    fn word_pairs(&self, prefix: &str) -> rustyline::Result<Vec<Pair>> {
        let dyn_list = self.dynamic.lock().map_err(|e| {
            rustyline::error::ReadlineError::Io(std::io::Error::other(format!(
                "completion lock: {e}"
            )))
        })?;
        let mut m: Vec<Pair> = self
            .static_words
            .iter()
            .chain(dyn_list.iter())
            .filter(|w| w.starts_with(prefix))
            .map(|w| Pair {
                display: w.clone(),
                replacement: w.clone(),
            })
            .collect();
        m.sort_by(|a, b| a.display.cmp(&b.display));
        m.dedup_by(|a, b| a.display == b.display);
        Ok(m)
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if let Ok(g) = self.snapshot.lock() {
            if let Some((start, methods)) = repl_arrow_method_completions(&g, line, pos) {
                let mut pairs: Vec<Pair> = methods
                    .into_iter()
                    .map(|m| Pair {
                        display: m.clone(),
                        replacement: m,
                    })
                    .collect();
                pairs.sort_by(|a, b| a.display.cmp(&b.display));
                pairs.dedup_by(|a, b| a.display == b.display);
                return Ok((start, pairs));
            }
        }

        let (start, prefix) = completion_word_start(line, pos);
        if prefix.starts_with('$') || prefix.starts_with('@') || prefix.starts_with('%') {
            return Ok((start, self.word_pairs(prefix)?));
        }

        let mut pairs = self.word_pairs(prefix)?;

        if let Ok((f_start, fpairs)) = self.file.complete_path(line, pos) {
            if !fpairs.is_empty() {
                if f_start == start {
                    pairs.extend(fpairs);
                } else if pairs.is_empty() {
                    return Ok((f_start, fpairs));
                }
            }
        }

        pairs.sort_by(|a, b| a.display.cmp(&b.display));
        pairs.dedup_by(|a, b| a.display == b.display);
        Ok((start, pairs))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Highlighter for ReplHelper {
    fn highlight_char(&self, _: &str, _: usize, _: bool) -> bool {
        false
    }
}

impl Validator for ReplHelper {}

impl Helper for ReplHelper {}

pub fn run(cli: &Cli) {
    let mut interp = Interpreter::new();
    crate::configure_interpreter(cli, &mut interp, "repl");

    let prelude = crate::module_prelude(cli);
    let static_words = build_static_completions();
    let dynamic = Arc::new(Mutex::new(interp.repl_completion_names()));
    let snapshot = Arc::new(Mutex::new(interp.repl_completion_snapshot()));

    let helper = ReplHelper {
        static_words,
        dynamic: Arc::clone(&dynamic),
        snapshot: Arc::clone(&snapshot),
        file: FilenameCompleter::new(),
    };

    let config = Config::builder().history_ignore_space(true).build();

    let mut rl = match Editor::with_config(config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("repl: cannot create readline: {}", e);
            process::exit(1);
        }
    };
    rl.set_helper(Some(helper));

    let hist = history_path();
    if hist.exists() {
        let _ = rl.load_history(&hist);
    }

    loop {
        if let Ok(mut g) = dynamic.lock() {
            *g = interp.repl_completion_names();
        }
        if let Ok(mut s) = snapshot.lock() {
            *s = interp.repl_completion_snapshot();
        }

        let read = rl.readline("perl> ");
        match read {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let low = trimmed.to_lowercase();
                if low == "exit" || low == "quit" {
                    break;
                }

                let _ = rl.add_history_entry(trimmed);

                let full = format!("{}{}", prelude, trimmed);
                let program = match perlrs::parse(&full) {
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
            Err(ReadlineError::Interrupted) => {
                println!("^C");
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("repl: {}", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&hist);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_method_completion_uses_blessed_class_and_subs() {
        let mut state = ReplCompletionSnapshot::default();
        state.subs = vec!["Pkg::foo".to_string()];
        state
            .blessed_scalars
            .insert("o".to_string(), "Pkg".to_string());
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
}
