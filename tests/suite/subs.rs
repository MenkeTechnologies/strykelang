use crate::common::*;

#[test]
fn basic_sub() {
    assert_eq!(eval_int("fn add ($a, $b) { $a + $b } add(3, 4)"), 7);
}

#[test]
fn recursive_fibonacci() {
    assert_eq!(
        eval_int("fn fib_n ($n) { return $n if $n <= 1; fib_n($n-1) + fib_n($n-2) } fib_n(10)"),
        55
    );
}

#[test]
fn return_exits_sub_before_following_statement() {
    assert_eq!(
        eval_int(
            "fn foo { \
                 if (1) { return 3; } \
                 9 \
             } \
             foo()",
        ),
        3
    );
}

#[test]
fn return_with_postfix_if() {
    assert_eq!(
        eval_int("fn foo ($n) { return 0 if $n <= 0; $n } foo(5)"),
        5
    );
    assert_eq!(
        eval_int("fn foo ($n) { return 0 if $n <= 0; $n } foo(-1)"),
        0
    );
}

#[test]
fn sub_with_prototype_two_scalars_uses_at_underscore() {
    assert_eq!(eval_int("fn add2 ($$) { $_0 + $_1 } add2(40, 2)"), 42);
}

#[test]
fn sub_stryke_signature_scalar_and_hash_destruct() {
    assert_eq!(
        eval_int(
            r#"fn move_to ($self, { x => $x, y => $y }) { $x + $y }
 move_to(0, { x => 10, y => 32 })"#
        ),
        42
    );
    assert_eq!(
        eval_int(
            r#"fn move_to ($self, { x => $x, y => $y }) { $x + $y }
               my $h = { x => 3, y => 4 }; move_to(bless({}, "P"), $h)"#
        ),
        7
    );
}

#[test]
fn sub_stryke_signature_array_destruct() {
    assert_eq!(
        eval_int(
            r#"fn pair_sum ([ $x, $y ]) { $x + $y }
 pair_sum([10, 32])"#
        ),
        42
    );
    assert_eq!(
        eval_int(
            r#"fn head3 ([ $a, $b, @rest ]) { $a + $b + len(@rest) }
               head3([1, 2, 30, 40])"#
        ),
        5
    );
}

#[test]
fn my_destructure_arrayref() {
    assert_eq!(
        eval_int(
            r#"my $aref = [10, 32, 5];
               my [$x, $y, @rest] = $aref;
               $x + $y + len(@rest)"#
        ),
        43
    );
}

#[test]
fn my_destructure_hashref() {
    assert_eq!(
        eval_int(
            r#"my $href = { name => 10, age => 32 };
               my { name => $n, age => $a } = $href;
               $n + $a"#
        ),
        42
    );
}

#[test]
fn my_destructure_arrayref_length_mismatch_dies() {
    use stryke::error::ErrorKind;
    let k = eval_err_kind(
        r#"my $r = [1];
           my [$a, $b] = $r;
           0"#,
    );
    assert!(
        matches!(k, ErrorKind::Die | ErrorKind::Runtime),
        "expected die/runtime error, got {:?}",
        k
    );
}

#[test]
fn sub_stryke_signature_only_scalars() {
    assert_eq!(eval_int(r#"fn add ($a, $b) { $a + $b } add(8, 34)"#), 42);
}

#[test]
fn sub_stryke_signature_prototype_builtin_undef() {
    assert_eq!(
        eval_int(
            r#"fn sig ($a) { $a }
               defined(prototype \&sig) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn anon_sub_stryke_signature() {
    assert_eq!(eval_int(r#"my $f = fn ($n) { $n * 7 }; $f->(6)"#), 42);
}

#[test]
fn coderef_invocation_with_arrow() {
    assert_eq!(eval_int(r#"fn dbl { _ * 2 } my $r = \&dbl; $r->(21)"#), 42,);
}
