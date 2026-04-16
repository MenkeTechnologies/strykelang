//! Reflection hashes exposed under the `perlrs::` namespace. One test per
//! hash, each pinning a representative key/value pair so regressions show
//! up as a failed lookup (not a silent drift). See `build.rs` for how the
//! source-of-truth tables are extracted and `src/builtins.rs` for the
//! constructor functions.

use crate::common::{eval_int, eval_string};

/// `%perlrs::perl_compats` — Perl 5 core keyword set from `is_perl_keyword`.
/// `keys` is a core builtin; `pmap` is not (it's a perlrs extension).
#[test]
fn perl_compats_marks_core_keywords_only() {
    assert_eq!(eval_int(r#"exists $perlrs::perl_compats{keys} ? 1 : 0"#), 1,);
    assert_eq!(eval_int(r#"exists $perlrs::perl_compats{pmap} ? 1 : 0"#), 0,);
}

/// `%perlrs::extensions` — names flagged by `perlrs_extension_name` (what
/// `--compat` mode rejects). `pmap` is an extension; `keys` is not.
#[test]
fn extensions_marks_perlrs_only_names() {
    assert_eq!(eval_int(r#"exists $perlrs::extensions{pmap} ? 1 : 0"#), 1,);
    assert_eq!(eval_int(r#"exists $perlrs::extensions{keys} ? 1 : 0"#), 0,);
}

/// `%perlrs::builtins` — union of perl_compats + extensions, value tells
/// which side the name came from.
#[test]
fn builtins_tags_each_name_with_its_origin() {
    assert_eq!(eval_string(r#"$perlrs::builtins{keys}"#), "perl");
    assert_eq!(eval_string(r#"$perlrs::builtins{pmap}"#), "extension");
}

/// `%perlrs::aliases` — 2nd+ names in each `try_builtin` arm → primary.
/// `tj` is the canonical short form for `to_json`; follow that wire.
#[test]
fn aliases_resolve_short_form_to_primary() {
    assert_eq!(eval_string(r#"$perlrs::aliases{tj}"#), "to_json");
    // Not an alias — the primary name itself isn't in the alias map.
    assert_eq!(eval_int(r#"exists $perlrs::aliases{to_json} ? 1 : 0"#), 0,);
}

/// `%perlrs::callable` — every callable spelling (primary, alias, or core
/// keyword) → its canonical name. Resolver for "what will this actually
/// dispatch to".
#[test]
fn callable_resolves_aliases_and_primaries_uniformly() {
    // Alias → primary.
    assert_eq!(eval_string(r#"$perlrs::callable{tj}"#), "to_json");
    // Primary → itself.
    assert_eq!(eval_string(r#"$perlrs::callable{to_json}"#), "to_json",);
    // Unknown name — not callable.
    assert_eq!(
        eval_int(r#"exists $perlrs::callable{definitely_not_a_builtin_xyz} ? 1 : 0"#),
        0,
    );
}

/// Short aliases `%b %a %e %pc %c` mirror the long `%perlrs::*` names.
/// Safe in the hash namespace — `e` is a sub-level extension, `$a`/`$b` are
/// scalar sort specials, so none of these collide when the sigil is `%`.
/// Each probe targets a distinct hash so we notice if the wiring swaps them.
#[test]
fn short_aliases_mirror_long_names() {
    assert_eq!(eval_string(r#"$b{pmap}"#), "extension"); // %b = %perlrs::builtins
    assert_eq!(eval_string(r#"$a{tj}"#), "to_json"); // %a = %perlrs::aliases
    assert_eq!(eval_int(r#"$pc{keys}"#), 1); // %pc = %perlrs::perl_compats
    assert_eq!(eval_int(r#"$e{pmap}"#), 1); // %e = %perlrs::extensions
    assert_eq!(eval_string(r#"$c{tj}"#), "to_json"); // %c = %perlrs::callable
}

/// The whole reason to split `is_perl_keyword` into `is_perl5_core` and
/// `perlrs_extension_name`: the two categories MUST be disjoint, otherwise
/// `%builtins`'s `"perl" | "extension"` tag is ambiguous. If a name starts
/// appearing in both lists this test fails loudly.
#[test]
fn perl_compats_and_extensions_are_disjoint() {
    let overlap = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %perlrs::perl_compats) {
            $n++ if exists $perlrs::extensions{$k};
        }
        $n
        "#,
    );
    assert_eq!(
        overlap, 0,
        "%perl_compats and %extensions overlap — a name is tagged as both Perl core and perlrs extension",
    );
}

/// Catches catastrophic regressions in `build.rs` or the dispatch tables
/// (empty scan, mismatched match arm, etc.). Loose floors chosen to survive
/// normal pruning while still catching 10x drops. If counts ever genuinely
/// fall below these, bump the floors — don't paper over a real loss.
#[test]
fn reflection_hashes_have_reasonable_sizes() {
    let builtins_n = eval_int(r#"scalar keys %perlrs::builtins"#);
    let compats_n = eval_int(r#"scalar keys %perlrs::perl_compats"#);
    let extensions_n = eval_int(r#"scalar keys %perlrs::extensions"#);
    let aliases_n = eval_int(r#"scalar keys %perlrs::aliases"#);
    let callable_n = eval_int(r#"scalar keys %perlrs::callable"#);

    assert!(
        builtins_n >= 200,
        "%builtins has only {builtins_n} entries — expected ~300+; scanner or dispatch regressed",
    );
    assert!(
        compats_n >= 100,
        "%perl_compats has only {compats_n} entries — expected ~180; is_perl5_core truncated?",
    );
    assert!(
        extensions_n >= 100,
        "%extensions has only {extensions_n} entries — expected ~150+; extension scanner regressed",
    );
    assert!(
        aliases_n >= 100,
        "%aliases has only {aliases_n} entries — expected ~280+; try_builtin arm extraction regressed",
    );
    assert!(
        callable_n >= builtins_n,
        "%callable ({callable_n}) should cover >= %builtins ({builtins_n}) since it includes aliases",
    );

    // Disjointness shows up here as a size invariant too.
    assert_eq!(
        builtins_n,
        compats_n + extensions_n,
        "|%builtins| should equal |%perl_compats| + |%extensions| — disjointness violated",
    );
}

/// `%callable` is the resolver, so it must cover names that never hit
/// `try_builtin` (e.g. ExprKind-modeled core ops like `uc`, `keys`). The
/// dispatch-backed case is tested in `callable_resolves_aliases_and_primaries_uniformly`;
/// this one pins the "fall-through to self-reference" branch in
/// `callable_hash_map`.
#[test]
fn callable_covers_exprkind_backed_core_names() {
    // `uc` is modeled as `ExprKind::Uc` in the parser — no try_builtin arm.
    assert_eq!(eval_string(r#"$perlrs::callable{uc}"#), "uc");
    // `keys` — same story.
    assert_eq!(eval_string(r#"$perlrs::callable{keys}"#), "keys");
}

/// Every `try_builtin` dispatch primary is reachable as a `%builtins`
/// entry. Catches the "someone added a dispatch arm without updating the
/// parser lists" regression at test time rather than letting reflection
/// silently omit the name. Short aliases (%b) cover the same dataset.
#[test]
fn every_dispatch_primary_is_in_builtins() {
    // Use `%perlrs::aliases` values — those are dispatch primaries by
    // construction (2nd+ arm names map to the first name in that arm).
    // Every aliased primary must show up in %builtins.
    let missing = eval_int(
        r#"
        my $n = 0;
        for my $alias (keys %perlrs::aliases) {
            my $primary = $perlrs::aliases{$alias};
            $n++ unless exists $perlrs::builtins{$primary};
        }
        $n
        "#,
    );
    assert_eq!(
        missing, 0,
        "some alias' primary name isn't in %builtins — dispatch arm drift vs parser keyword lists",
    );
}
