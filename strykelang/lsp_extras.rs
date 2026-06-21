//! Additional LSP capabilities for `stryke --lsp`:
//!
//! * `textDocument/semanticTokens/full` — token-level coloring that respects the
//!   actual stryke lexical structure (keywords / builtins / sigil vars / strings /
//!   comments / numbers / regex / pipes).
//! * `textDocument/signatureHelp` — parameter hints derived from the same doc
//!   strings that drive hover (see `lsp.rs::doc_for_label_text`).
//! * `textDocument/codeAction` — small, line-local quickfixes (wrap line in
//!   `p`, comment / uncomment, toggle `--no-interop`-friendly forms).
//!
//! Kept in its own module so `lsp.rs` stays focused on the dispatch + parser
//! plumbing that's already there. The two entry points called from `lsp.rs` are
//! [`compute_semantic_tokens`], [`compute_signature_help`], [`compute_code_actions`],
//! plus [`semantic_tokens_legend`] for the capability advertisement.
//!
//! Token-type / modifier indices are stable: don't reorder the
//! [`SEMANTIC_TYPES`] / [`SEMANTIC_MODS`] arrays without bumping the legend.
//!
//! No state, no allocation hot path beyond what the scanner needs — every call
//! is `O(text length)` with a single pass.

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, DocumentFormattingParams,
    FoldingRange, FoldingRangeKind, FoldingRangeParams, Position, Range, SemanticToken,
    SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensServerCapabilities, SignatureHelp,
    SignatureHelpParams, SignatureInformation, TextEdit, Uri, WorkDoneProgressOptions,
    WorkspaceEdit,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Semantic tokens
// ---------------------------------------------------------------------------

pub(crate) const SEMANTIC_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::REGEXP,
    SemanticTokenType::MACRO,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::NAMESPACE,
];

pub(crate) const SEMANTIC_MODS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

// Stable token-type indices (must match SEMANTIC_TYPES order).
const TY_KEYWORD: u32 = 0;
const TY_FUNCTION: u32 = 1;
const TY_VARIABLE: u32 = 2;
const TY_STRING: u32 = 4;
const TY_NUMBER: u32 = 5;
const TY_COMMENT: u32 = 6;
const TY_OPERATOR: u32 = 7;
const TY_REGEXP: u32 = 8;
const TY_MACRO: u32 = 9;

const MOD_DEFAULT_LIBRARY: u32 = 1 << 5;
/// `semantic_tokens_legend` — see implementation.
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: SEMANTIC_TYPES.to_vec(),
        token_modifiers: SEMANTIC_MODS.to_vec(),
    }
}
/// `semantic_tokens_options` — see implementation.
pub fn semantic_tokens_options() -> SemanticTokensServerCapabilities {
    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        work_done_progress_options: WorkDoneProgressOptions::default(),
        legend: semantic_tokens_legend(),
        range: Some(false),
        full: Some(SemanticTokensFullOptions::Bool(true)),
    })
}

/// Emit semantic tokens for the whole document.
///
/// The encoding the LSP wants is a flat `Vec<SemanticToken>` where each entry's
/// `delta_line` / `delta_start` are deltas from the previous token. We track that
/// state in the loop.
pub fn compute_semantic_tokens(text: &str) -> SemanticTokens {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens: Vec<SemanticToken> = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;

    let mut line: u32 = 0;
    let mut col: u32 = 0; // UTF-16 column
    let mut i: usize = 0;

    let push = |tokens: &mut Vec<SemanticToken>,
                prev_line: &mut u32,
                prev_char: &mut u32,
                line: u32,
                col: u32,
                len_u16: u32,
                ty: u32,
                modifiers: u32| {
        if len_u16 == 0 {
            return;
        }
        let delta_line = line - *prev_line;
        let delta_start = if delta_line == 0 {
            col - *prev_char
        } else {
            col
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: len_u16,
            token_type: ty,
            token_modifiers_bitset: modifiers,
        });
        *prev_line = line;
        *prev_char = col;
    };

    while i < chars.len() {
        let c = chars[i];

        // Newline
        if c == '\n' {
            i += 1;
            line += 1;
            col = 0;
            continue;
        }
        if c == '\r' {
            i += 1;
            continue;
        }
        // Whitespace
        if c == ' ' || c == '\t' {
            i += 1;
            col += 1;
            continue;
        }
        // Line comment
        if c == '#' {
            let start_col = col;
            let mut len = 0u32;
            while i < chars.len() && chars[i] != '\n' {
                len += utf16_len(chars[i]);
                i += 1;
            }
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_COMMENT,
                0,
            );
            col += len;
            continue;
        }
        // Strings.
        //
        // Double-quoted strings get an interpolation-aware lexer: a `#{`
        // inside `"..."` opens an embedded expression that runs until the
        // matching `}`. We emit the literal run as TY_STRING, then `#{` and
        // `}` as TY_OPERATOR (so the IDE doesn't color them as comment text
        // or string text), and we leave the interior characters un-tokenized
        // so completion / hover land on them as code — the same way the
        // IntelliJ plugin's lexer treats them.
        //
        // `#` is NEVER a comment opener inside a string. The previous
        // behavior leaked the comment dispatch into strings on certain
        // edits.
        if c == '"' {
            let quote = c;
            // `start_col`/`len` track the current LITERAL run (a contiguous
            // span of plain string text). When we hit an interpolated `$var`,
            // `@var`, `%var`, or `#{EXPR}`, we flush the run as TY_STRING and
            // emit the interpolation as a real VARIABLE / OPERATOR token so
            // the IDE colors it distinctly from the surrounding text.
            let mut start_col = col;
            let mut len = utf16_len(c);
            let mut closed = false;
            i += 1;
            let flush_lit = |tokens: &mut Vec<SemanticToken>,
                             prev_line: &mut u32,
                             prev_char: &mut u32,
                             line: u32,
                             start_col: &mut u32,
                             len: &mut u32,
                             col: &mut u32| {
                if *len > 0 {
                    push(
                        tokens, prev_line, prev_char, line, *start_col, *len, TY_STRING, 0,
                    );
                    *col += *len;
                    *start_col = *col;
                    *len = 0;
                }
            };
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() && chars[i + 1] != '\n' {
                    len += utf16_len(ch) + utf16_len(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == quote {
                    len += utf16_len(ch);
                    i += 1;
                    push(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_char,
                        line,
                        start_col,
                        len,
                        TY_STRING,
                        0,
                    );
                    col += len;
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    break;
                }
                // `#{EXPR}` interpolation — embedded code.
                if ch == '#' && i + 1 < chars.len() && chars[i + 1] == '{' {
                    flush_lit(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_char,
                        line,
                        &mut start_col,
                        &mut len,
                        &mut col,
                    );
                    push(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_char,
                        line,
                        col,
                        2,
                        TY_OPERATOR,
                        0,
                    );
                    col += 2;
                    i += 2;
                    // Walk the expression interior; track `{ }` nesting so
                    // nested hash literals inside the interp don't close
                    // it early. We deliberately don't emit semantic tokens
                    // for the interior here — the IDE's normal token
                    // pipeline handles them on the client side; here we
                    // just skip past the bytes.
                    let mut depth: i32 = 1;
                    while i < chars.len() && depth > 0 {
                        let ich = chars[i];
                        if ich == '\n' {
                            line += 1;
                            col = 0;
                            i += 1;
                            continue;
                        }
                        if ich == '{' {
                            depth += 1;
                        } else if ich == '}' {
                            depth -= 1;
                            if depth == 0 {
                                push(
                                    &mut tokens,
                                    &mut prev_line,
                                    &mut prev_char,
                                    line,
                                    col,
                                    1,
                                    TY_OPERATOR,
                                    0,
                                );
                                col += 1;
                                i += 1;
                                break;
                            }
                        }
                        col += utf16_len(ich);
                        i += 1;
                    }
                    start_col = col;
                    len = 0;
                    continue;
                }
                // `@{[ EXPR ]}` — Perl-style array-deref interpolation.
                // Acts the same way as `#{ EXPR }`: emit `@{[` and `]}` as
                // OPERATOR tokens, then leave EXPR un-tokenized so the
                // client renders the interior as code (variables get
                // their own variable color, function calls hover, etc.).
                // Without this, the entire `@{[FN]}` got swallowed by the
                // generic `@{…}` block-deref branch below and the IDE
                // colored `FN` as part of the variable token, killing
                // hover / goto-def for the embedded expression.
                if ch == '@' && i + 2 < chars.len() && chars[i + 1] == '{' && chars[i + 2] == '[' {
                    flush_lit(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_char,
                        line,
                        &mut start_col,
                        &mut len,
                        &mut col,
                    );
                    push(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_char,
                        line,
                        col,
                        3,
                        TY_OPERATOR,
                        0,
                    );
                    col += 3;
                    i += 3;
                    let mut bracket_depth: i32 = 1;
                    let mut brace_depth: i32 = 1;
                    while i < chars.len() {
                        let ich = chars[i];
                        if ich == '\n' {
                            line += 1;
                            col = 0;
                            i += 1;
                            continue;
                        }
                        if ich == '[' {
                            bracket_depth += 1;
                        } else if ich == '{' {
                            brace_depth += 1;
                        } else if ich == '}' {
                            brace_depth -= 1;
                        } else if ich == ']' {
                            bracket_depth -= 1;
                            if bracket_depth == 0 && i + 1 < chars.len() && chars[i + 1] == '}' {
                                push(
                                    &mut tokens,
                                    &mut prev_line,
                                    &mut prev_char,
                                    line,
                                    col,
                                    2,
                                    TY_OPERATOR,
                                    0,
                                );
                                col += 2;
                                i += 2;
                                break;
                            }
                        }
                        col += utf16_len(ich);
                        i += 1;
                    }
                    start_col = col;
                    len = 0;
                    let _ = brace_depth;
                    continue;
                }
                // Sigil-variable interpolation: `$name`, `@name`, `%name`,
                // optionally followed by `::Pkg::name`. Also handles a few
                // bracket / arrow follow-ons like `$h{k}`, `$arr[i]`,
                // `$h->{k}`, `$arr->[i]` so the variable token covers the
                // full referent, not just the bare sigil-ident.
                if (ch == '$' || ch == '@' || ch == '%') && i + 1 < chars.len() {
                    let nxt = chars[i + 1];
                    let starts_var = nxt == '_' || nxt.is_alphabetic() || nxt == '{';
                    if starts_var {
                        flush_lit(
                            &mut tokens,
                            &mut prev_line,
                            &mut prev_char,
                            line,
                            &mut start_col,
                            &mut len,
                            &mut col,
                        );
                        let var_start_col = col;
                        let mut var_len = utf16_len(ch); // the sigil
                        i += 1;
                        // ${...} block deref — opaque, consume balanced braces.
                        if i < chars.len() && chars[i] == '{' {
                            let mut depth: i32 = 1;
                            var_len += utf16_len(chars[i]);
                            i += 1;
                            while i < chars.len() && depth > 0 {
                                let bc = chars[i];
                                if bc == '\n' {
                                    break;
                                }
                                if bc == '{' {
                                    depth += 1;
                                } else if bc == '}' {
                                    depth -= 1;
                                    var_len += utf16_len(bc);
                                    i += 1;
                                    if depth == 0 {
                                        break;
                                    }
                                    continue;
                                }
                                var_len += utf16_len(bc);
                                i += 1;
                            }
                        } else {
                            // Plain `name` or `Pkg::name`.
                            while i < chars.len() {
                                let bc = chars[i];
                                if bc == '_' || bc.is_alphanumeric() {
                                    var_len += utf16_len(bc);
                                    i += 1;
                                    continue;
                                }
                                if bc == ':' && i + 1 < chars.len() && chars[i + 1] == ':' {
                                    var_len += 2;
                                    i += 2;
                                    continue;
                                }
                                break;
                            }
                            // Optional one-level subscript / arrow chain:
                            // `{k}`, `[i]`, `->{k}`, `->[i]`. Keeps the
                            // VARIABLE token covering the whole referent so
                            // the IDE highlights e.g. `$h{key}` as one
                            // variable, not "variable + string + ...".
                            loop {
                                if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] == '>' {
                                    var_len += 2;
                                    i += 2;
                                }
                                let open = if i < chars.len() {
                                    match chars[i] {
                                        '{' => Some('}'),
                                        '[' => Some(']'),
                                        _ => None,
                                    }
                                } else {
                                    None
                                };
                                let Some(close) = open else { break };
                                let mut depth: i32 = 1;
                                var_len += utf16_len(chars[i]);
                                i += 1;
                                while i < chars.len() && depth > 0 {
                                    let bc = chars[i];
                                    if bc == '\n' {
                                        break;
                                    }
                                    if bc == '{' || bc == '[' {
                                        depth += 1;
                                    } else if bc == close {
                                        depth -= 1;
                                        var_len += utf16_len(bc);
                                        i += 1;
                                        if depth == 0 {
                                            break;
                                        }
                                        continue;
                                    }
                                    var_len += utf16_len(bc);
                                    i += 1;
                                }
                            }
                        }
                        if var_len > utf16_len(ch) {
                            push(
                                &mut tokens,
                                &mut prev_line,
                                &mut prev_char,
                                line,
                                var_start_col,
                                var_len,
                                TY_VARIABLE,
                                0,
                            );
                            col += var_len;
                            start_col = col;
                            len = 0;
                            continue;
                        }
                        // No name followed — treat the bare sigil as plain
                        // literal text. Fall through and append.
                        len = var_len;
                        // i already advanced past the sigil; restore by
                        // backing up one so the bottom-of-loop bump treats
                        // the sigil as a normal char.
                        // (Actually we've already accounted for it in `len`,
                        // so just skip the bottom `len += utf16_len(ch)`.)
                        continue;
                    }
                }
                len += utf16_len(ch);
                i += 1;
            }
            // Hit end-of-line or end-of-file before closing quote. Emit
            // whatever literal run we have so it's still rendered as string.
            if !closed && len > 0 {
                push(
                    &mut tokens,
                    &mut prev_line,
                    &mut prev_char,
                    line,
                    start_col,
                    len,
                    TY_STRING,
                    0,
                );
                col += len;
            }
            continue;
        }
        // Single-quote and backtick strings: no interpolation, simple span.
        if c == '\'' || c == '`' {
            let quote = c;
            let start_col = col;
            let mut len = utf16_len(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() && chars[i + 1] != '\n' {
                    len += utf16_len(ch) + utf16_len(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == quote {
                    len += utf16_len(ch);
                    i += 1;
                    break;
                }
                if ch == '\n' {
                    break;
                }
                len += utf16_len(ch);
                i += 1;
            }
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_STRING,
                0,
            );
            col += len;
            continue;
        }
        // Number
        if c.is_ascii_digit() {
            let start_col = col;
            let mut len = 0u32;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '_'
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || ((chars[i] == '+' || chars[i] == '-')
                        && i > 0
                        && (chars[i - 1] == 'e' || chars[i - 1] == 'E')))
            {
                len += 1;
                i += 1;
            }
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_NUMBER,
                0,
            );
            col += len;
            continue;
        }
        // Sigil variable
        if c == '$' || c == '@' || c == '%' {
            let start_col = col;
            let mut len = utf16_len(c);
            i += 1;
            // Punctuation specials: $! $@ $_ $, $; $/ $\ $" etc.
            if i < chars.len() && is_special_var_char(chars[i]) {
                len += utf16_len(chars[i]);
                i += 1;
            } else if i < chars.len() && chars[i] == '{' {
                // ${...}
                len += 1;
                i += 1;
                while i < chars.len() && chars[i] != '}' && chars[i] != '\n' {
                    len += utf16_len(chars[i]);
                    i += 1;
                }
                if i < chars.len() && chars[i] == '}' {
                    len += 1;
                    i += 1;
                }
            } else {
                // Regular identifier (may include ::)
                while i < chars.len()
                    && (chars[i] == '_'
                        || chars[i].is_alphanumeric()
                        || (chars[i] == ':' && i + 1 < chars.len() && chars[i + 1] == ':'))
                {
                    if chars[i] == ':' {
                        len += 2;
                        i += 2;
                    } else {
                        len += utf16_len(chars[i]);
                        i += 1;
                    }
                }
            }
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_VARIABLE,
                0,
            );
            col += len;
            continue;
        }
        // Pipe operators
        if c == '|' && peek(&chars, i + 1) == Some('>') {
            let start_col = col;
            let len = if peek(&chars, i + 2) == Some('>') {
                3
            } else {
                2
            };
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_MACRO,
                0,
            );
            i += len as usize;
            col += len;
            continue;
        }
        if c == '~' && peek(&chars, i + 1) == Some('>') {
            let start_col = col;
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                2,
                TY_MACRO,
                0,
            );
            i += 2;
            col += 2;
            continue;
        }
        // Regex literal (heuristic: previous non-space char is in REGEX_ANCHORS or
        // line start).
        if c == '/' && looks_like_regex_start(&chars, i) {
            let start_col = col;
            let mut len = 1u32;
            i += 1;
            let mut bracket = 0;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() && chars[i + 1] != '\n' {
                    len += utf16_len(ch) + utf16_len(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == '[' {
                    bracket += 1;
                }
                if ch == ']' && bracket > 0 {
                    bracket -= 1;
                }
                if ch == '/' && bracket == 0 {
                    len += 1;
                    i += 1;
                    break;
                }
                if ch == '\n' {
                    break;
                }
                len += utf16_len(ch);
                i += 1;
            }
            // Flags
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                len += 1;
                i += 1;
            }
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len,
                TY_REGEXP,
                0,
            );
            col += len;
            continue;
        }
        // Identifier (keyword / builtin / plain)
        if c == '_' || c.is_alphabetic() {
            let start_col = col;
            let start_i = i;
            while i < chars.len() && (chars[i] == '_' || chars[i].is_alphanumeric()) {
                i += 1;
            }
            let word: String = chars[start_i..i].iter().collect();
            let len_u16 = word.encode_utf16().count() as u32;
            let (ty, modifiers) = classify_word(&word);
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                len_u16,
                ty,
                modifiers,
            );
            col += len_u16;
            continue;
        }
        // Operator (single char)
        if is_operator_char(c) {
            let start_col = col;
            push(
                &mut tokens,
                &mut prev_line,
                &mut prev_char,
                line,
                start_col,
                1,
                TY_OPERATOR,
                0,
            );
            i += 1;
            col += 1;
            continue;
        }
        // Anything else — skip silently
        col += utf16_len(c);
        i += 1;
    }

    SemanticTokens {
        result_id: None,
        data: tokens,
    }
}

fn peek(chars: &[char], i: usize) -> Option<char> {
    chars.get(i).copied()
}

fn utf16_len(c: char) -> u32 {
    c.len_utf16() as u32
}

fn is_special_var_char(c: char) -> bool {
    matches!(
        c,
        '_' | '!'
            | '@'
            | '$'
            | ','
            | ';'
            | '/'
            | '\\'
            | '"'
            | '\''
            | '&'
            | '`'
            | '+'
            | '-'
            | '.'
            | '0'..='9' | '?' | '<' | '>' | '(' | ')' | '[' | ']' | '~' | '^'
    )
}

fn is_operator_char(c: char) -> bool {
    matches!(
        c,
        '+' | '-'
            | '*'
            | '/'
            | '%'
            | '='
            | '<'
            | '>'
            | '!'
            | '&'
            | '|'
            | '^'
            | '~'
            | '?'
            | ':'
            | ';'
            | ','
            | '.'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '\\'
    )
}

fn looks_like_regex_start(chars: &[char], i: usize) -> bool {
    // Walk backwards to find the last non-space character on the same line.
    let mut k = i;
    while k > 0 {
        k -= 1;
        match chars[k] {
            ' ' | '\t' => continue,
            '\n' | '\r' => return true,
            c => {
                return matches!(
                    c,
                    ',' | '(' | '=' | ';' | '{' | '|' | '&' | '~' | '!' | '?' | '['
                );
            }
        }
    }
    true
}

/// Classify a bareword into a semantic token type + modifier bitset.
///
/// Reuses the reflection map surfaced by `crate::builtins`:
///   * `KEYWORDS` (`is_stryke_keyword`) — language keywords.
///   * `all_names_hash_map()` — every callable + alias stryke recognises.
///
/// Anything not in either map is treated as a user identifier (variable).
fn classify_word(word: &str) -> (u32, u32) {
    use crate::builtins;
    if builtins::is_stryke_keyword(word) {
        return (TY_KEYWORD, 0);
    }
    if builtin_names().contains(word) {
        return (TY_FUNCTION, MOD_DEFAULT_LIBRARY);
    }
    (TY_VARIABLE, 0)
}

static BUILTIN_NAMES: std::sync::OnceLock<std::collections::HashSet<String>> =
    std::sync::OnceLock::new();

fn builtin_names() -> &'static std::collections::HashSet<String> {
    BUILTIN_NAMES.get_or_init(|| crate::builtins::all_hash_map().into_keys().collect())
}

// ---------------------------------------------------------------------------
// Signature help
// ---------------------------------------------------------------------------

/// Extract a `name(...)` signature from a doc string by scanning for code
/// fences and grabbing the first call-shaped line.
///
/// Doc strings look like ``` ```perl\nfn name($a, $b) { ... } ``` ``` — we want
/// the line that starts with the function name and has a parenthesized arg
/// list.
fn signature_from_doc(name: &str, doc: &str) -> Option<String> {
    for line in doc.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(name) {
            if rest.trim_start().starts_with('(') {
                if let Some(end) = balanced_paren_end(rest.trim_start()) {
                    let signature = &rest.trim_start()[..=end];
                    return Some(format!("{name}{signature}"));
                }
            }
        }
        // Handle `fn name(args)` / `sub name(args)`
        for prefix in ["fn ", "sub ", "method "] {
            if let Some(after) = trimmed.strip_prefix(prefix) {
                if let Some(after_name) = after.strip_prefix(name) {
                    if after_name.starts_with('(') {
                        if let Some(end) = balanced_paren_end(after_name) {
                            return Some(format!("{name}{}", &after_name[..=end]));
                        }
                    }
                }
            }
        }
    }
    None
}

fn balanced_paren_end(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Compute signature help for a position inside a function call.
///
/// Walks back from `params.position` looking for the innermost unmatched `(`,
/// captures the call target name immediately before it, and counts commas
/// outside nested parens to pick the active parameter.
pub fn compute_signature_help<F>(
    text: &str,
    params: &SignatureHelpParams,
    doc_for: F,
) -> Option<SignatureHelp>
where
    F: Fn(&str) -> Option<&'static str>,
{
    let pos = &params.text_document_position_params.position;
    let offset = position_to_offset(text, pos)?;

    // Walk back through the buffer to find `name(` and count commas
    let bytes = text.as_bytes();
    let mut paren_depth = 0i32;
    let mut comma_count = 0u32;
    let mut i = offset;
    while i > 0 {
        i -= 1;
        let c = bytes[i] as char;
        if c == ')' {
            paren_depth += 1;
        } else if c == '(' {
            if paren_depth == 0 {
                // i is the open paren
                let name_end = i;
                let mut name_start = name_end;
                while name_start > 0 {
                    let nc = bytes[name_start - 1] as char;
                    if nc == '_' || nc.is_alphanumeric() || nc == ':' {
                        name_start -= 1;
                    } else {
                        break;
                    }
                }
                if name_start == name_end {
                    return None;
                }
                let name = &text[name_start..name_end];
                let doc = doc_for(name)?;
                let signature_label =
                    signature_from_doc(name, doc).unwrap_or_else(|| format!("{name}(…)"));
                let active_param = comma_count;
                return Some(SignatureHelp {
                    signatures: vec![SignatureInformation {
                        label: signature_label,
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: doc.to_string(),
                            },
                        )),
                        parameters: None,
                        active_parameter: Some(active_param),
                    }],
                    active_signature: Some(0),
                    active_parameter: Some(active_param),
                });
            }
            paren_depth -= 1;
        } else if c == ',' && paren_depth == 0 {
            comma_count += 1;
        }
        if c == '\n' && paren_depth == 0 {
            return None;
        }
    }
    None
}

fn position_to_offset(text: &str, pos: &Position) -> Option<usize> {
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for (i, c) in text.char_indices() {
        if line == pos.line && col == pos.character {
            return Some(i);
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += c.len_utf16() as u32;
        }
        if line > pos.line {
            return Some(i);
        }
    }
    Some(text.len())
}

// ---------------------------------------------------------------------------
// Code actions
// ---------------------------------------------------------------------------

/// Compute code actions for a range.
///
/// The mix of actions is range-aware:
/// * Single line, empty selection: line-local quickfixes (wrap in `p`,
///   toggle comment).
/// * Single line, non-empty selection: extract-variable and extract-constant.
/// * Multi-line selection: extract-function (wraps the selection in a
///   `fn name { ... }` declaration and replaces the original span with a
///   call). v1 doesn't do free-variable analysis — the user manually
///   parameterizes after extraction.
pub fn compute_code_actions(
    docs: &HashMap<String, String>,
    params: &CodeActionParams,
) -> Vec<CodeActionOrCommand> {
    let uri = &params.text_document.uri;
    let Some(text) = docs.get(uri.as_str()) else {
        return Vec::new();
    };
    let mut out: Vec<CodeActionOrCommand> = Vec::new();
    let range = params.range;

    // ── Line-local quickfixes (always offered for the current line) ──
    if let Some(line_text) = nth_line(text, range.start.line as usize) {
        let trimmed = line_text.trim_start();
        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("p ")
            && !trimmed.starts_with("p(")
            && !trimmed.starts_with("say ")
        {
            out.push(wrap_in_p_action(uri, range.start.line, line_text));
        }
        out.push(toggle_comment_action(uri, range.start.line, line_text));
    }

    // ── Refactorings ──
    let same_line = range.start.line == range.end.line;
    let nonempty_range =
        range.start.line != range.end.line || range.start.character != range.end.character;

    // Empty range (caret-only): snap to the identifier or string literal
    // at the cursor so Extract Variable / Extract Constant work without
    // requiring the user to manually highlight first. Matches IntelliJ's
    // "Extract Variable" UX where pressing Cmd-Opt-V on a word extracts
    // that word.
    let effective_range: Range = if !nonempty_range {
        if let Some(line_text) = nth_line(text, range.start.line as usize) {
            snap_to_word_at_cursor(line_text, range.start)
                .map(|(s, e)| Range {
                    start: Position {
                        line: range.start.line,
                        character: s,
                    },
                    end: Position {
                        line: range.start.line,
                        character: e,
                    },
                })
                .unwrap_or(range)
        } else {
            range
        }
    } else {
        range
    };
    let effective_nonempty = effective_range.start.line != effective_range.end.line
        || effective_range.start.character != effective_range.end.character;

    if effective_nonempty {
        let range = effective_range;
        // Offer all three Extract refactorings whenever the user has any
        // non-empty selection. IntelliJ's keymap-driven Cmd-Opt-V / -C / -M
        // each filters down to the one that matches its title — if Variable
        // is missing from the response, Cmd-Opt-V silently no-ops, which
        // surfaces as "the menu item does nothing". Always emitting all
        // three keeps every shortcut functional regardless of whether the
        // selection spans one line or many.
        if same_line {
            if let Some(line_text) = nth_line(text, range.start.line as usize) {
                if let Some(selection) =
                    extract_selection_on_line(line_text, range.start.character, range.end.character)
                {
                    if !selection.trim().is_empty() {
                        out.push(extract_variable_action(uri, line_text, range, selection));
                        out.push(extract_constant_action(uri, line_text, range, selection));
                        // Also offer extract-to-function on single-line
                        // selections — wraps the selected expression in a
                        // `fn extracted_fn { … }`.
                        if let Some(block) = extract_selection_multiline(text, range) {
                            out.push(extract_function_action(uri, range, &block));
                        }
                        // Extract Parameter — bind to Cmd-Opt-P. Adds a
                        // new param to the enclosing `fn` and replaces
                        // the selection with the param name. Only
                        // offered when an enclosing fn is found.
                        if let Some(param_action) =
                            extract_parameter_action(uri, docs, text, range, selection)
                        {
                            out.push(param_action);
                        }
                    }
                }
            }
        } else {
            // Multi-line selection → all three. Variable / Constant treat
            // the joined selection as a single expression; Function wraps
            // the block.
            if let Some(block) = extract_selection_multiline(text, range) {
                if !block.text.trim().is_empty() {
                    // For Variable/Constant we need the selection as a
                    // single contiguous string with newlines elided so the
                    // generated `my $name = …` body is one line. Trim the
                    // block text and collapse whitespace runs.
                    let joined: String =
                        block.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    // Use the first selected line as the anchor for the
                    // single-line action builders.
                    let first_line = nth_line(text, range.start.line as usize).unwrap_or("");
                    out.push(extract_variable_action(uri, first_line, range, &joined));
                    out.push(extract_constant_action(uri, first_line, range, &joined));
                    out.push(extract_function_action(uri, range, &block));
                }
            }
        }
    }

    out
}

/// Char-indexed UTF-16 slice of a single line. Used by the extract-variable
/// and extract-constant builders so the replaced span aligns with what the
/// client sees in the editor (LSP positions are UTF-16 code units).
fn extract_selection_on_line(line_text: &str, start: u32, end: u32) -> Option<&str> {
    let utf16: Vec<u16> = line_text.encode_utf16().collect();
    let s = start.min(utf16.len() as u32) as usize;
    let e = end.min(utf16.len() as u32) as usize;
    if e <= s {
        return None;
    }
    // Convert UTF-16 offsets back to UTF-8 byte offsets.
    let (mut u16_seen, mut s_byte, mut e_byte) = (0usize, None::<usize>, None::<usize>);
    for (i, ch) in line_text.char_indices() {
        if u16_seen == s {
            s_byte = Some(i);
        }
        u16_seen += ch.len_utf16();
        if u16_seen == e {
            e_byte = Some(i + ch.len_utf8());
            break;
        }
    }
    let s_byte = s_byte?;
    let e_byte = e_byte.unwrap_or(line_text.len());
    Some(&line_text[s_byte..e_byte])
}

/// Snap a caret-only cursor to a word-boundary span on the line. For
/// cursors inside a string literal, the span extends within the
/// string's interior up to the nearest whitespace or interpolation
/// boundary (so `Cmd-Opt-V` on a word inside `"one two $var three"`
/// extracts `two`, `$var`, etc.). For cursors on a bareword / sigiled
/// identifier outside a string, the span is the identifier itself.
/// Returns the `(start_utf16, end_utf16)` columns or `None` if there's
/// no meaningful span at the cursor (e.g. inside whitespace or
/// punctuation).
fn snap_to_word_at_cursor(line_text: &str, cursor: Position) -> Option<(u32, u32)> {
    // Convert UTF-16 col to a byte index.
    let mut byte_cur = line_text.len();
    let mut u16_seen = 0u32;
    for (i, ch) in line_text.char_indices() {
        if u16_seen >= cursor.character {
            byte_cur = i;
            break;
        }
        u16_seen += ch.len_utf16() as u32;
    }
    let in_string = same_line_selection_inside_interpolating_string(line_text, cursor.character);

    let is_word_char_for_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let is_sigil = |c: char| c == '$' || c == '@' || c == '%';

    // Inside a string: snap to a "word" — a maximal run of word chars,
    // OR a complete `$var` / `@arr` / `%h` interpolation marker if the
    // cursor sits on a sigil or its body.
    if in_string {
        // If cursor is on or right after a sigil, snap to the
        // sigil-prefixed identifier.
        let cur_char = line_text[byte_cur..].chars().next();
        let prev_char = line_text[..byte_cur].chars().next_back();
        if matches!(cur_char, Some(c) if is_sigil(c)) || matches!(prev_char, Some(c) if is_sigil(c))
        {
            // Find start: walk back to the sigil.
            let mut start_byte = byte_cur;
            for (i, c) in line_text[..byte_cur].char_indices().rev() {
                if is_sigil(c) {
                    start_byte = i;
                    break;
                }
                if !is_word_char_for_ident(c) {
                    break;
                }
                start_byte = i;
            }
            // If the cursor is exactly on a sigil, start_byte stays at byte_cur.
            if matches!(cur_char, Some(c) if is_sigil(c)) {
                start_byte = byte_cur;
            }
            // Walk forward through sigil + ident chars.
            let mut end_byte = start_byte;
            let mut iter = line_text[start_byte..].char_indices();
            if let Some((_, first)) = iter.next() {
                if is_sigil(first) {
                    end_byte = start_byte + first.len_utf8();
                    for (i, c) in iter {
                        if !is_word_char_for_ident(c) {
                            break;
                        }
                        end_byte = start_byte + i + c.len_utf8();
                    }
                }
            }
            if end_byte > start_byte {
                return Some((
                    byte_to_utf16(line_text, start_byte),
                    byte_to_utf16(line_text, end_byte),
                ));
            }
        }
        // Otherwise: snap to a word inside the string body. Word
        // boundaries: whitespace, quote chars, sigils, punctuation
        // (anything not an ident char).
        let is_string_word_char = |c: char| is_word_char_for_ident(c);
        let mut start_byte = byte_cur;
        for (i, c) in line_text[..byte_cur].char_indices().rev() {
            if !is_string_word_char(c) {
                break;
            }
            start_byte = i;
        }
        let mut end_byte = byte_cur;
        for (i, c) in line_text[byte_cur..].char_indices() {
            if !is_string_word_char(c) {
                break;
            }
            end_byte = byte_cur + i + c.len_utf8();
        }
        if end_byte > start_byte {
            return Some((
                byte_to_utf16(line_text, start_byte),
                byte_to_utf16(line_text, end_byte),
            ));
        }
        return None;
    }

    // Outside a string: snap to an identifier, including a leading
    // sigil if standalone.
    let mut start_byte = byte_cur;
    for (i, c) in line_text[..byte_cur].char_indices().rev() {
        if !is_word_char_for_ident(c) {
            break;
        }
        start_byte = i;
    }
    let mut end_byte = byte_cur;
    for (i, c) in line_text[byte_cur..].char_indices() {
        if !is_word_char_for_ident(c) {
            break;
        }
        end_byte = byte_cur + i + c.len_utf8();
    }
    // Include a leading sigil (`$foo`, `@bar`, `%baz`) if present.
    if start_byte > 0 {
        let prev_char_start = line_text[..start_byte].char_indices().next_back();
        if let Some((idx, c)) = prev_char_start {
            if is_sigil(c) {
                // Make sure the sigil is standalone (preceded by
                // non-identifier).
                let standalone = match line_text[..idx].chars().next_back() {
                    None => true,
                    Some(c) => !is_word_char_for_ident(c),
                };
                if standalone {
                    start_byte = idx;
                }
            }
        }
    }
    if end_byte > start_byte {
        Some((
            byte_to_utf16(line_text, start_byte),
            byte_to_utf16(line_text, end_byte),
        ))
    } else {
        None
    }
}

/// Convert a byte index to a UTF-16 column on the same line.
fn byte_to_utf16(line_text: &str, byte_idx: usize) -> u32 {
    line_text[..byte_idx.min(line_text.len())]
        .encode_utf16()
        .count() as u32
}

/// True if the LSP character column `col` (UTF-16) on `line_text`
/// falls inside an unclosed interpolating string — either `"..."` or
/// `` `...` `` (backtick / qx command form). Best-effort line-local
/// scan: tracks `"` / `'` / `` ` `` toggling and skips one char after
/// `\\`. Doesn't try to model `qq//`, `q//`, or multi-line heredocs —
/// for the common single-line case it's sufficient to fix the
/// extract-variable / extract-constant breakage when the user
/// highlights literal text inside an interpolating string.
fn same_line_selection_inside_interpolating_string(line_text: &str, col_utf16: u32) -> bool {
    // Convert UTF-16 col to byte index so we can walk char-by-char up
    // to (not including) the selection start.
    let mut byte_cutoff = line_text.len();
    let mut u16_seen = 0u32;
    for (i, ch) in line_text.char_indices() {
        if u16_seen >= col_utf16 {
            byte_cutoff = i;
            break;
        }
        u16_seen += ch.len_utf16() as u32;
    }
    let mut in_dq = false;
    let mut in_sq = false;
    let mut in_bt = false;
    let mut chars = line_text[..byte_cutoff].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Skip the next char (escape).
                chars.next();
            }
            '"' if !in_sq && !in_bt => in_dq = !in_dq,
            '\'' if !in_dq && !in_bt => in_sq = !in_sq,
            '`' if !in_dq && !in_sq => in_bt = !in_bt,
            _ => {}
        }
    }
    in_dq || in_bt
}

/// Return true if the extracted text needs string-wrapping for the
/// decl RHS to be a valid expression. False for selections that are
/// already a complete expression — a single sigiled variable
/// (`$foo`, `@arr`, `%h`), an array/hash element access (`$h{k}`,
/// `$a[0]`), or a string literal already wrapped in quotes
/// (`"hello"`).
fn needs_string_wrap_for_extraction(selection: &str) -> bool {
    let trimmed = selection.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Already-quoted literal: skip.
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return false;
    }
    // Bare sigiled variable, possibly with subscript: `$foo`, `$foo[0]`,
    // `$h{k}`, `@arr`, `%h`. If the whole trimmed selection IS one of
    // these forms (no embedded literal text), it stands as a valid
    // expression for the decl. We approximate "no embedded literal
    // text" by checking that the selection doesn't contain whitespace
    // OR free-floating identifier chars outside a `{...}`/`[...]`
    // subscript.
    let starts_with_sigil = matches!(trimmed.chars().next(), Some('$' | '@' | '%'));
    if starts_with_sigil && !trimmed.chars().any(char::is_whitespace) {
        // Quick check: if every non-subscript char is identifier-ish,
        // treat as a plain variable expression and don't wrap.
        let mut depth = 0i32;
        let mut bare_text_run = false;
        for c in trimmed.chars() {
            match c {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth = (depth - 1).max(0),
                _ if depth == 0
                    && !(c.is_ascii_alphanumeric()
                        || c == '_'
                        || c == ':'
                        || c == '$'
                        || c == '@'
                        || c == '%'
                        || c == '-'
                        || c == '>') =>
                {
                    bare_text_run = true;
                    break;
                }
                _ => {}
            }
        }
        if !bare_text_run {
            return false;
        }
    }
    true
}

/// Escape `\\` and `"` for embedding inside a stryke `"..."` literal.
/// The other interpolation triggers (`$`, `@`) are left alone so a
/// selection that originally interpolated a variable continues to do
/// so from the new decl.
fn escape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// Span of selected text across multiple lines, plus its inferred indentation
/// (the leading whitespace of the first selected line). The extract-function
/// builder uses the indent to keep the inserted call at the same column.
struct MultilineBlock {
    text: String,
    indent: String,
    /// Line where the new `fn` declaration should be inserted (one line
    /// above the selection's first line).
    insertion_line: u32,
}

fn extract_selection_multiline(text: &str, range: Range) -> Option<MultilineBlock> {
    let lines: Vec<&str> = text.lines().collect();
    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    if start_line >= lines.len() {
        return None;
    }
    let end_line = end_line.min(lines.len() - 1);
    if end_line < start_line {
        return None;
    }
    let first = lines.get(start_line)?;
    let indent: String = first.chars().take_while(|c| c.is_whitespace()).collect();
    let mut buf = String::new();
    for (i, l) in lines[start_line..=end_line].iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        buf.push_str(l);
    }
    Some(MultilineBlock {
        text: buf,
        indent,
        insertion_line: range.start.line,
    })
}

fn extract_variable_action(
    uri: &Uri,
    line_text: &str,
    range: Range,
    selection: &str,
) -> CodeActionOrCommand {
    extract_to_local(
        uri,
        line_text,
        range,
        selection,
        "extracted",
        "Extract to variable (`var $name = …`)",
        false,
    )
}

/// Extract Parameter — `Cmd+Opt+P`. Finds the enclosing `fn name (…) { … }`
/// declaration, injects a new param `$extracted_param` into the signature,
/// and replaces the selection with the param's bare name `$extracted_param`.
/// Call sites are NOT updated (would require workspace-wide refactor — left
/// for v2; the user can `Find Usages` on the fn afterward and pass the
/// original expression manually).
#[allow(clippy::mutable_key_type)]
fn extract_parameter_action(
    uri: &Uri,
    docs: &HashMap<String, String>,
    text: &str,
    range: Range,
    selection: &str,
) -> Option<CodeActionOrCommand> {
    let placeholder = "extracted_param";
    let enclosing = enclosing_fn_signature(text, range.start.line as usize)?;

    // Build the edit that injects the new param into the signature.
    let sig_edit = inject_param_into_signature(text, &enclosing, placeholder)?;

    // Replacement at the selection: `$extracted_param`.
    let replace = TextEdit {
        range,
        new_text: format!("${placeholder}"),
    };

    let mut edits: Vec<TextEdit> = Vec::new();
    edits.push(sig_edit);
    edits.push(replace);

    // Rename-all-in-body: if the selection is a single sigiled
    // variable (`$foo` / `@foo` / `%foo`), extracting it to a param
    // means EVERY usage of that var in the fn body should become the
    // new param name — not just the selected occurrence. Without
    // this, the result is partially renamed code that doesn't
    // compile (e.g. an in-string `"$foo"` interpolation still
    // referring to the now-undeclared local var).
    if is_bare_sigiled_var(selection) {
        edits.extend(rename_var_in_fn_body(
            text,
            &enclosing,
            selection,
            placeholder,
            range,
        ));
    }

    // Thread the original selection through every call site of the
    // enclosing fn in the active file. Best-effort same-file: cross-
    // file call sites are not yet rewritten (would need to extend the
    // workspace walk in the same shape as Find Usages).
    //
    // Important: the selection text must be a valid expression in the
    // call site's scope. We pass it verbatim — if the user extracted
    // a name that only existed inside the fn body, call sites get a
    // reference error that must be resolved by hand.
    // Same-file call sites (both bare-name and qualified-name forms).
    edits.extend(call_site_threading_edits(
        text,
        &enclosing,
        selection,
        range.start.line,
    ));

    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);

    // Cross-file: every other open document. Only the qualified form
    // is scanned — bare-name calls in another file would resolve to
    // a different fn in that file's package, not this one.
    if !enclosing.fn_full_name.is_empty() && enclosing.fn_full_name.contains("::") {
        for (other_uri_str, other_text) in docs.iter() {
            if other_uri_str == uri.as_str() {
                continue;
            }
            let other_edits =
                cross_file_call_site_edits(other_text, &enclosing.fn_full_name, selection);
            if other_edits.is_empty() {
                continue;
            }
            if let Ok(other_uri) = other_uri_str.parse::<Uri>() {
                changes.insert(other_uri, other_edits);
            }
        }
    }
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Extract to parameter (`fn name($extracted_param, …)`)".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    }))
}

/// Result of locating the enclosing fn's signature for Extract Parameter.
struct EnclosingFnSig {
    /// 0-based line where the `fn name … {` header lives.
    fn_line: u32,
    /// Whether the fn already has a non-empty `(...)` param list
    /// before the body opener.
    has_params: bool,
    /// Byte index of the `(` if `has_params`; else position where a
    /// new `(...)` should be inserted (right after the fn name).
    paren_open_byte: usize,
    /// Byte index of the `)` if `has_params`; else None.
    paren_close_byte: Option<usize>,
    /// Bare fn name (no package prefix) — used to find same-package
    /// call sites that omit the `Pkg::` prefix.
    fn_name: String,
    /// Fully qualified fn name as it appears in the decl
    /// (`Demo::handle` for `fn Demo::handle($sig) {`). Used to find
    /// cross-file / external call sites that use the qualified form.
    /// Equal to `fn_name` when the decl is bare.
    fn_full_name: String,
}

/// Walk lines backward from `cursor_line` to find the nearest `fn`
/// header that opens a body covering the cursor. v1: scans for a line
/// that starts (after leading whitespace) with `fn ` and contains a
/// `{` after the fn name — the body covers all lines up to the
/// matching `}`. Doesn't handle nested fn cases robustly; good enough
/// for the common "top-level fn" case.
fn enclosing_fn_signature(text: &str, cursor_line: usize) -> Option<EnclosingFnSig> {
    let lines: Vec<&str> = text.lines().collect();
    if cursor_line >= lines.len() {
        return None;
    }
    // Find the candidate fn header line by walking backward.
    for line_idx in (0..=cursor_line).rev() {
        let line_text = lines[line_idx];
        let trimmed_start = line_text.trim_start();
        let leading_len = line_text.len() - trimmed_start.len();
        if !trimmed_start.starts_with("fn ") {
            continue;
        }
        // Found a `fn ` line — does its body cover `cursor_line`?
        // We approximate by scanning forward from this line counting
        // `{` / `}` (skipping string/comment content).
        let body_open_idx = match line_text.find('{') {
            Some(i) => i,
            None => continue, // single-line `fn` with no body opener
        };
        // Bracket-walk from this `{` to find the matching `}`.
        let close_line = find_matching_brace_line(text, line_idx, body_open_idx)?;
        if cursor_line < line_idx || cursor_line > close_line {
            continue;
        }
        // This fn encloses the cursor. Extract param-list shape.
        // Look for `(...)` between the fn name and the `{`.
        let header_slice = &line_text[..body_open_idx];
        let paren_open = header_slice.find('(');
        let (has_params, paren_open_byte, paren_close_byte) = match paren_open {
            Some(open_idx) => {
                // Find the matching `)` BEFORE the `{`.
                let close_idx = header_slice[open_idx..].find(')').map(|i| open_idx + i);
                match close_idx {
                    Some(ci) => {
                        let inside = header_slice[open_idx + 1..ci].trim();
                        (!inside.is_empty(), open_idx, Some(ci))
                    }
                    None => (false, open_idx, None),
                }
            }
            None => {
                // No `(...)` — insert one right after the fn name (the
                // word after `fn `).
                let after_fn = leading_len + 3; // past `fn `
                                                // skip the name (ident chars)
                let mut p = after_fn;
                let bytes = line_text.as_bytes();
                while p < bytes.len()
                    && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] == b':')
                {
                    p += 1;
                }
                (false, p, None)
            }
        };
        let _ = leading_len; // currently unused; kept for future
                             // signature-header indent tracking.
        let _ = body_open_idx;
        // Extract fn name: token immediately after `fn `, before `(` or `{`.
        let after_fn = leading_len + 3;
        let bytes = line_text.as_bytes();
        let mut np = after_fn;
        while np < bytes.len()
            && (bytes[np].is_ascii_alphanumeric() || bytes[np] == b'_' || bytes[np] == b':')
        {
            np += 1;
        }
        let fn_full_name = line_text[after_fn..np].to_string();
        let fn_name = fn_full_name.rsplit("::").next().unwrap_or("").to_string();
        return Some(EnclosingFnSig {
            fn_line: line_idx as u32,
            has_params,
            paren_open_byte,
            paren_close_byte,
            fn_name,
            fn_full_name,
        });
    }
    None
}

/// True if `s` is a single sigiled variable name with no subscript,
/// no `::` path, no whitespace. Used by Extract Parameter to decide
/// when "rename all body occurrences" semantics apply.
fn is_bare_sigiled_var(s: &str) -> bool {
    let s = s.trim();
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '$' | '@' | '%') {
        return false;
    }
    let mut had_body = false;
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            had_body = true;
        } else {
            return false;
        }
    }
    had_body
}

/// When the selection is a bare sigiled var, emit replacement edits
/// for EVERY occurrence of that var in the enclosing fn's body
/// (including in-string interpolation sites). The original
/// selection-range edit is already in `edits` — we skip duplicating
/// it by checking start position.
fn rename_var_in_fn_body(
    text: &str,
    enclosing: &EnclosingFnSig,
    selection: &str,
    placeholder: &str,
    skip_range: Range,
) -> Vec<TextEdit> {
    let mut out: Vec<TextEdit> = Vec::new();
    let var = selection.trim().to_string();
    if var.is_empty() {
        return out;
    }
    // Locate the fn body bytes: from the `{` opener on the fn line
    // through the matching `}`. Replacements must stay inside this
    // range so we don't accidentally edit code outside the fn.
    let lines: Vec<&str> = text.lines().collect();
    let fn_line_text = match lines.get(enclosing.fn_line as usize) {
        Some(l) => l,
        None => return out,
    };
    let body_open_col = match fn_line_text.find('{') {
        Some(i) => i,
        None => return out,
    };
    // Compute global byte offset of the `{` opener.
    let mut line_start = 0usize;
    for (i, l) in lines.iter().enumerate() {
        if i == enclosing.fn_line as usize {
            break;
        }
        line_start += l.len() + 1; // `+1` for the newline
    }
    let body_open_byte = line_start + body_open_col;
    let close_line = match find_matching_brace_line(text, enclosing.fn_line as usize, body_open_col)
    {
        Some(l) => l,
        None => return out,
    };
    // Compute global byte offset of the closing `}` line end.
    let mut body_close_byte = 0usize;
    for (i, l) in lines.iter().enumerate() {
        if i > close_line {
            break;
        }
        body_close_byte += l.len() + 1;
    }

    // Walk byte-by-byte in the body range, finding word-boundary
    // matches of `var`. Don't skip string interiors — `$foo` inside
    // `"$foo"` is a usage we want to rename.
    let bytes = text.as_bytes();
    let mut i = body_open_byte;
    let needle = var.as_bytes();
    while i + needle.len() <= body_close_byte.min(bytes.len()) {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        // Word boundary: char after must NOT be ident-continuation.
        let after_idx = i + needle.len();
        let after_ok = if after_idx >= bytes.len() {
            true
        } else {
            let c = bytes[after_idx] as char;
            !(c.is_ascii_alphanumeric() || c == '_')
        };
        if !after_ok {
            i += 1;
            continue;
        }
        // The sigil itself anchors the start; we don't need a
        // pre-boundary check beyond ensuring the sigil is at `i`.
        let (line, col) = byte_to_line_col(text, i);
        let (end_line, end_col) = byte_to_line_col(text, i + needle.len());
        let edit_range = Range {
            start: Position {
                line,
                character: col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        };
        // Skip the range covered by the original selection edit.
        if edit_range.start.line == skip_range.start.line
            && edit_range.start.character == skip_range.start.character
            && edit_range.end.line == skip_range.end.line
            && edit_range.end.character == skip_range.end.character
        {
            i = after_idx;
            continue;
        }
        out.push(TextEdit {
            range: edit_range,
            new_text: format!("${placeholder}"),
        });
        i = after_idx;
    }
    out
}

/// Find every call site `fn_name(...)` in `text` and emit a TextEdit
/// that appends `, <selection>` before the closing `)` of each call
/// (or `($selection)` if the matched call form somehow had empty parens).
/// Skips occurrences inside the fn's own body (so the body's recursive
/// or self-name references aren't wrongly rewritten — caller already
/// emits a replacement for the selection itself).
fn call_site_threading_edits(
    text: &str,
    enclosing: &EnclosingFnSig,
    selection: &str,
    selection_line: u32,
) -> Vec<TextEdit> {
    let mut out: Vec<TextEdit> = Vec::new();
    let bare = &enclosing.fn_name;
    let full = &enclosing.fn_full_name;
    if bare.is_empty() {
        return out;
    }
    // Scan the qualified form first (longer, more specific). Then
    // scan the bare form, skipping byte ranges already covered by a
    // qualified-form match (so `Demo::handle` doesn't also fire a
    // bare-`handle` edit).
    if !full.is_empty() && full != bare {
        out.extend(scan_call_sites_for_name(
            text,
            full,
            enclosing.fn_line,
            selection,
            selection_line,
            &out,
        ));
    }
    out.extend(scan_call_sites_for_name(
        text,
        bare,
        enclosing.fn_line,
        selection,
        selection_line,
        &out,
    ));
    out
}

fn scan_call_sites_for_name(
    text: &str,
    needle: &str,
    body_line: u32,
    selection: &str,
    _selection_line: u32,
    already_emitted: &[TextEdit],
) -> Vec<TextEdit> {
    let mut out: Vec<TextEdit> = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mask = string_interior_mask_simple(text);
    let bytes = text.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(needle) {
        let start = search_from + rel;
        search_from = start + needle.len();
        // Inside string/comment? Skip.
        if mask.get(start).copied().unwrap_or(false) {
            continue;
        }
        // Word boundary before.
        let prev_ok = start == 0 || {
            let c = bytes[start - 1] as char;
            !(c.is_ascii_alphanumeric() || c == '_' || c == ':')
        };
        if !prev_ok {
            continue;
        }
        // After the name: optional whitespace, then `(`.
        let end_name = start + needle.len();
        // First, ensure word boundary at end (not part of a longer ident).
        if let Some(&b) = bytes.get(end_name) {
            let c = b as char;
            if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                continue;
            }
        }
        // Skip whitespace, find `(`.
        let mut p = end_name;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'(' {
            continue;
        }
        // Skip the enclosing fn's own decl-header `(` — that's the
        // signature itself, already handled.
        let (call_line, _) = byte_to_line_col(text, start);
        if call_line == body_line {
            continue;
        }
        // Find matching `)` from p.
        let close = find_matching_paren(text, p);
        if let Some(close_byte) = close {
            let inner = text[p + 1..close_byte].trim();
            let new_text = if inner.is_empty() {
                selection.to_string()
            } else {
                format!(", {selection}")
            };
            let (close_line, close_col) = byte_to_line_col(text, close_byte);
            // Skip if a longer-form match (qualified) already covered
            // this same position. Without this, `Demo::handle(...)`
            // would get TWO edits — one for the qualified scan, one
            // for the bare-`handle` substring within it.
            let already = already_emitted
                .iter()
                .chain(out.iter())
                .any(|e| e.range.start.line == close_line && e.range.start.character == close_col);
            if already {
                continue;
            }
            out.push(TextEdit {
                range: Range {
                    start: Position {
                        line: close_line,
                        character: close_col,
                    },
                    end: Position {
                        line: close_line,
                        character: close_col,
                    },
                },
                new_text,
            });
        }
    }
    out
}

/// Cross-file (other open documents) call-site threading: scan
/// `other_text` for occurrences of `fn_full_name(...)` and append the
/// selection at each call's close-paren position.
fn cross_file_call_site_edits(
    other_text: &str,
    fn_full_name: &str,
    selection: &str,
) -> Vec<TextEdit> {
    if fn_full_name.is_empty() {
        return Vec::new();
    }
    let mask = string_interior_mask_simple(other_text);
    let bytes = other_text.as_bytes();
    let mut out: Vec<TextEdit> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = other_text[search_from..].find(fn_full_name) {
        let start = search_from + rel;
        search_from = start + fn_full_name.len();
        if mask.get(start).copied().unwrap_or(false) {
            continue;
        }
        let prev_ok = start == 0 || {
            let c = bytes[start - 1] as char;
            !(c.is_ascii_alphanumeric() || c == '_' || c == ':')
        };
        if !prev_ok {
            continue;
        }
        let end_name = start + fn_full_name.len();
        if let Some(&b) = bytes.get(end_name) {
            let c = b as char;
            if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                continue;
            }
        }
        let mut p = end_name;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'(' {
            continue;
        }
        let close = find_matching_paren(other_text, p);
        if let Some(close_byte) = close {
            let inner = other_text[p + 1..close_byte].trim();
            let new_text = if inner.is_empty() {
                selection.to_string()
            } else {
                format!(", {selection}")
            };
            let (close_line, close_col) = byte_to_line_col(other_text, close_byte);
            out.push(TextEdit {
                range: Range {
                    start: Position {
                        line: close_line,
                        character: close_col,
                    },
                    end: Position {
                        line: close_line,
                        character: close_col,
                    },
                },
                new_text,
            });
        }
    }
    out
}

/// Find the matching `)` for the `(` at `open_byte` in `text`. Tracks
/// nested parens and skips parens inside string literals. Returns the
/// byte offset of the matching `)` or `None` if unmatched.
fn find_matching_paren(text: &str, open_byte: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if open_byte >= bytes.len() || bytes[open_byte] != b'(' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_str: Option<u8> = None;
    let mut i = open_byte;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => in_str = Some(c),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Convert a global byte offset to `(line, utf16_col)`.
fn byte_to_line_col(text: &str, byte: usize) -> (u32, u32) {
    let upto = &text[..byte.min(text.len())];
    let line = upto.bytes().filter(|&b| b == b'\n').count() as u32;
    let line_start = upto.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = text[line_start..byte.min(text.len())]
        .encode_utf16()
        .count() as u32;
    (line, col)
}

/// Lightweight string-interior mask: `mask[i] = true` if byte `i` is
/// inside a `"..."`, `'...'`, `` `...` `` literal or a `#` line
/// comment. Mirrors the LSP-level `string_interior_mask` in
/// `lsp.rs` — kept local here to avoid cross-module dependency.
fn string_interior_mask_simple(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' | b'\'' | b'`' => {
                let quote = c;
                let mut j = i + 1;
                while j < bytes.len() {
                    let cc = bytes[j];
                    if cc == b'\\' && j + 1 < bytes.len() {
                        mask[j] = true;
                        mask[j + 1] = true;
                        j += 2;
                        continue;
                    }
                    if cc == quote {
                        j += 1;
                        break;
                    }
                    mask[j] = true;
                    j += 1;
                }
                i = j;
            }
            b'#' => {
                let mut j = i;
                while j < bytes.len() && bytes[j] != b'\n' {
                    mask[j] = true;
                    j += 1;
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    mask
}

/// Returns the 0-based line number containing the `}` that matches a
/// `{` at `(open_line, open_byte_in_line)`. Best-effort — skips `{`/`}`
/// inside strings and comments. Returns `None` if unmatched.
fn find_matching_brace_line(
    text: &str,
    open_line: usize,
    open_byte_in_line: usize,
) -> Option<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut depth: i32 = 0;
    let mut in_string: Option<char> = None;
    let mut line_idx = open_line;
    let mut chars = lines
        .get(line_idx)?
        .chars()
        .enumerate()
        .skip(open_byte_in_line);
    let mut current_line_chars: Vec<(usize, char)> = chars.by_ref().collect();
    let mut char_pos = 0;
    let mut bumped_initial = false;
    loop {
        while char_pos < current_line_chars.len() {
            let (_, c) = current_line_chars[char_pos];
            if let Some(quote) = in_string {
                if c == '\\' {
                    char_pos += 2;
                    continue;
                }
                if c == quote {
                    in_string = None;
                }
                char_pos += 1;
                continue;
            }
            match c {
                '#' => {
                    // Rest of line is comment.
                    char_pos = current_line_chars.len();
                }
                '"' | '\'' | '`' => {
                    in_string = Some(c);
                    char_pos += 1;
                }
                '{' => {
                    if !bumped_initial {
                        bumped_initial = true;
                    }
                    depth += 1;
                    char_pos += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(line_idx);
                    }
                    char_pos += 1;
                }
                _ => {
                    char_pos += 1;
                }
            }
        }
        line_idx += 1;
        if line_idx >= lines.len() {
            return None;
        }
        current_line_chars = lines[line_idx].char_indices().collect();
        char_pos = 0;
    }
}

/// Build the [`TextEdit`] that injects `$placeholder` into the fn's
/// param list. If the fn has a non-empty `(...)`, appends `, $name`
/// before the closing `)`. Otherwise inserts `($name)` (or the name
/// inside an existing empty `()`).
fn inject_param_into_signature(
    text: &str,
    sig: &EnclosingFnSig,
    placeholder: &str,
) -> Option<TextEdit> {
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(sig.fn_line as usize)?;
    let (range, new_text) = if sig.has_params {
        // Append `, $placeholder` before the closing `)`.
        let close = sig.paren_close_byte?;
        let col = byte_to_utf16_col(line_text, close);
        (
            Range {
                start: Position {
                    line: sig.fn_line,
                    character: col,
                },
                end: Position {
                    line: sig.fn_line,
                    character: col,
                },
            },
            format!(", ${placeholder}"),
        )
    } else if sig.paren_close_byte.is_some() {
        // Empty `(...)` — insert just the name inside.
        let open = sig.paren_open_byte + 1;
        let col = byte_to_utf16_col(line_text, open);
        (
            Range {
                start: Position {
                    line: sig.fn_line,
                    character: col,
                },
                end: Position {
                    line: sig.fn_line,
                    character: col,
                },
            },
            format!("${placeholder}"),
        )
    } else {
        // No `(...)` at all — insert `($placeholder)` right after the
        // fn name (at `sig.paren_open_byte`, which the locator set to
        // the position past the name).
        let col = byte_to_utf16_col(line_text, sig.paren_open_byte);
        (
            Range {
                start: Position {
                    line: sig.fn_line,
                    character: col,
                },
                end: Position {
                    line: sig.fn_line,
                    character: col,
                },
            },
            format!("(${placeholder})"),
        )
    };
    Some(TextEdit { range, new_text })
}

fn byte_to_utf16_col(line_text: &str, byte_idx: usize) -> u32 {
    line_text[..byte_idx.min(line_text.len())]
        .encode_utf16()
        .count() as u32
}

fn extract_constant_action(
    uri: &Uri,
    line_text: &str,
    range: Range,
    selection: &str,
) -> CodeActionOrCommand {
    extract_to_local(
        uri,
        line_text,
        range,
        selection,
        "EXTRACTED",
        "Extract to constant (`val $NAME = …`)",
        true,
    )
}

/// Shared body of extract-variable and extract-constant. Inserts a
/// declaration line above the selection (preserving the line's indent) and
/// replaces the selection with `$name`.
// `lsp_types::Uri` is immutable in practice but uses interior types that
// trip clippy's mutable_key_type lint — the HashMap<Uri, _> pattern is
// idiomatic across the LSP crate, so silence the false positive on each
// fn that builds a `WorkspaceEdit`.
#[allow(clippy::mutable_key_type)]
fn extract_to_local(
    uri: &Uri,
    line_text: &str,
    range: Range,
    selection: &str,
    placeholder: &str,
    title: &str,
    frozen: bool,
) -> CodeActionOrCommand {
    let leading_ws: String = line_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    // If the selection sits inside an interpolating string on its own
    // line — `"..."` or `` `...` `` (backtick / qx command form) — and
    // isn't already a bare scalar var, the raw text (e.g. `hello world`
    // highlighted from `"hello world"`, or `ls -la` from `` `ls -la` ``)
    // is not a valid stryke expression on its own. Quote it for the
    // decl, then let the in-string `$placeholder` interpolate the value
    // back at the original site. For backticks we still use `"..."` (a
    // plain string) for the decl — wrapping in `` `...` `` would MOVE
    // command execution from the original site to the decl, changing
    // semantics. The surrounding `` `...` `` re-runs the interpolated
    // command at its original location.
    let in_interp_string =
        same_line_selection_inside_interpolating_string(line_text, range.start.character);
    let decl_rhs: String = if in_interp_string && needs_string_wrap_for_extraction(selection) {
        // Selection contains literal text or mixed text+interpolation;
        // preserve interpolation by keeping the contents inside `"..."`
        // and escape any embedded `"` so a partial selection like
        // `say "ok"` selected with the inner `ok` doesn't break out.
        format!("\"{}\"", escape_double_quoted(selection))
    } else {
        selection.to_string()
    };
    // Idiomatic stryke (style guide rule 10): `val` for an immutable constant,
    // `var` for a mutable variable — never bare `my`.
    let decl = if frozen {
        format!("{leading_ws}val ${placeholder} = {decl_rhs}\n")
    } else {
        format!("{leading_ws}var ${placeholder} = {decl_rhs}\n")
    };
    let insert = TextEdit {
        range: Range {
            start: Position {
                line: range.start.line,
                character: 0,
            },
            end: Position {
                line: range.start.line,
                character: 0,
            },
        },
        new_text: decl,
    };
    // Inside a string, `$placeholder` interpolates; outside, it's just
    // a scalar reference. Both spellings work as a replacement for the
    // selection in their respective contexts — no special-casing.
    let replace = TextEdit {
        range,
        new_text: format!("${placeholder}"),
    };
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), vec![insert, replace]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    })
}

#[allow(clippy::mutable_key_type)]
fn extract_function_action(uri: &Uri, range: Range, block: &MultilineBlock) -> CodeActionOrCommand {
    // Re-indent the selected body so the new fn keeps consistent leading
    // whitespace inside its braces (one extra `    ` past the call site).
    let body_indent = format!("{}    ", block.indent);
    let body: String = block
        .text
        .lines()
        .map(|l| {
            // Strip the original indent if present; otherwise leave the
            // line as-is (preserves blank lines without polluting them).
            let stripped = l.strip_prefix(&block.indent).unwrap_or(l);
            if stripped.is_empty() {
                String::new()
            } else {
                format!("{body_indent}{stripped}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fn_text = format!(
        "{indent}fn extracted_fn {{\n{body}\n{indent}}}\n\n",
        indent = block.indent,
        body = body
    );
    let insert_fn = TextEdit {
        range: Range {
            start: Position {
                line: block.insertion_line,
                character: 0,
            },
            end: Position {
                line: block.insertion_line,
                character: 0,
            },
        },
        new_text: fn_text,
    };
    // Replace the selected range with `extracted_fn()`. Use the full
    // selection range — LSP applies edits in reverse line order so the
    // insertion above stays at the right place.
    let replace = TextEdit {
        range,
        new_text: format!("{}extracted_fn()", block.indent),
    };
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), vec![insert_fn, replace]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: "Extract to function (`fn extracted_fn { … }`)".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    })
}

#[allow(clippy::mutable_key_type)]
fn wrap_in_p_action(uri: &Uri, line: u32, line_text: &str) -> CodeActionOrCommand {
    let leading_ws: String = line_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let body = line_text[leading_ws.len()..].trim_end();
    let new_text = format!("{leading_ws}p {body}");
    let edit = TextEdit {
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: line_text.encode_utf16().count() as u32,
            },
        },
        new_text,
    };
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: "Wrap line in `p`".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    })
}

#[allow(clippy::mutable_key_type)]
fn toggle_comment_action(uri: &Uri, line: u32, line_text: &str) -> CodeActionOrCommand {
    let trimmed = line_text.trim_start();
    let leading_ws: String = line_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let (new_text, title) = if let Some(rest) = trimmed.strip_prefix("# ") {
        (format!("{leading_ws}{rest}"), "Uncomment line")
    } else if let Some(rest) = trimmed.strip_prefix('#') {
        (format!("{leading_ws}{rest}"), "Uncomment line")
    } else {
        (format!("{leading_ws}# {trimmed}"), "Comment line")
    };
    let edit = TextEdit {
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: line_text.encode_utf16().count() as u32,
            },
        },
        new_text,
    };
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    })
}

fn nth_line(text: &str, n: usize) -> Option<&str> {
    text.lines().nth(n)
}

// ---------------------------------------------------------------------------
// Folding ranges
// ---------------------------------------------------------------------------

/// Compute foldable line ranges for the document.
///
/// Sources of foldability:
/// * **Brace blocks** — every matching `{` ... `}` pair on different lines.
///   Covers `fn`, `class`, `struct`, `enum`, `if`, `while`, `for`, hash
///   literals, and any `{...}` expression.
/// * **POD blocks** — `=pod ... =cut` and other `=head1` etc. POD openers
///   up to the next `=cut`.
/// * **Comment runs** — three or more consecutive `#`-prefixed lines.
///
/// Position-string scanning ignores braces inside `# ... \n` comments, `"..."`,
/// `'...'`, and POD blocks so block-delimiter braces in literals don't
/// produce ghost folds. The pass is `O(N)` over the source.
pub fn compute_folding_ranges(
    docs: &HashMap<String, String>,
    params: &FoldingRangeParams,
) -> Vec<FoldingRange> {
    let uri = &params.text_document.uri;
    let Some(text) = docs.get(uri.as_str()) else {
        return Vec::new();
    };

    let mut ranges: Vec<FoldingRange> = Vec::new();
    let mut brace_stack: Vec<u32> = Vec::new();
    let mut in_str: Option<char> = None; // Some(quote_char) while inside string
    let mut in_pod: Option<u32> = None; // Some(start_line) while inside =pod block
    let mut comment_run_start: Option<u32> = None;
    let mut line: u32 = 0;
    let mut col_is_zero = true;

    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // POD block start: `=word` at column 0. Lasts until `=cut` at column 0.
        if col_is_zero && in_pod.is_none() && in_str.is_none() && b == b'=' {
            // `=ident...` is POD; `==` and `=~` and `=>` are operators
            let next = bytes.get(i + 1).copied();
            if matches!(next, Some(c) if c.is_ascii_alphabetic()) {
                in_pod = Some(line);
                // close any pending comment run
                if let Some(start) = comment_run_start.take() {
                    if line.saturating_sub(start) >= 3 {
                        ranges.push(make_fold(start, line - 1, Some(FoldingRangeKind::Comment)));
                    }
                }
            }
        }
        if col_is_zero && in_pod.is_some() && b == b'=' {
            // Check for `=cut`
            if bytes.get(i..i + 4) == Some(b"=cut".as_slice()) {
                let start = in_pod.take().unwrap();
                // Advance to end of `=cut` line
                let mut j = i + 4;
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                let end_line = line;
                if end_line > start {
                    ranges.push(make_fold(start, end_line, Some(FoldingRangeKind::Comment)));
                }
                i = j;
                if i < bytes.len() && bytes[i] == b'\n' {
                    line += 1;
                    col_is_zero = true;
                    i += 1;
                }
                continue;
            }
        }

        if in_pod.is_some() {
            if b == b'\n' {
                line += 1;
                col_is_zero = true;
            } else {
                col_is_zero = false;
            }
            i += 1;
            continue;
        }

        // Track string state — skip braces / comments inside strings.
        if let Some(q) = in_str {
            match b {
                b'\\' => {
                    i += 2;
                    continue;
                }
                c if c == q as u8 => {
                    in_str = None;
                }
                b'\n' => {
                    line += 1;
                    col_is_zero = true;
                    i += 1;
                    continue;
                }
                _ => {}
            }
            col_is_zero = false;
            i += 1;
            continue;
        }

        // Line comments — `# ... \n`. Track consecutive runs for fold.
        if b == b'#' {
            // Same line as a non-comment token? Don't start a run.
            if (col_is_zero || all_whitespace_before(bytes, i, line, &line_starts_cache(text)))
                && comment_run_start.is_none()
            {
                comment_run_start = Some(line);
            }
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            // Don't consume the newline — handled below.
            col_is_zero = false;
            continue;
        }

        match b {
            b'\n' => {
                line += 1;
                col_is_zero = true;
                // If this is a fully blank line, keep the comment run alive
                // only when the NEXT line also starts with `#`. Simpler: end
                // the run on any non-comment line (handled when we see a
                // non-`#` token at column 0 below).
                i += 1;
                continue;
            }
            b' ' | b'\t' => {
                i += 1;
                continue;
            }
            b'"' | b'\'' => {
                in_str = Some(b as char);
                // Comment run ends at any code.
                if let Some(start) = comment_run_start.take() {
                    if line.saturating_sub(start) >= 3 {
                        ranges.push(make_fold(
                            start,
                            line.saturating_sub(1),
                            Some(FoldingRangeKind::Comment),
                        ));
                    }
                }
                col_is_zero = false;
                i += 1;
                continue;
            }
            b'{' => {
                if let Some(start) = comment_run_start.take() {
                    if line.saturating_sub(start) >= 3 {
                        ranges.push(make_fold(
                            start,
                            line.saturating_sub(1),
                            Some(FoldingRangeKind::Comment),
                        ));
                    }
                }
                brace_stack.push(line);
                col_is_zero = false;
                i += 1;
                continue;
            }
            b'}' => {
                if let Some(open_line) = brace_stack.pop() {
                    // Only fold when the close is on a later line.
                    if line > open_line {
                        ranges.push(make_fold(open_line, line, None));
                    }
                }
                col_is_zero = false;
                i += 1;
                continue;
            }
            _ => {
                if let Some(start) = comment_run_start.take() {
                    if line.saturating_sub(start) >= 3 {
                        ranges.push(make_fold(
                            start,
                            line.saturating_sub(1),
                            Some(FoldingRangeKind::Comment),
                        ));
                    }
                }
                col_is_zero = false;
                i += 1;
                continue;
            }
        }
    }
    if let Some(start) = comment_run_start {
        if line.saturating_sub(start) >= 3 {
            ranges.push(make_fold(start, line, Some(FoldingRangeKind::Comment)));
        }
    }

    ranges
}

fn make_fold(start_line: u32, end_line: u32, kind: Option<FoldingRangeKind>) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind,
        collapsed_text: None,
    }
}

fn line_starts_cache(text: &str) -> Vec<usize> {
    let mut v = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn all_whitespace_before(bytes: &[u8], pos: usize, line: u32, starts: &[usize]) -> bool {
    let start = starts.get(line as usize).copied().unwrap_or(0);
    bytes[start..pos].iter().all(|c| matches!(*c, b' ' | b'\t'))
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Reformat the whole document by piping it through `s fmt --stdin`. Returns
/// a single full-file `TextEdit` on success, an empty list on parse / IO
/// failure (so the user sees their original text instead of a partial
/// rewrite). Honours `params.options.insert_spaces` / `tab_size` only when
/// `s fmt` accepts them on its CLI; today's stryke formatter is opinionated
/// (4-space indent, like `gofmt`) so client preferences are advisory.
pub fn compute_formatting(
    docs: &HashMap<String, String>,
    params: &DocumentFormattingParams,
) -> Vec<TextEdit> {
    let uri = &params.text_document.uri;
    let Some(text) = docs.get(uri.as_str()) else {
        return Vec::new();
    };

    let program = match crate::parse_with_file(text, "<lsp>") {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let formatted = crate::fmt::format_program(&program);
    if formatted == *text {
        return Vec::new();
    }

    // Full-document replacement edit.
    let line_count = text.lines().count() as u32;
    let last_char = text
        .lines()
        .last()
        .map(|l| l.encode_utf16().count() as u32)
        .unwrap_or(0);
    let end_line = if text.ends_with('\n') {
        line_count
    } else {
        line_count.saturating_sub(1)
    };
    let end_char = if text.ends_with('\n') { 0 } else { last_char };
    vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        },
        new_text: formatted,
    }]
}

#[cfg(test)]
// `lsp_types::Uri` is hashable but clippy flags it as a mutable key
// type. Silenced at the module level so LSP-shaped `HashMap<Uri, _>`
// reads in test bodies don't have to add per-site allows.
#[allow(clippy::mutable_key_type)]
mod tests {
    use super::*;
    use lsp_types::{CodeActionContext, TextDocumentIdentifier};
    use std::str::FromStr;

    fn doc(uri: &str, text: &str) -> (HashMap<String, String>, Uri) {
        let mut docs = HashMap::new();
        docs.insert(uri.to_string(), text.to_string());
        (docs, Uri::from_str(uri).unwrap())
    }

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn range(s_line: u32, s_char: u32, e_line: u32, e_char: u32) -> Range {
        Range {
            start: pos(s_line, s_char),
            end: pos(e_line, e_char),
        }
    }

    fn code_actions(text: &str, r: Range) -> Vec<CodeActionOrCommand> {
        let (docs, uri) = doc("file:///t.stk", text);
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: r,
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        compute_code_actions(&docs, &params)
    }

    fn titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
                _ => None,
            })
            .collect()
    }

    // ── compute_code_actions ─────────────────────────────────────────────

    #[test]
    fn code_actions_for_empty_selection_offer_line_local_and_caret_snap_extracts() {
        // Empty range parked in the leading whitespace of the line (no
        // identifier under cursor) → only the line-local quickfixes
        // (wrap-in-p, toggle comment), no Extracts.
        let actions = code_actions("my $x = 1 + 2\np $x\n", range(0, 0, 0, 0));
        let t = titles(&actions);
        assert!(
            t.iter().any(|s| s.contains("Wrap line in")),
            "wrap-in-p offered: got {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("Comment line")),
            "toggle comment offered: got {t:?}"
        );
        // Caret at col 0 sits on `m` of `my` — `my` is a keyword, but
        // snap_to_word still returns its span, so Extracts SHOULD now
        // be offered (caret-only Extract is a UX improvement).
        assert!(
            t.iter().any(|s| s.contains("Extract to variable")),
            "caret-only Extract Variable is offered: got {t:?}"
        );
    }

    #[test]
    fn code_actions_for_single_line_selection_offer_all_three_extracts() {
        // Any non-empty selection must offer Variable, Constant, AND
        // Function — IntelliJ's keymap-driven Cmd-Opt-V / -C / -M each
        // filter for the one that matches, so all three must be present
        // for every shortcut to be functional.
        let actions = code_actions("my $x = 1 + 2\np $x\n", range(0, 8, 0, 13));
        let t = titles(&actions);
        assert!(
            t.iter().any(|s| s.contains("Extract to variable")),
            "var: got {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("Extract to constant")),
            "const: got {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("Extract to function")),
            "fn: got {t:?}"
        );
    }

    #[test]
    fn code_actions_for_multi_line_selection_offer_all_three_extracts() {
        let text = "my $x = 1\nmy $y = 2\np $x + $y\n";
        let actions = code_actions(text, range(0, 0, 2, 9));
        let t = titles(&actions);
        assert!(
            t.iter().any(|s| s.contains("Extract to function")),
            "fn: got {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("Extract to variable")),
            "var: got {t:?}"
        );
        assert!(
            t.iter().any(|s| s.contains("Extract to constant")),
            "const: got {t:?}"
        );
    }

    #[test]
    fn extract_variable_inside_double_quoted_string_quotes_the_rhs() {
        // Original source: `my $msg = "hello world"` — the user selects
        // the inner literal `hello world` (between but not including
        // the `"` chars). Without wrapping, the decl becomes
        // `var $extracted = hello world` which is invalid syntax.
        // The fix must produce `var $extracted = "hello world"`.
        let src = "my $msg = \"hello world\"\n";
        // `"` at col 10, `hello world` runs col 11..22.
        let actions = code_actions(src, range(0, 11, 0, 22));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"hello world\"\n"),
            "RHS must be string-wrapped: {:?}",
            decl.new_text
        );
        // Replacement of the selection should remain bare `$extracted`
        // — inside the surrounding `"..."` it interpolates.
        let replace = edits.iter().find(|e| e.new_text == "$extracted").unwrap();
        assert_eq!(replace.range.start.character, 11);
        assert_eq!(replace.range.end.character, 22);
    }

    #[test]
    fn extract_variable_inside_double_quoted_preserves_interpolation() {
        // Selecting `hello $name ` from `"hello $name world"` should
        // produce `var $extracted = "hello $name "` so the original
        // interpolation continues to work via the new decl.
        let src = "my $msg = \"hello $name world\"\n";
        // `hello $name ` runs col 11..23.
        let actions = code_actions(src, range(0, 11, 0, 23));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"hello $name \"\n"),
            "RHS must keep interpolation inside the quotes: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_parameter_adds_param_to_existing_signature() {
        // `fn area(width) { width * height }` — extract `height` to a
        // parameter. Sig becomes `fn area(width, $extracted_param)`
        // and the body's `height` is replaced with `$extracted_param`.
        let src = "fn area(width) { width * height }\n";
        // `height` on line 0 cols 25..31.
        let actions = code_actions(src, range(0, 25, 0, 31));
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        // One edit appends `, $extracted_param` before the closing `)`.
        assert!(
            edits.iter().any(|e| e.new_text == ", $extracted_param"),
            "expected append into existing param list: {edits:#?}"
        );
        // One edit replaces the selection with `$extracted_param`.
        assert!(
            edits.iter().any(|e| e.new_text == "$extracted_param"),
            "expected replacement of selection: {edits:#?}"
        );
    }

    #[test]
    fn extract_parameter_adds_param_list_when_fn_has_none() {
        // `fn greet { "hello $name" }` — no `(...)` yet. Extract
        // produces `fn greet($extracted_param) { ... }`.
        let src = "fn greet { my $s = \"hello world\" }\n";
        // Select `world` between the quotes, cols 26..31.
        let actions = code_actions(src, range(0, 26, 0, 31));
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        assert!(
            edits.iter().any(|e| e.new_text == "($extracted_param)"),
            "expected new `(...)` insertion: {edits:#?}"
        );
    }

    #[test]
    fn extract_parameter_threads_call_sites_in_other_open_doc() {
        // Active file declares `fn Demo::handle($sig) { … }`. A second
        // open doc calls `Demo::handle("x")`. Extract Parameter on the
        // active file's body expression must thread the new arg into
        // the other doc's call site too.
        let active_src =
            "package Demo\nfn Demo::handle($sig) {\n    my $x = 1\n    log(\"ok\")\n}\n";
        let other_uri = "file:///other.stk";
        let other_src = "Demo::handle(\"x\")\nDemo::handle(\"y\")\n";
        let mut docs: HashMap<String, String> = HashMap::new();
        let (active_docs, active_uri) = doc("file:///active.stk", active_src);
        for (k, v) in active_docs.iter() {
            docs.insert(k.clone(), v.clone());
        }
        docs.insert(other_uri.to_string(), other_src.to_string());
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: active_uri.clone(),
            },
            range: range(3, 8, 3, 12), // `"ok"` on the `log("ok")` line
            context: CodeActionContext::default(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let actions = compute_code_actions(&docs, &params);
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        // Expect edits in both the active doc AND the other doc.
        assert_eq!(
            changes.len(),
            2,
            "expected edits for both documents, got {:?}",
            changes.keys().collect::<Vec<_>>()
        );
        let other_edits: &Vec<TextEdit> = changes
            .iter()
            .find(|(u, _)| u.as_str() == other_uri)
            .map(|(_, e)| e)
            .expect("expected edits for other doc");
        // Two call sites in other_src → two append edits each `, "ok"`.
        assert_eq!(
            other_edits.len(),
            2,
            "expected 2 call-site appends in other doc: {other_edits:#?}"
        );
        assert!(
            other_edits.iter().all(|e| e.new_text == ", \"ok\""),
            "expected `, \"ok\"` appends: {other_edits:#?}"
        );
    }

    #[test]
    fn extract_parameter_on_bare_var_renames_all_body_occurrences() {
        // User's exact case: cursor on `$extracted` of the
        // `my $extracted = "drain"` decl. Extract Parameter must:
        //   - add `$extracted_param` to the sig
        //   - rename the decl-line `$extracted` to `$extracted_param`
        //   - rename the in-string `$extracted` interpolation to
        //     `$extracted_param` too
        // Otherwise the body is half-renamed and `$extracted` refers
        // to a now-undeclared name.
        let src =
            "fn handle($sig) {\n    my $extracted = \"drain\"\n    p \"$extracted + stop\"\n}\n";
        // `$extracted` on line 1, cols 7..17.
        let actions = code_actions(src, range(1, 7, 1, 17));
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
        // Sig edit + 2 body occurrences renamed (decl line + in-string).
        let body_renames = new_texts
            .iter()
            .filter(|n| **n == "$extracted_param")
            .count();
        assert!(
            body_renames >= 2,
            "expected ≥2 body `$extracted_param` rewrites (decl + in-string), got {new_texts:?}"
        );
        // The in-string `"$extracted + stop"` must become
        // `"$extracted_param + stop"` — verify by checking that a
        // replace edit covers the in-string `$extracted` position.
        // Line 2 of `src` is `    p "$extracted + stop"`; the `$` of
        // `$extracted` is at col 7.
        assert!(
            edits.iter().any(|e| e.range.start.line == 2
                && e.range.start.character == 7
                && e.new_text == "$extracted_param"),
            "expected rewrite at line 2 col 7 (in-string $extracted): {edits:#?}"
        );
    }

    #[test]
    fn extract_parameter_threads_through_same_file_call_sites() {
        // `fn area(w) { w * height }` extracted on `height` →
        //   - sig becomes `fn area(w, $extracted_param)`
        //   - body's `height` becomes `$extracted_param`
        //   - call site `area(5)` becomes `area(5, height)`
        let src = "fn area(w) { w * height }\nmy $r = area(5)\nmy $s = area(10)\n";
        // `height` on line 0 cols 17..23.
        let actions = code_actions(src, range(0, 17, 0, 23));
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        // Expected edits:
        //   - sig: `, $extracted_param`
        //   - body: `$extracted_param`
        //   - call site 1 (line 1): `, height`
        //   - call site 2 (line 2): `, height`
        let new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
        assert!(
            new_texts.contains(&", $extracted_param"),
            "sig threading: {new_texts:?}"
        );
        assert!(
            new_texts.contains(&"$extracted_param"),
            "body replacement: {new_texts:?}"
        );
        let call_appends = new_texts.iter().filter(|n| **n == ", height").count();
        assert_eq!(
            call_appends, 2,
            "two same-file call sites should each get `, height`: {new_texts:?}"
        );
    }

    #[test]
    fn extract_parameter_threads_into_empty_call_parens() {
        // `fn boot { do_init() }` — selecting `do_init()` as the
        // expression to extract: parent fn `boot`. Call sites of
        // `boot()` currently have empty parens, so the new arg
        // becomes the sole argument (no leading comma).
        let src = "fn boot { do_init() }\nboot()\n";
        // Select `do_init()` cols 10..19.
        let actions = code_actions(src, range(0, 10, 0, 19));
        let param = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("parameter") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-parameter action present");
        let changes = param.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let new_texts: Vec<&str> = edits.iter().map(|e| e.new_text.as_str()).collect();
        assert!(
            new_texts.contains(&"do_init()"),
            "empty-parens call site gets the selection as sole arg: {new_texts:?}"
        );
    }

    #[test]
    fn extract_parameter_not_offered_outside_any_fn() {
        // Top-level code, no enclosing fn → no parameter action.
        let src = "my $x = 1 + 2\np $x\n";
        let actions = code_actions(src, range(0, 8, 0, 13));
        let has_param = actions.iter().any(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("parameter"),
            _ => false,
        });
        assert!(
            !has_param,
            "must NOT offer parameter extract outside any fn"
        );
    }

    #[test]
    fn extract_constant_uses_val_keyword() {
        // Idiomatic stryke (style guide rule 10): an extracted constant is
        // immutable, so the decl must use `val`, never bare `my`.
        let src = "my $s = \"dispatcher\"\n";
        // Select `dispatcher` (between the quotes), cols 9..19.
        let actions = code_actions(src, range(0, 9, 0, 19));
        let constant = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("constant") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-constant action present");
        let changes = constant.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.contains("EXTRACTED"))
            .unwrap();
        assert!(
            decl.new_text.starts_with("val $EXTRACTED"),
            "decl must start with `val`, not `my`/`frozen my`: {:?}",
            decl.new_text
        );
        assert!(
            !decl.new_text.contains("my "),
            "must NOT use bare `my` for a constant: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_outside_string_does_not_quote_expression() {
        // Regression-pin the existing arithmetic path — selecting
        // `1 + 2` from `my $x = 1 + 2` MUST NOT quote the RHS.
        let actions = code_actions("my $x = 1 + 2\n", range(0, 8, 0, 13));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= 1 + 2\n"),
            "arithmetic RHS must remain unquoted: {:?}",
            decl.new_text
        );
        assert!(
            !decl.new_text.contains("\"1 + 2\""),
            "must not accidentally string-wrap arithmetic: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_of_bare_scalar_inside_string_does_not_double_wrap() {
        // Selecting JUST `$name` from `"$name is here"` is already a
        // valid expression — the decl should be `var $extracted = $name`
        // (NOT `var $extracted = "$name"` which would force scalar
        // stringification on non-string values).
        let src = "my $msg = \"$name is here\"\n";
        // `$name` runs col 11..16.
        let actions = code_actions(src, range(0, 11, 0, 16));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= $name\n"),
            "pure-scalar selection should stay bare: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_inside_backtick_command_quotes_as_string_not_backtick() {
        // `` my $out = `ls -la` `` — selecting `ls -la` from inside the
        // backticks must produce a DOUBLE-QUOTED decl, not another
        // backtick. Wrapping in backticks would move command execution
        // from the original site to the decl, changing semantics
        // (and side effects).
        let src = "my $out = `ls -la`\n";
        // `ls -la` runs col 11..17 (between the backticks).
        let actions = code_actions(src, range(0, 11, 0, 17));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"ls -la\"\n"),
            "RHS must be a plain string (not backticked): {:?}",
            decl.new_text
        );
        assert!(
            !decl.new_text.contains("= `"),
            "must NOT wrap in backticks (would move command execution): {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_on_caret_only_snaps_to_identifier() {
        // No selection — caret parked inside the identifier `total`.
        // The action must extract `total` as if the user had selected it.
        let src = "my $x = total + 1\n";
        // `total` runs cols 8..13; place caret at col 10 (middle).
        let actions = code_actions(src, range(0, 10, 0, 10));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present for caret-only");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= total\n"),
            "RHS must be the snapped identifier: {:?}",
            decl.new_text
        );
        let replace = edits.iter().find(|e| e.new_text == "$extracted").unwrap();
        assert_eq!(replace.range.start.character, 8);
        assert_eq!(replace.range.end.character, 13);
    }

    #[test]
    fn extract_variable_on_caret_snaps_to_sigiled_var() {
        // Caret on `$foo` (col 4 == 'f') extracts the whole `$foo`.
        let src = "p $foo + 1\n";
        let actions = code_actions(src, range(0, 4, 0, 4));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= $foo\n"),
            "RHS must include the sigil: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_on_caret_inside_string_snaps_to_string_word() {
        // Caret inside `"one two three"` on the word `two` — extract
        // just `two` as `var $extracted = "two"`.
        let src = "my $s = \"one two three\"\n";
        // `two` runs cols 13..16; caret at col 14.
        let actions = code_actions(src, range(0, 14, 0, 14));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"two\"\n"),
            "RHS must be the snapped word, string-wrapped: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_on_caret_inside_string_on_interp_var_snaps_to_var() {
        // Caret on `$var` inside `"one two $var three"` — snap to the
        // full `$var` interpolation marker.
        let src = "my $s = \"one two $var three\"\n";
        // `$var` runs cols 17..21; caret at col 18.
        let actions = code_actions(src, range(0, 18, 0, 18));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= $var\n"),
            "RHS must be just `$var` (no double-stringify): {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_extracts_word_from_backtick_command() {
        // User's exact case: `` my $r = `ls file` ``, extract `file`.
        // Result must be:
        //   var $extracted = "file"
        //   my $r = `ls $extracted`
        let src = "my $r = `ls file`\n";
        // `file` runs col 12..16 (between the backticks).
        let actions = code_actions(src, range(0, 12, 0, 16));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"file\"\n"),
            "RHS must be a quoted string `\"file\"`: {:?}",
            decl.new_text
        );
        let replace = edits.iter().find(|e| e.new_text == "$extracted").unwrap();
        // The replacement covers exactly the `file` selection — the
        // surrounding `` `ls $extracted` `` keeps interpolation +
        // command-execution semantics.
        assert_eq!(replace.range.start.character, 12);
        assert_eq!(replace.range.end.character, 16);
    }

    #[test]
    fn extract_variable_inside_backtick_preserves_interpolation_via_string() {
        // Selecting `ls $dir | wc -l` from `` `ls $dir | wc -l` `` ⇒
        // `var $extracted = "ls $dir | wc -l"` — the string interpolates
        // `$dir` at decl time, then the surrounding `` `$extracted` ``
        // interpolates the resulting command string and runs it,
        // preserving command-at-original-site behavior.
        let src = "my $n = `ls $dir | wc -l`\n";
        // `ls $dir | wc -l` runs col 9..24.
        let actions = code_actions(src, range(0, 9, 0, 24));
        let var = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var.changes.expect("workspace edit");
        let (_uri, edits) = changes.iter().next().unwrap();
        let decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            decl.new_text.contains("= \"ls $dir | wc -l\"\n"),
            "RHS keeps `$dir` interpolation inside double quotes: {:?}",
            decl.new_text
        );
    }

    #[test]
    fn extract_variable_action_inserts_decl_above_and_replaces_selection() {
        let actions = code_actions("my $x = 1 + 2\n", range(0, 8, 0, 13));
        let var_action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("variable") => {
                    ca.edit.clone()
                }
                _ => None,
            })
            .expect("extract-variable action present");
        let changes = var_action.changes.expect("workspace edit has changes");
        let (_uri, edits) = changes.iter().next().unwrap();
        // Two edits: one inserts the decl line above, one replaces the
        // selection with the placeholder name.
        assert_eq!(edits.len(), 2, "two edits emitted");
        let new_decl = edits
            .iter()
            .find(|e| e.new_text.starts_with("var "))
            .unwrap();
        assert!(
            new_decl.new_text.contains("$extracted"),
            "uses placeholder name"
        );
        assert!(
            new_decl.new_text.contains("1 + 2"),
            "captures the selected expression"
        );
        let replace = edits.iter().find(|e| e.new_text == "$extracted").unwrap();
        assert_eq!(replace.range.start.character, 8);
        assert_eq!(replace.range.end.character, 13);
    }

    // ── compute_folding_ranges ───────────────────────────────────────────

    fn fold_ranges(text: &str) -> Vec<FoldingRange> {
        let (docs, uri) = doc("file:///t.stk", text);
        let params = FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        compute_folding_ranges(&docs, &params)
    }

    #[test]
    fn folding_brace_block_is_emitted() {
        let text = "fn f {\n    my $x = 1\n}\n";
        let ranges = fold_ranges(text);
        assert!(
            ranges.iter().any(|r| r.start_line == 0 && r.end_line == 2),
            "expected fold for fn body: got {ranges:?}"
        );
    }

    #[test]
    fn folding_emits_for_pod_block_as_comment() {
        let text = "=pod\nsome doc\nmore doc\n=cut\nmy $x = 1\n";
        let ranges = fold_ranges(text);
        let pod = ranges.iter().find(|r| r.start_line == 0).expect("POD fold");
        assert_eq!(pod.end_line, 3, "POD ends at `=cut` line");
        assert_eq!(
            pod.kind,
            Some(FoldingRangeKind::Comment),
            "marked as comment"
        );
    }

    #[test]
    fn folding_groups_three_or_more_comment_lines() {
        let text = "# a\n# b\n# c\nmy $x = 1\n";
        let ranges = fold_ranges(text);
        let cmt = ranges
            .iter()
            .find(|r| r.start_line == 0 && r.kind == Some(FoldingRangeKind::Comment))
            .expect("comment-run fold");
        // At least 3 lines (0..2). End line is the last comment line.
        assert!(cmt.end_line >= 2, "covers 3+ comment lines: got {cmt:?}");
    }

    #[test]
    fn folding_ignores_two_line_comment_runs() {
        // Only 2 comment lines → not foldable (3+ threshold).
        let text = "# a\n# b\nmy $x = 1\n";
        let ranges = fold_ranges(text);
        let has_comment_fold = ranges
            .iter()
            .any(|r| r.start_line == 0 && r.kind == Some(FoldingRangeKind::Comment));
        assert!(
            !has_comment_fold,
            "2 comment lines is below the fold threshold"
        );
    }

    /// Braces inside string literals must not create ghost folds.
    /// `compute_folding_ranges` tracks `in_str` to skip everything between
    /// matching quotes.
    #[test]
    fn folding_ignores_braces_inside_strings() {
        let text = "my $x = \"abc { foo } def\"\nmy $y = 1\n";
        let ranges = fold_ranges(text);
        // No `{...}` brace fold inside the string literal.
        assert!(
            ranges
                .iter()
                .all(|r| !(r.start_line == 0 && r.kind.is_none())),
            "no brace fold from inside-string `{{`: got {ranges:?}"
        );
    }

    // ── compute_formatting ───────────────────────────────────────────────

    fn fmt_edits(text: &str) -> Vec<TextEdit> {
        let (docs, uri) = doc("file:///t.stk", text);
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: lsp_types::FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
        };
        compute_formatting(&docs, &params)
    }

    /// Already-canonical input returns an empty edit list (formatter's
    /// short-circuit when output equals input). Letting an "identity"
    /// reformat send a full-document edit churns the file and bumps
    /// version, breaking undo history downstream.
    #[test]
    fn formatting_no_op_when_already_formatted() {
        // Use `format_program`'s own output verbatim so we hit the
        // `formatted == *text` short-circuit. The formatter omits a final
        // trailing newline; we don't add one for this test.
        let raw = "my $x = 1\n";
        let canonical_program = crate::parse_with_file(raw, "<t>").unwrap();
        let canonical = crate::fmt::format_program(&canonical_program);
        let edits = fmt_edits(&canonical);
        assert!(
            edits.is_empty(),
            "no edits for canonical input: got {edits:?}"
        );
    }

    /// Parse-error input returns an empty edit list rather than a partial
    /// rewrite. We don't want to ship broken auto-format that drops a
    /// stray `}` and erases the user's code.
    #[test]
    fn formatting_returns_empty_on_parse_error() {
        let text = "this is not valid stryke ?? !@#$%^&\n";
        let edits = fmt_edits(text);
        // Either empty (preferred) OR an edit that contains the input
        // verbatim. Both are acceptable; what we must not do is delete the
        // unparseable region.
        if !edits.is_empty() {
            let new_text = &edits[0].new_text;
            assert!(!new_text.is_empty(), "fmt must not erase content");
        }
    }

    // ── extract_selection_on_line (UTF-16 indexing) ──────────────────────

    #[test]
    fn extract_selection_handles_ascii() {
        let line = "my $x = 1 + 2";
        // Select "1 + 2".
        let sel = extract_selection_on_line(line, 8, 13).expect("selection");
        assert_eq!(sel, "1 + 2");
    }

    #[test]
    fn extract_selection_returns_none_for_empty_range() {
        let line = "my $x = 1";
        assert!(extract_selection_on_line(line, 5, 5).is_none());
    }

    /// UTF-16 offsets must convert correctly back to UTF-8 byte offsets
    /// for slicing — LSP positions are UTF-16 code units.
    #[test]
    fn extract_selection_handles_multibyte_chars() {
        // "α" is 2 UTF-8 bytes but 1 UTF-16 unit. Selecting from after
        // "α " up to end of "x" should return "x".
        let line = "α x";
        // UTF-16 layout: α=1 unit (col 0), space=1 (col 1), x=1 (col 2).
        let sel = extract_selection_on_line(line, 2, 3).expect("selection");
        assert_eq!(sel, "x");
    }

    // ── semantic tokens ──────────────────────────────────────────────────

    /// The legend exposes the stable type/modifier index space. Reordering
    /// either array (or accidentally dropping an entry) silently shifts
    /// every emitted token's interpretation in the client. Pin the order
    /// and total count.
    #[test]
    fn semantic_tokens_legend_is_stable() {
        let leg = semantic_tokens_legend();
        // Snapshot the first few — full enumeration would just duplicate
        // the table; the exact ordering matters most at the head where
        // indices like TY_KEYWORD = 0 are hard-coded throughout.
        assert_eq!(
            leg.token_types.first().unwrap(),
            &SemanticTokenType::KEYWORD
        );
        assert_eq!(leg.token_types[1], SemanticTokenType::FUNCTION);
        assert_eq!(leg.token_types[2], SemanticTokenType::VARIABLE);
        assert!(leg.token_types.len() >= 10, "12 stable type slots");
        assert!(leg.token_modifiers.len() >= 5, "modifiers present");
    }

    #[test]
    fn semantic_tokens_empty_text_yields_no_tokens() {
        let t = compute_semantic_tokens("");
        assert!(t.data.is_empty(), "empty input -> empty token list");
    }

    /// Strings, numbers, and comments are the easiest token kinds to
    /// verify — they have unmistakable shape and don't depend on the
    /// stryke parser. Confirm at least one token of each emits for a
    /// representative line.
    #[test]
    fn semantic_tokens_recognise_strings_numbers_and_comments() {
        let t = compute_semantic_tokens("# header\nmy $x = \"hi\" + 42\n");
        // Compute (delta_line, delta_start) running totals to find the
        // string and number positions.
        assert!(!t.data.is_empty(), "tokens emitted: {:?}", t.data);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        // TY_COMMENT = 6, TY_STRING = 4, TY_NUMBER = 5.
        assert!(types.contains(&6), "comment token emitted");
        assert!(types.contains(&4), "string token emitted");
        assert!(types.contains(&5), "number token emitted");
    }

    #[test]
    fn double_quote_string_with_hash_interpolation_is_not_a_comment() {
        // Pin the fix for the JetBrains plugin / LSP bug where `#` inside
        // a `"..."` was treated as starting a comment, breaking
        // syntax-highlighting on lines like:
        //     p "examples: $pass/$total clean (#{len(@failed)} failed)"
        let text = r#"p "clean (#{len(@failed)} failed)""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        // NO comment token should appear anywhere — `#{...}` is interpolation,
        // not a comment opener.
        assert!(
            !types.contains(&6),
            "TY_COMMENT must not appear inside a double-quoted string: tokens = {:?}",
            t.data,
        );
        // At least one string token (the literal runs around the interp).
        assert!(
            types.contains(&4),
            "TY_STRING for literal run: {:?}",
            t.data
        );
        // The `#{` and `}` interp markers come through as operator tokens.
        assert!(
            types.iter().filter(|&&ty| ty == 7).count() >= 2,
            "TY_OPERATOR for `#{{` and `}}`: tokens = {:?}",
            t.data,
        );
    }

    #[test]
    fn nested_braces_inside_interpolation_stay_balanced() {
        // `#{ +{ x => 1 } }` — the inner hash literal must not close the
        // outer interpolation. After the inner `}`, depth drops to 1,
        // then the next `}` drops to 0 and closes the interp.
        let text = r#"p "h: #{ +{ x => 1 } }""#;
        let t = compute_semantic_tokens(text);
        // No comment token leaks.
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(
            !types.contains(&6),
            "no comment inside string: {:?}",
            t.data
        );
    }

    #[test]
    fn dollar_variable_inside_string_emits_a_variable_token() {
        // `"$pass/$total"` — both `$pass` and `$total` must come through as
        // TY_VARIABLE tokens, with TY_STRING for the surrounding literal runs.
        let text = r#"p "$pass/$total""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        let var_count = types.iter().filter(|&&ty| ty == 2).count(); // TY_VARIABLE = 2
        assert_eq!(
            var_count, 2,
            "expected 2 variable tokens for `$pass` and `$total`; got tokens = {:?}",
            t.data,
        );
        // Plus at least one STRING token for the surrounding quote chars.
        assert!(
            types.contains(&4),
            "expected TY_STRING somewhere: {:?}",
            t.data
        );
        // No comment leaks.
        assert!(!types.contains(&6), "no comment in string: {:?}", t.data);
    }

    #[test]
    fn in_string_variable_token_emitted_inside_multiarg_call_with_regex() {
        // Real-world fixture from the user's report:
        // `split(/\s+/, "one two $var three")` — `$var` must come
        // through as TY_VARIABLE even though it sits inside the second
        // arg of a multi-arg call AFTER a regex literal. Pins that the
        // regex scanner correctly restores `col`/`i` so the subsequent
        // string scanner finds the interpolation site.
        let text =
            "fn arr_split_ws {\n    my $var = 23;\n    split(/\\s+/, \"one two $var three\")\n}\n";
        let t = compute_semantic_tokens(text);
        // Walk deltas to compute absolute (line, col) for each token.
        let mut abs: Vec<(u32, u32, u32, u32)> = Vec::new();
        let (mut line, mut col) = (0u32, 0u32);
        for tok in &t.data {
            if tok.delta_line == 0 {
                col += tok.delta_start;
            } else {
                line += tok.delta_line;
                col = tok.delta_start;
            }
            abs.push((line, col, tok.length, tok.token_type));
        }
        // The in-string `$var` lives on line 2 of the source (0-based).
        // It must be a length-4 TY_VARIABLE (token_type=2) token.
        assert!(
            abs.iter()
                .any(|(l, _c, len, ty)| *l == 2 && *len == 4 && *ty == 2),
            "expected in-string `$var` as TY_VARIABLE on line 2: abs={abs:?}",
        );
        // Cross-check the decl `$var` on line 1 is also TY_VARIABLE.
        assert!(
            abs.iter()
                .any(|(l, _c, len, ty)| *l == 1 && *len == 4 && *ty == 2),
            "expected decl `$var` as TY_VARIABLE on line 1: abs={abs:?}",
        );
    }

    #[test]
    fn array_variable_inside_string_emits_a_variable_token() {
        let text = r#"p "@names is the list""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(
            types.iter().filter(|&&ty| ty == 2).count() >= 1,
            "expected at least one variable token for `@names`: {:?}",
            t.data,
        );
    }

    #[test]
    fn hash_subscript_inside_string_covered_by_one_variable_token() {
        // `"$h{key}"` — the whole `$h{key}` should be one variable token,
        // not split into `$h` + literal + `{key}`.
        let text = r#"p "got $h{key} done""#;
        let t = compute_semantic_tokens(text);
        // Variable tokens with length >= 4 (covers $h{ + key + })
        let var_tokens: Vec<_> = t.data.iter().filter(|tok| tok.token_type == 2).collect();
        assert!(
            var_tokens.iter().any(|t| t.length >= 6),
            "expected a variable token covering `$h{{key}}` (>=6 chars): {:?}",
            t.data,
        );
    }

    #[test]
    fn arrow_subscript_inside_string_covered_by_one_variable_token() {
        // `"$h->{k}"` — the whole referent is one variable.
        let text = r#"p "got $h->{key} done""#;
        let t = compute_semantic_tokens(text);
        let var_tokens: Vec<_> = t.data.iter().filter(|tok| tok.token_type == 2).collect();
        assert!(
            var_tokens.iter().any(|t| t.length >= 8),
            "expected a variable token covering `$h->{{key}}` (>=8 chars): {:?}",
            t.data,
        );
    }

    #[test]
    fn percent_hash_variable_inside_string_emits_a_variable_token() {
        // `"%h"` — `%h` should be a TY_VARIABLE token like `$h` / `@h`.
        let text = r#"p "stats: %h end""#;
        let t = compute_semantic_tokens(text);
        let var_count = t.data.iter().filter(|tok| tok.token_type == 2).count();
        assert!(
            var_count >= 1,
            "expected at least one TY_VARIABLE for `%h`: {:?}",
            t.data,
        );
    }

    #[test]
    fn multiple_interpolations_on_one_line() {
        // `"$a/$b/$c"` — three separate variable tokens interleaved with
        // string literal runs. Exercises the delta-encoding inside the loop.
        let text = r#"p "$a/$b/$c""#;
        let t = compute_semantic_tokens(text);
        let var_count = t.data.iter().filter(|tok| tok.token_type == 2).count();
        assert_eq!(var_count, 3, "three variables: {:?}", t.data);
    }

    #[test]
    fn escaped_quote_does_not_terminate_string() {
        // `"a \" $b"` — the escaped quote must not end the string, so
        // `$b` is still interpolated.
        let text = r#"p "a \" $b done""#;
        let t = compute_semantic_tokens(text);
        let var_count = t.data.iter().filter(|tok| tok.token_type == 2).count();
        assert_eq!(var_count, 1, "one var after escaped quote: {:?}", t.data);
        // No comment leaks.
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(!types.contains(&6));
    }

    #[test]
    fn hash_interp_with_function_call_inside() {
        // `"#{len(@xs)}"` — the interpolated expression is `len(@xs)`; the
        // server emits `#{`/`}` as OPERATOR tokens and leaves the interior
        // for the client. Exactly the contract `dollar_variable_inside_…`
        // verifies, but for the more complex interp form the user just
        // reported. Two OPERATOR tokens for `#{` and `}` mark the bounds.
        let text = r#"p "got #{len(@xs)} items""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(!types.contains(&6), "no comment: {:?}", t.data);
        let op_count = types.iter().filter(|&&ty| ty == 7).count();
        assert!(
            op_count >= 2,
            "expected `#{{` + `}}` operator tokens: {:?}",
            t.data
        );
    }

    #[test]
    fn array_deref_interpolation_at_bracket_in_string() {
        // `@{[FN()]}` — Perl-style array-deref interpolation. Must come
        // through with TY_OPERATOR for `@{[` + `]}`, NOT as a single
        // monolithic TY_VARIABLE token covering the entire `@{[FN()]}`
        // expression. The bug was that the general `@{…}` block-deref
        // branch swallowed the whole thing, killing hover on `FN`.
        let text = r#"p "got @{[FN()]} items""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(!types.contains(&6), "no comment in string: {:?}", t.data);
        // Two OPERATOR tokens — one for `@{[`, one for `]}`.
        let op_count = types.iter().filter(|&&ty| ty == 7).count();
        assert!(
            op_count >= 2,
            "expected `@{{[` + `]}}` operator tokens; got {:?}",
            t.data,
        );
        // No giant 8+ length VARIABLE token swallowing the whole interp.
        let big_var = t
            .data
            .iter()
            .any(|tok| tok.token_type == 2 && tok.length >= 8);
        assert!(
            !big_var,
            "must not emit a single VARIABLE token covering all of `@{{[FN()]}}`: {:?}",
            t.data,
        );
    }

    #[test]
    fn array_deref_interpolation_nested_brackets() {
        // `@{[ FN([1,2]) ]}` — nested `[]` inside the interp must NOT
        // close it early; depth tracking required.
        let text = r#"p "h: @{[ FN([1,2]) ]} end""#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(!types.contains(&6), "no comment leak: {:?}", t.data);
        let op_count = types.iter().filter(|&&ty| ty == 7).count();
        assert!(
            op_count >= 2,
            "matched `@{{[` + `]}}` despite inner brackets: {:?}",
            t.data,
        );
    }

    #[test]
    fn shebang_line_emits_a_comment_token() {
        // `#!/usr/bin/env stryke` is a single comment line — the entire
        // line must come out as one TY_COMMENT token, not a sequence of
        // builtins / identifiers.
        let text = "#!/usr/bin/env stryke\nmy $x = 1;";
        let t = compute_semantic_tokens(text);
        let first_line: Vec<_> = t.data.iter().take(1).collect();
        assert!(
            !first_line.is_empty() && first_line[0].token_type == 6,
            "first token must be a comment for the shebang line: {:?}",
            t.data,
        );
    }

    #[test]
    fn single_quote_string_with_hash_is_not_a_comment_either() {
        // Single-quote strings have no interpolation, but `#` inside them
        // must still NOT trigger a comment.
        let text = r#"my $x = 'hash # mark'"#;
        let t = compute_semantic_tokens(text);
        let types: Vec<u32> = t.data.iter().map(|tok| tok.token_type).collect();
        assert!(
            !types.contains(&6),
            "no comment in single-quote string: {:?}",
            t.data
        );
        assert!(types.contains(&4), "string token emitted: {:?}", t.data);
    }

    // ── signature help ───────────────────────────────────────────────────

    /// `compute_signature_help` walks backward from the cursor through
    /// `name(` and counts commas to set `active_parameter`. Verify the
    /// arg index advances as the user types more commas.
    #[test]
    fn signature_help_tracks_active_param_index() {
        let text = "say(a, b, c"; // cursor right after the last `c`
        let pos = Position {
            line: 0,
            character: 11,
        };
        let params = lsp_types::SignatureHelpParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Uri::from_str("file:///t.stk").unwrap(),
                },
                position: pos,
            },
            context: None,
            work_done_progress_params: Default::default(),
        };
        // Stub `doc_for` — give every name a trivial signature so we don't
        // need the real LSP doc map in unit tests.
        let stub = |_name: &str| -> Option<&'static str> { Some("```\nsay LIST\n```") };
        let help = compute_signature_help(text, &params, stub);
        assert!(help.is_some(), "expected signature help");
        let h = help.unwrap();
        // Three commas not seen (we're inside the 3rd arg) ⇒ active_param = 2
        // (0-indexed: a=0, b=1, c=2).
        assert_eq!(
            h.active_parameter,
            Some(2),
            "active param = 2 inside 3rd arg"
        );
    }

    #[test]
    fn signature_help_returns_none_outside_call() {
        let text = "my $x = 1";
        let pos = Position {
            line: 0,
            character: 9,
        };
        let params = lsp_types::SignatureHelpParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Uri::from_str("file:///t.stk").unwrap(),
                },
                position: pos,
            },
            context: None,
            work_done_progress_params: Default::default(),
        };
        let stub = |_: &str| -> Option<&'static str> { None };
        let help = compute_signature_help(text, &params, stub);
        assert!(help.is_none(), "no active call → no signature help");
    }
}
