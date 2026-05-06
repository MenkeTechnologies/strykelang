//! Behavior-pinning batch Z (2026-05-05): URL/Email parts, File stats, System, Math.

use crate::common::*;

// ── URL & Email Parts ───────────────────────────────────────────────────────

#[test]
fn url_parts_extraction() {
    let url = "https://user:pass@example.com:8080/path/to/page?q=1";
    assert_eq!(eval_string(&format!(r#"url_host('{}')"#, url)), "user:pass@example.com:8080");
    assert_eq!(eval_string(&format!(r#"url_path('{}')"#, url)), "/path/to/page");
    assert_eq!(eval_string(&format!(r#"url_scheme('{}')"#, url)), "https");
}

#[test]
fn email_parts_extraction() {
    let email = "gemini.cli@google.com";
    assert_eq!(eval_string(&format!(r#"email_local('{}')"#, email)), "gemini.cli");
    assert_eq!(eval_string(&format!(r#"email_domain('{}')"#, email)), "google.com");
}

// ── File Stat Helpers ────────────────────────────────────────────────────────

#[test]
fn file_stat_smoke() {
    // Cargo.toml should exist and be readable
    assert!(eval_int(r#"file_size("Cargo.toml")"#) > 100);
    assert_eq!(eval_int(r#"is_readable("Cargo.toml")"#), 1);
    assert_eq!(eval_int(r#"is_symlink("Cargo.toml")"#), 0);
    
    // Absolute path check
    assert_eq!(eval_int(r#"path_is_abs("/tmp")"#), 1);
    assert_eq!(eval_int(r#"path_is_abs("Cargo.toml")"#), 0);
}

// ── More List Helpers ────────────────────────────────────────────────────────

#[test]
fn list_helpers_z() {
    assert_eq!(eval_int("all_eq(1, 1, 1)"), 1);
    assert_eq!(eval_int("all_eq(1, 2, 1)"), 0);
    
    assert_eq!(eval_int("range_of(10, 5, 20)"), 15); // 20 - 5
    
    assert_eq!(eval_string(r#"longest("tiny", "huge-mungous", "small")"#), "huge-mungous");
    assert_eq!(eval_string(r#"shortest("tiny", "huge-mungous", "small")"#), "tiny");
    
    assert_eq!(eval_int("distinct_count(1, 2, 2, 3, 1)"), 3);
}

// ── System Metadata ──────────────────────────────────────────────────────────

#[test]
fn system_metadata_smoke() {
    assert!(!eval_string("os_arch()").is_empty());
    assert!(!eval_string("os_family()").is_empty());
    
    let endian = eval_string("endianness()");
    assert!(endian == "little" || endian == "big");
    
    let width = eval_int("pointer_width()");
    assert!(width == 32 || width == 64);
}

// ── Math & Trig (Extended) ──────────────────────────────────────────────────

#[test]
fn math_trig_smoke() {
    // tan(0) = 0
    assert_eq!(eval_int("tan(0)"), 0);
    // atan(0) = 0
    assert_eq!(eval_int("atan(0)"), 0);
    
    // sinh(0) = 0, cosh(0) = 1
    assert_eq!(eval_int("sinh(0)"), 0);
    assert_eq!(eval_int("cosh(0)"), 1);
}

// ── Miscellaneous ───────────────────────────────────────────────────────────

#[test]
fn misc_z_smoke() {
    assert_eq!(eval_int(r#"is_uuid(uuid_v4())"#), 1);
    assert_eq!(eval_int(r#"is_uuid("abc-123")"#), 0);
    
    // refresh_stashes should run without error
    eval("refresh_stashes()");
}
