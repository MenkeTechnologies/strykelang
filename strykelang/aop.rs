//! Aspect-oriented programming primitives for stryke.
//!
//! Mirrors the design of [zshrs's `intercept`](../../zshrs/src/exec.rs) (call-time advice
//! on user subs, glob pointcuts, `before`/`after`/`around` kinds, `proceed()` for around).
//! See `Interpreter::run_intercepts_for_call` and `Op::RegisterAdvice`.
//!
//! Surface (parsed by `parser::parse_advice_decl`):
//! ```ignore
//! before "<glob>" { ... }   # run before the matched sub; sees $INTERCEPT_NAME, @INTERCEPT_ARGS
//! after  "<glob>" { ... }   # run after; sees $INTERCEPT_MS, $INTERCEPT_US, $? (retval)
//! around "<glob>" { ... }   # wrap; must call proceed() to invoke the original
//! ```

use crate::ast::{AdviceKind, Block};

/// One AOP advice record. Stored in `Interpreter::intercepts`.
#[derive(Debug, Clone)]
pub struct Intercept {
    /// Auto-incremented removal id (1-based).
    pub id: u32,
    pub kind: AdviceKind,
    /// Glob pointcut matched against the called sub's bare name.
    pub pattern: String,
    /// Advice body (parsed AST; executed via `Interpreter::exec_block`).
    pub body: Block,
}

/// Per-call advice context — pushed when entering an `around` block, popped on exit.
/// Read by the `proceed` builtin to invoke the original sub with saved args.
#[derive(Debug, Clone)]
pub struct InterceptCtx {
    pub name: String,
    pub args: Vec<crate::value::PerlValue>,
    /// Set true when `proceed()` runs the original.
    pub proceeded: bool,
    /// Captured return value of the original after `proceed()`.
    pub retval: crate::value::PerlValue,
}

/// Glob match: `*` (any sequence), `?` (any one char), other chars literal.
/// Mirrors zshrs's `intercept_matches` (exec.rs:3723-3739) — minimal POSIX-glob subset.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == name {
        return true;
    }
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], s: &[u8]) -> bool {
    // Iterative backtracking matcher (no regex dep, no recursion blowup).
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star_pi, mut star_si): (Option<usize>, usize) = (None, 0);
    while si < s.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = Some(pi);
            star_si = si;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn exact() {
        assert!(glob_match("foo", "foo"));
        assert!(!glob_match("foo", "bar"));
    }

    #[test]
    fn star() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("foo*", "foobar"));
        assert!(glob_match("*bar", "foobar"));
        assert!(glob_match("f*r", "foobar"));
        assert!(!glob_match("foo*", "barfoo"));
    }

    #[test]
    fn question() {
        assert!(glob_match("f?o", "foo"));
        assert!(!glob_match("f?o", "fxxo"));
    }

    #[test]
    fn empty() {
        assert!(glob_match("", ""));
        assert!(glob_match("*", ""));
        assert!(!glob_match("", "x"));
    }
}
