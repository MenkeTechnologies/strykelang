//! Interactive REPL for the `pe` binary (readline, history, tab-completion).

use std::path::PathBuf;
use std::process;
use std::sync::{Arc, Mutex};

use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};

use crate::Cli;
use perlrs::error::ErrorKind;
use perlrs::interpreter::Interpreter;
use perlrs::token::KEYWORDS;
use perlrs::value::PerlValue;

/// Extra builtin names not listed in [`KEYWORDS`](perlrs::token::KEYWORDS).
const EXTRA_KEYWORDS: &[&str] = &["deque", "heap", "ppool", "barrier"];

fn history_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".perlrs_history"))
        .unwrap_or_else(|| PathBuf::from(".perlrs_history"))
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
fn line_before(line: &str, pos: usize) -> (usize, &str) {
    let pos = pos.min(line.len());
    let before = line.get(..pos).unwrap_or("");
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| {
            c.is_whitespace() || matches!(c, '(' | ',' | ';' | '[' | '{' | '|')
        })
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    (start, line.get(start..pos).unwrap_or(""))
}

struct ReplHelper {
    static_words: Vec<String>,
    dynamic: Arc<Mutex<Vec<String>>>,
}

impl Completer for ReplHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        let (start, prefix) = line_before(line, pos);
        let dyn_list = self.dynamic.lock().map_err(|e| {
            rustyline::error::ReadlineError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("completion lock: {e}"),
            ))
        })?;
        let mut m: Vec<String> = self
            .static_words
            .iter()
            .chain(dyn_list.iter())
            .filter(|w| w.starts_with(prefix))
            .cloned()
            .collect();
        m.sort();
        m.dedup();
        Ok((start, m))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(
        &self,
        _line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> Option<String> {
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

    let helper = ReplHelper {
        static_words,
        dynamic: Arc::clone(&dynamic),
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
                        if !matches!(v, PerlValue::Undef) {
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
    use super::line_before;

    #[test]
    fn line_before_word_at_cursor() {
        let s = "print $foo";
        let (st, pre) = line_before(s, s.len());
        assert_eq!(st, 6);
        assert_eq!(pre, "$foo");
    }

    #[test]
    fn line_before_start_of_word_after_space() {
        let s = "my $x";
        // Cursor at `$` (byte 3): prefix is empty; completion can match `$x` etc.
        let (st, pre) = line_before(s, 3);
        assert_eq!(st, 3);
        assert_eq!(pre, "");
    }
}
