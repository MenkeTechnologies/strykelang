//! `--compat` `@_` aliasing of the caller's variables (BUG-312). The perl
//! byte-diff lives in `parity/cases/20077_sub_args_alias_caller_variables.pl`;
//! these pin the bookkeeping that case does not reach.

use crate::common::*;

fn eval_compat(code: &str) -> String {
    with_global_flags(|| {
        stryke::set_compat_mode(true);
        let out = eval_locked(code).to_string();
        stryke::set_compat_mode(false);
        out
    })
}

#[test]
fn write_after_shift_reaches_the_later_argument() {
    assert_eq!(
        eval_compat(
            r#"sub set_second { my $self = shift; $_[0] = "x$self"; }
               my ($a, $b) = ("a", "b"); set_second($a, $b); "$a $b""#
        ),
        "a xa"
    );
}

#[test]
fn nested_call_writes_do_not_leak_to_the_outer_caller() {
    // `mid` shifts its argument (a stack-args sub, so its own frame is not
    // tracked); `w` writes its own `$_[0]`, which aliases `mid`'s `$a` — not
    // the variable passed to `mid`.
    assert_eq!(
        eval_compat(
            r#"sub w { $_[0] = "w" } sub mid { my $a = shift; w($a); 1 }
               my $v = "v"; mid($v); $v"#
        ),
        "v"
    );
}
