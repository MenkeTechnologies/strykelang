//! Pin `zip` semantics: pad-to-longest with undef on shorter sides,
//! n-ary input arity, empty-arg result, and arrayref-of-arrayrefs
//! shape. Probed against the running interpreter on 2026-05-23
//! before pinning.
//!
//! Most languages (Python `zip`, Rust `Iterator::zip`) stop at the
//! shortest. Stryke's `zip` pads — this test pins the contract so a
//! future change to "stop at shortest" cannot land silently.

use crate::common::*;

#[test]
fn zip_equal_length_pairs() {
    let code = r#"
        my @r = zip([1, 2, 3], [4, 5, 6]);
        len(@r) == 3
            && join(",", @{$r[0]}) eq "1,4"
            && join(",", @{$r[2]}) eq "3,6"
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_pads_shorter_array_with_empty() {
    // First array longer → second pair gets undef/empty filler in
    // the position belonging to the shorter array.
    let code = r#"
        my @r = zip([1, 2, 3, 99], [4, 5, 6]);
        my $fill_is_empty_or_undef =
            !defined($r[3]->[1]) || $r[3]->[1] eq "";
        len(@r) == 4 && $r[3]->[0] == 99 && $fill_is_empty_or_undef
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_pads_first_array_when_it_is_shorter() {
    let code = r#"
        my @r = zip([1], [2, 3, 4]);
        my $r1_left_blank =
            !defined($r[1]->[0]) || $r[1]->[0] eq "";
        len(@r) == 3 && $r[0]->[0] == 1 && $r1_left_blank && $r[2]->[1] == 4
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_three_arrays_yields_triples() {
    let code = r#"
        my @r = zip([1, 2, 3], [4, 5, 6], [7, 8, 9]);
        len(@r) == 3
            && len(@{$r[0]}) == 3
            && join(",", @{$r[1]}) eq "2,5,8"
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_two_empty_arrays_returns_empty() {
    let code = r#"
        my @r = zip([], []);
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn zip_inner_elements_are_array_refs() {
    let code = r#"
        my @r = zip([1, 2], [3, 4]);
        ref($r[0]) eq "ARRAY" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_round_trip_via_map_pair_dereference() {
    // zip + map gives the classic "interleave two arrays" effect.
    let code = r#"
        my @pairs = zip([1, 2, 3], ["a", "b", "c"]);
        my @flat = map { ($_->[0], $_->[1]) } @pairs;
        join(":", @flat) eq "1:a:2:b:3:c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_pads_with_unequal_lengths_three_arrays() {
    // Pad-to-longest still applies with arity ≥ 3.
    let code = r#"
        my @r = zip([1, 2, 3, 4], [10, 20], [100, 200, 300]);
        len(@r) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
