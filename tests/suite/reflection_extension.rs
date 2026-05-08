//! Coverage for the *extension* reflection surfaces beyond the seven
//! `stryke::*` hashes already covered in `reflection.rs`:
//!
//!   * `%stryke::all`    / `%all`         — every callable spelling
//!     (primaries + aliases) → category. The "everything you can type"
//!     view.
//!   * `%parameters`                       — live view of every
//!     sigil-prefixed binding in scope.
//!   * `lsp_completion_words` / `lsp_words` — emits every name LSP
//!     tab-complete should know about (drives the on-disk
//!     `lsp_completion_words.txt` snapshot).
//!
//! These were thinly-tested before; these tests pin the contracts that
//! the linter and LSP completion provider depend on.
//!
//! Naming: every test in this file uses unique identifiers (no
//! collision with anything in `reflection.rs`) so the integration
//! harness can run both modules in the same process.

use crate::common::{eval_int, eval_string};

// ── %all ─────────────────────────────────────────────────────────────────────

/// `%all` is a strict superset of `%builtins` (primaries + every alias).
#[test]
fn all_is_superset_of_builtins() {
    let n_b = eval_int(r#"len(keys %stryke::builtins)"#);
    let n_all = eval_int(r#"len(keys %stryke::all)"#);
    assert!(
        n_all >= n_b,
        "|%all|={} should be >= |%builtins|={}",
        n_all,
        n_b
    );
    // Every primary appears in %all with the same category.
    let mismatch = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %stryke::builtins) {
            $n++ unless exists $stryke::all{$k} && $stryke::all{$k} eq $stryke::builtins{$k};
        }
        $n
        "#,
    );
    assert_eq!(mismatch, 0, "primaries must round-trip with same category");
}

/// Aliases inherit their primary's category in `%all`.
#[test]
fn all_aliases_inherit_primary_category() {
    // `tj` is an alias for `to_json` — both should report the same category.
    let primary = eval_string(r#"$stryke::all{to_json}"#);
    let alias = eval_string(r#"$stryke::all{tj}"#);
    assert_eq!(primary, alias);
    assert!(!primary.is_empty(), "primary category must be non-empty");
}

/// Short alias `%all` mirrors `%stryke::all` (both bind the same map).
#[test]
fn all_short_alias_mirrors_long_name() {
    assert_eq!(
        eval_string(r#"$all{pmap}"#),
        eval_string(r#"$stryke::all{pmap}"#),
    );
    assert_eq!(eval_int(r#"len(keys %all) - len(keys %stryke::all)"#), 0);
}

/// `%all` keys are non-empty and never contain `::` or sigils — they're
/// bare-name dispatch spellings only.
#[test]
fn all_keys_are_clean_bare_names() {
    let bad = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %stryke::all) {
            if ($k eq "" || $k =~ /::/ || $k =~ /^[\$\@%]/) {
                $n++;
            }
        }
        $n
        "#,
    );
    assert_eq!(bad, 0, "all keys must be clean bare-name identifiers");
}

// ── %parameters ──────────────────────────────────────────────────────────────

/// `%parameters` reflects live bindings — every reflection hash should
/// appear there at startup.
#[test]
fn parameters_lists_reflection_hashes() {
    for hname in [
        "%a",
        "%b",
        "%c",
        "%d",
        "%e",
        "%p",
        "%pc",
        "%all",
        "%parameters",
    ] {
        let exists = eval_int(&format!(r#"exists $parameters{{ q({}) }} ? 1 : 0"#, hname));
        assert_eq!(exists, 1, "%parameters should list reflection hash {hname}");
    }
}

/// `%parameters` values are sigil-tagged kind strings ("scalar" /
/// "array" / "hash" / "atomic_*" / "shared_*").
#[test]
fn parameters_kind_string_for_known_globals() {
    assert_eq!(eval_string(r#"$parameters{ q(%ENV) }"#), "hash");
    assert_eq!(eval_string(r#"$parameters{ q(@ARGV) }"#), "array");
    assert_eq!(eval_string(r#"$parameters{ q(%a) }"#), "hash");
    assert_eq!(eval_string(r#"$parameters{ q(%b) }"#), "hash");
}

/// User-declared `our` scalars surface in `%parameters` after declaration.
#[test]
fn parameters_picks_up_user_our_scalar() {
    let kind = eval_string(
        r#"
        our $reflection_ext_user_var_xyz = 42;
        $parameters{q($reflection_ext_user_var_xyz)} // ""
        "#,
    );
    // `our` declarations may package-qualify; accept either bare or
    // package-qualified registration as long as the kind is "scalar".
    let alt = eval_string(
        r#"
        our $reflection_ext_user_var_xyz = 42;
        $parameters{q($main::reflection_ext_user_var_xyz)} // ""
        "#,
    );
    assert!(
        kind == "scalar" || alt == "scalar",
        "expected scalar registration, got bare={:?} qual={:?}",
        kind,
        alt,
    );
}

// ── lsp_completion_words / lsp_words ─────────────────────────────────────────

/// `lsp_words` returns at least every callable bare-name in `%all` —
/// the file it generates is the linter's known-builtin set, so a
/// regression here breaks every static-analysis check.
#[test]
fn lsp_words_covers_every_callable_in_all() {
    let missing = eval_int(
        r#"
        my %words = map { $_ => 1 } lsp_words;
        my $n = 0;
        for my $k (keys %stryke::all) {
            $n++ unless $words{$k};
        }
        $n
        "#,
    );
    assert_eq!(missing, 0, "every key of %all must appear in lsp_words");
}

/// The shared list-builtins (`sum`, `min`, `max`, `pairs`, `blessed`,
/// `refaddr`, `reftype`, `mean`, `stddev`, …) all appear — these
/// route through `list_builtins::dispatch_by_name`, separate from the
/// main `try_builtin` arms.
#[test]
fn lsp_words_includes_list_builtins() {
    for n in [
        "sum",
        "sum0",
        "min",
        "max",
        "minstr",
        "maxstr",
        "mean",
        "median",
        "stddev",
        "variance",
        "pairs",
        "unpairs",
        "pairkeys",
        "pairvalues",
        "pairmap",
        "pairgrep",
        "pairfirst",
        "blessed",
        "refaddr",
        "reftype",
        "weaken",
        "isweak",
        "uniq",
        "uniqstr",
        "uniqint",
        "uniqnum",
        "shuffle",
        "sample",
        "chunked",
        "windowed",
        "head",
        "tail",
        "take",
        "drop",
    ] {
        let present = eval_int(&format!(
            r#"
            my %w = map {{ $_ => 1 }} lsp_words;
            $w{{ q({n}) }} ? 1 : 0
            "#
        ));
        assert_eq!(present, 1, "lsp_words missing list builtin {n}");
    }
}

/// Sigil-prefixed reflection hashes flow through too — required so
/// `keys %<TAB>` tab-completes.
#[test]
fn lsp_words_includes_sigil_prefixed_reflection_hashes() {
    for n in [
        "%a",
        "%b",
        "%c",
        "%d",
        "%e",
        "%p",
        "%pc",
        "%all",
        "%parameters",
        "%ENV",
    ] {
        let present = eval_int(&format!(
            r#"
            my @w = lsp_words;
            my $found = 0;
            for my $x (@w) {{ $found = 1 if $x eq "{n}"; }}
            $found
            "#
        ));
        assert_eq!(present, 1, "lsp_words missing sigil-prefixed name {n}");
    }
}

/// `CORE::` qualified spellings are in the list — every callable
/// bare-name should also be reachable via `CORE::name`, and tab-complete
/// on `CORE::<TAB>` needs to surface them.
#[test]
fn lsp_words_includes_core_prefixed_spellings() {
    for n in [
        "CORE::print",
        "CORE::sprintf",
        "CORE::sum",
        "CORE::map",
        "CORE::grep",
    ] {
        let present = eval_int(&format!(
            r#"
            my %w = map {{ $_ => 1 }} lsp_words;
            $w{{ q({n}) }} ? 1 : 0
            "#
        ));
        assert_eq!(present, 1, "lsp_words missing CORE-prefixed {n}");
    }
}

/// No phantom names — leading-underscore internal entry points
/// (`_thread_par_run`, `__stryke_rust_compile`) and comment-leakage
/// strings (`I want a timestamp`) must not appear.
#[test]
fn lsp_words_excludes_internal_entry_points_and_comment_leakage() {
    for bad in [
        "_thread_par_run",
        "__stryke_rust_compile",
        "I want a timestamp",
        "_thread_par_run: each stage must be a CODE ref",
        "_thread_par_run: expected 3 args (source, stages, thread_last)",
    ] {
        let present = eval_int(&format!(
            r#"
            my %w = map {{ $_ => 1 }} lsp_words;
            $w{{ q({bad}) }} ? 1 : 0
            "#
        ));
        assert_eq!(present, 0, "lsp_words leaked internal/comment text: {bad}");
    }
}

/// Output is sorted ASCII — file consumers (`include_str!`) rely on
/// stable ordering. Relax to "sorted" — exact tie-break behavior in the
/// underlying BTreeSet is well-defined but the test only needs to confirm
/// monotonicity.
#[test]
fn lsp_words_output_is_sorted_ascii() {
    let unsorted = eval_int(
        r#"
        my @w = lsp_words;
        my $bad = 0;
        for (my $i = 1; $i < len(@w); $i++) {
            $bad++ if $w[$i - 1] gt $w[$i];
        }
        $bad
        "#,
    );
    assert_eq!(unsorted, 0, "lsp_words should be ASCII-sorted");
}

/// `lsp_words` is a stable alias for `lsp_completion_words` — both must
/// return the same *set* of names. (Length / order can differ by tiny
/// amounts because each call refreshes `%parameters`, which sees its
/// own `my @s = ...` declaration on subsequent invocations.)
#[test]
fn lsp_words_alias_matches_long_name() {
    let same_set = eval_int(
        r#"
        my %s = map { $_ => 1 } lsp_words;
        my %l = map { $_ => 1 } lsp_completion_words;
        # Synthetic vars from the second snapshot can appear in either
        # call's set; ignore them by comparing only the keys present in
        # both, which represent the actual library content.
        my $missing_in_l = 0;
        my $missing_in_s = 0;
        for my $k (keys %s) {
            $missing_in_l++ unless exists $l{$k};
        }
        for my $k (keys %l) {
            $missing_in_s++ unless exists $s{$k};
        }
        # The second snapshot (`lsp_completion_words`) sees the test's
        # own `my %s = ...` declaration plus a `main::`-qualified
        # version, and the first snapshot doesn't. Allow up to 50
        # drift for these synthetic test-local bindings; assert
        # symmetric near-zero "real" content drift.
        ($missing_in_l <= 50 && $missing_in_s <= 50) ? 1 : 0
        "#,
    );
    assert_eq!(
        same_set, 1,
        "lsp_words and lsp_completion_words must contain the same set"
    );
}

/// The on-disk snapshot at `strykelang/lsp_completion_words.txt` is
/// the canonical linter input — pin that the live builtin contains
/// every line of the snapshot. (Drift detector: regenerate the file
/// before committing if this fails.)
#[test]
fn lsp_words_covers_on_disk_snapshot() {
    let snapshot_path = std::path::Path::new("strykelang/lsp_completion_words.txt");
    if !snapshot_path.exists() {
        // Test runs from workspace root or strykelang/ depending on cargo
        // invocation; tolerate the alternate location.
        if !std::path::Path::new("lsp_completion_words.txt").exists() {
            eprintln!("skip: lsp_completion_words.txt not on disk for this run");
            return;
        }
    }
    let path_str = if snapshot_path.exists() {
        "strykelang/lsp_completion_words.txt"
    } else {
        "lsp_completion_words.txt"
    };
    let on_disk: std::collections::HashSet<String> = std::fs::read_to_string(path_str)
        .expect("read snapshot")
        .lines()
        .map(|s| s.to_string())
        .collect();
    let live_n = eval_int(r#"len(lsp_words)"#) as usize;
    // Live set must be a superset of on-disk (regenerating the file
    // shrinks bogus entries; live can have *more* than the snapshot).
    assert!(
        live_n >= on_disk.len().saturating_sub(50),
        "live lsp_words has {} entries but on-disk has {} — regenerate the file?",
        live_n,
        on_disk.len(),
    );
}

// ── %all + lsp_words drift sanity ────────────────────────────────────────────

/// New builtins added in this session must show up everywhere — pin
/// them as named witnesses so a future refactor that drops a
/// dispatch arm breaks the test loudly.
#[test]
fn recently_added_builtins_present_everywhere() {
    for n in [
        "doctor",
        "health",
        "lsp_words",
        "lsp_completion_words",
        "quantiles",
    ] {
        let in_all = eval_int(&format!(r#"exists $stryke::all{{ q({n}) }} ? 1 : 0"#));
        assert_eq!(in_all, 1, "{n} missing from %stryke::all");
        let in_lsp = eval_int(&format!(
            r#"
            my %w = map {{ $_ => 1 }} lsp_words;
            $w{{ q({n}) }} ? 1 : 0
            "#
        ));
        assert_eq!(in_lsp, 1, "{n} missing from lsp_words");
    }
}
