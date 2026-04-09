//! `eval` and `$@` (eval_error) behavior.

use crate::common::*;

#[test]
fn eval_sets_at_on_runtime_failure() {
    assert_eq!(
        eval_int(
            r#"eval("1/0"); \
               $@ ne "" ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn eval_clears_at_on_success_after_failure() {
    assert_eq!(eval_int(r#"eval("1/0"); eval("2+2"); $@ eq "" ? 1 : 0"#), 1);
}

#[test]
fn eval_successful_computed_expression() {
    assert_eq!(eval_int(r#"eval("6 * 7")"#), 42);
}
