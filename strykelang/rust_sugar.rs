//! Source-level desugaring for `rust { ... }` FFI blocks.
//!
//! Why pre-lex and not a new `StmtKind`? The parity roadmap pins a rule: "Do not add new
//! `ExprKind`/`StmtKind` variants for new behavior." So `rust { ... }` is surfaced as a
//! BEGIN-wrapped builtin call — syntactic sugar only. At the source level we replace the
//! block with:
//!
//! ```text
//!     BEGIN { __stryke_rust_compile(q‹SOURCE›, $LINE); }
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
        //
        // CRITICAL: this scanner copies the source byte-by-byte. Every emitted run MUST go
        // through `out.push_str(&code[a..b])` (a slice of the original `&str`) so multi-byte
        // UTF-8 sequences round-trip intact. Casting individual bytes via `bytes[i] as char`
        // would treat each byte as a U+00XX codepoint and re-encode each high byte as a
        // two-byte UTF-8 sequence (E2→C3 A2, 94→C2 94, 80→C2 80) — silently mangling every
        // `─`, `→`, `§`, emoji, accented char etc. anywhere in the source as soon as any
        // occurrence of `rust` (e.g. `# Let me trust ...`) makes the fast path miss.
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
                // Perl line comment to end of line. Copy as a UTF-8-preserving slice.
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                out.push_str(&code[start..i]);
                continue;
            }
            b'"' | b'\'' | b'`' => {
                // Double- / single- / backtick-quoted string. Treat `\X` as an escape.
                // Body bytes are copied as a single UTF-8-preserving slice; the scanner
                // only inspects ASCII control bytes (the quote itself, `\`, `\n`), so it
                // never lands inside a multi-byte sequence.
                let quote = c;
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < bytes.len() {
                        if bytes[i + 1] == b'\n' {
                            line += 1;
                        }
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if b == b'\n' {
                        line += 1;
                    }
                    if b == quote {
                        break;
                    }
                }
                out.push_str(&code[start..i]);
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

        // Candidate: identifier start? Only single-byte ASCII letters / `_` start an
        // identifier; any non-ASCII byte is the leading byte of a UTF-8 multi-byte
        // sequence and must be copied via a slice (NOT cast to `char`) along with its
        // continuation bytes, otherwise high-bit bytes get double-encoded (see CRITICAL
        // comment above).
        let is_ident_start = c.is_ascii_alphabetic() || c == b'_';
        if !is_ident_start {
            let start = i;
            // Advance past this whole UTF-8 codepoint: 1 byte for ASCII (c < 0x80),
            // 2/3/4 bytes for a leading byte 0xC0..=0xF7.
            let step = if c < 0x80 {
                1
            } else if c < 0xC0 {
                // Stray continuation byte — shouldn't happen in well-formed UTF-8
                // input. Skip it conservatively as 1 byte.
                1
            } else if c < 0xE0 {
                2
            } else if c < 0xF0 {
                3
            } else {
                4
            };
            i = (i + step).min(bytes.len());
            out.push_str(&code[start..i]);
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
        "BEGIN {{ __stryke_rust_compile(\"{}\", {}); }}",
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

    // Regression: byte-by-byte `out.push(b as char)` silently mangled every
    // UTF-8 multi-byte sequence — `─` (E2 94 80) became `── ` (C3 A2 C2 94 C2 80
    // …) and similar for `§`, `→`, emoji, etc. — as soon as any `rust`
    // substring (including innocuous `# Let me trust ...` comments) made the
    // `code.contains("rust")` fast path miss. The fix copies via `push_str`
    // slices of the original `&str`. This test pins UTF-8 round-trip across
    // every emit site: leading non-ident byte, comment body, string literal
    // body, and post-fast-path passthrough.
    #[test]
    fn desugar_preserves_utf8_when_fast_path_misses() {
        let src = "\
# Section banner — must preserve em dash / arrow / § across the desugar
my $banner = \"── §1 vectors ──────────\";
p \"$banner — adjacent →\";
# Force the desugar to actually scan (any `rust` substring triggers it).
# trust me — this is what makes the fast path miss.
";
        assert_eq!(
            desugar_rust_blocks(src),
            src,
            "non-rust source must round-trip byte-for-byte through the desugarer"
        );
    }

    #[test]
    fn simple_rust_block_is_replaced_with_begin_call() {
        let src =
            "rust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\nprint add(1, 2);\n";
        let out = desugar_rust_blocks(src);
        assert!(out.contains("BEGIN"), "no BEGIN: {out}");
        assert!(out.contains("__stryke_rust_compile"), "no builtin call");
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
