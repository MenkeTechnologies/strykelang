//! Pin array-interpolation forms inside double-quoted strings per
//! `docs/STYLE_GUIDE.md` §1a: `@arr` (joined by `$"`, default
//! space), `@$ref` arrayref deref interp, `$arr[N]` element,
//! `@arr[i,j]` slice. Probed against the running interpreter on
//! 2026-05-23.

use crate::common::*;

#[test]
fn array_var_interp_joins_with_default_separator_space() {
    let code = r#"
        my @a = (1, 2, 3);
        my $s = "arr: @a";
        $s eq "arr: 1 2 3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_var_interp_empty_yields_empty_join() {
    let code = r#"
        my @a;
        my $s = "[@a]";
        $s eq "[]" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_var_interp_single_element_no_separator() {
    let code = r#"
        my @a = (42);
        my $s = "v=@a";
        $s eq "v=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dollar_quote_separator_changes_join_char() {
    // $" is the array-interpolation separator (default " ").
    let code = r#"
        $" = "-";
        my @a = (1, 2, 3);
        my $s = "j=@a";
        $s eq "j=1-2-3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrayref_deref_interp_with_at_dollar() {
    let code = r#"
        my $r = [10, 20, 30];
        my $s = "ref: @$r";
        $s eq "ref: 10 20 30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_element_interp_via_dollar_bracket() {
    let code = r#"
        my @a = (10, 20, 30);
        my $s = "first=$a[0] last=$a[-1]";
        $s eq "first=10 last=30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_interp_with_index_list() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my $s = "pick=@a[1,3]";
        $s eq "pick=20 40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_interp_with_range() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my $s = "mid=@a[1..3]";
        $s eq "mid=20 30 40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_interp_inside_longer_sentence() {
    let code = r#"
        my @nums = (1, 2, 3);
        my $s = "the values are @nums today.";
        $s eq "the values are 1 2 3 today." ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_interp_then_scalar_interp_in_same_string() {
    let code = r#"
        my @a = (10, 20);
        my $name = "list";
        my $s = "$name: @a";
        $s eq "list: 10 20" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_interp_consistent_with_explicit_join() {
    let code = r#"
        my @a = ("alpha", "beta", "gamma");
        my $auto = "[@a]";
        my $manual = "[" . join(" ", @a) . "]";
        $auto eq $manual ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
