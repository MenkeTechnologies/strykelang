//! Reflection hashes exposed under the `perlrs::` namespace. See `build.rs`
//! for how the source-of-truth tables are extracted and `src/builtins.rs`
//! for the constructor functions. The API is three hashes:
//!
//!   - `%perlrs::builtins`     (`%b`) — name → category string
//!   - `%perlrs::aliases`      (`%a`) — alias → primary
//!   - `%perlrs::descriptions` (`%d`) — name → one-line LSP summary (sparse)

use crate::common::{eval_int, eval_string};

/// `%builtins` values are category strings, not set placeholders. Parallel
/// ops should tag as `"parallel"`; core Perl list ops as `"array"`; etc.
/// If categorization drifts, these specific probes catch it.
#[test]
fn builtins_values_are_category_strings() {
    // perlrs extensions — from `// ── parallel ──` comment block.
    assert_eq!(eval_string(r#"$perlrs::builtins{pmap}"#), "parallel");
    // Perl core — from `// ── array / list ──` comment block.
    assert_eq!(eval_string(r#"$perlrs::builtins{map}"#), "array / list");
    // Perl core — string category.
    assert_eq!(eval_string(r#"$perlrs::builtins{uc}"#), "string");
    // Dispatch primary that isn't in either categorized list falls through to
    // "uncategorized" rather than disappearing — catches drift in the
    // `is_perl5_core` / `perlrs_extension_name` section headers.
    assert_eq!(
        eval_int(r#"exists $perlrs::builtins{pmap} ? 1 : 0"#),
        1,
        "pmap must be in %builtins",
    );
    assert_eq!(
        eval_int(r#"exists $perlrs::builtins{map} ? 1 : 0"#),
        1,
        "map must be in %builtins",
    );
    // No empty-string values anywhere.
    let empty = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %perlrs::builtins) {
            $n++ if $perlrs::builtins{$k} eq "";
        }
        $n
        "#,
    );
    assert_eq!(empty, 0, "every %builtins value should be a non-empty category");
}

/// Category strings let you `grep` for kind — the main unlock vs. the old
/// `"perl" | "extension"` tag. If no parallel ops show up under the query
/// something is seriously wrong with category extraction.
#[test]
fn builtins_category_grep_surfaces_known_ops() {
    let parallel_count = eval_int(
        r#"
        my $n = 0;
        for my $k (keys %perlrs::builtins) {
            $n++ if $perlrs::builtins{$k} eq "parallel";
        }
        $n
        "#,
    );
    assert!(
        parallel_count >= 20,
        "expected >= 20 parallel ops, got {parallel_count} — section-comment parsing regressed?",
    );
}

/// `%aliases` — 2nd+ names in each `try_builtin` arm → primary. Unchanged
/// from the previous reflection surface, still useful.
#[test]
fn aliases_resolve_short_form_to_primary() {
    assert_eq!(eval_string(r#"$perlrs::aliases{tj}"#), "to_json");
    // The primary itself isn't in the alias map.
    assert_eq!(eval_int(r#"exists $perlrs::aliases{to_json} ? 1 : 0"#), 0);
    // Every alias value is a real %builtins entry — no dangling primaries.
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
    assert_eq!(dangling, 0, "alias → primary pointer must hit %builtins");
}

/// `%descriptions` is sparse — only names with LSP hover docs. It should
/// cover the heavily-documented ops (`pmap`, `to_json`, `fan`, …) but
/// there's no requirement that every builtin has a description.
#[test]
fn descriptions_cover_documented_names() {
    // `pmap` has a rich LSP doc → description should start with a
    // meaningful sentence.
    let d = eval_string(r#"$perlrs::descriptions{pmap}"#);
    assert!(
        d.len() > 10,
        "%descriptions{{pmap}} should be a real sentence, got {:?}",
        d,
    );
    // A random non-existent name returns empty string (or undef stringified).
    assert_eq!(
        eval_int(r#"exists $perlrs::descriptions{definitely_not_a_builtin_xyz} ? 1 : 0"#),
        0,
    );
    // Description set is strictly smaller than builtins — it's sparse, not
    // one-per-name.
    let n_desc = eval_int(r#"scalar keys %perlrs::descriptions"#);
    let n_built = eval_int(r#"scalar keys %perlrs::builtins"#);
    assert!(
        n_desc > 0 && n_desc <= n_built,
        "descriptions ({n_desc}) must be nonempty and <= builtins ({n_built})",
    );
}

/// Short aliases mirror the long names. Safe in the hash sigil namespace
/// (no collision with `$a`/`$b` sort specials or the `e` extension sub).
#[test]
fn short_aliases_mirror_long_names() {
    assert_eq!(eval_string(r#"$b{pmap}"#), "parallel"); // %b = %perlrs::builtins
    assert_eq!(eval_string(r#"$a{tj}"#), "to_json"); // %a = %perlrs::aliases
    // %d is sparse — just check the wiring.
    let same = eval_int(
        r#"
        my $a_long = $perlrs::descriptions{pmap};
        my $a_short = $d{pmap};
        $a_long eq $a_short ? 1 : 0
        "#,
    );
    assert_eq!(same, 1, "%d should mirror %perlrs::descriptions");
}

/// Loose floors on each hash. Catches catastrophic regressions (empty scan,
/// busted match-arm extraction) while leaving room for normal pruning.
#[test]
fn reflection_hashes_have_reasonable_sizes() {
    let builtins_n = eval_int(r#"scalar keys %perlrs::builtins"#);
    let aliases_n = eval_int(r#"scalar keys %perlrs::aliases"#);
    let desc_n = eval_int(r#"scalar keys %perlrs::descriptions"#);

    assert!(
        builtins_n >= 200,
        "%builtins has only {builtins_n} entries — expected ~300+",
    );
    assert!(
        aliases_n >= 100,
        "%aliases has only {aliases_n} entries — expected ~280+",
    );
    assert!(
        desc_n >= 10,
        "%descriptions has only {desc_n} entries — LSP doc extraction regressed?",
    );
    assert!(
        desc_n < builtins_n,
        "%descriptions ({desc_n}) should be sparse, strictly smaller than %builtins ({builtins_n})",
    );
}
