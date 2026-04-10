//! Extra control-flow coverage: `do {} while`, labels, `do {}` scalar blocks.

use crate::common::*;

#[test]
fn do_while_executes_body_before_condition() {
    assert_eq!(
        eval_int(
            "my $i = 0; \
             do { $i = $i + 1; } while ($i < 3); \
             $i",
        ),
        3
    );
}

#[test]
fn do_while_runs_body_at_least_once_when_condition_false() {
    assert_eq!(
        eval_int(
            "my $i = 0; \
             do { $i = $i + 1; } while (0); \
             $i",
        ),
        1
    );
}

#[test]
fn labeled_last_breaks_outer_while() {
    assert_eq!(
        eval_int(
            "my $x = 0; \
             L: while ($x < 100) { \
                 $x = $x + 1; \
                 last L; \
             } \
             $x",
        ),
        1
    );
}

#[test]
fn do_block_returns_last_expression() {
    assert_eq!(eval_int("do { 1; 2; 3 }"), 3);
}

/// Last statement in `do { }` inherits outer wantarray (`grep` must see list context).
#[test]
fn do_block_propagates_list_context_to_last_expression() {
    assert_eq!(
        eval_int(
            r#"my @l=(1,2,3,2,1); my @u = do { my %seen; grep { !$seen{$_}++ } @l }; scalar @u"#,
        ),
        3
    );
}
