//! `lsp_words` is the source of truth for LSP / REPL / `s docs
//! --list-all` completion. Two pinned invariants:
//!
//!   * **Internal compiler scratch is filtered.** The bytecode
//!     compiler interns names like `__pf_foreach_list__`,
//!     `__pf_foreach_i__`, `__foreach_list__`, `__foreach_i__`,
//!     `__list_assign_tmp__`, `__list_assign_swap__` whenever it
//!     emits postfix-foreach / list-assign bytecode. These leak into
//!     `%parameters` via `refresh_parameters_hash` and would surface
//!     as "completable identifiers" without an explicit
//!     `is_internal_scratch_name` reject — flooding the menu with
//!     `@__pf_foreach_list__` etc.
//!
//!   * **Perl special variables are present.** `$_`, `$/`, `$0`,
//!     `$$`, `$!`, `$@`, `$|`, `$^O`, `$^V`, `$^X`, the regex
//!     captures `$1`..`$9`, etc — these may not be in scope when
//!     `lsp_words` runs (no `<>` read yet, no eval error pending), so
//!     the scope walk misses them. Hard-coded `SPECIAL_VARS` list
//!     guarantees they always show up on `$<TAB>`.
//!
//! Perl-defined `__NAME__` tokens (`__FILE__`, `__LINE__`,
//! `__PACKAGE__`, `__SUB__`, `__DATA__`, `__END__`) stay because
//! they're user-facing.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

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

fn lsp_words() -> Option<HashSet<String>> {
    let bin = stryke_binary()?;
    let out = Command::new(&bin)
        .args(["-e", r#"for (lsp_words()) { print "$_\n" }"#])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect(),
    )
}

#[test]
fn internal_compiler_scratch_is_not_in_lsp_words() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    // Each scratch name is interned both bare and with sigils as it
    // gets written/read by the compiler — verify all sigil-shapes
    // are absent.
    let scratches = [
        "__pf_foreach_list__",
        "__pf_foreach_i__",
        "__foreach_list__",
        "__foreach_i__",
        "__list_assign_tmp__",
        "__list_assign_swap__",
    ];
    for name in scratches {
        for prefix in ["", "$", "@", "%", "&"] {
            let q = format!("{prefix}{name}");
            assert!(
                !words.contains(&q),
                "lsp_words leaked compiler scratch name {q:?}",
            );
        }
    }
}

#[test]
fn perl_topic_and_match_specials_are_present() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for v in &["$_", "$&", "$'", "$+"] {
        assert!(
            words.contains(*v),
            "special var {v:?} missing from lsp_words",
        );
    }
}

#[test]
fn perl_caret_specials_are_present() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    // The most-used caret vars; if these slip out, half the
    // platform-detection idioms in user scripts break completion.
    for v in &["$^O", "$^V", "$^X", "$^W"] {
        assert!(
            words.contains(*v),
            "caret special var {v:?} missing from lsp_words",
        );
    }
}

#[test]
fn perl_separator_and_process_specials_are_present() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for v in &["$/", "$\\", "$,", "$;", "$0", "$$", "$!", "$@", "$|", "$."] {
        assert!(
            words.contains(*v),
            "process/separator special {v:?} missing from lsp_words",
        );
    }
}

#[test]
fn regex_capture_scalars_are_present() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for n in 1..=9 {
        let v = format!("${n}");
        assert!(
            words.contains(&v),
            "regex capture {v:?} missing from lsp_words",
        );
    }
}

#[test]
fn special_arrays_and_hashes_are_present() {
    let Some(words) = lsp_words() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for v in &["@_", "@+", "@-", "@F", "@ARGV", "@INC", "%ENV", "%SIG"] {
        assert!(
            words.contains(*v),
            "special array/hash {v:?} missing from lsp_words",
        );
    }
}

#[test]
fn docs_list_all_inherits_special_vars() {
    // `s docs --list-all` is the shell-completion driver; whatever
    // `lsp_words` emits has to flow through into `--list-all`. If
    // this divergence appears, completion is dead.
    let Some(bin) = stryke_binary() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let out = Command::new(&bin)
        .args(["docs", "--list-all"])
        .output()
        .expect("run docs --list-all");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // `--list-all` formats as ` NN. name`. Strip the prefix and
    // check membership.
    let names: HashSet<String> = stdout
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let dot = t.find(". ")?;
            Some(t[dot + 2..].trim().to_string())
        })
        .collect();
    for v in &["$_", "$^V", "$/", "$0", "$!", "@INC", "%ENV"] {
        assert!(
            names.contains(*v),
            "docs --list-all missing special var {v:?}",
        );
    }
    // And no compiler scratch leaks here either.
    for v in &[
        "__pf_foreach_list__",
        "@__pf_foreach_list__",
        "$__list_assign_tmp__",
    ] {
        assert!(
            !names.contains(*v),
            "docs --list-all leaks compiler scratch {v:?}",
        );
    }
}
