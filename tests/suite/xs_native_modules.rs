//! Core XS modules stryke provides natively (BUG-319): `List::Util`,
//! `Scalar::Util`, `POSIX`. The `--compat` side is pinned byte-for-byte
//! against perl in `parity/cases/20074..20076`.

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
fn use_loads_no_pm_and_records_inc() {
    // Loading List/Util.pm from @INC used to reach Exporter.pm and die parsing it.
    assert_eq!(
        eval_string(
            r#"use List::Util qw(sum); require POSIX; join(",", sum(1, 2), exists $INC{"POSIX.pm"} ? 1 : 0)"#
        ),
        "3,1"
    );
}

#[test]
fn qualified_functions_need_the_module_loaded() {
    assert_eq!(eval_string(r#"use POSIX (); POSIX::fmod(-7, 3)"#), "-1");
    assert_eq!(
        eval_err_kind(r#"List::Util::max(1, 2)"#),
        stryke::error::ErrorKind::Runtime
    );
}

#[test]
fn compat_block_first_call_and_scalar_context() {
    assert_eq!(
        eval_compat(
            r#"use List::Util qw(first reduce head pairkeys);
               my $f = first { $_ > 3 } 3, 1, 4, 1, 5;
               my $r = reduce { $a * $b } 1 .. 5;
               my $h = head(2, 5, 6, 7);
               my $k = pairkeys(a => 1, b => 2);
               "$f $r $h $k""#
        ),
        "4 120 6 b"
    );
}

#[test]
fn compat_user_sub_beats_an_imported_name() {
    assert_eq!(
        eval_compat(r#"use List::Util qw(sum max); sub sum { "mine" } sum(1) . max(1, 5)"#),
        "mine5"
    );
}

#[test]
fn compat_posix_constants_take_no_arguments() {
    assert_eq!(eval_compat(r#"use POSIX; INT_MAX + 1"#), "2147483648");
    assert_eq!(eval_compat(r#"use POSIX qw(floor); floor(1e20)"#), "1e+20");
}
