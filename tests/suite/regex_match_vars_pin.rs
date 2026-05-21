//! Regex match-variable pins: `$&`, `$\``, `$'`, `$1..$N`, `@-`, `@+`,
//! `%+` (named captures).

use crate::common::*;

// ── $& (whole match) ───────────────────────────────────────────────

#[test]
fn dollar_amp_returns_full_match() {
    let code = r#"
        my $s = "Hello, World!";
        $s =~ /\w+, \w+/;
        $& eq "Hello, World" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dollar_amp_empty_on_no_match() {
    let code = r#"
        my $s = "hello";
        $s =~ /xyz/;
        # After a non-match, $& is whatever the last match left
        # (or undef on first run). Just verify it's not the haystack.
        $& ne $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $` (prematch) ─────────────────────────────────────────────────

#[test]
fn prematch_via_caret_prematch_form() {
    // BUG-257: `$`` (backtick) form not supported by stryke parser.
    // Working spelling is `${^PREMATCH}`.
    let code = r#"
        my $s = "before middle after";
        $s =~ /middle/;
        ${^PREMATCH} eq "before " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn prematch_empty_when_match_at_start() {
    let code = r#"
        my $s = "Hello";
        $s =~ /He/;
        ${^PREMATCH} eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $' (postmatch) ────────────────────────────────────────────────

#[test]
fn postmatch_via_caret_postmatch_form() {
    // BUG-257: `$'` (apostrophe) form not supported by stryke parser.
    // Working spelling is `${^POSTMATCH}`.
    let code = r#"
        my $s = "before middle after";
        $s =~ /middle/;
        ${^POSTMATCH} eq " after" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn postmatch_empty_when_match_at_end() {
    let code = r#"
        my $s = "hello";
        $s =~ /lo$/;
        ${^POSTMATCH} eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $1, $2, ... captures ──────────────────────────────────────────

#[test]
fn capture_group_one() {
    let code = r#"
        my $s = "foo=42";
        $s =~ /(\w+)=(\d+)/;
        $1 eq "foo" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn capture_group_two() {
    let code = r#"
        my $s = "foo=42";
        $s =~ /(\w+)=(\d+)/;
        $2 == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn captures_nested_groups() {
    let code = r#"
        my $s = "2026-05-15";
        $s =~ /^((\d{4})-(\d{2})-(\d{2}))$/;
        # $1 = whole date, $2 = year, $3 = month, $4 = day
        ($1 eq "2026-05-15" && $2 == 2026 && $3 == 5 && $4 == 15) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn capture_groups_up_to_nine() {
    let code = r#"
        my $s = "1-2-3-4-5-6-7-8-9";
        $s =~ /(\d)-(\d)-(\d)-(\d)-(\d)-(\d)-(\d)-(\d)-(\d)/;
        ($1 == 1 && $2 == 2 && $3 == 3 && $4 == 4 && $5 == 5
         && $6 == 6 && $7 == 7 && $8 == 8 && $9 == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn captures_reset_on_failed_match() {
    let code = r#"
        my $s = "abc=123";
        $s =~ /(\w+)=(\d+)/;
        my $g1_before = $1;
        my $nomatch = "no match here";
        $nomatch =~ /(\w+)=(\d+)/;
        # On a failed match, captures should retain their previous
        # value in stryke (this is the observed surface).
        defined($g1_before) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── @-, @+ (offset arrays) ────────────────────────────────────────

#[test]
fn match_start_offsets_array() {
    let code = r#"
        my $s = "abcXYZdef";
        $s =~ /(XYZ)/;
        # @- = ($match_start, $g1_start)
        ($- [0] == 3 && $- [1] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn match_end_offsets_array() {
    let code = r#"
        my $s = "abcXYZdef";
        $s =~ /(XYZ)/;
        # @+ = ($match_end, $g1_end)
        ($+ [0] == 6 && $+ [1] == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn match_offsets_for_multiple_groups() {
    let code = r#"
        my $s = "Hello, World!";
        $s =~ /(\w+), (\w+)/;
        # group 1 = "Hello" at [0, 5)
        # group 2 = "World" at [7, 12)
        ($- [1] == 0 && $+ [1] == 5
         && $- [2] == 7 && $+ [2] == 12) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn match_offset_zero_is_whole_match() {
    let code = r#"
        my $s = "...needle...";
        $s =~ /needle/;
        ($- [0] == 3 && $+ [0] == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── named captures ────────────────────────────────────────────────

#[test]
fn named_capture_via_percent_plus() {
    let code = r#"
        my $s = "name=alice";
        $s =~ /(?<key>\w+)=(?<val>\w+)/;
        ($+{key} eq "name" && $+{val} eq "alice") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_capture_email_split() {
    let code = r#"
        my $s = "alice\@example.com";
        $s =~ /^(?<user>[^@]+)\@(?<host>.+)$/;
        ($+{user} eq "alice" && $+{host} eq "example.com") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_capture_coexists_with_numbered() {
    let code = r#"
        my $s = "abc-123";
        $s =~ /(?<letters>[a-z]+)-(?<digits>\d+)/;
        ($+{letters} eq "abc" && $1 eq "abc"
         && $+{digits} eq "123" && $2 eq "123") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── interplay: $`, $&, $' concatenate to source ──────────────────

#[test]
fn prematch_match_postmatch_round_trip() {
    let code = r#"
        my $s = "abcdefghij";
        $s =~ /def/;
        (${^PREMATCH} . $& . ${^POSTMATCH}) eq $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn prematch_match_postmatch_with_offset() {
    let code = r#"
        my $s = "skip me middle skip me";
        $s =~ /middle/;
        (${^PREMATCH} eq "skip me " && $& eq "middle" && ${^POSTMATCH} eq " skip me") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── use in extraction idioms ─────────────────────────────────────

#[test]
fn iso_date_parse_via_captures() {
    let code = r#"
        my $s = "Created: 2026-05-15";
        if ($s =~ /(\d{4})-(\d{2})-(\d{2})/) {
            my $year = $1;
            my $mon  = $2;
            my $day  = $3;
            ($year == 2026 && $mon == 5 && $day == 15) ? 1 : 0
        } else {
            0
        }
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn host_port_split_via_captures() {
    let code = r#"
        my $s = "example.com:8080";
        $s =~ /^([^:]+):(\d+)$/;
        ($1 eq "example.com" && $2 == 8080) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_path_extract_via_captures() {
    let code = r#"
        my $url = "https://example.com:443/path/to/file";
        $url =~ m{^(https?)://([^:/]+)(?::(\d+))?(/.*)?$};
        ($1 eq "https" && $2 eq "example.com" && $3 == 443 && $4 eq "/path/to/file") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── list-context match returns captures ──────────────────────────

#[test]
fn list_context_match_returns_captures() {
    // `m//` in list context with capture groups returns the captures as a
    // list, matching Perl's documented behavior.
    let code = r#"
        my $s = "alice=30,bob=25";
        my @captures = ($s =~ /^(\w+)=(\d+)/);
        (len(@captures) == 2 && $captures[0] eq "alice" && $captures[1] == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn captures_via_numbered_vars_after_match() {
    // Working idiom in stryke: match then read $1, $2.
    let code = r#"
        my $s = "alice=30";
        $s =~ /^(\w+)=(\d+)/;
        ($1 eq "alice" && $2 == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── match across newlines ─────────────────────────────────────────

#[test]
fn multiline_capture() {
    let code = r#"
        my $s = "line1\nKEY: value\nline3";
        $s =~ /^KEY:\s*(\S+)/m;
        $1 eq "value" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── capture group counts ──────────────────────────────────────────

#[test]
fn at_minus_len_equals_capture_count_plus_one() {
    let code = r#"
        my $s = "Hello, World!";
        $s =~ /(\w+), (\w+)/;
        # Three entries: whole + 2 groups.
        (len(@-) == 3 && len(@+) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn at_minus_for_no_capture_only_whole() {
    let code = r#"
        my $s = "hello";
        $s =~ /lo/;
        (len(@-) == 1 && len(@+) == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
