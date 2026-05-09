//! Tests for `%stryke::keywords` (`%k`) and the `%all = %a + %b + %k`
//! invariant. Companion to `builtins.rs::KEYWORDS` /
//! `keywords_hash_map()` / `is_stryke_keyword()`.

use crate::builtins::{
    aliases_hash_map, all_hash_map, builtins_hash_map, is_stryke_keyword, keywords_hash_map,
    KEYWORDS,
};
use crate::run;

fn rs(s: &str) -> String {
    run(s).expect("run").to_string()
}

// ── KEYWORDS const sanity ──────────────────────────────────────────────

#[test]
fn keywords_const_is_sorted() {
    // Required for binary_search in is_stryke_keyword.
    let names: Vec<&str> = KEYWORDS.iter().map(|(n, _)| *n).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "KEYWORDS must be alphabetically sorted");
}

#[test]
fn keywords_const_has_no_duplicates() {
    let mut names: Vec<&str> = KEYWORDS.iter().map(|(n, _)| *n).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "KEYWORDS must have no duplicate names");
}

#[test]
fn keywords_const_uses_known_categories() {
    const VALID: &[&str] = &[
        "aop",
        "concurrency",
        "control",
        "decl",
        "exception",
        "operator",
        "oo",
        "phase",
        "quote",
        "special",
        "visibility",
    ];
    for (name, cat) in KEYWORDS {
        assert!(
            VALID.contains(cat),
            "keyword {name:?} has unknown category {cat:?} — extend the VALID list \
             in keywords_hash_tests if intentional",
        );
    }
}

// ── is_stryke_keyword ──────────────────────────────────────────────────

#[test]
fn is_stryke_keyword_true_for_canonical_keywords() {
    for kw in ["if", "while", "my", "sub", "fn", "class", "BEGIN", "and"] {
        assert!(is_stryke_keyword(kw), "{kw} should be a keyword");
    }
}

#[test]
fn is_stryke_keyword_false_for_callable_builtins() {
    // These are dispatch primaries, not keywords.
    for name in ["map", "grep", "print", "pmap", "to_json", "len"] {
        assert!(!is_stryke_keyword(name), "{name} is a builtin, not a keyword");
    }
}

#[test]
fn is_stryke_keyword_false_for_unknown_names() {
    for name in ["nonexistent_xyz", "", "MY", "If"] {
        assert!(!is_stryke_keyword(name), "{name:?} should not be a keyword");
    }
}

// ── keywords_hash_map ─────────────────────────────────────────────────

#[test]
fn keywords_map_size_matches_const() {
    assert_eq!(keywords_hash_map().len(), KEYWORDS.len());
}

#[test]
fn keywords_map_categories_round_trip() {
    let m = keywords_hash_map();
    for (name, cat) in KEYWORDS {
        let v = m.get(*name).expect("keyword present");
        assert_eq!(v.to_string(), *cat, "category for {name}");
    }
}

// ── disjointness: %b vs %k ────────────────────────────────────────────

#[test]
fn builtins_and_keywords_are_disjoint() {
    let b = builtins_hash_map();
    let k = keywords_hash_map();
    let collisions: Vec<&String> = b.keys().filter(|n| k.contains_key(*n)).collect();
    assert!(
        collisions.is_empty(),
        "%b and %k must be disjoint, but these names appear in both: {collisions:?}",
    );
}

#[test]
fn aliases_and_keywords_are_disjoint() {
    let a = aliases_hash_map();
    let k = keywords_hash_map();
    let collisions: Vec<&String> = a.keys().filter(|n| k.contains_key(*n)).collect();
    assert!(
        collisions.is_empty(),
        "%a and %k must be disjoint, but these names appear in both: {collisions:?}",
    );
}

// ── %all = %a + %b + %k invariant ─────────────────────────────────────

#[test]
fn all_contains_every_keyword() {
    let all = all_hash_map();
    for (name, _) in KEYWORDS {
        assert!(
            all.contains_key(*name),
            "%all is missing keyword {name:?}",
        );
    }
}

#[test]
fn all_contains_every_builtin_primary() {
    let all = all_hash_map();
    let b = builtins_hash_map();
    for name in b.keys() {
        assert!(all.contains_key(name), "%all is missing builtin {name:?}");
    }
}

#[test]
fn all_contains_every_alias() {
    let all = all_hash_map();
    let a = aliases_hash_map();
    for name in a.keys() {
        assert!(all.contains_key(name), "%all is missing alias {name:?}");
    }
}

#[test]
fn all_size_equals_sum_of_disjoint_parts() {
    // |%all| == |%a| + |%b| + |%k| only holds because the three are disjoint
    // (the dedicated disjointness tests above confirm that). If any of those
    // tests fail, this one will too — telling us the union math drifted.
    let a = aliases_hash_map();
    let b = builtins_hash_map();
    let k = keywords_hash_map();
    let all = all_hash_map();
    assert_eq!(
        all.len(),
        a.len() + b.len() + k.len(),
        "%all should be the disjoint union of %a + %b + %k",
    );
}

// ── runtime: short alias %k is wired and frozen ────────────────────────

#[test]
fn k_short_alias_lookup() {
    assert_eq!(rs("$k{if}"), "control");
    assert_eq!(rs("$k{class}"), "decl");
    assert_eq!(rs("$k{async}"), "concurrency");
    assert_eq!(rs("$k{eval}"), "exception");
    assert_eq!(rs("$k{BEGIN}"), "phase");
    assert_eq!(rs("$k{and}"), "operator");
    assert_eq!(rs("$k{pub}"), "visibility");
    assert_eq!(rs("$k{extends}"), "oo");
}

#[test]
fn k_long_qualified_lookup() {
    assert_eq!(rs("$stryke::keywords{my}"), "decl");
    assert_eq!(rs("$stryke::keywords{return}"), "control");
}

#[test]
fn k_does_not_contain_builtins() {
    // map/grep/print are callable primaries, never keywords.
    assert_eq!(rs("exists $k{map} ? 1 : 0"), "0");
    assert_eq!(rs("exists $k{grep} ? 1 : 0"), "0");
    assert_eq!(rs("exists $k{print} ? 1 : 0"), "0");
}

#[test]
fn b_does_not_contain_keywords() {
    // The user invariant: %b is callable-only, no syntactic keywords leak in.
    assert_eq!(rs("exists $b{if} ? 1 : 0"), "0");
    assert_eq!(rs("exists $b{my} ? 1 : 0"), "0");
    assert_eq!(rs("exists $b{class} ? 1 : 0"), "0");
    assert_eq!(rs("exists $b{while} ? 1 : 0"), "0");
}

#[test]
fn all_resolves_keywords_and_aliases() {
    // Keyword: %all tag matches the %k category exactly.
    assert_eq!(rs("$all{while}"), "control");
    // Alias: %all tag matches the primary's %b category — transitive
    // resolution without going through %a. Whatever category the primary
    // carries, the alias inherits it.
    let primary_cat = rs("$b{to_json}");
    let alias_cat = rs("$all{tj}");
    assert_eq!(alias_cat, primary_cat);
    assert!(!alias_cat.is_empty(), "$all{{tj}} should not be empty");
}

#[test]
fn k_keys_count_matches_const_len() {
    let n = KEYWORDS.len();
    assert_eq!(rs("scalar keys %k"), n.to_string());
}
