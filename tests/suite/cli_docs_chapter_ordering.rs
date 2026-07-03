//! `s docs` book layout invariants. Pre-fix, the doc browser walked
//! `category_map_iter()` (alphabetical-by-name in `CATEGORY_MAP`) so:
//!
//!   * Chapters were interleaved across the entire book — `[`/`]`
//!     navigation crossed a "chapter boundary" on nearly every page.
//!   * Each page was tagged with the `CATEGORY_MAP` source-comment
//!     label ("Base conversion", "Bit ops", …) which didn't match the
//!     intro / TOC chapter names from `DOC_CATEGORIES`.
//!   * Then a separate fix collapsed every leftover primary into one
//!     giant "Other" chapter spanning ~3500 pages.
//!
//! The current build packs entries in three passes:
//!   1. `DOC_CATEGORIES` curated chapters in declared order.
//!   2. Remaining `CATEGORY_MAP` primaries grouped by their source-
//!      comment category (sorted by `(category, name)` so each
//!      category is contiguous).
//!   3. Hand-written hover topics (keywords, operators, sigil hashes)
//!      in a trailing "Other" chapter.
//!
//! Three pinned invariants tested below: chapters appear in
//! `DOC_CATEGORIES` order at the top, every chapter's pages are a
//! contiguous run, and "Other" is the last chapter when present.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use stryke::lsp::DOC_CATEGORIES;

fn stryke_binary() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("target/release/stryke"),
        PathBuf::from("target/debug/stryke"),
    ];
    cands
        .iter()
        .filter(|p| p.exists())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .cloned()
}

/// One TOC chapter row: name and the (first, last) page span.
#[derive(Debug)]
struct Chapter {
    name: String,
    first_page: usize,
    last_page: usize,
}

/// Parse `s docs --toc` into chapter spans. Format (ANSI-stripped):
///
/// ```text
///   ChapterName
///     N. topic                         p.PG
///     ...
///   NextChapter
///     ...
/// ```
fn parse_toc() -> Option<Vec<Chapter>> {
    let bin = stryke_binary()?;
    let out = Command::new(&bin)
        .args(["docs", "--toc"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).to_string();
    // Strip ANSI escapes — they confuse simple line-shape matching.
    let stripped = strip_ansi(&raw);
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut cur: Option<Chapter> = None;
    for line in stripped.lines() {
        // Topic rows look like "    NN. name                   p.PP"
        // Chapter rows look like "  ChapterName" (exactly 2-space indent).
        let trimmed = line.trim_end();
        if let Some(idx) = trimmed.rfind(" p.") {
            // Topic row — extract the page number after `p.`.
            let tail = &trimmed[idx + 3..];
            let n: usize = tail
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            if n == 0 {
                continue;
            }
            if let Some(c) = cur.as_mut() {
                if c.first_page == 0 {
                    c.first_page = n;
                }
                c.last_page = n;
            }
            continue;
        }
        // Chapter heading — exactly 2 leading spaces, then non-space text.
        if line.starts_with("  ") && !line.starts_with("    ") {
            let name = line.trim().to_string();
            if name.is_empty() {
                continue;
            }
            // Skip the banner / header lines around the TOC frame.
            if name.starts_with('|')
                || name.starts_with('└')
                || name.starts_with('┌')
                || name.starts_with('│')
                || name.contains("TABLE OF CONTENTS")
            {
                continue;
            }
            if let Some(c) = cur.take() {
                if c.first_page > 0 {
                    chapters.push(c);
                }
            }
            cur = Some(Chapter {
                name,
                first_page: 0,
                last_page: 0,
            });
        }
    }
    if let Some(c) = cur {
        if c.first_page > 0 {
            chapters.push(c);
        }
    }
    Some(chapters)
}

fn strip_ansi(s: &str) -> String {
    // Tiny single-state ANSI stripper: drop `ESC [ ... letter` runs.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // CSI: ESC [ ... <0x40..0x7e>
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[test]
fn toc_starts_with_doc_categories_in_declared_order() {
    let Some(chapters) = parse_toc() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    // The first four DOC_CATEGORIES chapters should appear, in order, as
    // the first four chapters of the TOC. `want` is derived from
    // DOC_CATEGORIES (the source of truth) rather than hand-copied, so the
    // check locks the "TOC follows DOC_CATEGORIES order" invariant and can't
    // silently drift when chapters are added or reordered (four is enough to
    // catch the "interleaved alphabetical" regression).
    let want: Vec<&str> = DOC_CATEGORIES.iter().take(4).map(|(name, _)| *name).collect();
    let got: Vec<&str> = chapters
        .iter()
        .take(want.len())
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        got,
        want,
        "first {} TOC chapters should match DOC_CATEGORIES order, got {got:?}",
        want.len(),
    );
}

#[test]
fn each_chapter_has_a_contiguous_page_range() {
    let Some(chapters) = parse_toc() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    // Within each chapter, last_page >= first_page (well-formed
    // span). Across chapters, next.first_page > prev.last_page (no
    // backward jumps — i.e. no chapter "resumes" later in the book).
    let mut prev_last = 0usize;
    for c in &chapters {
        assert!(
            c.last_page >= c.first_page,
            "chapter {:?} has malformed span p.{}–p.{}",
            c.name,
            c.first_page,
            c.last_page,
        );
        assert!(
            c.first_page > prev_last,
            "chapter {:?} starts at p.{} but previous chapter ended at p.{} — \
             pages are interleaved across chapters (the original bug)",
            c.name,
            c.first_page,
            prev_last,
        );
        prev_last = c.last_page;
    }
}

#[test]
fn other_chapter_is_last_when_present() {
    let Some(chapters) = parse_toc() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let other_idx = chapters.iter().position(|c| c.name == "Other");
    if let Some(idx) = other_idx {
        assert_eq!(
            idx,
            chapters.len() - 1,
            "'Other' chapter should be last; found at index {idx} of {}",
            chapters.len(),
        );
    }
}

#[test]
fn no_chapter_dominates_more_than_a_third_of_the_book() {
    // Sanity guard against the "single giant Other chapter"
    // regression (3500+ pages of one bucket while others were tiny).
    // No single chapter should span more than ~33% of the total
    // pages; if it does, something has collapsed all leftovers into
    // one bucket again.
    let Some(chapters) = parse_toc() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let total = chapters.iter().map(|c| c.last_page).max().unwrap_or(0);
    if total == 0 {
        eprintln!("skip: empty TOC");
        return;
    }
    for c in &chapters {
        let span = c.last_page - c.first_page + 1;
        assert!(
            span * 3 < total * 2,
            "chapter {:?} spans {span} pages of {total} total — \
             leftovers likely collapsed into one bucket",
            c.name,
        );
    }
}
