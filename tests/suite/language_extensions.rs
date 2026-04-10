//! Tree-interpreter extensions: `retry`, `rate_limit`, `every`, `gen` / `yield`.

use crate::common::*;

#[test]
fn retry_succeeds_first_attempt() {
    assert_eq!(eval_int(r#"retry { 42 } times => 3"#), 42);
}

#[test]
fn gen_yield_next_returns_value_and_more_flag() {
    assert_eq!(
        eval_string(
            r#"my $g = gen { yield 7; yield 8; };
            my $a = $g->next;
            my $b = $g->next;
            my $c = $g->next;
            join(",", $a->[0], $a->[1], $b->[0], $b->[1], $c->[0], $c->[1])"#,
        ),
        "7,1,8,1,,0"
    );
}
