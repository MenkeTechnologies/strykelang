//! Generates reflection tables from the source at compile time so
//! `%builtins` / `%aliases` / `%descriptions` never drift from the real
//! parser / dispatcher / LSP docs.
//!
//! Emits `$OUT_DIR/reflection.rs` with:
//!   - `BUILTIN_ARMS: &[&[&str]]`    — per-arm names from `try_builtin`
//!     (used for the `%aliases` alias → primary map).
//!   - `CATEGORY_MAP: &[(&str, &str)]` — name → category string, parsed
//!     from the `// ── category ──` section comments in `is_perl5_core`
//!     and `perlrs_extension_name`. Category strings are lowercase,
//!     human-readable ("parallel", "string", "filesystem", ...).
//!   - `DESCRIPTIONS: &[(&str, &str)]` — name → first-line hover doc,
//!     harvested from the `doc_for_label_text` match in `src/lsp.rs`.
//!     Sparse — only labels that have a doc entry show up.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/builtins.rs");
    println!("cargo:rerun-if-changed=src/parser.rs");
    println!("cargo:rerun-if-changed=src/lsp.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let builtins_src = fs::read_to_string("src/builtins.rs").expect("read src/builtins.rs");
    let parser_src = fs::read_to_string("src/parser.rs").expect("read src/parser.rs");
    let lsp_src = fs::read_to_string("src/lsp.rs").expect("read src/lsp.rs");

    let arms = extract_try_builtin_arms(&builtins_src);
    // `is_perl5_core` uses `matches!(name, …)` (parens), `perlrs_extension_name`
    // uses `match name { … }` (braces). Different block markers per fn.
    let core_cats = extract_categorized_names(&parser_src, "fn is_perl5_core", "matches!");
    let ext_cats = extract_categorized_names(&parser_src, "fn perlrs_extension_name", "match name");
    // Descriptions: /// doc comments from builtins.rs (primary source for ~990 fns)
    // merged with hand-written lsp.rs entries (keywords, operators, reflection hashes).
    let mut descriptions = extract_builtin_doc_comments(&builtins_src, &arms);
    let lsp_descs = extract_lsp_descriptions(&lsp_src);
    let builtin_keys: std::collections::HashSet<String> =
        descriptions.iter().map(|(k, _)| k.clone()).collect();
    for (k, v) in lsp_descs {
        if !builtin_keys.contains(&k) {
            descriptions.push((k, v));
        }
    }
    descriptions.sort();
    descriptions.dedup_by(|a, b| a.0 == b.0);

    // Two source-partitioned tables fed into `%pc` (Perl 5 core) and
    // `%e` (extensions) respectively. Dispatch primaries that aren't in
    // either parser list are still extensions at runtime — fold them into
    // the extension table with an "uncategorized" category so `%e`
    // covers everything actually callable.
    let mut core_pairs: Vec<(String, String)> = core_cats.clone();
    core_pairs.sort();
    core_pairs.dedup_by(|a, b| a.0 == b.0);

    let core_set: std::collections::HashSet<String> =
        core_pairs.iter().map(|(n, _)| n.clone()).collect();

    let mut ext_pairs: Vec<(String, String)> = Vec::new();
    let mut ext_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, cat) in &ext_cats {
        if core_set.contains(name.as_str()) {
            continue; // keep core / extension disjoint
        }
        if ext_seen.insert(name.clone()) {
            ext_pairs.push((name.clone(), cat.clone()));
        }
    }
    for arm in &arms {
        if let Some(primary) = arm.first() {
            if core_set.contains(primary.as_str()) {
                continue;
            }
            if ext_seen.insert(primary.clone()) {
                ext_pairs.push((primary.clone(), "uncategorized".to_string()));
            }
        }
    }

    ext_pairs.sort();

    // `ALL_CATEGORY_MAP` — every callable spelling (primary + every alias)
    // → its category. Aliases inherit their primary's category so
    // `scalar keys %all` is a clean total-callables count and lookups on
    // short forms (`$all{tj}`) work directly. Stays separate from `%b` so
    // primaries-only counts / queries don't inflate.
    let primary_to_cat: std::collections::HashMap<String, String> = core_pairs
        .iter()
        .chain(ext_pairs.iter())
        .map(|(n, c)| (n.clone(), c.clone()))
        .collect();
    let mut all_pairs: Vec<(String, String)> = Vec::new();
    let mut all_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (n, c) in core_pairs.iter().chain(ext_pairs.iter()) {
        if all_seen.insert(n.clone()) {
            all_pairs.push((n.clone(), c.clone()));
        }
    }
    for arm in &arms {
        let Some((primary, rest)) = arm.split_first() else {
            continue;
        };
        let Some(cat) = primary_to_cat.get(primary) else {
            continue;
        };
        for alias in rest {
            if all_seen.insert(alias.clone()) {
                all_pairs.push((alias.clone(), cat.clone()));
            }
        }
    }
    all_pairs.sort();

    // Merged `%b` (name → category) is just the concatenation: core first,
    // then extensions. Sort once more to keep `keys %b` alphabetical.
    let mut cat_pairs: Vec<(String, String)> = core_pairs.to_vec();
    cat_pairs.extend(ext_pairs.iter().cloned());
    cat_pairs.sort();

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = PathBuf::from(out_dir).join("reflection.rs");
    let mut body = String::new();
    body.push_str("// GENERATED by build.rs — do not edit.\n\n");

    body.push_str("pub(crate) const BUILTIN_ARMS: &[&[&str]] = &[\n");
    for arm in &arms {
        body.push_str("    &[");
        for (i, n) in arm.iter().enumerate() {
            if i > 0 {
                body.push_str(", ");
            }
            body.push_str(&format!("{:?}", n));
        }
        body.push_str("],\n");
    }
    body.push_str("];\n\n");

    body.push_str("pub(crate) const CATEGORY_MAP: &[(&str, &str)] = &[\n");
    for (n, c) in &cat_pairs {
        body.push_str(&format!("    ({:?}, {:?}),\n", n, c));
    }
    body.push_str("];\n\n");

    body.push_str("pub(crate) const CORE_CATEGORY_MAP: &[(&str, &str)] = &[\n");
    for (n, c) in &core_pairs {
        body.push_str(&format!("    ({:?}, {:?}),\n", n, c));
    }
    body.push_str("];\n\n");

    body.push_str("pub(crate) const EXT_CATEGORY_MAP: &[(&str, &str)] = &[\n");
    for (n, c) in &ext_pairs {
        body.push_str(&format!("    ({:?}, {:?}),\n", n, c));
    }
    body.push_str("];\n\n");

    body.push_str("pub(crate) const ALL_CATEGORY_MAP: &[(&str, &str)] = &[\n");
    for (n, c) in &all_pairs {
        body.push_str(&format!("    ({:?}, {:?}),\n", n, c));
    }
    body.push_str("];\n\n");

    body.push_str("pub(crate) const DESCRIPTIONS: &[(&str, &str)] = &[\n");
    for (n, d) in &descriptions {
        body.push_str(&format!("    ({:?}, {:?}),\n", n, d));
    }
    body.push_str("];\n");

    fs::write(&dest, body).expect("write reflection.rs");
}

/// Scan the `match name` block inside `pub(crate) fn try_builtin` and
/// return each arm's quoted names in source order. Skips `CORE::` and
/// `builtin::` prefixed forms — those are qualifier aliases of the plain
/// name already present in the same arm and would otherwise show up as
/// bogus uncategorized "primaries" when an arm starts with the qualified
/// spelling.
fn extract_try_builtin_arms(src: &str) -> Vec<Vec<String>> {
    let fn_pos = src
        .find("pub(crate) fn try_builtin")
        .expect("try_builtin not found");
    let after = &src[fn_pos..];
    let match_rel = after
        .find("match name {")
        .expect("`match name {` not found inside try_builtin");
    let body_start = fn_pos + match_rel + "match name {".len();
    let body_end = find_matching_rbrace(src.as_bytes(), body_start);
    let body = &src[body_start..body_end];

    let mut arms: Vec<Vec<String>> = Vec::new();
    let bb = body.as_bytes();
    let mut inner = 0i32;
    let mut arm_start = 0usize;
    let mut i = 0usize;
    while i < body.len() {
        let c = bb[i];
        if c == b'"' {
            i = skip_string(bb, i);
            continue;
        }
        if c == b'/' && bb.get(i + 1) == Some(&b'/') {
            while i < body.len() && bb[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        match c {
            b'{' => inner += 1,
            b'}' => inner -= 1,
            b',' if inner == 0 => arm_start = i + 1,
            b'=' if inner == 0 && bb.get(i + 1) == Some(&b'>') => {
                let mut names = Vec::new();
                extract_quoted(&body[arm_start..i], &mut names);
                // Drop qualifier-prefixed spellings (`CORE::eof`, `builtin::tell`) —
                // they're duplicate dispatch entries for the plain name already
                // in the same arm and pollute reflection with pseudo-primaries.
                names.retain(|n| !n.contains("::"));
                if !names.is_empty() {
                    arms.push(names);
                }
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    arms
}

/// Parse a function with `// ── category ──` section comments above groups
/// of quoted names, returning one (name, category) pair per listed name.
/// Works for both `is_perl5_core` (matches!) and `perlrs_extension_name`
/// (match name { … }).
fn extract_categorized_names(
    src: &str,
    fn_marker: &str,
    block_marker: &str,
) -> Vec<(String, String)> {
    let fn_pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} not found", fn_marker));
    let after = &src[fn_pos..];
    let block_rel = after
        .find(block_marker)
        .unwrap_or_else(|| panic!("`{}` not found inside {}", block_marker, fn_marker));
    let block_start_abs = fn_pos + block_rel;

    // Find the opening delimiter (`{` for `match name {`, `(` for `matches!(`).
    let bytes = src.as_bytes();
    let mut i = block_start_abs + block_marker.len();
    let (open, close) = loop {
        if i >= src.len() {
            panic!("no opening delimiter after {}", block_marker);
        }
        match bytes[i] {
            b'{' => break (b'{', b'}'),
            b'(' => break (b'(', b')'),
            _ => i += 1,
        }
    };
    let body_start = i + 1;
    let body_end = find_matching_close(bytes, body_start, open, close);
    let body = &src[body_start..body_end];

    // Walk line-by-line; every `// ── category ──` comment resets the
    // current category, every subsequent quoted string belongs to it.
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut current_cat = String::from("uncategorized");
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("//") {
            if let Some(cat) = parse_section_header(rest) {
                current_cat = cat;
                continue;
            }
            // Non-section comments inside the block don't alter category.
            continue;
        }
        let mut names = Vec::new();
        extract_quoted(line, &mut names);
        for n in names {
            pairs.push((n, current_cat.clone()));
        }
    }
    pairs
}

/// Parse a `// ── category name ──` style comment. Returns `Some(category)`
/// when the line is a section header — preserves `/` and spaces inside the
/// label (so `"array / list"` round-trips), strips only the unicode box
/// drawing ruling and its ASCII fallback.
fn parse_section_header(comment_body: &str) -> Option<String> {
    // Require a ruling run — comments without one are commentary.
    if !comment_body.contains("──")
        && !comment_body.contains("──────")
        && !comment_body.contains("─")
    {
        return None;
    }
    let mut cleaned = String::new();
    for c in comment_body.chars() {
        if c == '─' {
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }
    let label = cleaned.trim();
    if label.is_empty() {
        return None;
    }
    // Take everything up to the first colon (e.g. "parallel:" → "parallel").
    let label = label.split(':').next().unwrap_or(label).trim();
    // Collapse runs of whitespace to single spaces so the label stays tidy
    // after ruling removal.
    let label: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.is_empty() {
        return None;
    }
    // Reject noisy ones: the perlrs_extension_name list has a stray
    // "perlrs extensions that produce lists or have special syntax"
    // header from the pre-split era — if we ever see it again, tag it so.
    if label.len() > 40 {
        return None;
    }
    Some(label.to_lowercase())
}

/// Scan `fn doc_for_label_text` in `src/lsp.rs` for `"label" | "alias" => "markdown"` arms.
/// Returns (label, first_sentence) pairs — aliases are expanded (each label
/// in an arm's LHS maps to the same description's opening sentence).
fn extract_lsp_descriptions(src: &str) -> Vec<(String, String)> {
    let Some(fn_pos) = src.find("fn doc_for_label_text") else {
        return Vec::new();
    };
    let after = &src[fn_pos..];
    let Some(match_rel) = after.find("match key") else {
        return Vec::new();
    };
    let body_start = fn_pos + match_rel + "match key".len();
    let bytes = src.as_bytes();
    let mut i = body_start;
    while i < src.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= src.len() {
        return Vec::new();
    }
    let block_start = i + 1;
    let block_end = find_matching_rbrace(bytes, block_start);
    let body = &src[block_start..block_end];

    let mut out: Vec<(String, String)> = Vec::new();
    let bb = body.as_bytes();
    let mut inner = 0i32;
    let mut arm_start = 0usize;
    let mut j = 0usize;
    while j < body.len() {
        let c = bb[j];
        if c == b'"' {
            j = skip_string(bb, j);
            continue;
        }
        if c == b'/' && bb.get(j + 1) == Some(&b'/') {
            while j < body.len() && bb[j] != b'\n' {
                j += 1;
            }
            continue;
        }
        match c {
            b'{' => inner += 1,
            b'}' => inner -= 1,
            b',' if inner == 0 => arm_start = j + 1,
            b'=' if inner == 0 && bb.get(j + 1) == Some(&b'>') => {
                let lhs = &body[arm_start..j];
                let mut labels = Vec::new();
                extract_quoted(lhs, &mut labels);
                // RHS is the first quoted string after `=>`.
                let mut k = j + 2;
                while k < body.len() && bb[k] != b'"' && !(bb[k] == b',' && inner == 0) {
                    // Could be a function call like `arity_doc(...)` — then no leading string.
                    if bb[k] == b',' {
                        break;
                    }
                    k += 1;
                }
                let desc = if k < body.len() && bb[k] == b'"' {
                    let end = skip_string(bb, k);
                    let raw = &body[k + 1..end - 1];
                    first_sentence(raw)
                } else {
                    String::new()
                };
                if !desc.is_empty() {
                    for l in labels {
                        out.push((l, desc.clone()));
                    }
                }
                j += 2;
                continue;
            }
            _ => {}
        }
        j += 1;
    }
    // Dedupe — same label can appear if multiple arms match it (shouldn't).
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Take the first sentence of a markdown doc string. Strips leading backticks,
/// unescapes common sequences, and trims to 200 chars max. Aims for a
/// single-line description fit for a hash value.
fn first_sentence(raw: &str) -> String {
    // Unescape the Rust string-literal escapes that show up in the LSP docs.
    let mut s = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => s.push('\n'),
                Some('t') => s.push('\t'),
                Some('r') => s.push('\r'),
                Some('\\') => s.push('\\'),
                Some('"') => s.push('"'),
                Some('\'') => s.push('\''),
                Some(other) => s.push(other),
                None => break,
            }
            continue;
        }
        s.push(c);
    }
    // First paragraph, first sentence.
    let first_para = s.split("\n\n").next().unwrap_or(&s);
    let first_para = first_para.lines().next().unwrap_or(first_para).trim();
    // Truncate at the first `. ` (period + space) for a natural sentence stop.
    let mut sentence = first_para.to_string();
    if let Some(idx) = sentence.find(". ") {
        sentence.truncate(idx + 1);
    }
    // Hard cap to keep hash values reasonable.
    const MAX: usize = 200;
    if sentence.chars().count() > MAX {
        let truncated: String = sentence.chars().take(MAX - 1).collect();
        sentence = format!("{}…", truncated);
    }
    sentence.trim().to_string()
}

fn find_matching_rbrace(bytes: &[u8], start: usize) -> usize {
    find_matching_close(bytes, start, b'{', b'}')
}

fn find_matching_close(bytes: &[u8], start: usize, open: u8, close: u8) -> usize {
    let mut depth = 1i32;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i = skip_string(bytes, i);
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    panic!("unterminated block starting at {}", start);
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'"' {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn extract_quoted(slice: &str, out: &mut Vec<String>) {
    let bytes = slice.as_bytes();
    let mut i = 0usize;
    while i < slice.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < slice.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j = (j + 2).min(slice.len());
                    continue;
                }
                j += 1;
            }
            if j > start {
                out.push(slice[start..j].to_string());
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Extract `///` doc comments from `fn builtin_*` in `src/builtins.rs` and
/// map each to its dispatch names. Makes `///` comments the single source of
/// truth for `%perlrs::descriptions` and LSP hover.
fn extract_builtin_doc_comments(src: &str, _arms: &[Vec<String>]) -> Vec<(String, String)> {
    // Scan dispatch: "name" => Some(builtin_xyz( to build name->fn map
    let mut name_to_fn: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let dispatch_re =
        regex::Regex::new(r#""([a-z_][a-z0-9_:]*)"[^=]*=>[^(]*\((builtin_\w+)\("#).unwrap();
    for cap in dispatch_re.captures_iter(src) {
        name_to_fn.insert(cap[1].to_string(), cap[2].to_string());
    }

    // Scan fn builtin_* and collect preceding /// comments
    let mut fn_docs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("fn builtin_") || trimmed.starts_with("pub fn builtin_")) {
            continue;
        }
        let fn_name = trimmed
            .split('(')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .last()
            .unwrap_or("");
        if fn_name.is_empty() {
            continue;
        }

        let mut doc_lines: Vec<&str> = Vec::new();
        let mut j = i as isize - 1;
        while j >= 0 {
            let prev = lines[j as usize].trim();
            if prev.starts_with("///") {
                doc_lines.push(
                    prev.strip_prefix("/// ")
                        .unwrap_or(prev.strip_prefix("///").unwrap_or(prev)),
                );
                j -= 1;
            } else if prev.is_empty() {
                j -= 1;
            } else {
                break;
            }
        }
        if doc_lines.is_empty() {
            continue;
        }
        doc_lines.reverse();
        let doc = doc_lines.join(" ").trim().to_string();
        if !doc.is_empty() {
            fn_docs.insert(fn_name.to_string(), doc);
        }
    }

    // Build (dispatch_name, doc) pairs
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (dispatch_name, fn_name) in &name_to_fn {
        if let Some(doc) = fn_docs.get(fn_name) {
            if seen.insert(dispatch_name.clone()) {
                out.push((dispatch_name.clone(), first_sentence_build(doc)));
            }
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn first_sentence_build(s: &str) -> String {
    let mut sentence = s.to_string();
    if let Some(idx) = sentence.find(". ") {
        sentence.truncate(idx + 1);
    }
    if sentence.chars().count() > 200 {
        let truncated: String = sentence.chars().take(199).collect();
        sentence = format!("{}\u{2026}", truncated);
    }
    sentence
}
