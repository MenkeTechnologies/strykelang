//! `$_` aliasing in `map { }` / `grep { }` blocks and `for ($scalar)` loops.
//! Perl aliases `$_` to each element of the source, so a block that edits
//! `$_` (`s///`, `tr///`, `chomp`, `++`, `*=`) edits the array or scalar in
//! place. Before this landed, map/grep blocks worked on a copy and `for ($s)`
//! left `$s` unchanged, and an edit made inside a nested bare block of a
//! `for` body was dropped. Expected values are what `perl` prints.

use crate::common::*;

#[test]
fn map_block_substitution_edits_source_array() {
    let code = r#"
        my @w = ("x1", "y1", "x2");
        map { s/x/z/ } @w;
        "@w"
    "#;
    assert_eq!(eval_string(code), "z1 y1 z2");
}

#[test]
fn map_block_trim_returns_and_edits() {
    // The classic trim-in-map idiom: both the result and the source change.
    let code = r#"
        my @l = (" a ", " b");
        my @t = map { s/^\s+|\s+$//g; $_ } @l;
        "[@l][@t]"
    "#;
    assert_eq!(eval_string(code), "[a b][a b]");
}

#[test]
fn map_block_list_tail_still_flattens_after_edit() {
    let code = r#"
        my @a = (1, 2, 3);
        my @b = map { $_ *= 10; ($_, $_ + 1) } @a;
        "@a|@b"
    "#;
    assert_eq!(eval_string(code), "10 20 30|10 11 20 21 30 31");
}

#[test]
fn map_block_chomp_and_tr_edit_in_place() {
    let code = r#"
        my @a = ("x\n", "y\n");
        map { chomp } @a;
        my @u = ("a1", "b2");
        map { tr/a-z/A-Z/ } @u;
        "@a|@u"
    "#;
    assert_eq!(eval_string(code), "x y|A1 B2");
}

#[test]
fn map_block_without_writes_leaves_source_alone() {
    let code = r#"
        my @a = (1, 2, 3);
        my @b = map { my $x = $_; $x * 2 } @a;
        "@a|@b"
    "#;
    assert_eq!(eval_string(code), "1 2 3|2 4 6");
}

#[test]
fn grep_block_increment_edits_source_array() {
    let code = r#"
        my @a = (1, 2, 3);
        my $n = grep { $_++ } @a;
        "$n|@a"
    "#;
    assert_eq!(eval_string(code), "3|2 3 4");
}

#[test]
fn grep_block_returns_edited_elements() {
    // grep returns the elements themselves, so the kept values carry the edit.
    let code = r#"
        my @a = (1, 2, 3, 4);
        my @e = grep { $_ *= 2; $_ > 4 } @a;
        "@a|@e"
    "#;
    assert_eq!(eval_string(code), "2 4 6 8|6 8");
}

#[test]
fn map_and_grep_alias_inside_sub_body() {
    let code = r#"
        my $c = sub {
            my @b = (1, 2);
            map { $_ *= 3 } @b;
            my @g = (1, 2, 3);
            grep { $_++ } @g;
            "@b|@g"
        };
        $c->()
    "#;
    assert_eq!(eval_string(code), "3 6|2 3 4");
}

#[test]
fn our_array_is_aliased_too() {
    let code = r#"
        our @g = (1, 2);
        map { $_ *= 5 } @g;
        "@g"
    "#;
    assert_eq!(eval_string(code), "5 10");
}

#[test]
fn for_single_scalar_aliases_loop_topic() {
    let code = r#"
        my $s = "  pad  ";
        for ($s) { s/^\s+//; s/\s+$// }
        "[$s]"
    "#;
    assert_eq!(eval_string(code), "[pad]");
}

#[test]
fn for_single_scalar_named_loop_var_and_postfix_form() {
    let code = r#"
        my $s = "abc";
        for my $x ($s) { $x .= "!" }
        my $t = "abc";
        s/b/B/ for $t;
        "$s $t"
    "#;
    assert_eq!(eval_string(code), "abc! aBc");
}

#[test]
fn for_single_scalar_copy_source_is_untouched() {
    let code = r#"
        my $x = 5;
        my $y = $x;
        for ($y) { $_++ }
        "$x $y"
    "#;
    assert_eq!(eval_string(code), "5 6");
}

#[test]
fn for_body_nested_bare_block_edit_reaches_array() {
    // The write to `$_` inside `{ ... }` used to stay in the inner block's
    // frame and vanish at the closing brace.
    let code = r#"
        my @b = (1, 2, 3);
        for (@b) { { $_ *= 2 } }
        "@b"
    "#;
    assert_eq!(eval_string(code), "2 4 6");
}

#[test]
fn map_restores_outer_topic() {
    // map localizes `$_`: the caller's topic survives the aliased edits.
    let code = r#"
        $_ = "outer";
        my @a = (1, 2);
        map { $_ += 1 } @a;
        "$_|@a"
    "#;
    assert_eq!(eval_string(code), "outer|2 3");
}
