//! Pin the zsh `_stryke` completion shim against two regressions:
//!
//!   * **`_describe` colon-parsing collapse.** zsh's `_describe`
//!     treats `:` as the name/description separator. Feeding it
//!     `::`-qualified topic names (`CORE::print`, `main::sum`,
//!     `%main::ENV`, `@main::a`, `%stryke::builtins`) parses each as
//!     `name=CORE` / `description=:print`, collapsing thousands of
//!     spellings into duplicate `CORE` / `@main` / `%main` /
//!     `%stryke` rows. Topics MUST go through `compadd -a`, never
//!     `_describe`.
//!
//!   * **Flags / topics in the same group.** Without
//!     `_alternative`, both flag and topic candidates flow into one
//!     completion list and `--help`-style flag rows interleave with
//!     bare topic names. The shim must register flags and topics as
//!     two distinct tags via `_alternative`.
//!
//! Source-only test (no zsh / shell required) — the completion file
//! is a static artifact and the regressions live in its text.

use std::fs;
use std::path::PathBuf;

fn completion_path() -> PathBuf {
    PathBuf::from("completions/_stryke")
}

fn completion_text() -> Option<String> {
    fs::read_to_string(completion_path()).ok()
}

#[test]
fn stryke_docs_topics_use_compadd_not_describe() {
    let Some(src) = completion_text() else {
        eprintln!("skip: completions/_stryke not present");
        return;
    };
    // Locate the topic-feeder helper. It must call `compadd` to add
    // the topic array and MUST NOT pass the array to `_describe`.
    let topics_fn = src
        .split("_stryke_docs_topics()")
        .nth(1)
        .expect("missing _stryke_docs_topics helper");
    // Cut off at the closing brace of the helper so we don't bleed
    // into other helpers.
    let body_end = topics_fn.find("\n}").expect("helper has closing brace");
    let body = &topics_fn[..body_end];
    assert!(
        body.contains("compadd"),
        "_stryke_docs_topics should call `compadd` to feed topics",
    );
    assert!(
        !body.contains("_describe"),
        "_stryke_docs_topics must NOT pass topics through `_describe` — \
         colons in `CORE::name` / `%main::X` get parsed as the \
         name/description separator and collapse the menu",
    );
}

#[test]
fn stryke_docs_uses_alternative_to_separate_flags_and_topics() {
    let Some(src) = completion_text() else {
        eprintln!("skip: completions/_stryke not present");
        return;
    };
    // The dispatcher (`_stryke_docs`) must wire flags and topics
    // through `_alternative` so each lands in its own tagged section.
    let docs_fn_idx = src.find("_stryke_docs()").expect("missing _stryke_docs");
    let after = &src[docs_fn_idx..];
    let body_end = after.find("\n}").expect("dispatcher has closing brace");
    let body = &after[..body_end];
    assert!(
        body.contains("_alternative"),
        "_stryke_docs should call `_alternative` so flags + topics \
         show up as separate completion sections",
    );
    // Both helper hooks must be referenced from the dispatcher.
    assert!(
        body.contains("_stryke_docs_flags"),
        "_stryke_docs should reference the flags helper",
    );
    assert!(
        body.contains("_stryke_docs_topics"),
        "_stryke_docs should reference the topics helper",
    );
}

#[test]
fn stryke_docs_completion_invokes_list_all() {
    // Topics come from `s docs --list-all` — the only command that
    // exposes every callable spelling (primaries + aliases + qualified
    // forms + sigil-prefixed reflection hashes + special vars).
    // Falling back to `--list` would silently drop alias completion.
    let Some(src) = completion_text() else {
        eprintln!("skip: completions/_stryke not present");
        return;
    };
    assert!(
        src.contains("docs --list-all"),
        "completion must drive topics from `s docs --list-all`",
    );
}
