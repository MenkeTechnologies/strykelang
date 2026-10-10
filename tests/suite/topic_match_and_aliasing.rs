//! Native-mode (non-`--compat`) regressions for the topic-match and
//! foreach-aliasing fixes; the `--compat` side is pinned byte-for-byte against
//! perl in `parity/cases/20067..20073`.

use crate::common::*;

#[test]
fn foreach_over_frozen_array_reads_every_element() {
    // The alias write-back must not try to store into a `val` array: it used
    // to die with "cannot modify frozen array" after the first iteration.
    assert_eq!(
        eval_string(r#"val @a = (1, 2, 3); my $s = ""; for (@a) { $s .= $_ } $s"#),
        "123"
    );
    assert_eq!(
        eval_string(r#"val @a = (1, 2, 3); my $s = ""; $s .= $_ for @a; $s"#),
        "123"
    );
}

#[test]
fn postfix_for_writes_topic_back_and_restores_it() {
    assert_eq!(
        eval_string(r#"my @a = (1, 2, 3); $_ *= 10 for @a; join(",", @a)"#),
        "10,20,30"
    );
    assert_eq!(
        eval_string(r#"$_ = "keep"; my $n = 0; $n += $_ for 1..3; "$_ $n""#),
        "keep 6"
    );
}

#[test]
fn while_match_g_advances_pos() {
    assert_eq!(
        eval_string(r#"$_ = "a1b2c3"; my @d; while (/(\d)/g) { push @d, $1 } join(",", @d)"#),
        "1,2,3"
    );
}

#[test]
fn map_block_ending_in_an_array_yields_its_elements() {
    assert_eq!(
        eval_string(r#"join(",", map { my @p = ($_, $_ * 2); @p } 1, 2)"#),
        "1,2,2,4"
    );
}

#[test]
fn dynamic_bind_match_in_list_context_returns_captures() {
    assert_eq!(
        eval_string(r#"my $re = qr/(\w)=(\d)/; my ($k, $v) = "a=5" =~ $re; "$k$v""#),
        "a5"
    );
}

#[test]
fn interpolated_qr_keeps_its_own_flags() {
    assert_eq!(
        eval_string(r#"my $re = qr/ab/i; "xAB" =~ /x$re/ ? "y" : "n""#),
        "y"
    );
    assert_eq!(
        eval_string(r#"my $re = qr/ab/; "xAB" =~ /x$re/i ? "y" : "n""#),
        "n"
    );
    assert_eq!(eval_string(r#"my $re = qr/ab/i; "$re""#), "(?^i:ab)");
}

#[test]
fn regex_builtins_accept_a_stringified_qr_value() {
    // `qr//` reaches a builtin as Perl's `(?^FLAGS:...)` text; the Rust regex
    // engine rejects the `^` group unless it is rewritten first.
    assert_eq!(
        eval_string(r#"assert_match(qr/^\d+\.\d+\.\d+$/, "1.2.3")"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"is_match(qr/ab/i, "xAB") . is_match(qr/ab/, "xAB")"#),
        "10"
    );
    assert_eq!(
        eval_string(r#"join ",", match_all(qr/\d+/, "a1b22c333")"#),
        "1,22,333"
    );
}
