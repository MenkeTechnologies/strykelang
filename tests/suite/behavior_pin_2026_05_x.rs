//! Behavior-pinning batch X (2026-05-05): Collection Helpers, List Ops, More String.

use crate::common::*;

// ── Collection Helpers ───────────────────────────────────────────────────────

#[test]
fn collection_uniqueness() {
    assert_eq!(eval_int("all_unique(1, 2, 3)"), 1);
    assert_eq!(eval_int("all_unique(1, 2, 1)"), 0);
    assert_eq!(eval_int("has_duplicates(1, 2, 1)"), 1);
}

#[test]
fn collection_aggregates() {
    assert_eq!(eval_int("sum_of(1, 2, 3, 4)"), 10);
    assert_eq!(eval_int("product_of(1, 2, 3, 4)"), 24);
    assert_eq!(eval_int("max_of(10, 5, 20, 15)"), 20);
    assert_eq!(eval_int("min_of(10, 5, 20, 15)"), 5);
}

#[test]
fn collection_zip_and_pairwise() {
    let code = r#"
        my $h = zipmap(["a", "b"], [1, 2]);
        # Verify hash by manual lookup to avoid sort/keys issues
        $h->{a} . ":" . $h->{b}
    "#;
    assert_eq!(eval_string(code), "1:2");

    let code2 = r#"
        my @p = pairwise(1, 2, 3, 4);
        # [[1,2], [2,3], [3,4]]
        join(":", map { join(",", @$_) } @p)
    "#;
    assert_eq!(eval_string(code2), "1,2:2,3:3,4");
}

#[test]
fn collection_transpose_list() {
    let code = r#"
        my @l1 = (1, 2);
        my @l2 = (3, 4);
        my @t = transpose(\@l1, \@l2);
        # [[1,3], [2,4]]
        join(":", map { join(",", @$_) } @t)
    "#;
    assert_eq!(eval_string(code), "1,3:2,4");
}

// ── List Operations ──────────────────────────────────────────────────────────

#[test]
fn list_rle_roundtrip() {
    let code = r#"
        my @data = (1, 1, 2, 3, 3, 3);
        my @encoded = rle(@data);
        # [[1,2], [2,1], [3,3]] - Note: rle in stryke returns [val, count] as strings?
        # Let's check logic: out.push([string(p), integer(c)])
        my @decoded = rld(@encoded);
        join(",", @decoded)
    "#;
    assert_eq!(eval_string(code), "1,1,2,3,3,3");
}

#[test]
fn list_sliding_and_batch() {
    let code = r#"
        my @s = sliding_pairs(1, 2, 3);
        # [[1,2], [2,3]]
        join(":", map { join(",", @$_) } @s)
    "#;
    assert_eq!(eval_string(code), "1,2:2,3");

    let code2 = r#"
        my @b = batch(2, 1, 2, 3, 4, 5);
        # [[1,2], [3,4], [5]]
        join(":", map { join(",", @$_) } @b)
    "#;
    assert_eq!(eval_string(code2), "1,2:3,4:5");
}

// ── More String & Misc ───────────────────────────────────────────────────────

#[test]
fn string_ciphers() {
    assert_eq!(eval_string(r#"rot13("abc")"#), "nop");
    assert_eq!(eval_string(r#"caesar_shift("abc", 1)"#), "bcd");
}

#[test]
fn misc_dice_roll() {
    // dice_roll(sides)
    for _ in 0..10 {
        let n = eval_int(r#"dice_roll(6)"#);
        assert!((1..=6).contains(&n));
    }
}

#[test]
fn list_flatten_deep() {
    let code = r#"
        my @l = (1, [2, [3, 4]], 5);
        join(",", flatten_deep(@l))
    "#;
    assert_eq!(eval_string(code), "1,2,3,4,5");
}
