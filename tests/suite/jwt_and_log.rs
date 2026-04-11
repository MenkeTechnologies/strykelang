//! JWT (HS256) and structured logging builtins.

use crate::common::{eval, eval_int, eval_string};

#[test]
fn jwt_roundtrip_hs256() {
    let code = r#"
        my $t = jwt_encode({ user => "user1", n => 42 }, "s3cr3t", alg => "HS256");
        my $p = jwt_decode($t, "s3cr3t");
        $p->{user} . ":" . $p->{n}
    "#;
    assert_eq!(eval_string(code), "user1:42");
}

#[test]
fn jwt_decode_unsafe_skips_verify() {
    let code = r#"
        my $t = jwt_encode({ a => 1 }, "k", alg => "HS256");
        my $u = jwt_decode_unsafe($t);
        $u->{a}
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn jwt_rejects_wrong_secret() {
    let code = r#"
        my $t = jwt_encode({ x => 1 }, "good", alg => "HS256");
        eval { jwt_decode($t, "bad") };
        $@ ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn jwt_expired_fails() {
    let code = r#"
        my $exp = time() - 60;
        my $t = jwt_encode({ exp => $exp }, "k", alg => "HS256");
        eval { jwt_decode($t, "k") };
        $@ =~ /expired/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn log_level_get_set_and_filter() {
    let code = r#"
        log_level("trace");
        my $a = log_info("x");
        log_level("error");
        my $b = log_info("y");
        log_level(undef);
        $a == 1 && $b == 0
    "#;
    assert!(eval(code).is_true());
}

#[test]
fn log_json_emits_object_keys() {
    let code = r#"
        log_level("info");
        my $s = log_json("info", "hi", { k => 1 });
        $s == 1
    "#;
    assert!(eval(code).is_true());
}
