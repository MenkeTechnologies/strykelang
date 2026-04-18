//! Reflection hashes exposed under the `perlrs::` namespace plus short
//! one-char aliases. Seven hashes; every direct lookup is O(1).
//!
//!   %b  / %perlrs::builtins      — name → category
//!   %pc / %perlrs::perl_compats  — subset: Perl 5 core only
//!   %e  / %perlrs::extensions    — subset: perlrs-only
//!   %a  / %perlrs::aliases       — alias → primary
//!   %d  / %perlrs::descriptions  — name → LSP one-liner (sparse)
//!   %c  / %perlrs::categories    — category → arrayref of names
//!   %p  / %perlrs::primaries     — primary → arrayref of aliases

use crate::common::{eval_int, eval_string};

/// `%builtins` values are category strings, not set placeholders.
#[test]
fn builtins_values_are_category_strings() {
    assert_eq!(eval_string(r#"$perlrs::builtins{pmap}"#), "parallel");
    assert_eq!(eval_string(r#"$perlrs::builtins{map}"#), "array / list");
    assert_eq!(eval_string(r#"$perlrs::builtins{uc}"#), "string");

    let empty = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %perlrs::builtins) {
            $n++ if $perlrs::builtins{$k} eq "";
        }
        $n
        "#,
    );
    assert_eq!(
        empty, 0,
        "every %builtins value should be a non-empty category"
    );
}

/// `%perl_compats` holds only Perl 5 core; `%extensions` only perlrs-only.
/// Together they partition `%builtins` exactly.
#[test]
fn perl_compats_and_extensions_partition_builtins() {
    let n_b = eval_int(r#"scalar keys %perlrs::builtins"#);
    let n_pc = eval_int(r#"scalar keys %perlrs::perl_compats"#);
    let n_e = eval_int(r#"scalar keys %perlrs::extensions"#);
    assert_eq!(
        n_b,
        n_pc + n_e,
        "|%builtins|={n_b} but |%perl_compats|+|%extensions|={pc}+{e}={sum} — disjointness broken",
        n_b = n_b,
        pc = n_pc,
        e = n_e,
        sum = n_pc + n_e,
    );

    // Sample membership.
    assert_eq!(
        eval_int(r#"exists $perlrs::perl_compats{keys} ? 1 : 0"#),
        1,
        "keys must be in %perl_compats",
    );
    assert_eq!(
        eval_int(r#"exists $perlrs::extensions{pmap} ? 1 : 0"#),
        1,
        "pmap must be in %extensions",
    );
    // Disjointness on a known name.
    assert_eq!(
        eval_int(r#"exists $perlrs::extensions{keys} ? 1 : 0"#),
        0,
        "keys is core, must not be in %extensions",
    );
}

/// Category values must be the same in `%builtins` and its source subset,
/// whichever side it came from.
#[test]
fn subset_values_match_builtins() {
    assert_eq!(
        eval_string(r#"$perlrs::perl_compats{map}"#),
        eval_string(r#"$perlrs::builtins{map}"#),
    );
    assert_eq!(
        eval_string(r#"$perlrs::extensions{pmap}"#),
        eval_string(r#"$perlrs::builtins{pmap}"#),
    );
}

/// `%aliases` — 2nd+ arm names → primary. Every alias' primary is a real
/// `%builtins` entry (no dangling targets).
#[test]
fn aliases_resolve_short_form_to_primary() {
    assert_eq!(eval_string(r#"$perlrs::aliases{tj}"#), "to_json");
    assert_eq!(eval_int(r#"exists $perlrs::aliases{to_json} ? 1 : 0"#), 0);

    let dangling = eval_int(
        r#"
        my $n = 0;
        for my $alias (keys %perlrs::aliases) {
            my $primary = $perlrs::aliases{$alias};
            $n++ unless exists $perlrs::builtins{$primary};
        }
        $n
        "#,
    );
    assert_eq!(dangling, 0);
}

/// `%descriptions` is sparse — only documented names.
#[test]
fn descriptions_cover_documented_names() {
    let d = eval_string(r#"$perlrs::descriptions{pmap}"#);
    assert!(
        d.len() > 10,
        "%d{{pmap}} should be real sentence, got {:?}",
        d
    );
    assert_eq!(
        eval_int(r#"exists $perlrs::descriptions{definitely_not_a_builtin_xyz} ? 1 : 0"#),
        0,
    );
    let n_desc = eval_int(r#"scalar keys %perlrs::descriptions"#);
    let n_all = eval_int(r#"scalar keys %perlrs::all"#);
    assert!(
        n_desc > 0 && n_desc <= n_all,
        "%descriptions ({n_desc}) should be between 1 and |%all| ({n_all}) — \
         it includes both primaries and aliases when the LSP arm is shared",
    );
}

/// `%categories` is the inverted `%builtins`: category → arrayref of names.
/// O(1) reverse-query without scanning.
#[test]
fn categories_inverted_index_returns_name_arrayrefs() {
    // Expected category tags from the section comments.
    let n_parallel = eval_int(r#"scalar @{ $perlrs::categories{parallel} }"#);
    assert!(
        n_parallel >= 20,
        "expected ≥20 parallel ops, got {n_parallel}",
    );
    // The contents of `$c{string}` should match every `%builtins` entry
    // whose value is "string".
    let mismatch = eval_int(
        r#"
        my %from_c = map { $_ => 1 } @{ $perlrs::categories{"string"} };
        my @from_b = grep { $perlrs::builtins{$_} eq "string" } keys %perlrs::builtins;
        my $n = 0;
        for my $k (@from_b) { $n++ unless $from_c{$k}; }
        $n += scalar(keys %from_c) - scalar(@from_b);
        $n
        "#,
    );
    assert_eq!(
        mismatch, 0,
        "%categories[string] should match grep {{ $b{{_}} eq 'string' }} keys %b",
    );
}

/// `%primaries` is the inverted `%aliases`: primary → arrayref of its
/// aliases. Primaries with no aliases still appear (empty arrayref) so
/// `exists $p{foo}` reliably means "is foo a dispatch primary".
#[test]
fn primaries_inverted_index_returns_alias_arrayrefs() {
    // `to_json` has `tj` as an alias.
    let tj_in = eval_int(
        r#"
        my $aliases = $perlrs::primaries{to_json}
        my $found = 0
        for my $a (@$aliases) { $found = 1 if $a eq "tj"; }
        $found
        "#,
    );
    assert_eq!(tj_in, 1, "to_json's aliases should include 'tj'");

    // `basename` has `bn` as an alias.
    let bn_in = eval_int(
        r#"
        my $aliases = $perlrs::primaries{basename}
        my $found = 0
        for my $a (@$aliases) { $found = 1 if $a eq "bn"; }
        $found
        "#,
    );
    assert_eq!(bn_in, 1);

    // Every primary in %p is a real builtin (value ne "uncategorized" is
    // not required — primaries can be any dispatch first-name).
    let dangling = eval_int(
        r#"
        my $n = 0
        for my $primary (keys %perlrs::primaries) {
            $n++ unless exists $perlrs::builtins{$primary};
        }
        $n
        "#,
    );
    assert_eq!(
        dangling, 0,
        "every %primaries key should be a known builtin"
    );
}

/// Short aliases mirror long names. Seven one-char hashes: b, pc, e, a, d, c, p.
#[test]
fn short_aliases_mirror_long_names() {
    assert_eq!(eval_string(r#"$b{pmap}"#), "parallel");
    assert_eq!(eval_string(r#"$pc{map}"#), "array / list");
    assert_eq!(eval_string(r#"$e{pmap}"#), "parallel");
    assert_eq!(eval_string(r#"$a{tj}"#), "to_json");
    // %d and %c/%p use arrayref values — spot-check non-empty.
    assert!(eval_int(r#"length($d{pmap}) > 0 ? 1 : 0"#) == 1);
    assert!(eval_int(r#"scalar @{ $c{parallel} } > 0 ? 1 : 0"#) == 1);
    assert!(eval_int(r#"scalar @{ $p{to_json} } > 0 ? 1 : 0"#) == 1);
}

/// Every `try_builtin` dispatch primary must land in either `is_perl5_core`
/// or `perlrs_extension_name` — otherwise `--compat` mode silently accepts
/// it (bypasses the `perlrs_extension_name` gate) and `%builtins` tags it
/// `"uncategorized"` instead of a real category.
///
/// On failure, the message lists every offender so the fix is mechanical:
/// add each name to the appropriate `// ── category ──` section in
/// `src/parser.rs`. Rebuild the test to confirm.
#[test]
fn every_dispatch_primary_is_categorized() {
    let out = eval_string(
        r#"
        my @bad
        for my $name (sort keys %perlrs::builtins) {
            push @bad, $name if $perlrs::builtins{$name} eq "uncategorized"
        }
        join ",", @bad
        "#,
    );
    assert!(
        out.is_empty(),
        "uncategorized dispatch primaries — add each to a `// ── category ──`\n\
         section in parser.rs (is_perl5_core or perlrs_extension_name):\n    {out}",
    );
}

/// Catastrophic-regression floors on each hash.
#[test]
fn reflection_hashes_have_reasonable_sizes() {
    assert!(eval_int(r#"scalar keys %perlrs::builtins"#) >= 200);
    assert!(eval_int(r#"scalar keys %perlrs::perl_compats"#) >= 80);
    assert!(eval_int(r#"scalar keys %perlrs::extensions"#) >= 100);
    assert!(eval_int(r#"scalar keys %perlrs::aliases"#) >= 100);
    assert!(eval_int(r#"scalar keys %perlrs::descriptions"#) >= 10);
    assert!(eval_int(r#"scalar keys %perlrs::categories"#) >= 10);
    assert!(eval_int(r#"scalar keys %perlrs::primaries"#) >= 100);
}
