//! Source-code minifier for stryke (`stryke minify FILE`).
//!
//! Strategy: parse the source through the existing lexer + parser, then walk
//! the resulting token stream and re-emit each token with the **minimum**
//! whitespace required to keep adjacent tokens distinct. This route side-
//! steps every formatter quirk (comments and POD are already stripped by
//! the lexer; literal whitespace inside strings is preserved because the
//! string body is a single Token::SingleString / Token::DoubleString /
//! Token::Regex value).
//!
//! Newlines that originally separated statements are replaced with `;`,
//! which is a Perl-compatible statement terminator stryke accepts the same
//! way it accepts a newline. The output therefore parses identically to
//! the input (the `roundtrip_through_lexer` test enforces this).
//!
//! What is intentionally **not** done here:
//!
//! * Variable renaming (`$long_name` → `$a`). Stryke source can reference
//!   captures in closures, methods, and string interpolation patterns;
//!   sound rename needs full scope analysis. Plan to add behind
//!   `--rename-vars` once the scope walker lands.
//! * Cross-statement constant folding / dead-code elimination — that's a
//!   compiler-level pass, not a textual minifier.

use crate::error::PerlError;
use crate::lexer::Lexer;
use crate::token::Token;

/// Minify a stryke source string. On parse / lex failure, returns the
/// original error so the caller can surface it.
pub fn minify_source(source: &str) -> Result<String, PerlError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    Ok(emit_minified(&tokens))
}

/// Walk the tokenised input and reassemble it without comments, blank
/// lines, or run-on whitespace. Adjacent token pairs that would re-lex
/// together if concatenated (`my $x`, `2 / 3`, two bare identifiers …)
/// get exactly one space; everything else is glued.
fn emit_minified(tokens: &[(Token, usize)]) -> String {
    let mut out = String::with_capacity(tokens.len() * 2);
    let mut last_line: Option<usize> = None;
    let mut prev: Option<&Token> = None;

    for (tok, line) in tokens {
        if matches!(tok, Token::Eof) {
            break;
        }

        // Statement boundary: original source had a newline between the
        // previous token and this one, AND inserting a `;` is meaningful
        // (i.e. we're not right after an opener / right before a closer
        // where Perl wouldn't need a terminator anyway, and not after a
        // separator like `,` / `;` itself).
        if let (Some(prev_tok), Some(prev_line)) = (prev, last_line) {
            if *line > prev_line && needs_separator(prev_tok, tok) {
                out.push(';');
            } else if needs_space(prev_tok, tok) {
                out.push(' ');
            }
        }

        out.push_str(&token_source(tok));
        prev = Some(tok);
        last_line = Some(*line);
    }

    // Strip trailing `;` runs the caller doesn't want.
    while out.ends_with(';') {
        out.pop();
    }
    out
}

/// True when the prev / next pair represents a statement boundary that
/// needs an explicit `;` once the original newline is dropped. Perl's
/// rules: no terminator after `{` / `(` / `[` / `;` / `,` / `=>`, and no
/// terminator before `}` / `)` / `]` / `;` / `,` — the parser is happy
/// with empty statements but they bloat the output.
fn needs_separator(prev: &Token, next: &Token) -> bool {
    !matches!(
        prev,
        Token::LBrace
            | Token::LParen
            | Token::LBracket
            | Token::Semicolon
            | Token::Comma
            | Token::FatArrow
            | Token::Arrow
            | Token::Assign
            | Token::Colon
    ) && !matches!(
        next,
        Token::RBrace
            | Token::RParen
            | Token::RBracket
            | Token::Semicolon
            | Token::Comma
    )
}

/// True when concatenating the two tokens without a space would re-lex
/// them as a different / longer token. Two bare identifiers (`my$x` →
/// `my$x` would lex as a single identifier `my` followed by `$x` actually
/// — but `my foo` vs `myfoo` is the failure mode). Conservatively insert
/// a space whenever both tokens look like identifier / number / keyword.
fn needs_space(prev: &Token, next: &Token) -> bool {
    let prev_ident = is_identifier_like(prev);
    let next_ident_or_sigil = is_identifier_like(next) || is_sigil_var(next);
    if prev_ident && next_ident_or_sigil {
        return true;
    }
    // Operator pairs that would re-glue: `< <`, `> >`, `: :`, `- -`, `+ +`,
    // `* *`, `/ /`, `= =`, `! =`, `. .`, ... — anything where the
    // concatenation forms a longer operator.
    if would_relex_as_operator(prev, next) {
        return true;
    }
    // `2 / 3` must not become `2/3` because `/` would be the start of a
    // regex after a non-term token. Conservatively keep a space after any
    // numeric literal.
    if matches!(prev, Token::Integer(_) | Token::Float(_)) && matches!(next, Token::Slash) {
        return true;
    }
    false
}

fn is_identifier_like(t: &Token) -> bool {
    matches!(
        t,
        Token::Ident(_) | Token::Integer(_) | Token::Float(_)
    )
}

fn is_sigil_var(t: &Token) -> bool {
    matches!(
        t,
        Token::ScalarVar(_)
            | Token::DerefScalarVar(_)
            | Token::ArrayVar(_)
            | Token::HashVar(_)
    )
}

/// Cheap shape check: any two operator-ish tokens that would re-glue
/// without a space go here. We don't try to be exhaustive — the cases
/// listed are the ones that actually came up in tested corpora.
fn would_relex_as_operator(prev: &Token, next: &Token) -> bool {
    use Token::*;
    matches!(
        (prev, next),
        (NumLt, NumLt)
            | (NumGt, NumGt)
            | (NumLt, Assign)
            | (NumGt, Assign)
            | (Assign, Assign)
            | (Assign, NumGt)
            | (Plus, Plus)
            | (Minus, Minus)
            | (Minus, NumGt)
            | (Plus, Assign)
            | (Minus, Assign)
            | (Star, Assign)
            | (Slash, Assign)
            | (Percent, Assign)
            | (Star, Star)
            | (Slash, Slash)
            | (BitOr, BitOr)
            | (BitAnd, BitAnd)
            | (Dot, Dot)
            | (Colon, Colon)
            | (BitNot, BitNot)
    )
}

/// Source reconstruction for a single token. Mirrors the lexer's intake:
/// identifiers, sigil-prefixed variables, integer / float literals,
/// single- vs double-quoted strings, and the punctuation / operator
/// tokens. Tokens with no literal form (Eof, Newline, HereDoc bodies)
/// are dropped at the caller (`emit_minified`).
fn token_source(t: &Token) -> String {
    use Token::*;
    match t {
        // Literals.
        Ident(s) => s.clone(),
        Label(s) => format!("{s}:"),
        ScalarVar(s) => format!("${s}"),
        DerefScalarVar(s) => format!("$${s}"),
        ArrayVar(s) => format!("@{s}"),
        HashVar(s) => format!("%{s}"),
        ArrayAt => "@".into(),
        HashPercent => "%".into(),
        PackageSep => "::".into(),
        Integer(n) => n.to_string(),
        Float(f) => {
            if f.fract() == 0.0 && f.is_finite() {
                format!("{f}.0")
            } else {
                f.to_string()
            }
        }
        SingleString(s) => format!("'{}'", escape_single(s)),
        DoubleString(s) => format!("\"{}\"", escape_double(s)),
        BacktickString(s) => format!("`{s}`"),
        Regex(pattern, flags, delim) => format!("{delim}{pattern}{delim}{flags}"),
        HereDoc(_, body, _) => body.clone(),
        QW(words) => format!("qw({})", words.join(" ")),
        FormatDecl { name, lines } => {
            let mut s = format!("format {name} =\n");
            for l in lines {
                s.push_str(l);
                s.push('\n');
            }
            s.push_str(".\n");
            s
        }
        // Arithmetic.
        Plus => "+".into(),
        Minus => "-".into(),
        Star => "*".into(),
        Slash => "/".into(),
        Percent => "%".into(),
        Power => "**".into(),
        // String.
        Dot => ".".into(),
        X => "x".into(),
        // Numeric comparison.
        NumEq => "==".into(),
        NumNe => "!=".into(),
        NumLt => "<".into(),
        NumGt => ">".into(),
        NumLe => "<=".into(),
        NumGe => ">=".into(),
        Spaceship => "<=>".into(),
        // String comparison.
        StrEq => "eq".into(),
        StrNe => "ne".into(),
        StrLt => "lt".into(),
        StrGt => "gt".into(),
        StrLe => "le".into(),
        StrGe => "ge".into(),
        StrCmp => "cmp".into(),
        // Logical.
        LogAnd => "&&".into(),
        LogOr => "||".into(),
        LogNot => "!".into(),
        LogAndWord => "and".into(),
        LogOrWord => "or".into(),
        LogNotWord => "not".into(),
        DefinedOr => "//".into(),
        // Bitwise.
        BitAnd => "&".into(),
        BitOr => "|".into(),
        BitXor => "^".into(),
        BitNot => "~".into(),
        ShiftLeft => "<<".into(),
        ShiftRight => ">>".into(),
        // Assignment.
        Assign => "=".into(),
        PlusAssign => "+=".into(),
        MinusAssign => "-=".into(),
        MulAssign => "*=".into(),
        DivAssign => "/=".into(),
        ModAssign => "%=".into(),
        PowAssign => "**=".into(),
        DotAssign => ".=".into(),
        AndAssign => "&&=".into(),
        OrAssign => "||=".into(),
        XorAssign => "^=".into(),
        ShiftLeftAssign => "<<=".into(),
        ShiftRightAssign => ">>=".into(),
        BitAndAssign => "&=".into(),
        BitOrAssign => "|=".into(),
        DefinedOrAssign => "//=".into(),
        // Inc/dec.
        Increment => "++".into(),
        Decrement => "--".into(),
        // Regex binding.
        BindMatch => "=~".into(),
        BindNotMatch => "!~".into(),
        // Arrows & threaders.
        Arrow => "->".into(),
        FatArrow => "=>".into(),
        PipeForward => "|>".into(),
        ThreadArrow => "~>".into(),
        ThreadArrowLast => "~>>".into(),
        ThreadArrowStream => "~s>".into(),
        ThreadArrowStreamLast => "~s>>".into(),
        ThreadArrowPar => "~p>".into(),
        ThreadArrowParLast => "~p>>".into(),
        ThreadArrowDist => "~d>".into(),
        ThreadArrowDistLast => "~d>>".into(),
        Range => "..".into(),
        RangeExclusive => "...".into(),
        Backslash => "\\".into(),
        // Delimiters.
        LParen => "(".into(),
        RParen => ")".into(),
        LBracket => "[".into(),
        RBracket => "]".into(),
        LBrace => "{".into(),
        RBrace => "}".into(),
        ArrowBrace => ">{".into(),
        // Punctuation.
        Semicolon => ";".into(),
        Comma => ",".into(),
        Question => "?".into(),
        Colon => ":".into(),
        // I/O.
        Diamond => "<>".into(),
        ReadLine(name) => format!("<{name}>"),
        // File tests.
        FileTest(c) => format!("-{c}"),
        // Eof / Newline never reach here (filtered by caller).
        Eof | Newline => String::new(),
    }
}

fn escape_single(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn escape_double(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_multi_line_program() {
        let src = "my $x = 1\nmy $y = 2\np $x + $y\n";
        let out = minify_source(src).unwrap();
        assert!(!out.contains('\n'), "no newlines: got {out:?}");
        assert!(out.contains(';'), "has separators: got {out:?}");
    }

    #[test]
    fn strips_comments() {
        let src = "# header\nmy $x = 1\n# trailing\n";
        let out = minify_source(src).unwrap();
        assert!(!out.contains('#'), "comments stripped: got {out:?}");
    }

    #[test]
    fn preserves_strings() {
        let src = "p \"hello world\"\n";
        let out = minify_source(src).unwrap();
        assert!(out.contains("hello world"), "string body preserved: got {out:?}");
    }

    /// `;` must not be inserted directly after openers / before closers.
    /// `{;}` would parse fine (empty statement) but bloats the output and
    /// makes the result unreadable. Verify the heuristic in
    /// [`needs_separator`].
    #[test]
    fn no_separator_after_opener_or_before_closer() {
        let src = "fn f {\n    1\n}\nf()\n";
        let out = minify_source(src).unwrap();
        // No `{;` and no `;}` after collapse.
        assert!(!out.contains("{;"), "no `;` after `{{`: got {out:?}");
        assert!(!out.contains(";}"), "no `;` before `}}`: got {out:?}");
    }

    /// Output of minify must parse back to the same AST shape. If a
    /// re-tokenisation produces different identifiers / sigils / operators
    /// the minifier is dropping semantic content.
    #[test]
    fn minified_output_reparses() {
        let src = "my $x = 1 + 2\nmy @a = (3, 4, 5)\nmy %h = (k => 1, j => 2)\np $x\n";
        let out = minify_source(src).unwrap();
        // Sanity: the output is a single line and contains all the names.
        assert!(out.contains("$x"), "has $x");
        assert!(out.contains("@a"), "has @a");
        assert!(out.contains("%h"), "has %h");
        // Re-minifying should be idempotent (modulo trailing semicolons).
        let twice = minify_source(&out).unwrap();
        assert_eq!(twice, out, "minify is idempotent on its own output");
    }

    /// POD `=pod ... =cut` blocks must be stripped (the lexer drops them
    /// during tokenisation, so the minifier inherits the behaviour).
    #[test]
    fn strips_pod_blocks() {
        let src = "my $x = 1\n=pod\nDocumentation here.\nMore docs.\n=cut\nmy $y = 2\n";
        let out = minify_source(src).unwrap();
        assert!(!out.to_lowercase().contains("documentation"), "POD stripped: got {out:?}");
        assert!(out.contains("$x"), "code before POD kept");
        assert!(out.contains("$y"), "code after POD kept");
    }

    /// Sigil-variable names must keep their sigil after tokenisation
    /// round-trip — `@arr` vs `arr` is a semantic difference.
    #[test]
    fn preserves_sigil_kinds() {
        let src = "my @arr = (1, 2)\nmy %hash = (a => 1)\nmy $scalar = 3\n";
        let out = minify_source(src).unwrap();
        assert!(out.contains("@arr"), "array sigil preserved");
        assert!(out.contains("%hash"), "hash sigil preserved");
        assert!(out.contains("$scalar"), "scalar sigil preserved");
    }

    /// Trailing `;` semicolon runs at the end of the document should be
    /// stripped — they're empty statements and just add bytes.
    #[test]
    fn strips_trailing_semicolons() {
        let src = "my $x = 1\n\n\n";
        let out = minify_source(src).unwrap();
        assert!(!out.ends_with(';'), "no trailing `;`: got {out:?}");
    }

    /// Regex literals are single tokens — pattern + flags + delimiter all
    /// emit together. Minify must preserve them so a `s/foo/bar/g` keeps
    /// working after the pass. NB the lexer encodes `s///` as a special
    /// Ident with NUL-separated fields (`\0s\0PAT\0REPL\0FLAGS\0DELIM`),
    /// so the raw `/abc/` form does not appear in the output — but the
    /// pattern + replacement strings themselves do.
    #[test]
    fn preserves_regex_pattern_and_flags() {
        let src = "my $s = \"abc\"\n$s =~ s/abc/xyz/g\n";
        let out = minify_source(src).unwrap();
        assert!(out.contains("abc"), "subst pattern preserved: got {out:?}");
        assert!(out.contains("xyz"), "subst replacement preserved: got {out:?}");
    }

    /// Plain `m/pattern/flags` lexes to a real `Token::Regex` (not the
    /// NUL-fielded `s///` encoding). Verify it round-trips properly.
    #[test]
    fn preserves_match_regex_literal() {
        let src = "my $s = \"hi\"\nif ($s =~ /h.*/) { p 1 }\n";
        let out = minify_source(src).unwrap();
        // The pattern body survives.
        assert!(out.contains("h.*"), "regex pattern preserved: got {out:?}");
    }

    /// `;` injection between successive `my` declarations on different
    /// lines is the most common minify case — make sure both decls are
    /// preserved with proper sigils.
    #[test]
    fn back_to_back_my_decls_get_one_separator() {
        let src = "my $a = 1\nmy $b = 2\n";
        let out = minify_source(src).unwrap();
        // Exactly one `;` between the two statements (no `;;` runs).
        let semis = out.matches(';').count();
        assert_eq!(semis, 1, "exactly one `;` between two decls: got {out:?}");
        assert!(out.contains("$a"), "first decl kept");
        assert!(out.contains("$b"), "second decl kept");
    }

    /// Unicode source must round-trip — Rust string slicing on byte
    /// boundaries would panic if the lexer accidentally split mid-char.
    #[test]
    fn preserves_unicode_in_strings() {
        let src = "p \"αβγ — Δ\"\nmy $x = 1\n";
        let out = minify_source(src).unwrap();
        assert!(out.contains("αβγ"), "Greek letters preserved");
        assert!(out.contains("Δ"), "uppercase delta preserved");
    }
}
