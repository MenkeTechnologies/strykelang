//! Regex capture-group semantics pins.

use crate::common::*;

// ── Numbered groups $1..$9 ─────────────────────────────────────────

#[test]
fn three_numbered_groups() {
    let code = r#"
        "alice:30:engineer" =~ /^(\w+):(\d+):(\w+)$/;
        ($1 eq "alice" && $2 == 30 && $3 eq "engineer") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn five_numbered_groups() {
    let code = r#"
        "1-2-3-4-5" =~ /^(\d)-(\d)-(\d)-(\d)-(\d)$/;
        ($1 == 1 && $2 == 2 && $3 == 3 && $4 == 4 && $5 == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested groups ─────────────────────────────────────────────────

#[test]
fn nested_groups_outer_then_inner() {
    let code = r#"
        "abc123def" =~ /^([a-z]+(\d+))([a-z]+)$/;
        # $1 = whole abc123, $2 = inner 123, $3 = def.
        ($1 eq "abc123" && $2 == 123 && $3 eq "def") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn three_level_nesting() {
    let code = r#"
        "((nested))" =~ /^(\((\((\w+)\))\))$/;
        ($1 eq "((nested))" && $2 eq "(nested)" && $3 eq "nested") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Non-capturing groups ───────────────────────────────────────────

#[test]
fn non_capturing_group_skipped() {
    let code = r#"
        "abc123" =~ /^(?:[a-z]+)(\d+)$/;
        # (?:...) is non-capturing; first cap is (\d+) = 123.
        $1 == 123 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mixed_capturing_and_non_capturing() {
    let code = r#"
        "hello-world-42" =~ /^(\w+)(?:-)(\w+)(?:-)(\d+)$/;
        ($1 eq "hello" && $2 eq "world" && $3 == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Named groups ──────────────────────────────────────────────────

#[test]
fn named_group_captures() {
    let code = r#"
        "alice=30" =~ /^(?<key>\w+)=(?<value>\d+)$/;
        ($+{key} eq "alice" && $+{value} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_group_also_numbered() {
    let code = r#"
        "answer=42" =~ /^(?<k>\w+)=(?<v>\d+)$/;
        ($1 eq "answer" && $2 == 42 && $+{k} eq "answer" && $+{v} == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Alternation with captures ─────────────────────────────────────

#[test]
fn alternation_first_alt_captures() {
    let code = r#"
        "42" =~ /^(?:(\d+)|(\w+))$/;
        # \d+ matches; \w+ branch never taken.
        ($1 eq "42" && !defined($2)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn alternation_second_alt_captures() {
    let code = r#"
        "abc" =~ /^(?:(\d+)|([a-z]+))$/;
        # First branch fails; second branch matches.
        (!defined($1) && $2 eq "abc") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Backreference within pattern ──────────────────────────────────

#[test]
fn backref_matches_same_text() {
    let code = r#"
        ("abcabc" =~ /^(abc)\1$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn backref_with_quantifier() {
    let code = r#"
        ("xxxyyy" =~ /^(x+)(y+)$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_backref() {
    let code = r#"
        ("foofoo" =~ /^(?<w>\w+)\k<w>$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Optional groups ───────────────────────────────────────────────

#[test]
fn optional_group_unmatched_is_undef() {
    let code = r#"
        "abc" =~ /^(\w+?)(\d+)?$/;
        # $1 = "abc"; $2 = undef.
        ($1 eq "abc" && !defined($2)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn optional_group_matched_is_value() {
    let code = r#"
        "abc42" =~ /^(\w+?)(\d+)?$/;
        ($1 eq "abc" && $2 == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Groups with quantifier ────────────────────────────────────────

#[test]
fn repeated_group_last_iteration_wins() {
    let code = r#"
        "ababab" =~ /^(ab)+$/;
        # In repeated captures, $1 is the last iteration: "ab".
        $1 eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn alt_repeated_group() {
    let code = r#"
        "abc" =~ /^([abc])+$/;
        # Last iteration: $1 = "c".
        $1 eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Capture-group inside group ────────────────────────────────────

#[test]
fn group_with_internal_alt() {
    let code = r#"
        "GET /home" =~ /^(GET|POST) (\/\w+)$/;
        ($1 eq "GET" && $2 eq "/home") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed: nested + non-capturing + named ─────────────────────────

#[test]
fn complex_url_extraction() {
    let code = r#"
        "https://example.com:8080/path" =~ m{
            ^(?<scheme>https?)
            ://
            (?<host>[^:/]+)
            (?::(?<port>\d+))?
            (?<path>/.*)?$
        }x;
        ($+{scheme} eq "https"
            && $+{host} eq "example.com"
            && $+{port} == 8080
            && $+{path} eq "/path") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty capture group ───────────────────────────────────────────

#[test]
fn empty_capture_when_quantifier_zero() {
    let code = r#"
        "abc" =~ /^(\d*)abc$/;
        $1 eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Group with anchor ─────────────────────────────────────────────

#[test]
fn group_with_word_boundary() {
    let code = r#"
        "the cat sat on the mat" =~ /\b(c\w+)\b/;
        $1 eq "cat" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Capture in /g loop preserves last match per iteration ────────

#[test]
fn g_loop_captures_per_iteration() {
    let code = r#"
        my $s = "a=1, b=2, c=3";
        my @captures;
        while ($s =~ /(\w+)=(\d+)/g) {
            push @captures, "$1:$2";
        }
        join(",", @captures) eq "a:1,b:2,c:3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Non-capturing in s/// replacement ─────────────────────────────

#[test]
fn s_with_non_capturing_group() {
    let code = r#"
        my $s = "hello-world";
        $s =~ s/^(?:hello)-(\w+)$/$1/;
        $s eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Captures in qr// reuse ────────────────────────────────────────

#[test]
fn qr_pattern_captures_reusable() {
    let code = r#"
        my $re = qr/^(\w+)=(\d+)$/;
        my %h;
        for my $kv ("alice=30", "bob=25", "carol=42") {
            if ($kv =~ $re) {
                $h{$1} = $2;
            }
        }
        ($h{alice} == 30 && $h{bob} == 25 && $h{carol} == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 9 captures ────────────────────────────────────────────────────

#[test]
fn nine_numbered_captures() {
    let code = r#"
        "1234567890" =~ /^(\d)(\d)(\d)(\d)(\d)(\d)(\d)(\d)(\d)/;
        ($1 == 1 && $5 == 5 && $9 == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Non-capturing prevents $N pollution ──────────────────────────

#[test]
fn non_capturing_does_not_consume_numeric_slot() {
    let code = r#"
        "abc123def" =~ /^(?:[a-z]+)(\d+)(?:[a-z]+)$/;
        # Only one capture; $1 = "123".
        ($1 == 123 && !defined($2)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
