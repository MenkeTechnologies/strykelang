//! Render raw hover-doc markdown (from `lsp::doc_text_for`) into a terminal
//! display string. Same visual surface as the `stryke docs` interactive
//! browser — bold topic heading, dim rule, cyan inline `backticks`, green
//! ```code fences``` — but without the pager chrome (header/footer banner,
//! page navigation hints), so a single doc page can be returned by the
//! `docs(TOPIC)` builtin and printed inline at the REPL.
//!
//! Colors are conditional: callers pass `colored=true` only when stdout is
//! a TTY so piped / file-redirected output stays clean.

const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_RESET: &str = "\x1b[0m";

/// Render a doc page (heading + rule + body) for terminal display.
/// `text` is the raw markdown returned by `lsp::doc_text_for`. When
/// `colored` is false, ANSI sequences are stripped to empty strings so
/// the output is plain UTF-8 suitable for files / pipes / non-tty sinks.
pub fn render_doc(topic: &str, text: &str, colored: bool) -> String {
    let (c, g, d, n) = if colored {
        (ANSI_CYAN, ANSI_GREEN, ANSI_DIM, ANSI_RESET)
    } else {
        ("", "", "", "")
    };
    let rule_len = topic.chars().count().max(20).min(76);
    let mut out = String::with_capacity(text.len() + 256);
    out.push_str(&format!("{c}{topic}{n}\n"));
    out.push_str(&format!("{d}{}{n}\n", "─".repeat(rule_len)));
    let mut in_code = false;
    for line in text.split('\n') {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str(&format!("{g}  {line}{n}\n"));
        } else if line.trim().is_empty() {
            out.push('\n');
        } else {
            out.push_str(&render_inline_code(line, c, n));
            out.push('\n');
        }
    }
    out
}

/// Replace `\`backtick\`` spans with `color`-prefixed / `reset`-suffixed
/// runs. Unmatched trailing backticks pass through unchanged so doc text
/// containing literal `` ` `` (e.g. shell snippets) doesn't corrupt the
/// terminal state.
fn render_inline_code(line: &str, color: &str, reset: &str) -> String {
    let mut out = String::with_capacity(line.len() + 64);
    let mut in_tick = false;
    for ch in line.chars() {
        if ch == '`' {
            if in_tick {
                out.push_str(reset);
            } else {
                out.push_str(color);
            }
            in_tick = !in_tick;
        } else {
            out.push(ch);
        }
    }
    if in_tick {
        out.push_str(reset);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_mode_strips_ansi() {
        let r = render_doc("pmap", "Hello `world`", false);
        assert!(!r.contains('\x1b'), "plain mode must not emit ANSI: {:?}", r);
        assert!(r.contains("pmap"));
        // Backticks are consumed by the inline-code renderer; in plain
        // mode the color/reset wrappers collapse to empty strings, so the
        // literal `word` segment stays in the output without its delimiters.
        assert!(r.contains("Hello world"), "body text preserved: {:?}", r);
    }

    #[test]
    fn colored_mode_wraps_inline_ticks() {
        let r = render_doc("pmap", "Hello `world`", true);
        assert!(r.contains("\x1b[36m"), "colored must emit cyan: {:?}", r);
        assert!(r.contains("\x1b[0m"), "colored must reset: {:?}", r);
    }

    #[test]
    fn code_fence_lines_render_green() {
        let r = render_doc("pmap", "intro\n```perl\nmy $x = 1\n```\nafter", true);
        assert!(r.contains("\x1b[32m"), "fenced block must emit green: {:?}", r);
        assert!(!r.contains("```"), "fence markers must be hidden: {:?}", r);
    }
}
