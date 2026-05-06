//! Behavior-pinning batch AC (2026-05-05): Set Ops, String Character Analysis, Predicates.

use crate::common::*;

// ── Set Operations ──────────────────────────────────────────────────────────

#[test]
fn set_operations_ac() {
    let code = r#"
        my @u = array_union([1, 2], [2, 3]);
        my @i = array_intersection([1, 2], [2, 3]);
        my @d = array_difference([1, 2], [2, 3]);
        my @s = symmetric_diff([1, 2], [2, 3]);
        
        join(":", join(",", sort(@u)), join(",", sort(@i)), join(",", sort(@d)), join(",", sort(@s)))
    "#;
    assert_eq!(eval_string(code), "1,2,3:2:1:1,3");
}

// ── String Character Analysis ───────────────────────────────────────────────

#[test]
fn string_analysis_ac() {
    assert_eq!(eval_int("is_vowel('a')"), 1);
    assert_eq!(eval_int("is_vowel('b')"), 0);
    assert_eq!(eval_int("is_consonant('b')"), 1);
    assert_eq!(eval_int("is_consonant('a')"), 0);
    
    assert_eq!(eval_int("count_vowels('hello')"), 2);
    assert_eq!(eval_int("count_consonants('hello')"), 3);
    
    assert_eq!(eval_string(r#"reverse_words("hello world")"#), "world hello");
    assert_eq!(eval_string(r#"rot47("abc")"#), "234");
}

// ── Hash Selection ──────────────────────────────────────────────────────────

#[test]
fn hash_selection_ac() {
    let code = r#"
        my $h = { a => 1, b => 2, c => 3 };
        my $p = pick_keys($h, "a", "c");
        my $o = omit_keys($h, "a", "c");
        join(":", join(",", sort(keys($p))), join(",", sort(keys($o))))
    "#;
    assert_eq!(eval_string(code), "a,c:b");
}

// ── More Predicates ─────────────────────────────────────────────────────────

#[test]
fn predicates_ac() {
    assert_eq!(eval_int("is_pair([1, 2])"), 1);
    assert_eq!(eval_int("is_pair([1])"), 0);
    assert_eq!(eval_int("is_triple([1, 2, 3])"), 1);
    
    assert_eq!(eval_int("is_empty_arr([])"), 1);
    assert_eq!(eval_int("is_empty_arr([1])"), 0);
    assert_eq!(eval_int("is_empty_hash({})"), 1);
    
    assert_eq!(eval_int("is_subset([1, 2], [1, 2, 3])"), 1);
    assert_eq!(eval_int("is_permutation([1, 2, 3], [3, 2, 1])"), 1);
}

// ── Search ──────────────────────────────────────────────────────────────────

#[test]
fn search_ac() {
    // binary_search(target, sorted_list)
    assert_eq!(eval_int("binary_search(3, 1, 2, 3, 4, 5)"), 2);
    assert_eq!(eval_int("binary_search(6, 1, 2, 3, 4, 5)"), -1);
    
    // linear_search(target, list)
    assert_eq!(eval_int("linear_search(3, 5, 4, 3, 2, 1)"), 2);
}
