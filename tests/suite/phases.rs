//! BEGIN / CHECK / INIT / UNITCHECK / END ordering. Catches regressions in the phase pipeline
//! (collect subs & phase blocks, run BEGIN → UNITCHECK → CHECK → INIT → main → END).

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
fn check_init_unitcheck_run_before_main_in_perl_order() {
    // Top-level `$s = ""` runs *after* UNITCHECK/CHECK/INIT (same as Perl), so it would wipe
    // phase appends. Initialize the buffer in BEGIN so phases see the same stash scalar as main.
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            BEGIN { $s = "" }
            UNITCHECK { $s .= "a" }
            UNITCHECK { $s .= "b" }
            CHECK { $s .= "c" }
            CHECK { $s .= "d" }
            INIT { $s .= "e" }
            INIT { $s .= "f" }
            $s .= "m";
            $s"#,
        ),
        "badcefm",
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
