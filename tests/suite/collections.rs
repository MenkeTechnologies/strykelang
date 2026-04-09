use crate::common::*;

#[test]
fn scalar_variables() {
    assert_eq!(eval_int("my $x = 42; $x"), 42);
    assert_eq!(eval_string(r#"my $s = "hello"; $s"#), "hello");
}

#[test]
fn array_variables() {
    assert_eq!(eval_int("my @a = (1,2,3); $a[1]"), 2);
    assert_eq!(eval_int("my @a = (1,2,3); scalar @a"), 3);
}

#[test]
fn array_negative_index() {
    assert_eq!(eval_int("my @a = (10, 20, 30); $a[-1]"), 30);
}

#[test]
fn hash_variables() {
    assert_eq!(eval_int(r#"my %h = ("a", 1, "b", 2); $h{b}"#), 2);
}

#[test]
fn hash_fat_arrow() {
    assert_eq!(
        eval_int("my %h = (aa => 10, bb => 20); $h{aa} + $h{bb}"),
        30
    );
}

#[test]
fn push_pop() {
    assert_eq!(eval_int("my @a = (1,2,3); push @a, 4; $a[3]"), 4);
    assert_eq!(eval_int("my @a = (1,2,3); pop @a"), 3);
    assert_eq!(eval_int("my @a = (1,2,3); pop @a; scalar @a"), 2);
}

#[test]
fn shift_unshift() {
    assert_eq!(eval_int("my @a = (1,2,3); shift @a"), 1);
    assert_eq!(eval_int("my @a = (1,2,3); unshift @a, 0; $a[0]"), 0);
}

#[test]
fn unshift_multiple_values() {
    assert_eq!(
        eval_string(r#"my @a = (1); unshift @a, 2, 3; join(",", @a)"#),
        "2,3,1"
    );
}

#[test]
fn splice_remove_insert() {
    assert_eq!(
        eval_string(r#"my @a = (1,2,3,4,5); join(",", splice @a, 1, 2)"#),
        "2,3"
    );
    assert_eq!(
        eval_int("my @a = (1,2,3,4,5); splice @a, 1, 2; scalar @a"),
        3
    );
}

#[test]
fn map_grep() {
    assert_eq!(eval_int("my @a = map { $_ * 2 } (1,2,3); $a[2]"), 6);
    assert_eq!(
        eval_int("my @a = grep { $_ > 2 } (1,2,3,4,5); scalar @a"),
        3
    );
}

#[test]
fn sort_array() {
    assert_eq!(eval_string(r#"join(",", sort("c","a","b"))"#), "a,b,c");
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (3,1,2))"#),
        "1,2,3"
    );
}

#[test]
fn reverse_array() {
    assert_eq!(eval_string(r#"join(",", reverse(1,2,3))"#), "3,2,1");
}

#[test]
fn array_slice() {
    assert_eq!(
        eval_string(r#"my @a = (10, 20, 30); join(",", @a[0, 2])"#),
        "10,30"
    );
}

#[test]
fn range_operator() {
    assert_eq!(eval_int("my @a = (1..5); scalar @a"), 5);
    assert_eq!(eval_int("my @a = (1..5); $a[4]"), 5);
}

#[test]
fn hash_delete_exists() {
    assert_eq!(
        eval_int("my %h = (a => 1, b => 2); delete $h{a}; exists $h{a} ? 1 : 0"),
        0
    );
    assert_eq!(
        eval_int("my %h = (a => 1, b => 2); exists $h{b} ? 1 : 0"),
        1
    );
}

#[test]
fn hash_keys_values() {
    assert_eq!(
        eval_int("my %h = (a => 1, b => 2, c => 3); scalar keys %h"),
        3
    );
}

#[test]
fn hash_values_builtin() {
    assert_eq!(
        eval_int("my %h = (a => 1, b => 2, c => 3); my $s = 0; foreach my $v (values %h) { $s = $s + $v; } $s"),
        6
    );
}

#[test]
fn set_new_union_intersection() {
    let code = r#"
        my $s = Set->new(1, 2, 3);
        my $t = Set->new(2, 3, 4);
        my $u = $s | $t;
        my $i = $s & $t;
        scalar $u * 100 + scalar $i
    "#;
    assert_eq!(eval_int(code), 402);
}

#[test]
fn mysync_set_union_intersection() {
    let code = r#"
        mysync $s = Set->new(1, 2, 3);
        mysync $t = Set->new(2, 3, 4);
        my $u = $s | $t;
        my $i = $s & $t;
        scalar $u * 100 + scalar $i
    "#;
    assert_eq!(eval_int(code), 402);
}

#[test]
fn mysync_set_or_assign() {
    let code = r#"
        mysync $s = Set->new(1);
        my $t = Set->new(2);
        $s |= $t;
        scalar $s
    "#;
    assert_eq!(eval_int(code), 2);
}
