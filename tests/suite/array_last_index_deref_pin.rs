//! `$#{EXPR}` / `$#$name` — last index of an array reached through a
//! reference, in code and inside double-quoted strings. Before this landed,
//! `$#` followed by `{` or `$` lexed as the deprecated output-format variable
//! `$#`, so `0..$#$r` and `$#{ $h->{list} }` were parse errors ("print on
//! unopened filehandle $#"). Expected values are what `perl` prints.

use crate::common::*;

#[test]
fn last_index_of_arrayref_via_dollar_hash_dollar() {
    let code = r#"
        my $r = [10, 20, 30];
        $#$r
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn last_index_of_arrayref_via_braced_body() {
    let code = r#"
        my $r = [10, 20, 30];
        $#{$r} + $#{ $r }
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn last_index_of_nested_arrayref_expression_body() {
    // The braced body is a general expression, not just a variable.
    let code = r#"
        my $h = { list => [1, 2, 3, 4] };
        $#{ $h->{list} }
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn last_index_of_empty_arrayref_is_minus_one() {
    let code = r#"
        my $r = [];
        $#$r == -1 && $#{$r} == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn braced_plain_name_is_the_named_array_last_index() {
    // `$#{a}` and `$#{ a }` are `$#a`, not a deref of a scalar `$a`.
    let code = r#"
        my @a = (5, 6);
        join(",", $#a, $#{a}, $#{ a })
    "#;
    assert_eq!(eval_string(code), "1,1,1");
}

#[test]
fn range_to_last_index_of_arrayref_drives_a_loop() {
    let code = r#"
        my $r = [7, 8, 9];
        my $out = "";
        for my $i (0 .. $#$r) { $out .= $i }
        for my $i (0 .. $#{$r}) { $out .= $r->[$i] }
        $out
    "#;
    assert_eq!(eval_string(code), "012789");
}

#[test]
fn last_index_of_arrayref_as_an_index() {
    let code = r#"
        my $r = [1, 2, 3];
        $$r[$#$r] * 10 + $r->[$#{$r}]
    "#;
    assert_eq!(eval_int(code), 33);
}

#[test]
fn last_index_of_arrayref_interpolates_in_string() {
    let code = r#"
        my $r = [1, 2, 3];
        my $h = { list => [1, 2] };
        "a=$#$r b=$#{$r} c=$#{ $h->{list} } d=$#{[1,2,3,4]}"
    "#;
    assert_eq!(eval_string(code), "a=2 b=2 c=1 d=3");
}

#[test]
fn last_index_of_arrayref_through_package_qualified_scalar() {
    let code = r#"
        our $Demo::Lid::r = [1, 2, 3, 4, 5];
        $#$Demo::Lid::r
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn deprecated_output_format_variable_still_lexes_alone() {
    // A bare `$#` not followed by `{` / `$` / a name keeps its old reading
    // (Perl's removed `$#` output-format variable), so `$# = 1` still parses.
    let code = r#"
        no strict;
        $# = 1;
        my @a = (1, 2, 3);
        $#a
    "#;
    assert_eq!(eval_int(code), 2);
}
