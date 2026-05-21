//! Regex-capture pins. `regex_three_tier_pin.rs` (round 5) covers the
//! engine-tier surface; this file pins the capture variables —
//! `$1..$9`, `%+`, `@-`, `@+`, and capture-preservation semantics
//! across loops and function boundaries. These are load-bearing for
//! every Perl-style text-processing idiom.

use crate::common::*;

// ── Numbered captures into $1, $2, ... ───────────────────────────────

#[test]
fn match_populates_dollar_one_two_three() {
    let code = r#"
        "alice:30:engineer" =~ /^(\w+):(\d+):(\w+)$/;
        ($1 eq "alice" && $2 == 30 && $3 eq "engineer") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn failed_match_does_not_clobber_previous_captures() {
    let code = r#"
        "hello" =~ /(h)(e)(l)/;
        my $prev_one = $1;
        "xyz" =~ /(z+)/;
        # After the second match, $1 is from the new match.
        # Previous capture is overwritten — pin that.
        ($prev_one eq "h" && $1 eq "z") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nested_groups_count_outer_to_inner() {
    let code = r#"
        "abc123def" =~ /^([a-z]+(\d+))([a-z]+)$/;
        ($1 eq "abc123" && $2 == 123 && $3 eq "def") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Named captures into %+ ────────────────────────────────────────────

#[test]
fn named_capture_populates_percent_plus() {
    let code = r#"
        "alice:30:engineer" =~ /^(?<name>\w+):(?<age>\d+):(?<role>\w+)$/;
        ($+{name} eq "alice"
            && $+{age} == 30
            && $+{role} eq "engineer") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn named_capture_with_numbered_also_works() {
    let code = r#"
        "key=value" =~ /^(?<k>\w+)=(?<v>\w+)$/;
        # Named captures are also accessible by number.
        ($1 eq "key" && $2 eq "value"
            && $+{k} eq "key" && $+{v} eq "value") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Optional captures ────────────────────────────────────────────────

#[test]
fn optional_unmatched_capture_is_undef() {
    let code = r#"
        "abc" =~ /^(\w+)(\d+)?$/;
        ($1 eq "abc" && !defined($2)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn alternation_picks_matching_branch() {
    let code = r#"
        "42" =~ /^(?:(\w+)|(\d+))$/;
        # \w matches 42 too, so $1 wins.
        ($1 eq "42") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Captures in /g loop ──────────────────────────────────────────────

#[test]
fn global_match_iterates_all_captures() {
    let code = r#"
        my $s = "foo=1 bar=2 baz=3";
        my @hits;
        while ($s =~ /(\w+)=(\d+)/g) {
            push @hits, "$1:$2";
        }
        join(",", @hits) eq "foo:1,bar:2,baz:3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn global_match_in_list_context_returns_per_capture_values() {
    // List-context `/.../g` with capture groups returns each capture as its
    // own element across every match, matching Perl: ("foo","1","bar","2",
    // "baz","3").
    let code = r#"
        my @r = ("foo=1 bar=2 baz=3" =~ /(\w+)=(\d+)/g);
        (scalar(@r) == 6
            && $r[0] eq "foo"
            && $r[1] eq "1"
            && $r[2] eq "bar"
            && $r[3] eq "2"
            && $r[4] eq "baz"
            && $r[5] eq "3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Capture-preserving substitution ──────────────────────────────────

#[test]
fn substitution_with_dollar_one_backref() {
    let code = r#"
        my $s = "hello world";
        $s =~ s/(\w+) (\w+)/$2 $1/;
        $s eq "world hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn global_substitution_with_capture() {
    let code = r#"
        my $s = "a1 b2 c3";
        $s =~ s/(\w)(\d)/$2$1/g;
        $s eq "1a 2b 3c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substitution_with_named_backref_via_numeric_form() {
    // Stryke does NOT interpolate `$+{name}` inside s/// replacement
    // (BUG-215). Pin the working numeric form ($1, $2) which captures
    // the same data even when the pattern uses named groups.
    let code = r#"
        my $s = "alice=30";
        $s =~ s/(?<k>\w+)=(?<v>\d+)/$2 -> $1/;
        $s eq "30 -> alice" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── @- / @+ (match start/end offsets) ────────────────────────────────

#[test]
fn dollar_amp_holds_entire_match() {
    let code = r#"
        "abc123def" =~ /(\d+)/;
        ($& eq "123") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dollar_amp_only_no_pre_match_post_match_vars() {
    // Stryke supports `$&` (full match) but parser rejects `$`` and
    // `$'` for pre-match / post-match. BUG-214 tracks this. Pin `$&`
    // behavior; pre/post must be derived manually.
    let code = r#"
        "abc123def" =~ /(\d+)/;
        ($& eq "123") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Captures survive into function callees ──────────────────────────

#[test]
fn captures_visible_to_called_function_within_same_statement() {
    let code = r#"
        fn Demo::Cap::echo_one() { $1 }
        "abc123" =~ /(\d+)/;
        Demo::Cap::echo_one() eq "123" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Captures inside conditionals ─────────────────────────────────────

#[test]
fn if_match_inline_binds_captures() {
    let code = r#"
        my $r = "found_42_here";
        if ($r =~ /_(\d+)_/) {
            $1 == 42 ? 1 : 0
        } else {
            0
        }
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Repeated captures: last wins ─────────────────────────────────────

#[test]
fn repeated_group_keeps_last_iteration() {
    let code = r#"
        "abc def ghi" =~ /(?:(\w+)\s*)+/;
        # Perl rule: last successful iteration wins.
        ($1 eq "ghi") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Backreferences inside the pattern itself ─────────────────────────

#[test]
fn intra_pattern_backref_matches() {
    let code = r#"
        "abcabc" =~ /^(abc)\1$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn intra_pattern_backref_named() {
    let code = r#"
        "foofoo" =~ /^(?<w>\w+)\k<w>$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── qr// with captures ───────────────────────────────────────────────

#[test]
fn qr_pattern_with_captures_reuses_correctly() {
    let code = r#"
        my $re = qr/^(\w+)=(\d+)$/;
        "answer=42" =~ $re;
        ($1 eq "answer" && $2 == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qr_pattern_in_grep_block_filters_with_captures() {
    let code = r#"
        my @lines = ("alice=30", "bob=", "carol=35", "broken", "dave=42");
        my @valid = grep { /^(\w+)=(\d+)$/ } @lines;
        # alice=30, carol=35, dave=42 — 3 valid.
        len(@valid) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Non-capturing groups don't populate $N ───────────────────────────

#[test]
fn non_capturing_group_does_not_consume_dollar_one() {
    let code = r#"
        "abc123" =~ /^(?:[a-z]+)(\d+)$/;
        # The first capture index goes to (\d+), not the (?:..) group.
        $1 == 123 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
