//! Behavior-pinning batch AG (2026-05-05): JSON Helpers, Env, IDs.

use crate::common::*;

// ── JSON Helpers ─────────────────────────────────────────────────────────────

#[test]
fn json_helpers_ag() {
    // escape_json(s)
    assert_eq!(eval_string(r#"escape_json('a"b')"#), r#"a\"b"#);

    // json_minify(hash) returns minified string
    let code = r#"
        my $h = { a => 1, b => 2 };
        json_minify($h)
    "#;
    assert_eq!(eval_string(code), r#"{"a":1,"b":2}"#);
}

// ── Env & IDs ────────────────────────────────────────────────────────────────

#[test]
fn env_ids_ag() {
    assert_eq!(eval_int("env_has('PATH')"), 1);
    assert!(!eval_string("env_get('PATH')").is_empty());

    // token(n)
    assert_eq!(eval_int("len(token(16))"), 16);
    assert_eq!(eval_int("len(token(32))"), 32);
}

// ── TTY Checks ───────────────────────────────────────────────────────────────

#[test]
fn system_ae_extras() {
    // script_name() returns binary name or integration test name
    let s = eval_string("script_name()");
    assert!(s.contains("stryke") || s.contains("integration"));

    // argc() should be >= 0
    assert!(eval_int("argc()") >= 0);
}

#[test]
fn file_times_ag() {
    // file_atime and file_ctime return unix timestamps
    assert!(eval_int("file_atime('Cargo.toml')") > 1700000000);
    assert!(eval_int("file_ctime('Cargo.toml')") > 1700000000);
}

#[test]
fn network_mac_ag() {
    // net_mac() should return a MAC address (formatted or empty if no wifi/eth)
    let mac = eval_string("net_mac()");
    if !mac.is_empty() {
        assert!(mac.contains(":") || mac.contains("-"));
    }
}
