//! Regression pin: `print`/`say`/`p` with a single scalar argument must
//! not absorb the next statement as an indirect-filehandle argument list.
//!
//! Before the fix, the parser saw `p $j` followed by a term-start token on
//! the next line and treated `$j` as a filehandle, then parsed the
//! following statement as the print arguments. At runtime that surfaced
//! as `print on unopened filehandle <stringified-args>`. The fix in
//! `parser::parse_print_like` requires the candidate-argument token to
//! share a source line with the indirect-handle scalar.

use crate::common::*;

#[test]
fn p_scalar_followed_by_my_decl_is_topic_print() {
    // Before the fix this raised: "print on unopened filehandle abc".
    let v = eval_int(
        r#"
            my $j = "abc"
            p $j
            123
        "#,
    );
    assert_eq!(v, 123);
}

#[test]
fn p_scalar_followed_by_another_p_is_topic_print() {
    let v = eval_int(
        r#"
            my $j = "abc"
            p $j
            p "x"
            7
        "#,
    );
    assert_eq!(v, 7);
}

#[test]
fn print_scalar_handle_on_same_line_still_treated_as_handle() {
    // Same-line `print $fh "msg"` must still parse `$fh` as the indirect
    // filehandle. We assert the parser path remains active by triggering
    // the runtime error "print on unopened filehandle $fh" on undefined
    // handle.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my $fh = "nope"; print $fh "msg""#);
    assert_eq!(kind, ErrorKind::Runtime);
}

#[test]
fn p_followed_by_from_json_does_not_eat_call() {
    // Pins the original report: `p $json` followed by `my $back = from_json($json)`.
    let v = eval_int(
        r#"
            my $json = q|{"k":1}|
            p $json
            my $back = from_json($json)
            $back->{k}
        "#,
    );
    assert_eq!(v, 1);
}
