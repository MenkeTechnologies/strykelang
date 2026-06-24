//! Kotlin-flavored collection constructors: `listOf` / `mapOf` / `setOf`
//! families. List/array/sequence variants build a stryke array; map/set
//! variants build a hash / set. `sorted*` orders by key.

use crate::common::*;

#[test]
fn list_of_builds_array() {
    assert_eq!(eval_int("my @a = listOf(10, 20, 30); len(@a)"), 3);
    assert_eq!(eval_int("my @a = listOf(10, 20, 30); $a[1]"), 20);
}

#[test]
fn list_aliases_all_build_arrays() {
    // mutableListOf / arrayListOf / arrayOf / sequenceOf are constructor aliases.
    assert_eq!(eval_int("len(mutableListOf(1, 2))"), 2);
    assert_eq!(eval_int("len(arrayListOf(1, 2, 3))"), 3);
    assert_eq!(eval_int("len(arrayOf(1, 2, 3, 4))"), 4);
    assert_eq!(eval_int("len(sequenceOf(1))"), 1);
}

#[test]
fn empty_constructors_are_empty() {
    assert_eq!(eval_int("len(emptyList())"), 0);
    assert_eq!(eval_int("len(emptyArray())"), 0);
    assert_eq!(eval_int("len(emptySequence())"), 0);
}

#[test]
fn list_of_not_null_drops_undef() {
    assert_eq!(eval_int("len(listOfNotNull(1, undef, 3, undef))"), 2);
    assert_eq!(eval_int("my @a = listOfNotNull(1, undef, 9); $a[1]"), 9);
}

#[test]
fn array_of_nulls_fills_undef() {
    assert_eq!(eval_int("len(arrayOfNulls(4))"), 4);
    assert_eq!(eval_int("my @a = arrayOfNulls(3); defined($a[0]) ? 1 : 0"), 0);
}

#[test]
fn map_of_flat_pairs() {
    assert_eq!(eval_int(r#"my $h = mapOf("a", 1, "b", 2); $h->{a}"#), 1);
    assert_eq!(eval_int(r#"my $h = mapOf("a", 1, "b", 2); $h->{b}"#), 2);
}

#[test]
fn map_of_array_pairs() {
    assert_eq!(eval_int(r#"my $h = mapOf(["a", 7], ["b", 8]); $h->{a} + $h->{b}"#), 15);
}

#[test]
fn empty_map_has_no_keys() {
    assert_eq!(eval_int("my $h = emptyMap(); len(keys %$h)"), 0);
}

#[test]
fn sorted_map_orders_keys() {
    assert_eq!(
        eval_string(r#"my $h = sortedMapOf("z", 1, "a", 2, "m", 3); join(",", keys %$h)"#),
        "a,m,z",
    );
}

#[test]
fn set_of_dedups() {
    // setOf yields a set; flattening to a list drops duplicates.
    assert_eq!(eval_int("my @s = setOf(1, 2, 2, 3, 3, 3); len(@s)"), 3);
}

#[test]
fn empty_set_is_empty() {
    assert_eq!(eval_int("my @s = emptySet(); len(@s)"), 0);
}

#[test]
fn sorted_set_orders_and_dedups() {
    assert_eq!(eval_string("my @s = sortedSetOf(3, 1, 2, 1); join(\",\", @s)"), "1,2,3");
}
