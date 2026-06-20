//! `shell_quote`/`shell_split`/`shellwords`, `from_jsonl`/`to_jsonl`,
//! `from_ini`/`to_ini`, `base64url_encode`/`base64url_decode`, and `time_ago`.

use crate::common::*;

// ── shlex ────────────────────────────────────────────────────────────

#[test]
fn shell_quote_passes_safe_bare() {
    assert_eq!(eval_string(r#"shell_quote("plain.txt")"#), "plain.txt");
}

#[test]
fn shell_quote_wraps_spaces() {
    assert_eq!(eval_string(r#"shell_quote("a b")"#), "'a b'");
}

#[test]
fn shell_quote_empty_is_quotes() {
    assert_eq!(eval_string(r#"shell_quote("")"#), "''");
}

#[test]
fn shell_quote_escapes_single_quote() {
    assert_eq!(eval_string(r#"shell_quote("it's")"#), r"'it'\''s'");
}

#[test]
fn shell_split_honors_quotes() {
    assert_eq!(
        eval_string(r#"join("|", shell_split(q{cmd -x 'a b' "c d"}))"#),
        "cmd|-x|a b|c d",
    );
}

#[test]
fn shellwords_is_alias() {
    assert_eq!(eval_string(r#"join("|", shellwords("one  two"))"#), "one|two");
}

#[test]
fn shell_split_backslash_escape() {
    assert_eq!(eval_string(r#"join("|", shell_split(q{a\ b c}))"#), "a b|c");
}

// ── JSON Lines ───────────────────────────────────────────────────────

#[test]
fn from_jsonl_parses_lines_skipping_blanks() {
    assert_eq!(
        eval_string("val @r = from_jsonl(qq{{\"id\":1}\n\n{\"id\":2}\n}); \"$r[0]{id}$r[1]{id}\""),
        "12",
    );
}

#[test]
fn to_jsonl_serializes_list() {
    assert_eq!(
        eval_string(r#"to_jsonl({a => 1}, {a => 2})"#),
        "{\"a\":1}\n{\"a\":2}\n",
    );
}

#[test]
fn jsonl_round_trips() {
    assert_eq!(
        eval_string(r#"len(from_jsonl(to_jsonl([10, 20, 30])))"#),
        "3",
    );
}

// ── INI ──────────────────────────────────────────────────────────────

#[test]
fn from_ini_globals_and_sections() {
    assert_eq!(
        eval_string("val $h = from_ini(qq{debug=1\n[db]\nhost=localhost\n}); \"$h->{debug}|$h->{db}{host}\""),
        "1|localhost",
    );
}

#[test]
fn from_ini_skips_comments() {
    assert_eq!(
        eval_string("val $h = from_ini(qq{; a comment\n# another\nkey=val\n}); $h->{key}"),
        "val",
    );
}

#[test]
fn to_ini_globals_before_sections() {
    assert_eq!(
        eval_string(r#"to_ini({debug => 1, server => {port => 8080}})"#),
        "debug=1\n[server]\nport=8080\n",
    );
}

// ── base64url ────────────────────────────────────────────────────────

#[test]
fn base64url_encode_is_urlsafe_padless() {
    // "hi?>" -> standard "aGk/Pg==" -> url-safe no-pad "aGk_Pg".
    assert_eq!(eval_string(r#"base64url_encode("hi?>")"#), "aGk_Pg");
}

#[test]
fn base64url_round_trips() {
    assert_eq!(
        eval_string(r#"base64url_decode(base64url_encode("token data ~ 12"))"#),
        "token data ~ 12",
    );
}

// ── time_ago ─────────────────────────────────────────────────────────

#[test]
fn time_ago_past_hours() {
    assert_eq!(eval_string(r#"time_ago(time() - 7200)"#), "2 hours ago");
}

#[test]
fn time_ago_future_day() {
    assert_eq!(eval_string(r#"time_ago(time() + 90000)"#), "in 1 day");
}

#[test]
fn time_ago_just_now() {
    assert_eq!(eval_string(r#"time_ago(time())"#), "just now");
}

#[test]
fn from_now_is_alias() {
    assert_eq!(eval_string(r#"from_now(time() - 120)"#), "2 minutes ago");
}
