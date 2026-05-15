//! Three-tier regex engine pins. Stryke auto-escalates between:
//!
//!   Tier 1 — Rust `regex` (DFA, linear time, no backrefs/lookaround)
//!   Tier 2 — `fancy-regex`   (regex + backrefs + lookaround)
//!   Tier 3 — `pcre2`         (PCRE-only verbs)
//!
//! The escalation is supposed to be invisible — callers write
//! ordinary `m//`/`s///`/`qr//` and the right engine handles it.
//! These pins lock the feature set so a silent tier downgrade
//! (e.g. dropping fancy-regex) would fail CI.

use crate::common::*;

// ── Tier 1: vanilla regex (DFA) ──────────────────────────────────────

#[test]
fn tier1_basic_digit_match() {
    let code = r#"
        my $s = "user_id=42 latency=137ms";
        ($s =~ /user_id=(\d+)/ && $1 == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tier1_anchored_match() {
    let code = r#"
        ("foo bar" =~ /^foo/) && !("bar foo" =~ /^foo/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tier1_global_match_returns_all() {
    let code = r#"
        my @ms = "abc 123 def 456 ghi 789" =~ /(\d+)/g;
        join(",", @ms) eq "123,456,789" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Named captures populate %+ ───────────────────────────────────────

#[test]
fn named_captures_populate_plus_hash() {
    let code = r#"
        my $s = "alice\@example.com";
        ($s =~ /(?<user>\w+)@(?<host>[\w.]+)/
            && $+{user} eq "alice"
            && $+{host} eq "example.com") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_captures_survive_in_array_context() {
    let code = r#"
        my $s = "2026-05-15";
        if ($s =~ /(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})/) {
            ($+{y} == 2026 && $+{m} == 5 && $+{d} == 15) ? 1 : 0
        } else { 0 }
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Tier 2: backrefs (fancy-regex escalation) ────────────────────────

#[test]
fn tier2_backreference_matches_repeated_group() {
    let code = r#"
        # "abab" → matches "(.+)\1"; "abc" → doesn't.
        ("abab" =~ /^(.+)\1$/ && !("abc" =~ /^(.+)\1$/)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tier2_palindrome_via_backref() {
    let code = r#"
        my @results;
        for my $w ("abba", "stryke", "racecar", "hello") {
            push @results,
                ($w =~ /^(.)(.)\2\1$/ || $w =~ /^(.)(.).*\2\1$/)
                    ? "y" : "n";
        }
        join("", @results) eq "ynyn" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Tier 2: lookahead / lookbehind ───────────────────────────────────

#[test]
fn tier2_positive_lookahead_isolates_capture() {
    let code = r#"
        my $text = "foo bar baz bar qux quux bar";
        my @words;
        while ($text =~ /(\w+)(?= bar)/g) {
            push @words, $1;
        }
        join(",", @words) eq "foo,baz,quux" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tier2_negative_lookahead() {
    let code = r#"
        # Find numbers NOT followed by a unit suffix.
        my $text = "1ms 2 3kb 4 5 6mb";
        my @loose;
        while ($text =~ /\b(\d+)(?![a-z])/g) { push @loose, $1 }
        join(",", @loose) eq "2,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tier2_positive_lookbehind() {
    let code = r#"
        my $text = "price: \$42 cost: \$99 fee: \$3";
        my @amounts;
        while ($text =~ /(?<=\$)(\d+)/g) { push @amounts, $1 }
        join(",", @amounts) eq "42,99,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Compiled regex via qr// — reusable across matches ─────────────────

#[test]
fn qr_compiled_regex_reuses_across_matches() {
    let code = r#"
        my $iso = qr/^(\d{4})-(\d{2})-(\d{2})$/;
        my $hits = 0;
        for my $d ("2026-05-15", "not-a-date", "1999-12-31", "12-3-456") {
            $hits++ if $d =~ $iso;
        }
        $hits == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qr_compiled_regex_with_named_capture() {
    let code = r#"
        my $iso = qr/^(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})$/;
        if ("2026-05-15" =~ $iso) {
            ($+{y} == 2026 && $+{m} == 5 && $+{d} == 15) ? 1 : 0
        } else { 0 }
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── s/// substitution ────────────────────────────────────────────────

#[test]
fn substitution_simple_replace() {
    let code = r#"
        my $s = "alpha,beta,gamma";
        $s =~ s/,/ | /g;
        $s eq "alpha | beta | gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substitution_with_backreference() {
    let code = r#"
        my $s = "hello world";
        $s =~ s/(\w+)/<$1>/g;
        $s eq "<hello> <world>" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substitution_case_insensitive() {
    let code = r#"
        my $s = "Hello HELLO heLLo";
        $s =~ s/hello/X/ig;
        $s eq "X X X" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Quantifier semantics ─────────────────────────────────────────────

#[test]
fn quantifier_greedy_vs_non_greedy() {
    let code = r#"
        my $s = "<a><b><c>";
        my @greedy;
        while ($s =~ /<(.+)>/g)  { push @greedy, $1 }
        my @nongreedy;
        my $t = $s;
        while ($t =~ /<(.+?)>/g) { push @nongreedy, $1 }
        # Greedy captures "a><b><c" once; non-greedy captures "a", "b", "c"
        (len(@greedy) == 1 && len(@nongreedy) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Char-class shorthands ────────────────────────────────────────────

#[test]
fn char_class_shorthand_works() {
    let code = r#"
        my $s = "abc 123 XYZ";
        my @digits = $s =~ /\d/g;
        my @upper  = $s =~ /[A-Z]/g;
        my @lower  = $s =~ /[a-z]/g;
        (len(@digits) == 3 && len(@upper) == 3 && len(@lower) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-line + dotall flags ────────────────────────────────────────

#[test]
fn dotall_s_flag_matches_newlines() {
    let code = r#"
        my $text = "line1\nline2\nline3";
        # Without /s, `.+` stops at newlines.
        my $nos = ($text =~ /line1.+line3/) ? 1 : 0;
        my $s   = ($text =~ /line1.+line3/s) ? 1 : 0;
        ($nos == 0 && $s == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn multiline_m_flag_anchors_per_line() {
    let code = r#"
        my $text = "alpha\nbeta\ngamma";
        my @lines;
        while ($text =~ /^(\w+)$/gm) { push @lines, $1 }
        join(",", @lines) eq "alpha,beta,gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr/// — character translation ────────────────────────────────────

#[test]
fn tr_translates_characters() {
    let code = r#"
        my $s = "hello";
        $s =~ tr/a-z/A-Z/;
        $s eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_count_returns_substitutions() {
    let code = r#"
        my $s = "hello world";
        my $n = ($s =~ tr/o/0/);
        ($s eq "hell0 w0rld" && $n == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cross-feature: regex in pipe-forward grep ───────────────────────

#[test]
fn pipe_grep_with_regex_predicate() {
    let code = r#"
        my @log = (
            "INFO  request handled",
            "ERROR timeout on backend",
            "INFO  response sent",
            "ERROR auth failed",
            "DEBUG cache miss",
        );
        my @errors = @log |> grep { /^ERROR/ };
        len(@errors) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
