//! Generates reflection tables from the source at compile time so
//! `%builtins` / `%aliases` / `%descriptions` never drift from the real
//! parser / dispatcher / LSP docs.
//!
//! Emits `$OUT_DIR/reflection.rs` with:
//!   - `BUILTIN_ARMS: &[&[&str]]`    — per-arm names from `try_builtin`
//!     (used for the `%aliases` alias → primary map).
//!   - `CATEGORY_MAP: &[(&str, &str)]` — name → category string, parsed
//!     from the `// ── category ──` section comments in `is_perl5_core`
//!     and `stryke_extension_name`. Category strings are lowercase,
//!     human-readable ("parallel", "string", "filesystem", ...).
//!   - `DESCRIPTIONS: &[(&str, &str)]` — name → first-line hover doc,
//!     harvested from the `doc_for_label_text` match in `src/lsp.rs`.
//!     Sparse — only labels that have a doc entry show up.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=strykelang/builtins.rs");
    println!("cargo:rerun-if-changed=strykelang/parser.rs");
    println!("cargo:rerun-if-changed=strykelang/lsp.rs");
    println!("cargo:rerun-if-changed=strykelang/list_builtins.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let builtins_src =
        fs::read_to_string("strykelang/builtins.rs").expect("read strykelang/builtins.rs");
    let parser_src = fs::read_to_string("strykelang/parser.rs").expect("read strykelang/parser.rs");
    let lsp_src = fs::read_to_string("strykelang/lsp.rs").expect("read strykelang/lsp.rs");
    let list_builtins_src = fs::read_to_string("strykelang/list_builtins.rs")
        .expect("read strykelang/list_builtins.rs");

    let arms = extract_try_builtin_arms(&builtins_src);
    // `is_perl5_core` uses `matches!(name, …)` (parens), `stryke_extension_name`
    // uses `match name { … }` (braces). Different block markers per fn.
    let core_cats = extract_categorized_names(&parser_src, "fn is_perl5_core", "matches!");
    let mut ext_cats =
        extract_categorized_names(&parser_src, "fn stryke_extension_name", "match name");
    // List builtins are dispatched via `list_builtins::dispatch_by_name`,
    // not the main `try_builtin` arms — so build.rs has to scan them
    // separately or `%b` / `%all` won't list `sum` / `min` / `max` /
    // `pairs` / etc. Fold them into the extension category list with
    // a uniform "list / aggregate" bucket.
    ext_cats.extend(extract_list_builtin_names(&list_builtins_src));

    // Syntactic-builtin aliases (`fi`→`filter`, `nums`→`numbers`, …). These
    // dispatch through the parser's `FuncCall`/`GrepExpr` arms, never through
    // `try_builtin`, so without this they'd masquerade as standalone primaries
    // in `%b` (or vanish entirely) instead of resolving in `%a`. Source of
    // truth is the dispatch itself: `extract_funccall_aliases` reads each arm's
    // `name:` literal, `KEYWORD_BUILTIN_ALIASES` supplies the enum-family pairs.
    let mut raw_aliases = extract_funccall_aliases(&parser_src);
    raw_aliases.extend(extract_two_col_const(
        &parser_src,
        "KEYWORD_BUILTIN_ALIASES",
    ));
    // `count`/`len`/`cnt`/`size` are a contested synonym cluster with no
    // uncontested primary (dispatch impl-name is `count`, STYLE_GUIDE §5 picks
    // `len`). These four must stay `%b` primaries, so they're excluded *as
    // aliases* (never demoted). The check is alias-side only: a NEW short alias
    // pointing INTO the cluster (`l`→`len`) is still allowed — it adds a
    // spelling to `%a` without demoting any cluster member.
    const ALIAS_EXCLUDE: &[&str] = &["len", "cnt", "count", "size"];
    // Names that serve as a canonical (`name:` target) anywhere are real
    // primaries and must never be demoted, even when they also appear on some
    // other arm's LHS (e.g. `len` on the `count` arm). `try_builtin` first-slot
    // primaries are protected too: a name like `uid` is a real builtin AND a
    // FuncCall alias of `uuid` — context-dependent, so it stays a primary.
    let mut canonical_set: std::collections::HashSet<String> =
        raw_aliases.iter().map(|(_, p)| p.clone()).collect();
    for arm in &arms {
        if let Some(primary) = arm.first() {
            canonical_set.insert(primary.clone());
        }
    }
    // Resolve to a deduped alias→primary map (`BTreeMap` → deterministic,
    // sorted emit). Later rows win, so the `KEYWORD_BUILTIN_ALIASES` supplement
    // (appended last) overrides any FuncCall-arm collision.
    let mut alias_to_primary: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (alias, primary) in &raw_aliases {
        if alias == primary
            || ALIAS_EXCLUDE.contains(&alias.as_str())
            || canonical_set.contains(alias)
        {
            continue;
        }
        alias_to_primary.insert(alias.clone(), primary.clone());
    }
    let syn_alias_set: std::collections::HashSet<String> =
        alias_to_primary.keys().cloned().collect();
    // Descriptions: /// doc comments from builtins.rs (primary source for ~1100 fns)
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
    core_pairs.retain(|(n, _)| !syn_alias_set.contains(n)); // alias → `%a`, not core `%b`
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
        if syn_alias_set.contains(name) {
            continue; // alias spelling — belongs in `%a`, not `%b`
        }
        if ext_seen.insert(name.clone()) {
            ext_pairs.push((name.clone(), cat.clone()));
        }
    }
    for arm in &arms {
        if let Some(primary) = arm.first() {
            if core_set.contains(primary.as_str()) || syn_alias_set.contains(primary) {
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
    // Syntactic-builtin aliases join `%all`, inheriting their primary's
    // category (so `$all{fi}` returns `filter`'s category). Drop any whose
    // primary isn't a known callable — a stale arm would otherwise leak an
    // uncategorized spelling into the registry.
    for (alias, primary) in &alias_to_primary {
        let Some(cat) = primary_to_cat.get(primary) else {
            continue;
        };
        if all_seen.insert(alias.clone()) {
            all_pairs.push((alias.clone(), cat.clone()));
        }
    }
    all_pairs.sort();

    // Only keep alias pairs whose primary is a real callable — mirrors the
    // `%all` guard above so `SYNTACTIC_ALIASES`, `%a`, and `%all` agree.
    let syn_alias_pairs: Vec<(String, String)> = alias_to_primary
        .iter()
        .filter(|(_, p)| primary_to_cat.contains_key(*p))
        .map(|(a, p)| (a.clone(), p.clone()))
        .collect();

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

    // `SYNTACTIC_ALIASES` — alias spelling → primary for builtins dispatched
    // through the parser's `FuncCall`/`GrepExpr` arms (not `try_builtin`).
    // `aliases_hash_map` merges these into `%a` alongside the `BUILTIN_ARMS`
    // aliases so `$a{fi}` == `"filter"`, `$a{nums}` == `"numbers"`, etc.
    body.push_str("pub(crate) const SYNTACTIC_ALIASES: &[(&str, &str)] = &[\n");
    for (a, p) in &syn_alias_pairs {
        body.push_str(&format!("    ({:?}, {:?}),\n", a, p));
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
            b'}' => {
                inner -= 1;
                // Block-bodied arms (`"name" => { ... }`) end at this
                // `}` whether or not a trailing comma follows. Reset
                // arm_start so the next arm's LHS scan doesn't span
                // back into this arm's body — without this, names like
                // `__stryke_rust_compile` (declared with a block body)
                // got merged with the next arm's pattern strings,
                // polluting reflection with phantom multi-name arms.
                if inner == 0 {
                    arm_start = i + 1;
                }
            }
            b',' if inner == 0 => arm_start = i + 1,
            b'=' if inner == 0 && bb.get(i + 1) == Some(&b'>') => {
                let mut names = Vec::new();
                extract_quoted(&body[arm_start..i], &mut names);
                // Drop qualifier-prefixed spellings (`CORE::eof`, `builtin::tell`) —
                // they're duplicate dispatch entries for the plain name already
                // in the same arm and pollute reflection with pseudo-primaries.
                names.retain(|n| !n.contains("::"));
                // Drop quoted strings that aren't valid bare-name dispatch
                // identifiers — block-body arms without a trailing comma
                // leak comment text and error-message string literals from
                // the previous arm's body into the next arm's LHS scan
                // ("I want a timestamp", "_thread_par_run: expected …").
                // A real builtin name is `[A-Za-z_!?]` then `[A-Za-z0-9_]*`,
                // optionally with a leading sigil — none contain spaces,
                // punctuation, or colons.
                names.retain(|n| is_valid_builtin_name(n));
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

/// True if `s` is a public bare-name builtin identifier eligible for
/// reflection / completion. Filters out:
///   - Comment text and error-message string literals (any name that
///     would fail an `[A-Za-z_][A-Za-z0-9_]*` ident shape).
///   - Internal entry points whose name starts with `_` (stryke uses a
///     leading underscore — `_thread_par_run`, `__stryke_rust_compile`
///     — for runtime-only dispatch arms; users should never call them
///     and the LSP shouldn't suggest them).
fn is_valid_builtin_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parse `pub const LIST_BUILTIN_NAMES: &[&str] = &[ ... ]` from
/// `list_builtins.rs` and return one `(name, category)` pair per entry.
/// All list builtins land in the "list / aggregate" category (sum, min,
/// max, pairs, blessed, refaddr, …); `%b` users can re-categorize at
/// the source if finer-grained groupings become useful.
fn extract_list_builtin_names(src: &str) -> Vec<(String, String)> {
    let marker = "pub const LIST_BUILTIN_NAMES";
    let Some(start) = src.find(marker) else {
        return Vec::new();
    };
    // Skip past the type annotation (`&[&str]` has its own `[`/`]`).
    // The actual array opens at the `[` AFTER `= &`. Anchor on `= &[`
    // so future signature edits (`Vec<&str>` etc.) still match cleanly
    // — the literal `&[` after the equals is the arr-open marker.
    let after = &src[start..];
    let Some(rel) = after.find("= &[") else {
        return Vec::new();
    };
    let body_start = start + rel + "= &[".len();
    let bytes = src.as_bytes();
    let body_end = find_matching_close(bytes, body_start, b'[', b']');
    let body = &src[body_start..body_end];
    let mut out = Vec::new();
    for line in body.lines() {
        let mut names = Vec::new();
        extract_quoted(line, &mut names);
        for n in names {
            out.push((n, "list / aggregate".to_string()));
        }
    }
    out
}

/// Scan `parser.rs` for `"a" | "b" | … => ExprKind::FuncCall { name: "X", … }`
/// dispatch arms and return `(spelling, canonical)` for every LHS spelling,
/// where `canonical` is the literal `name:` the arm dispatches to. This IS the
/// alias source of truth — the parser's own dispatch decides that `nums` means
/// `numbers`, so reflection reads it straight off the arm and never drifts.
///
/// Skips arms whose `name:` is a non-literal (`name: name.to_string()`, used by
/// the pass-through fallback) — those carry no canonical. Every alias arm in
/// the source has its LHS and `=> ExprKind::FuncCall {` on one line, so a
/// line-anchored scan is sufficient; the `name:` literal is matched within the
/// next few lines of the same arm body.
fn extract_funccall_aliases(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(arrow) = line.find("=> ExprKind::FuncCall {") else {
            continue;
        };
        let mut names = Vec::new();
        extract_quoted(&line[..arrow], &mut names);
        names.retain(|n| is_valid_builtin_name(n));
        if names.is_empty() {
            continue;
        }
        // Canonical = first `name: "…"` literal within this arm body (look a
        // few lines ahead; bail on the non-literal pass-through form).
        let mut canonical: Option<String> = None;
        for probe in lines.iter().skip(i).take(5) {
            if let Some(pos) = probe.find("name:") {
                let rest = probe[pos + "name:".len()..].trim_start();
                if let Some(stripped) = rest.strip_prefix('"') {
                    if let Some(end) = stripped.find('"') {
                        canonical = Some(stripped[..end].to_string());
                    }
                }
                break; // first `name:` decides — literal or pass-through
            }
        }
        let Some(canonical) = canonical else { continue };
        if !is_valid_builtin_name(&canonical) {
            continue;
        }
        for n in names {
            out.push((n, canonical.clone()));
        }
    }
    out
}

/// Parse a `pub(crate) const NAME: &[(&str, &str)] = &[ ("a","b"), … ]` table
/// of two-column rows from `src` and return each `(col0, col1)` pair. Used for
/// `KEYWORD_BUILTIN_ALIASES` (the enum-family alias supplement the FuncCall
/// scan can't see). Each non-comment row carries exactly two quoted strings.
fn extract_two_col_const(src: &str, const_name: &str) -> Vec<(String, String)> {
    let Some(start) = src.find(const_name) else {
        return Vec::new();
    };
    let after = &src[start..];
    let Some(rel) = after.find("= &[") else {
        return Vec::new();
    };
    let body_start = start + rel + "= &[".len();
    let bytes = src.as_bytes();
    let body_end = find_matching_close(bytes, body_start, b'[', b']');
    let body = &src[body_start..body_end];
    let mut out = Vec::new();
    for line in body.lines() {
        let mut names = Vec::new();
        extract_quoted(line, &mut names);
        if names.len() == 2 {
            out.push((names[0].clone(), names[1].clone()));
        }
    }
    out
}

/// Parse a function with `// ── category ──` section comments above groups
/// of quoted names, returning one (name, category) pair per listed name.
/// Works for both `is_perl5_core` (matches!) and `stryke_extension_name`
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
            // Same filter as `extract_try_builtin_arms` — drop
            // internal/private names and anything that isn't a clean
            // identifier (LHS scans can pick up garbage).
            if is_valid_builtin_name(&n) {
                pairs.push((n, current_cat.clone()));
            }
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
    // Reject noisy ones: the stryke_extension_name list has a stray
    // "stryke extensions that produce lists or have special syntax"
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
    // `doc_for_label_text` has an inner `match key` (sigil-preserving arms with
    // `=> Some("...")`); descriptions live in the outer `let md = match key`.
    let anchor = "let md: &'static str = match key";
    let Some(match_rel) = after.find(anchor) else {
        return Vec::new();
    };
    let body_start = fn_pos + match_rel + anchor.len();
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
/// truth for `%stryke::descriptions` and LSP hover.
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
