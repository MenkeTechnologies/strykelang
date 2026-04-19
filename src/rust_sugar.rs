//! Source-level desugaring for `rust { ... }` FFI blocks.
//!
//! Why pre-lex and not a new `StmtKind`? The parity roadmap pins a rule: "Do not add new
//! `ExprKind`/`StmtKind` variants for new behavior." So `rust { ... }` is surfaced as a
//! BEGIN-wrapped builtin call — syntactic sugar only. At the source level we replace the
//! block with:
//!
//! ```text
//!     BEGIN { __forge_rust_compile(q‹SOURCE›, $LINE); }
//! ```
//!
//! so later phases (lexer / parser / interpreter) see normal Perl. The runtime builtin
//! [`crate::rust_ffi::compile_and_register`] handles rustc invocation, dlopen, and sub
//! registration.
//!
//! The scanner is intentionally simple: it treats `rust` as a keyword only when it sits
//! at a Perl statement boundary (start-of-file, or after `;`, `{`, `}`, or a newline not
//! inside a string). Inside the block body we use Rust's brace/string/comment rules.
//! False positives on exotic constructs fall through unchanged — the existing lexer then
//! reports the normal Perl error.

/// Rewrite `code` so every top-level `rust { ... }` statement becomes a BEGIN-wrapped call
/// into the FFI runtime. Returns the modified source, or the original if no block exists.
///
/// This is called by [`crate::parse_with_file`] before the lexer runs. It is also a no-op
/// safe function: any source not containing a `rust {` sequence is returned unchanged with
/// minimal overhead (one `memmem`-style scan).
pub fn desugar_rust_blocks(code: &str) -> String {
    // Fast path: no candidate substring → no work. Avoids full tokenization on every parse.
    if !code.contains("rust") {
        return code.to_string();
    }
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut can_start_stmt = true; // start-of-file is a statement boundary
    let mut line = 1usize;

    while i < bytes.len() {
        let c = bytes[i];

        // Perl-side skipping: stay out of strings, regex literals, and comments so we never
        // match `rust {` that is really inside `"...rust { ..."` or `# rust { ...`.
        // Note: newlines DO NOT imply a statement boundary in Perl — expressions routinely
        // span lines. Only `;`, `{`, `}` reset `can_start_stmt`.
        match c {
            b'\n' => {
                out.push('\n');
                i += 1;
                line += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                out.push(c as char);
                i += 1;
                continue;
            }
            b'#' => {
                // Perl line comment to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }
            b'"' | b'\'' | b'`' => {
                // Double- / single- / backtick-quoted string. Treat `\X` as an escape.
                let quote = c;
                out.push(c as char);
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < bytes.len() {
                        out.push(b as char);
                        out.push(bytes[i + 1] as char);
                        if bytes[i + 1] == b'\n' {
                            line += 1;
                        }
                        i += 2;
                        continue;
                    }
                    out.push(b as char);
                    i += 1;
                    if b == b'\n' {
                        line += 1;
                    }
                    if b == quote {
                        break;
                    }
                }
                can_start_stmt = false;
                continue;
            }
            b';' | b'{' | b'}' => {
                out.push(c as char);
                i += 1;
                can_start_stmt = true;
                continue;
            }
            _ => {}
        }

        // Candidate: identifier start?
        let is_ident_start = c.is_ascii_alphabetic() || c == b'_';
        if !is_ident_start {
            out.push(c as char);
            i += 1;
            can_start_stmt = false;
            continue;
        }

        // Read the identifier.
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let ident = &code[start..i];

        // Qualify: only `rust` at a statement boundary, with an opening `{` following after
        // optional whitespace. Anything else stays untouched.
        if ident == "rust" && can_start_stmt {
            // Peek past whitespace (newlines allowed) for `{`.
            let mut j = i;
            let mut inline_newlines = 0usize;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\r' | b'\n') {
                if bytes[j] == b'\n' {
                    inline_newlines += 1;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                // Found `rust { ... }` — scan the Rust body with Rust rules.
                if let Some((body, end)) = scan_rust_block(bytes, j) {
                    let block_line = line;
                    // Total newlines consumed across `rust`→`{` whitespace + Rust body +
                    // trailing `}`. We pad the replacement with the same number of `\n`
                    // characters so later parser diagnostics keep their source line number.
                    let body_newlines = body.bytes().filter(|&b| b == b'\n').count();
                    let total_newlines = inline_newlines + body_newlines;
                    out.push_str(&emit_begin_call(body, block_line));
                    for _ in 0..total_newlines {
                        out.push('\n');
                    }
                    line += total_newlines;
                    i = end;
                    can_start_stmt = true;
                    continue;
                }
                // Unbalanced — let the normal lexer report the error.
            }
        }

        out.push_str(ident);
        can_start_stmt = false;
    }
    out
}

/// Scan a Rust `{ ... }` block starting at the `{` byte at `open`. Returns `(body, end)`
/// where `body` excludes the outer braces and `end` is the byte index immediately after
/// the closing `}`. Handles string literals, raw strings (`r"..."` / `r#"..."#`),
/// character literals, line comments, and nested block comments per Rust's lexer.
fn scan_rust_block(bytes: &[u8], open: usize) -> Option<(&str, usize)> {
    debug_assert_eq!(bytes[open], b'{');
    let mut i = open + 1;
    let body_start = i;
    let mut depth: i32 = 1;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let body = std::str::from_utf8(&bytes[body_start..i]).ok()?;
                    return Some((body, i + 1));
                }
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment.
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Nested block comment.
                i += 2;
                let mut cdepth: i32 = 1;
                while i < bytes.len() && cdepth > 0 {
                    if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        cdepth += 1;
                        i += 2;
                    } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        cdepth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'"' => {
                // Rust string literal; `\"` escapes, `\\` escapes.
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if i + 1 < bytes.len() => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'r' if i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') => {
                // Raw string: `r"..."` or `r#"..."#` with any number of hashes.
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < bytes.len() && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j >= bytes.len() || bytes[j] != b'"' {
                    // Not a raw string start — treat as identifier.
                    i += 1;
                    continue;
                }
                j += 1;
                // Find closing `"` followed by exactly `hashes` hashes.
                while j < bytes.len() {
                    if bytes[j] == b'"' {
                        let mut k = j + 1;
                        let mut matched = 0;
                        while matched < hashes && k < bytes.len() && bytes[k] == b'#' {
                            matched += 1;
                            k += 1;
                        }
                        if matched == hashes {
                            j = k;
                            break;
                        }
                        j += 1;
                    } else {
                        j += 1;
                    }
                }
                i = j;
            }
            b'\'' => {
                // Char literal or lifetime. If followed by a valid char-literal body + `'`,
                // skip it. Otherwise treat as lifetime (no closing quote). Cheap heuristic:
                // advance past `\x` escapes and one ascii char, then check for `'`.
                let mut j = i + 1;
                if j < bytes.len() && bytes[j] == b'\\' && j + 1 < bytes.len() {
                    j += 2;
                    // `\u{...}`
                    if j < bytes.len() && bytes[j] == b'{' {
                        while j < bytes.len() && bytes[j] != b'}' {
                            j += 1;
                        }
                        if j < bytes.len() {
                            j += 1;
                        }
                    }
                } else if j < bytes.len() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'\'' {
                    i = j + 1;
                } else {
                    // Lifetime — skip the single quote and continue.
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Build the replacement Perl source for a `rust { BODY }` block. Uses `q<>` brackets with
/// an escape-proof `q«»` style delimiter to avoid colliding with the body content.
fn emit_begin_call(body: &str, line: usize) -> String {
    // Use an unusual 4-char delimiter very unlikely to appear in Rust source; the Perl lexer
    // accepts `q{…}` with nested braces, but we cannot guarantee Rust has balanced braces at
    // the q{} level, so base64-encode the body instead and pass as a plain double-quoted
    // string. That sidesteps all delimiter collision worries.
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    format!(
        "BEGIN {{ __forge_rust_compile(\"{}\", {}); }}",
        encoded, line
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rust_keyword_pass_through() {
        let src = "print 'hello';\n";
        assert_eq!(desugar_rust_blocks(src), src);
    }

    #[test]
    fn rust_keyword_in_string_is_not_expanded() {
        let src = "print \"rust { not a block }\";\n";
        assert_eq!(desugar_rust_blocks(src), src);
    }

    #[test]
    fn rust_keyword_in_comment_is_not_expanded() {
        let src = "# rust { not a block }\nprint 1;\n";
        assert_eq!(desugar_rust_blocks(src), src);
    }

    #[test]
    fn simple_rust_block_is_replaced_with_begin_call() {
        let src =
            "rust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\nprint add(1, 2);\n";
        let out = desugar_rust_blocks(src);
        assert!(out.contains("BEGIN"), "no BEGIN: {out}");
        assert!(out.contains("__forge_rust_compile"), "no builtin call");
        assert!(!out.contains("pub extern"), "Rust body leaked: {out}");
        assert!(out.contains("print add(1, 2);"));
    }

    #[test]
    fn rust_block_with_nested_braces_is_balanced() {
        let src = "rust { pub extern \"C\" fn f() -> i64 { let v = vec![1,2,3]; v.iter().sum::<i64>() } }";
        let out = desugar_rust_blocks(src);
        assert!(out.starts_with("BEGIN"));
        assert!(!out.contains("pub extern"), "Rust body leaked");
    }

    #[test]
    fn rust_block_with_string_containing_brace() {
        let src = "rust { pub extern \"C\" fn g() -> i64 { let s = \"}\"; s.len() as i64 } }";
        let out = desugar_rust_blocks(src);
        assert!(out.starts_with("BEGIN"), "desugar failed: {out}");
        assert!(!out.contains("pub extern"));
    }

    #[test]
    fn rust_block_with_raw_string() {
        let src = "rust { pub extern \"C\" fn h() -> i64 { let s = r#\"}\"#; s.len() as i64 } }";
        let out = desugar_rust_blocks(src);
        assert!(out.starts_with("BEGIN"), "desugar failed");
    }

    #[test]
    fn rust_block_with_line_comment() {
        let src = "rust { // } closing brace in comment\n pub extern \"C\" fn j() {} }";
        let out = desugar_rust_blocks(src);
        assert!(out.starts_with("BEGIN"), "desugar failed");
    }

    #[test]
    fn rust_block_with_block_comment() {
        let src = "rust { /* } closing brace */ pub extern \"C\" fn k() {} }";
        let out = desugar_rust_blocks(src);
        assert!(out.starts_with("BEGIN"), "desugar failed");
    }

    #[test]
    fn identifier_starting_with_rust_is_not_matched() {
        let src = "my $rusty = 1; sub rusty { 1 }\n";
        assert_eq!(desugar_rust_blocks(src), src);
    }

    #[test]
    fn rust_keyword_mid_expression_is_not_matched() {
        // Here `rust` is not at a statement boundary (after `=`), so it is left alone.
        let src = "my $x = rust { 1 }\n";
        assert_eq!(desugar_rust_blocks(src), src);
    }
}
