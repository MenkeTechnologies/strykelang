//! BEGIN / END ordering and compile-time hooks. These catch regressions in the two-pass
//! interpreter pipeline (collect subs & phases, run BEGIN, run main, run END).

use crate::common::*;

#[test]
fn begin_defines_subroutine_visible_to_main() {
    assert_eq!(
        eval_int(
            "BEGIN { sub from_begin { 42 } } \
             from_begin()",
        ),
        42
    );
}

#[test]
fn begin_runs_before_main_and_can_set_global_scalar() {
    // `my` in main has not run when BEGIN executes; use a package/global scalar so the
    // assignment is visible to later main-line code (matches minimal Perl semantics here).
    // Package global without `my`/`our` — disable strict vars for this snippet (implicit stash write).
    assert_eq!(eval_int("no strict 'vars'; BEGIN { $seen = 1 } $seen"), 1);
}

#[test]
fn main_return_value_not_replaced_by_empty_end_block() {
    assert_eq!(
        eval_int(
            "END { } \
             7",
        ),
        7
    );
}

#[test]
fn end_block_runs_after_main_without_panicking() {
    let code = "my $x = 1; END { $x = 2 }; $x";
    let program = perlrs::parse(code).expect("parse");
    let mut interp = perlrs::interpreter::Interpreter::new();
    let v = interp.execute(&program).expect("execute");
    assert_eq!(
        v.to_int(),
        1,
        "return value is last main expression before END side effects"
    );
}
