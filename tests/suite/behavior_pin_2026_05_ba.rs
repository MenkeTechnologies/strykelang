//! Behavior-pinning batch BA (2026-05-08): Sweeping final unpinned parity bugs from parity/ docs.

use crate::common::*;

#[test]
fn capture_exit_method_is_unimplemented() {
    let err = eval_err_kind(r#"my $r = capture("echo hi"); $r->exit;"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("UnknownMethod"),
        "expected runtime error for missing exit method on capture, got {:?}",
        err
    );
}

#[test]
fn die_with_hashref_preserves_ref_now() {
    // Was a known gap, but now works correctly.
    let out = eval_string(
        r#"eval { die { a => 1 } };
           ref($@)"#
    );
    assert_eq!(out, "HASH");
}

#[test]
fn regex_named_captures_work_now() {
    // Was a known gap, but now works correctly.
    let out = eval_string(
        r#""abc" =~ /(?<name>b)/ ? $+{name} : "none""#
    );
    assert_eq!(out, "b");
}

#[test]
fn pack_unpack_a_format_works_now() {
    // Was a known gap, but now works correctly.
    let out = eval_string(
        r#"my ($u) = unpack("A4", "test  ");
           $u"#
    );
    assert_eq!(out, "test");
}

#[test]
fn two_closures_sharing_state_works_in_compat_now() {
    // DESIGN-001 prohibits shared state outside of --compat.
    // Was listed as a gap in PARITY_ROADMAP where even in --compat it failed with "Not a code reference".
    let out = with_global_flags(|| {
        stryke::set_compat_mode(true);
        let val = eval_locked(
            r#"my $n = 0;
               my $f1 = sub { $n++ };
               my $f2 = sub { $n++ };
               $f1->();
               $f2->();
               $n"#
        ).to_string();
        stryke::set_compat_mode(false);
        val
    });
    assert_eq!(out, "2");
}
