//! splice() array surgery pins. Covers all four signatures:
//!   splice(@arr)            — empty everything
//!   splice(@arr, OFFSET)    — slice from offset to end
//!   splice(@arr, OFFSET, LENGTH)         — remove
//!   splice(@arr, OFFSET, LENGTH, LIST)   — replace/insert

use crate::common::*;

// ── basic remove ──────────────────────────────────────────────────

#[test]
fn splice_remove_middle() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        splice(@arr, 1, 2);
        join(",", @arr) eq "1,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_returns_removed_elements() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @gone = splice(@arr, 1, 2);
        join(",", @gone) eq "2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_one_returns_scalar() {
    let code = r#"
        my @arr = (10, 20, 30, 40);
        my $only = splice(@arr, 2, 1);
        ($only == 30 && join(",", @arr) eq "10,20,40") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_from_beginning() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @first_two = splice(@arr, 0, 2);
        (join(",", @first_two) eq "1,2" && join(",", @arr) eq "3,4,5") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_from_end_via_offset() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @tail = splice(@arr, 3);
        (join(",", @tail) eq "4,5" && join(",", @arr) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_zero_length_is_noop() {
    let code = r#"
        my @arr = (1, 2, 3);
        my @gone = splice(@arr, 1, 0);
        (len(@gone) == 0 && join(",", @arr) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── negative offset ──────────────────────────────────────────────

#[test]
fn splice_negative_offset_grabs_tail() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @last_two = splice(@arr, -2);
        (join(",", @last_two) eq "4,5" && join(",", @arr) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_negative_offset_with_length() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @one = splice(@arr, -3, 1);
        (join(",", @one) eq "3" && join(",", @arr) eq "1,2,4,5") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── insert via length=0 ──────────────────────────────────────────

#[test]
fn splice_insert_via_zero_length() {
    let code = r#"
        my @arr = (1, 2, 3);
        splice(@arr, 1, 0, 99);
        join(",", @arr) eq "1,99,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_insert_multiple_via_zero_length() {
    let code = r#"
        my @arr = (1, 2, 3);
        splice(@arr, 1, 0, 7, 8, 9);
        join(",", @arr) eq "1,7,8,9,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_insert_at_beginning() {
    let code = r#"
        my @arr = (3, 4, 5);
        splice(@arr, 0, 0, 1, 2);
        join(",", @arr) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_insert_at_end() {
    let code = r#"
        my @arr = (1, 2, 3);
        splice(@arr, len(@arr), 0, 4, 5);
        join(",", @arr) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── replace ──────────────────────────────────────────────────────

#[test]
fn splice_replace_same_length() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @old = splice(@arr, 1, 2, 22, 33);
        (join(",", @old) eq "2,3" && join(",", @arr) eq "1,22,33,4,5") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_replace_with_more() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        splice(@arr, 1, 2, 77, 88, 99);
        join(",", @arr) eq "1,77,88,99,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_replace_with_fewer() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        splice(@arr, 0, 3, 100);
        join(",", @arr) eq "100,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_replace_with_empty_is_delete() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        splice(@arr, 1, 3);
        join(",", @arr) eq "1,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── all-arguments forms ──────────────────────────────────────────

#[test]
fn splice_no_args_clears_array() {
    let code = r#"
        my @arr = (1, 2, 3);
        splice(@arr);
        len(@arr) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_no_args_returns_full_array() {
    let code = r#"
        my @arr = (1, 2, 3, 4);
        my @all = splice(@arr);
        (join(",", @all) eq "1,2,3,4" && len(@arr) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── edge cases: empty, past-end ──────────────────────────────────

#[test]
fn splice_empty_array_returns_empty() {
    let code = r#"
        my @arr;
        my @r = splice(@arr, 0, 5);
        (len(@r) == 0 && len(@arr) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_offset_past_end_returns_empty() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @r = splice(@arr, 10, 5);
        (len(@r) == 0 && join(",", @arr) eq "1,2,3,4,5") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_length_past_end_capped() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @r = splice(@arr, 3, 99);
        # Length capped to remaining (2 items).
        (join(",", @r) eq "4,5" && join(",", @arr) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── idiomatic uses ───────────────────────────────────────────────

#[test]
fn splice_shift_equivalent() {
    let code = r#"
        my @arr = (1, 2, 3);
        my $head = splice(@arr, 0, 1);
        ($head == 1 && join(",", @arr) eq "2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_pop_equivalent() {
    let code = r#"
        my @arr = (1, 2, 3);
        my $tail = splice(@arr, -1);
        ($tail == 3 && join(",", @arr) eq "1,2") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_unshift_equivalent_via_offset_zero() {
    let code = r#"
        my @arr = (3, 4);
        splice(@arr, 0, 0, 1, 2);
        join(",", @arr) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_push_equivalent_via_offset_end() {
    let code = r#"
        my @arr = (1, 2);
        splice(@arr, len(@arr), 0, 3, 4);
        join(",", @arr) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice from a list source ────────────────────────────────────

#[test]
fn splice_inject_array_flattens_to_count_per_bug_253() {
    // Stryke surface: when an array is passed as the LIST argument to
    // splice(), it's evaluated in scalar context (its length) rather
    // than flattened. Documented as BUG-253. Workaround: explicit
    // splat via @{[@arr]} or list-literal construction.
    let code = r#"
        my @target = (1, 2, 5, 6);
        my @injection = (3, 4);
        splice(@target, 2, 0, @injection);
        # Per BUG-253: only len(@injection) = 2 is inserted, not 3,4.
        join(",", @target) eq "1,2,2,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_inject_via_list_literal_works() {
    // Workaround for BUG-253: write the elements as a literal list.
    let code = r#"
        my @target = (1, 2, 5, 6);
        splice(@target, 2, 0, 3, 4);
        join(",", @target) eq "1,2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice on big array ───────────────────────────────────────────

#[test]
fn splice_remove_middle_of_1k() {
    let code = r#"
        my @arr = (1:1000);
        my @middle = splice(@arr, 100, 800);
        (len(@middle) == 800 && len(@arr) == 200) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice undo via splice again ─────────────────────────────────

#[test]
fn splice_reinsert_via_array_only_inserts_count_per_bug_253() {
    // Per BUG-253: the reinsert flattens to len(@gone) = 2 only.
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my @gone = splice(@arr, 1, 2);   # gone = (2, 3); arr = (1, 4, 5)
        splice(@arr, 1, 0, @gone);       # inserts 2 (len(@gone)), not 2,3
        join(",", @arr) eq "1,2,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice in scalar context ─────────────────────────────────────

#[test]
fn splice_scalar_context_returns_last_removed() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my $last = scalar(splice(@arr, 1, 3));
        $last == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice with array ref via deref ──────────────────────────────

#[test]
fn splice_via_arrayref_deref() {
    let code = r#"
        my $ref = [1, 2, 3, 4, 5];
        my @gone = splice(@$ref, 1, 2);
        (join(",", @gone) eq "2,3" && join(",", @$ref) eq "1,4,5") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
