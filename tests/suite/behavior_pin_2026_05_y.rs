//! Behavior-pinning batch Y (2026-05-05): Git, Network, IDs, Dates, Misc.

use crate::common::*;

// ── Git Operations ──────────────────────────────────────────────────────────

#[test]
fn git_metadata_smoke() {
    // git_root() should contain "strykelang"
    assert!(eval_string("git_root()")
        .to_lowercase()
        .contains("strykelang"));

    let code = r#"
        my @files = git_files();
        len(@files) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);

    let code2 = r#"
        my @branches = git_branches();
        # Should have at least 'main' or 'master' or some branch
        len(@branches) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code2), 1);
}

// ── Network Metadata ────────────────────────────────────────────────────────

#[test]
fn network_smoke() {
    // hostname should not be empty
    assert!(!eval_string("net_hostname()").is_empty());

    assert_eq!(eval_int(r#"is_valid_ipv6("::1")"#), 1);
    assert_eq!(eval_int(r#"is_valid_ipv6("127.0.0.1")"#), 0);

    assert_eq!(
        eval_int(r#"is_valid_url("https://github.com/google/gemini-cli")"#),
        1
    );
    assert_eq!(eval_int(r#"is_valid_url("not-a-url")"#), 0);
}

// ── ID Generators ───────────────────────────────────────────────────────────

#[test]
fn id_generators_smoke() {
    assert_eq!(eval_string("uuid_v4()").len(), 36);
    assert_eq!(eval_string("nanoid()").len(), 21);
    assert_eq!(eval_string("short_id()").len(), 7);

    // token(len)
    assert_eq!(eval_string("token(10)").len(), 10);
}

// ── Date Helpers ─────────────────────────────────────────────────────────────

#[test]
fn date_predicates_and_names() {
    assert_eq!(eval_int("is_leap(2024)"), 1);
    assert_eq!(eval_int("is_leap(2025)"), 0);

    // February 2024 has 29 days
    assert_eq!(eval_int("days_in_month(2024, 2)"), 29);
    assert_eq!(eval_int("days_in_month(2025, 2)"), 28);

    assert_eq!(eval_string("month_name(5)"), "May");
    assert_eq!(eval_string("weekday_name(2)"), "Tuesday");
}

// ── String & Misc Extensions ────────────────────────────────────────────────

#[test]
fn string_misc_smoke() {
    assert_eq!(eval_int("ascii_ord('A')"), 65);
    assert_eq!(eval_string("ascii_chr(66)"), "B");

    assert_eq!(eval_int(r#"count_char("mississippi", "s")"#), 4);

    assert_eq!(eval_string(r#"lcp("flower", "flow", "flight")"#), "fl");
}

// ── System & Env ─────────────────────────────────────────────────────────────

#[test]
fn system_env_smoke() {
    // env_has("PATH") should be true on almost any system
    assert_eq!(eval_int(r#"env_has("PATH")"#), 1);

    // username should not be empty
    assert!(!eval_string("username()").is_empty());

    // home_dir should be an absolute path
    let home = eval_string("home_dir()");
    assert!(home.starts_with("/") || home.contains(r#":\"#)); // unix or windows
}
