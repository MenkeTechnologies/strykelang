//! Offline generator for `docs/reference.html` — the full `stryke docs` corpus
//! rendered as a static HTML page using the same cyberpunk styling as
//! `docs/index.html`. Run with `cargo run --bin gen-docs` before pushing to
//! GitHub Pages.
//!
//! Source of truth: `stryke::lsp::DOC_CATEGORIES` (chapter → topic names)
//! and `stryke::lsp::doc_text_for(topic)` (raw markdown: `lsp.rs` plus
//! `lsp_docs_domains.rs` fallbacks). Topics not in
//! any `DOC_CATEGORIES` chapter land under a synthetic "Other" chapter so
//! nothing silently vanishes.
//!
//! The markdown → HTML converter is intentionally minimal (in-house, no
//! crate dependency): it handles what the LSP docs actually use — fenced
//! `perl code blocks, inline backticks, paragraph breaks, `###` headings,
//! and bullet lists — which is 95%+ of the corpus. Anything weirder falls
//! through as escaped text.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use stryke::lsp::{doc_text_for, doc_topics, DOC_CATEGORIES};

fn main() {
    let out_path = PathBuf::from("docs/reference.html");
    let html = build_page();
    fs::write(&out_path, html).expect("write docs/reference.html");
    println!("wrote {}", out_path.display());
    stamp_index_version();
}

/// Rewrite the `stryke v…` line in `docs/index.html` so the hub page stays
/// in sync with `Cargo.toml` without hand-editing. Relies on a stable id
/// selector (`id="strykeBuildLine"`) on the `<p class="docs-build-line">`
/// so we can swap just that line without touching the rest of the markup.
fn stamp_index_version() {
    let path = PathBuf::from("docs/index.html");
    let Ok(src) = fs::read_to_string(&path) else {
        println!("note: docs/index.html not found, skipping version stamp");
        return;
    };
    let version = env!("CARGO_PKG_VERSION");
    let needle_start = r#"<p class="docs-build-line" id="strykeBuildLine">"#;
    let needle_end = "</p>";
    let Some(s) = src.find(needle_start) else {
        println!("note: build-line marker not found in docs/index.html, skipping");
        return;
    };
    let after = s + needle_start.len();
    let Some(e_rel) = src[after..].find(needle_end) else {
        return;
    };
    let e = after + e_rel;

    // Pull live reflection counts so the build line stays in sync with
    // every release — `gen-docs` is the canonical regenerator, so the
    // numbers reflect WHATEVER the current source tree exposes.
    let n_builtins = stryke::builtins::builtins_hash_map().len();
    let n_all = stryke::builtins::all_hash_map().len();
    let fmt_count = |n: usize| -> String {
        // ASCII thousand separators (`10,335`). Walk right-to-left
        // so the comma cadence is independent of total length.
        let s = n.to_string();
        let mut rev = String::with_capacity(s.len() + s.len() / 3);
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                rev.push(',');
            }
            rev.push(c);
        }
        rev.chars().rev().collect()
    };
    let n_builtins_str = fmt_count(n_builtins);
    let n_all_str = fmt_count(n_all);

    // Build line shape: `stryke v{V} · {tagline segments…} · {N} builtins ({M} keys in %all) · {trailing}`.
    // The `{tagline segments…}` and `{trailing}` parts stay verbatim;
    // we splice the dynamic counts in by pattern. If the pattern
    // isn't present (older index.html), append the count segment to
    // the existing tail.
    let current = &src[after..e];
    let tail = current.find(" · ").map(|i| &current[i..]).unwrap_or("");
    let count_segment = format!(" · {n_builtins_str} builtins ({n_all_str} keys in %all)");
    let tail_with_counts: String =
        if let Some(re) = regex_replace_count_segment(tail, &count_segment) {
            re
        } else {
            // No prior counts segment — insert one before the final ` · `
            // trailing tagline (LSP + DAP + JetBrains plugin).
            if let Some(last_sep) = tail.rfind(" · ") {
                format!(
                    "{}{}{}",
                    &tail[..last_sep],
                    count_segment,
                    &tail[last_sep..]
                )
            } else {
                format!("{tail}{count_segment}")
            }
        };

    let replacement = format!("stryke v{version}{tail_with_counts}");
    let mut new_src = if current == replacement {
        src.clone()
    } else {
        format!(
            "{}{}{}{}",
            &src[..after],
            replacement,
            needle_end,
            &src[e + needle_end.len()..]
        )
    };

    // Replace EVERY occurrence of the count phrase in the file
    // (build line + tutorial-subtitle + any other prose mention).
    // Two shapes accepted:
    //   ` · N builtins (M keys in %all)`         — build-line form
    //   `, N builtins in %b (M keys in %all)`    — subtitle form
    let mut changed = false;
    while let Some(rep) = regex_replace_count_segment(&new_src, &count_segment) {
        if rep == new_src {
            break;
        }
        new_src = rep;
        changed = true;
    }
    let subtitle_segment = format!(", {n_builtins_str} builtins in %b ({n_all_str} keys in %all)");
    while let Some(rep) = regex_replace_subtitle_count(&new_src, &subtitle_segment) {
        if rep == new_src {
            break;
        }
        new_src = rep;
        changed = true;
    }

    if changed || current != replacement {
        fs::write(&path, &new_src).expect("write docs/index.html");
        println!(
            "stamped docs/index.html with v{version} ({n_builtins_str} builtins, {n_all_str} keys in %all)"
        );
    }

    // LOC stats — prefer `tokei` (it strips comments + blanks, gives
    // proper "code" lines). Fall back to a wc-style line counter if
    // tokei isn't installed. Stryke `.stk` files aren't a recognized
    // language in tokei yet, so they always use the wc fallback.
    let n_rust_src = tokei_code_lines("strykelang", "Rust")
        .unwrap_or_else(|| count_lines_under("strykelang", "rs"));
    let n_tests =
        tokei_code_lines("tests", "Rust").unwrap_or_else(|| count_lines_under("tests", "rs"));
    let n_examples = count_lines_under("examples", "stk");
    let loc_segment = format!(
        "{} Rust src · {} test src · {} example src",
        fmt_count(n_rust_src),
        fmt_count(n_tests),
        fmt_count(n_examples),
    );
    match stamp_loc_span(&new_src, &loc_segment) {
        LocStampOutcome::Rewritten(s) => {
            fs::write(&path, &s).expect("write docs/index.html");
            println!("stamped docs/index.html LOC: {loc_segment}");
        }
        LocStampOutcome::Unchanged => {
            println!("LOC unchanged: {loc_segment}");
        }
        LocStampOutcome::NoPlaceholder => {
            println!(
                "LOC: {loc_segment} (no <span id=\"strykeLoc\"> placeholder in docs/index.html)"
            );
        }
    }
}

/// Run `tokei -o json <root>` and return the `code` line count for
/// the requested language (`"Rust"`, `"Perl"`, …). Returns `None` if
/// tokei is missing, fails, or doesn't have the language section.
fn tokei_code_lines(root: &str, lang: &str) -> Option<usize> {
    let output = std::process::Command::new("tokei")
        .arg("-o")
        .arg("json")
        .arg(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let code = json.get(lang)?.get("code")?.as_u64()? as usize;
    Some(code)
}

/// Replace the `, N,NNN builtins in %b (M,MMM keys in %all)` form
/// used by the tutorial-subtitle paragraph. Comma-prefix, ` in %b `
/// inside the count — different shape from the build-line form
/// (` · N builtins (M keys in %all)`).
fn regex_replace_subtitle_count(text: &str, new_seg: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b',' && bytes[i + 1] == b' ' {
            let segment_start = i;
            let mut j = i + 2;
            let n_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b',') {
                j += 1;
            }
            if j == n_start {
                i += 1;
                continue;
            }
            let middle = " builtins in %b (";
            if !text[j..].starts_with(middle) {
                i += 1;
                continue;
            }
            j += middle.len();
            let m_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b',') {
                j += 1;
            }
            if j == m_start {
                i += 1;
                continue;
            }
            let end = " keys in %all)";
            if !text[j..].starts_with(end) {
                i += 1;
                continue;
            }
            j += end.len();
            return Some(format!(
                "{}{}{}",
                &text[..segment_start],
                new_seg,
                &text[j..]
            ));
        }
        i += 1;
    }
    None
}

/// Result of attempting to stamp a `<span id="strykeLoc">` placeholder.
enum LocStampOutcome {
    /// Page didn't have the placeholder — caller should announce
    /// the numbers as console-only.
    NoPlaceholder,
    /// Placeholder present but text already matches — no rewrite.
    Unchanged,
    /// Placeholder present, text rewritten — caller writes the page.
    Rewritten(String),
}

fn stamp_loc_span(src: &str, new_text: &str) -> LocStampOutcome {
    let open = r#"<span id="strykeLoc">"#;
    let close = "</span>";
    let Some(s) = src.find(open) else {
        return LocStampOutcome::NoPlaceholder;
    };
    let after = s + open.len();
    let Some(e_rel) = src[after..].find(close) else {
        return LocStampOutcome::NoPlaceholder;
    };
    let e = after + e_rel;
    if &src[after..e] == new_text {
        return LocStampOutcome::Unchanged;
    }
    LocStampOutcome::Rewritten(format!("{}{}{}", &src[..after], new_text, &src[e..]))
}

/// Sum line counts of every `*.<ext>` file under `root`. Walks
/// directories recursively; skips `target/` and any path containing
/// `.git`. Returns 0 if `root` doesn't exist.
fn count_lines_under(root: &str, ext: &str) -> usize {
    let mut total = 0usize;
    let root_path = PathBuf::from(root);
    if !root_path.exists() {
        return 0;
    }
    let mut stack: Vec<PathBuf> = vec![root_path];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|s| s.to_str()) == Some(ext) {
                if let Ok(s) = fs::read_to_string(&p) {
                    total += s.lines().count();
                }
            }
        }
    }
    total
}

/// Replace the existing `· N,NNN builtins (M,MMM keys in %all)` segment
/// in `tail` (everything from the first ` · ` onward) with `new_seg`.
/// Returns `Some(...)` on hit, `None` on miss (caller falls back to
/// inserting a fresh segment).
fn regex_replace_count_segment(tail: &str, new_seg: &str) -> Option<String> {
    // Match ` · ` then digits + optional thousand-separator commas then
    // ` builtins (` then digits + optional commas then ` keys in %all)`.
    // Hand-rolled to avoid pulling in a regex crate for one-shot use.
    let bytes = tail.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        // Look for " · " (UTF-8 for ` · ` = " \xc2\xb7 ").
        if bytes[i] == b' '
            && i + 2 < bytes.len()
            && bytes[i + 1] == 0xc2
            && bytes[i + 2] == 0xb7
            && i + 3 < bytes.len()
            && bytes[i + 3] == b' '
        {
            let segment_start = i;
            // After " · ", consume `[0-9,]+ builtins (`.
            let mut j = i + 4;
            let n_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b',') {
                j += 1;
            }
            if j == n_start {
                i += 1;
                continue;
            }
            let middle = " builtins (";
            if !tail[j..].starts_with(middle) {
                i += 1;
                continue;
            }
            j += middle.len();
            let m_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b',') {
                j += 1;
            }
            if j == m_start {
                i += 1;
                continue;
            }
            let end = " keys in %all)";
            if !tail[j..].starts_with(end) {
                i += 1;
                continue;
            }
            j += end.len();
            // Found the segment from `segment_start..j`; replace.
            return Some(format!(
                "{}{}{}",
                &tail[..segment_start],
                new_seg,
                &tail[j..]
            ));
        }
        i += 1;
    }
    None
}

fn build_page() -> String {
    // Collect (chapter, topic, markdown) in category order, pulling the
    // leftovers into an "Other" chapter at the end.
    let mut chapters: Vec<(&str, Vec<(&str, &'static str)>)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut seen_text_ptrs: HashSet<usize> = HashSet::new();

    for &(chapter, topics) in DOC_CATEGORIES {
        let mut rows: Vec<(&str, &'static str)> = Vec::new();
        for &t in topics {
            let Some(md) = doc_text_for(t) else { continue };
            let ptr = md.as_ptr() as usize;
            // Same pointer = alias to an already-rendered doc; skip the dup.
            if !seen_text_ptrs.insert(ptr) {
                seen.insert(t);
                continue;
            }
            rows.push((t, md));
            seen.insert(t);
        }
        if !rows.is_empty() {
            chapters.push((chapter, rows));
        }
    }

    // Leftovers: any doc_topics entry not claimed by a chapter above.
    let mut other: Vec<(&str, &'static str)> = Vec::new();
    for t in doc_topics() {
        if seen.contains(t) {
            continue;
        }
        let Some(md) = doc_text_for(t) else { continue };
        let ptr = md.as_ptr() as usize;
        if !seen_text_ptrs.insert(ptr) {
            continue;
        }
        other.push((t, md));
    }
    if !other.is_empty() {
        chapters.push(("Other", other));
    }

    let total_topics: usize = chapters.iter().map(|(_, r)| r.len()).sum();
    let chapter_count = chapters.len();

    // ── render ──────────────────────────────────────────────────────────
    let mut out = String::with_capacity(1_000_000);
    out.push_str(HEAD);
    out.push_str(&format!(
        r#"  <header class="tutorial-header">
    <div class="tutorial-header-inner">
      <div>
        <h1 class="tutorial-brand">// STRYKE — FULL REFERENCE</h1>
        <nav class="tutorial-crumbs" aria-label="Breadcrumb">
          <a href="index.html">Docs</a>
          <span class="sep">/</span>
          <span class="current">Reference</span>
          <span class="sep">/</span>
          <a href="https://github.com/MenkeTechnologies/strykelang" target="_blank" rel="noopener noreferrer">GitHub</a>
        </nav>
        <p class="docs-build-line">stryke v{version} · {total_topics} topics · {chapter_count} chapters · generated from <code>strykelang/lsp.rs</code></p>
      </div>
      <div class="tutorial-toolbar">
        <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>
        <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>
        <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>
        <a class="btn btn-secondary" href="index.html">Hub</a>
        <a class="btn btn-secondary" href="https://github.com/MenkeTechnologies/strykelang" target="_blank" rel="noopener noreferrer">GitHub</a>
      </div>
    </div>
  </header>

  <div class="hub-scheme-strip">
    <div class="hub-scheme-strip-inner">
      <span class="hud-scheme-label">// Color scheme</span>
      <div class="scheme-grid" id="hudSchemeGrid"></div>
    </div>
  </div>

  <main class="tutorial-main">
    <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>LANGUAGE REFERENCE</h2>
    <p class="tutorial-subtitle">Every builtin, keyword, alias, and extension with an LSP hover doc — rendered from the exact markdown that `stryke docs` shows in the terminal. Jump via the chapter index, or <kbd>Ctrl+F</kbd> for a specific name.</p>
"#,
        version = env!("CARGO_PKG_VERSION"),
        total_topics = total_topics,
        chapter_count = chapter_count,
    ));

    // Chapter index
    out.push_str(
        r#"    <section class="tutorial-section">
      <h2>Chapters</h2>
      <ul class="chapter-index">
"#,
    );
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            "        <li><a href=\"#ch-{slug}\">{chapter}</a> <span class=\"chapter-count\">{n}</span></li>\n",
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
    }
    out.push_str("      </ul>\n    </section>\n");

    // Chapters and their topics
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            r#"    <section class="tutorial-section" id="ch-{slug}">
      <h2>{chapter}</h2>
      <p class="chapter-meta">{n} topics</p>
"#,
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
        for (topic, md) in rows {
            let topic_slug = slugify(topic);
            let topic_escaped = html_escape(topic);
            out.push_str("      <article class=\"doc-entry\" id=\"doc-");
            out.push_str(&topic_slug);
            out.push_str("\">\n        <h3><a class=\"doc-anchor\" href=\"#doc-");
            out.push_str(&topic_slug);
            out.push_str("\">#</a> <code>");
            out.push_str(&topic_escaped);
            out.push_str("</code></h3>\n");
            out.push_str(&markdown_to_html(md));
            out.push_str("      </article>\n");
        }
        out.push_str("    </section>\n");
    }

    out.push_str(FOOT);
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Minimal markdown → HTML converter. Scope: what the LSP corpus actually
// uses. Blocks: fenced code, `### heading`, blank-line-separated paragraphs,
// `-`/`*` bullet lists. Inlines: `backtick code`. Everything else is HTML-
// escaped and passes through as plain text.
// ─────────────────────────────────────────────────────────────────────────
fn markdown_to_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + md.len() / 4);
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Fenced code block: ```LANG … ```
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            let lang = rest.trim().to_string();
            let lang_attr = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"lang-{}\"", html_escape(&lang))
            };
            out.push_str(&format!("        <pre><code{lang_attr}>"));
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                out.push_str(&html_escape(l));
                out.push('\n');
                i += 1;
            }
            out.push_str("</code></pre>\n");
            continue;
        }

        // Heading: `###` (the only level the corpus uses).
        if let Some(body) = line.strip_prefix("### ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }
        if let Some(body) = line.strip_prefix("## ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }

        // Bullet list.
        if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
            out.push_str("        <ul>\n");
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_start();
                let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
                    break;
                };
                out.push_str(&format!("          <li>{}</li>\n", inline(item)));
                i += 1;
            }
            out.push_str("        </ul>\n");
            continue;
        }

        // Blank line → paragraph boundary.
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Paragraph: accumulate contiguous non-blank, non-block lines.
        let mut para = String::new();
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if l.trim().is_empty()
                || t.starts_with("```")
                || t.starts_with("### ")
                || t.starts_with("## ")
                || t.starts_with("- ")
                || t.starts_with("* ")
            {
                break;
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(l.trim());
            i += 1;
        }
        if !para.is_empty() {
            out.push_str(&format!("        <p>{}</p>\n", inline(&para)));
        }
    }
    out
}

/// Inline pass: `backtick code` spans and `**bold**` spans, otherwise
/// HTML-escape. Single `*em*` is intentionally not supported because the
/// lsp corpus contains perl-syntax text like `$_ * 2` and `*foo` typeglobs
/// that would generate false matches; bold uses doubled `**` which avoids
/// that collision.
fn inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if bytes[i] == b'`' {
            // Find matching backtick.
            let start = i + 1;
            let mut j = start;
            while j < s.len() && bytes[j] != b'`' {
                j += 1;
            }
            if j < s.len() {
                out.push_str("<code>");
                out.push_str(&html_escape(&s[start..j]));
                out.push_str("</code>");
                i = j + 1;
                continue;
            }
        }
        // `**bold**`. Require the closing `**` to also exist; otherwise fall
        // through and treat the literal `**` as text.
        if i + 1 < s.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < s.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    break;
                }
                j += 1;
            }
            if j + 1 < s.len() && bytes[j] == b'*' && bytes[j + 1] == b'*' {
                out.push_str("<strong>");
                out.push_str(&inline(&s[start..j]));
                out.push_str("</strong>");
                i = j + 2;
                continue;
            }
        }
        // Default: html-escape this one char.
        let c = &s[i..i + char_len(bytes, i)];
        out.push_str(&html_escape(c));
        i += c.len();
    }
    out
}

fn char_len(bytes: &[u8], i: usize) -> usize {
    let b = bytes[i];
    if b < 0x80 {
        1
    } else if b & 0xE0 == 0xC0 {
        2
    } else if b & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

const HEAD: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark light">
  <meta name="description" content="stryke full reference — every builtin, keyword, alias, and extension with its LSP hover doc rendered as a static page.">
  <title>stryke — Reference</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&amp;family=Share+Tech+Mono&amp;display=swap" rel="stylesheet">
  <link rel="stylesheet" href="hud-static.css">
  <link rel="stylesheet" href="tutorial.css">
  <style>
    .tutorial-main { max-width: 72rem; }
    .docs-build-line {
      margin: 0.35rem 0 0;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px; color: var(--text-dim);
      letter-spacing: 0.03em; max-width: 42rem; opacity: 0.75;
    }
    .hub-scheme-strip {
      border-bottom: 1px dashed var(--border);
      background: color-mix(in srgb, var(--bg-secondary) 85%, transparent);
      padding: 0.55rem 1.5rem 0.65rem; position: relative;
    }
    .hub-scheme-strip-inner {
      max-width: 72rem; margin: 0 auto;
      display: flex; align-items: center; gap: 0.85rem;
    }
    .hub-scheme-strip .hud-scheme-label {
      flex: 0 0 auto;
      font-family: 'Orbitron', sans-serif; font-size: 9px; font-weight: 700;
      letter-spacing: 2px; text-transform: uppercase; color: var(--accent);
    }
    .hub-scheme-strip .scheme-grid {
      flex: 1 1 auto;
      display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 6px;
    }
    @media (max-width: 720px) {
      .hub-scheme-strip-inner { flex-direction: column; align-items: stretch; }
      .hub-scheme-strip .scheme-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    }

    .chapter-index {
      list-style: none; padding: 0; margin: 0;
      display: grid; grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr));
      gap: 0.3rem;
    }
    .chapter-index li {
      border: 1px solid var(--border); padding: 0.45rem 0.65rem; border-radius: 2px;
      background: color-mix(in srgb, var(--bg-card) 92%, transparent);
      display: flex; justify-content: space-between; align-items: baseline;
    }
    .chapter-index li a {
      color: var(--cyan); text-decoration: none; font-size: 13px;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-index li a:hover { color: var(--accent-light); }
    .chapter-count {
      font-size: 10px; color: var(--text-muted);
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-meta {
      font-size: 11px; color: var(--text-muted); margin: -0.3rem 0 0.8rem;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }

    .doc-entry {
      margin: 1rem 0 1.4rem;
      padding: 0.75rem 0.9rem 0.5rem;
      border-left: 2px solid var(--cyan);
      background: color-mix(in srgb, var(--bg) 94%, transparent);
      border-radius: 2px;
    }
    .doc-entry h3 {
      margin: 0 0 0.45rem;
      font-family: 'Orbitron', sans-serif;
      font-size: 13px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--cyan);
    }
    .doc-entry h3 code {
      color: var(--accent-light); background: transparent; border: none;
      padding: 0; font-size: 1em; letter-spacing: 0.5px;
    }
    .doc-entry .doc-anchor {
      color: var(--text-muted); font-size: 0.85em; margin-right: 0.25rem;
      text-decoration: none;
    }
    .doc-entry .doc-anchor:hover { color: var(--accent); }
    .doc-entry h4 {
      font-family: 'Orbitron', sans-serif;
      font-size: 11px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--accent-light);
      margin: 0.8rem 0 0.3rem;
    }
    .doc-entry p {
      font-size: 13px; line-height: 1.6; color: var(--text-dim);
      margin: 0.35rem 0;
    }
    .doc-entry p code, .doc-entry li code {
      color: var(--accent-light); font-size: 12px;
    }
    .doc-entry ul { margin: 0.3rem 0 0.5rem; padding-left: 1.25rem; }
    .doc-entry li { font-size: 13px; color: var(--text-dim); line-height: 1.55; margin: 0.2rem 0; }
    .doc-entry pre {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 12px;
      background: var(--bg); border: 1px solid var(--border);
      border-radius: 2px;
      padding: 0.7rem 0.9rem; overflow-x: auto;
      color: var(--text); margin: 0.5rem 0;
      box-shadow: inset 0 0 18px rgba(0, 0, 0, 0.35);
    }
    .doc-entry pre code { color: var(--text); background: transparent; border: none; padding: 0; }
    [data-theme="light"] .doc-entry pre { box-shadow: inset 0 0 10px rgba(0, 0, 0, 0.05); }

    kbd {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px;
      padding: 1px 6px;
      background: var(--bg-secondary);
      border: 1px solid var(--border);
      border-bottom-width: 2px;
      border-radius: 3px;
      color: var(--cyan);
    }
  </style>
</head>
<body>
  <div class="app tutorial-app" id="docsApp">
    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>
    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>
"##;

const FOOT: &str = r#"  </main>
  </div>
  <script src="hud-theme.js"></script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::markdown_to_html;
    use stryke::lsp::doc_text_for;

    /// Regression: domain docs with many inline ` spans (e.g. tensor notation) must
    /// survive `markdown_to_html` so `docs/reference.html` matches LSP hover text.
    #[test]
    fn harris_response_md_roundtrips_to_html() {
        let md = doc_text_for("harris_response").expect("harris_response documented");
        assert!(
            md.contains("structure tensor"),
            "doc_text_for: len={}",
            md.len()
        );
        let html = markdown_to_html(md);
        assert!(
            html.contains("structure tensor"),
            "markdown_to_html: len={}",
            html.len()
        );
    }
}
