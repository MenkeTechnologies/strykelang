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
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, Position, Range,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensServerCapabilities, SignatureHelp, SignatureHelpParams, SignatureInformation,
    TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
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

pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: SEMANTIC_TYPES.to_vec(),
        token_modifiers: SEMANTIC_MODS.to_vec(),
    }
}

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
        let delta_start = if delta_line == 0 { col - *prev_char } else { col };
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_COMMENT, 0);
            col += len;
            continue;
        }
        // Strings
        if c == '"' || c == '\'' || c == '`' {
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_STRING, 0);
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_NUMBER, 0);
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_VARIABLE, 0);
            col += len;
            continue;
        }
        // Pipe operators
        if c == '|' && peek(&chars, i + 1) == Some('>') {
            let start_col = col;
            let len = if peek(&chars, i + 2) == Some('>') { 3 } else { 2 };
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_MACRO, 0);
            i += len as usize;
            col += len;
            continue;
        }
        if c == '~' && peek(&chars, i + 1) == Some('>') {
            let start_col = col;
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, 2, TY_MACRO, 0);
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len, TY_REGEXP, 0);
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
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, len_u16, ty, modifiers);
            col += len_u16;
            continue;
        }
        // Operator (single char)
        if is_operator_char(c) {
            let start_col = col;
            push(&mut tokens, &mut prev_line, &mut prev_char, line, start_col, 1, TY_OPERATOR, 0);
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
        '_' | '!' | '@' | '$' | ',' | ';' | '/' | '\\' | '"' | '\''
            | '&' | '`' | '+' | '-' | '.' | '0'..='9' | '?' | '<' | '>' | '('
            | ')' | '['  | ']' | '~' | '^'
    )
}

fn is_operator_char(c: char) -> bool {
    matches!(
        c,
        '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|'
            | '^' | '~' | '?' | ':' | ';' | ',' | '.' | '(' | ')' | '[' | ']'
            | '{' | '}' | '\\'
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
    BUILTIN_NAMES.get_or_init(|| {
        crate::builtins::all_hash_map().into_keys().collect()
    })
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
/// v1: a single hand-curated quickfix — *Wrap selection in `p ` (print)*. The
/// goal is to advertise the capability with at least one genuinely useful
/// transformation; richer diagnostic-tied fixes follow in v2 once the lint
/// pipeline emits structured codes.
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
        // Toggle line comment.
        out.push(toggle_comment_action(uri, range.start.line, line_text));
    }
    out
}

fn wrap_in_p_action(uri: &Uri, line: u32, line_text: &str) -> CodeActionOrCommand {
    let leading_ws: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();
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

fn toggle_comment_action(uri: &Uri, line: u32, line_text: &str) -> CodeActionOrCommand {
    let trimmed = line_text.trim_start();
    let leading_ws: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();
    let (new_text, title) = if trimmed.starts_with("# ") {
        (format!("{leading_ws}{}", &trimmed[2..]), "Uncomment line")
    } else if trimmed.starts_with('#') {
        (format!("{leading_ws}{}", &trimmed[1..]), "Uncomment line")
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
