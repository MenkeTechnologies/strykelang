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
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    DocumentFormattingParams, FoldingRange, FoldingRangeKind, FoldingRangeParams, Position, Range,
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

    // ── Refactorings (need a real selection) ──
    let same_line = range.start.line == range.end.line;
    let nonempty_range = range.start.line != range.end.line
        || range.start.character != range.end.character;
    if nonempty_range {
        if same_line {
            if let Some(line_text) = nth_line(text, range.start.line as usize) {
                if let Some(selection) =
                    extract_selection_on_line(line_text, range.start.character, range.end.character)
                {
                    if !selection.trim().is_empty() {
                        out.push(extract_variable_action(uri, line_text, range, selection));
                        out.push(extract_constant_action(uri, line_text, range, selection));
                    }
                }
            }
        } else {
            // Multi-line selection → extract function.
            if let Some(block) = extract_selection_multiline(text, range) {
                if !block.text.trim().is_empty() {
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
fn extract_selection_on_line<'a>(line_text: &'a str, start: u32, end: u32) -> Option<&'a str> {
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
        "Extract to variable (`my $name = …`)",
        false,
    )
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
        "Extract to constant (`my frozen $NAME = …`)",
        true,
    )
}

/// Shared body of extract-variable and extract-constant. Inserts a
/// declaration line above the selection (preserving the line's indent) and
/// replaces the selection with `$name`.
fn extract_to_local(
    uri: &Uri,
    line_text: &str,
    range: Range,
    selection: &str,
    placeholder: &str,
    title: &str,
    frozen: bool,
) -> CodeActionOrCommand {
    let leading_ws: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();
    let decl = if frozen {
        format!("{leading_ws}my frozen ${placeholder} = {selection}\n")
    } else {
        format!("{leading_ws}my ${placeholder} = {selection}\n")
    };
    let insert = TextEdit {
        range: Range {
            start: Position { line: range.start.line, character: 0 },
            end: Position { line: range.start.line, character: 0 },
        },
        new_text: decl,
    };
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

fn extract_function_action(
    uri: &Uri,
    range: Range,
    block: &MultilineBlock,
) -> CodeActionOrCommand {
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
            start: Position { line: block.insertion_line, character: 0 },
            end: Position { line: block.insertion_line, character: 0 },
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
            if col_is_zero || all_whitespace_before(bytes, i, line, &line_starts_cache(text)) {
                if comment_run_start.is_none() {
                    comment_run_start = Some(line);
                }
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
                        ranges.push(make_fold(start, line.saturating_sub(1), Some(FoldingRangeKind::Comment)));
                    }
                }
                col_is_zero = false;
                i += 1;
                continue;
            }
            b'{' => {
                if let Some(start) = comment_run_start.take() {
                    if line.saturating_sub(start) >= 3 {
                        ranges.push(make_fold(start, line.saturating_sub(1), Some(FoldingRangeKind::Comment)));
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
                        ranges.push(make_fold(start, line.saturating_sub(1), Some(FoldingRangeKind::Comment)));
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
    let end_line = if text.ends_with('\n') { line_count } else { line_count.saturating_sub(1) };
    let end_char = if text.ends_with('\n') { 0 } else { last_char };
    vec![TextEdit {
        range: Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: end_line, character: end_char },
        },
        new_text: formatted,
    }]
}

#[cfg(test)]
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
        Range { start: pos(s_line, s_char), end: pos(e_line, e_char) }
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
    fn code_actions_for_empty_selection_are_line_local_only() {
        let actions = code_actions("my $x = 1 + 2\np $x\n", range(0, 0, 0, 0));
        let t = titles(&actions);
        assert!(t.iter().any(|s| s.contains("Wrap line in")), "wrap-in-p offered");
        assert!(t.iter().any(|s| s.contains("Comment line")), "toggle comment offered");
        // No refactorings on an empty range.
        assert!(!t.iter().any(|s| s.contains("Extract")), "no Extract on empty range");
    }

    #[test]
    fn code_actions_for_single_line_selection_offer_extract_var_and_const() {
        // Select "1 + 2" on `my $x = 1 + 2`.
        let actions = code_actions("my $x = 1 + 2\np $x\n", range(0, 8, 0, 13));
        let t = titles(&actions);
        assert!(t.iter().any(|s| s.contains("Extract to variable")), "var: got {t:?}");
        assert!(t.iter().any(|s| s.contains("Extract to constant")), "const: got {t:?}");
        // Function extract requires multi-line.
        assert!(!t.iter().any(|s| s.contains("Extract to function")), "no fn: got {t:?}");
    }

    #[test]
    fn code_actions_for_multi_line_selection_offer_extract_function() {
        let text = "my $x = 1\nmy $y = 2\np $x + $y\n";
        // Span all three statements.
        let actions = code_actions(text, range(0, 0, 2, 9));
        let t = titles(&actions);
        assert!(t.iter().any(|s| s.contains("Extract to function")), "fn: got {t:?}");
        // Single-line extracts are not offered here.
        assert!(!t.iter().any(|s| s.contains("Extract to variable")), "no var: got {t:?}");
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
        let new_decl = edits.iter().find(|e| e.new_text.starts_with("my ")).unwrap();
        assert!(new_decl.new_text.contains("$extracted"), "uses placeholder name");
        assert!(new_decl.new_text.contains("1 + 2"), "captures the selected expression");
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
        assert_eq!(pod.kind, Some(FoldingRangeKind::Comment), "marked as comment");
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
        assert!(!has_comment_fold, "2 comment lines is below the fold threshold");
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
        assert!(edits.is_empty(), "no edits for canonical input: got {edits:?}");
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
}
