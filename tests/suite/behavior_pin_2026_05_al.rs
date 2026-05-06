//! Behavior-pinning batch AL (2026-05-06): Set Ops, String Character Analysis, Predicates.

use crate::common::*;

// ── Set Operations ──────────────────────────────────────────────────────────

#[test]
fn set_operations_al() {
    let code = r#"
        my @u = array_union([1, 2, 5], [2, 3, 5]);
        my @i = array_intersection([1, 2, 5], [2, 3, 5]);
        my @d = array_difference([1, 2, 5], [2, 3, 5]);
        my @s = symmetric_diff([1, 2, 5], [2, 3, 5]);
        
        join(":", join(",", sort(@u)), join(",", sort(@i)), join(",", sort(@d)), join(",", sort(@s)))
    "#;
    assert_eq!(eval_string(code), "1,2,3,5:2,5:1:1,3");
}

// ── String Character Analysis ───────────────────────────────────────────────

#[test]
fn string_analysis_al() {
    assert_eq!(eval_int("is_vowel('E')"), 1);
    assert_eq!(eval_int("is_vowel('z')"), 0);
    assert_eq!(eval_int("is_consonant('Z')"), 1);
    assert_eq!(eval_int("is_consonant('e')"), 0);

    assert_eq!(eval_int("count_vowels('hEllO')"), 2);
    assert_eq!(eval_int("count_consonants('hEllO')"), 3);

    assert_eq!(
        eval_string(r#"reverse_words("hello world from rust")"#),
        "rust from world hello"
    );
    assert_eq!(eval_string(r#"rot47("`aBc`")"#), "12q41");
}

// ── Hash Selection ──────────────────────────────────────────────────────────

#[test]
fn hash_selection_al() {
    let code = r#"
        my $h = { a => 1, b => 2, c => 3, d => 4, e => 5 };
        my $p = pick_keys($h, "a", "c", "e");
        my $o = omit_keys($h, "a", "c", "e");
        join(":", join(",", sort(keys($p))), join(",", sort(keys($o))))
    "#;
    assert_eq!(eval_string(code), "a,c,e:b,d");
}

// ── More Predicates ─────────────────────────────────────────────────────────

#[test]
fn predicates_al() {
    assert_eq!(eval_int("is_pair([1, 2, 3])"), 0);
    assert_eq!(eval_int("is_pair([])"), 0);
    assert_eq!(eval_int("is_triple([1, 2, 3, 4])"), 0);

    assert_eq!(eval_int("is_empty_arr([undef])"), 0);
    assert_eq!(eval_int("is_empty_hash({a => undef})"), 0);

    assert_eq!(eval_int("is_subset([1, 2, 4], [1, 2, 3])"), 0);
    assert_eq!(eval_int("is_permutation([1, 2, 3], [3, 2, 1, 4])"), 0);
}

// ── Search ──────────────────────────────────────────────────────────────────

#[test]
fn search_al() {
    // binary_search(target, sorted_list)
    assert_eq!(eval_int("binary_search(1, 1, 2, 3, 4, 5)"), 0);
    assert_eq!(eval_int("binary_search(5, 1, 2, 3, 4, 5)"), 4);
    assert_eq!(eval_int("binary_search(0, 1, 2, 3, 4, 5)"), -1);

    // linear_search(target, list)
    assert_eq!(eval_int("linear_search(5, 5, 4, 3, 2, 1)"), 0);
    assert_eq!(eval_int("linear_search(1, 5, 4, 3, 2, 1)"), 4);
    assert_eq!(eval_int("linear_search(0, 5, 4, 3, 2, 1)"), -1);
}
