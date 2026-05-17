//! Extra compound-assign coverage for `$h->{k} OP= EXPR`, beyond the
//! `+=` / `-=` / `*=` pins already in `hashref_assignment_pin.rs`.
//!
//! The `Op::SetArrowHashKeep` fix (May 2026) ensures every compound-op
//! path leaves the new value on the stack so the statement-level `Pop`
//! discards a real value rather than corrupting the caller frame. These
//! cases pin the less-common operators so a future regression in any of
//! them is caught.

use crate::common::*;

// ── arithmetic ───────────────────────────────────────────────────────────────

#[test]
fn arrow_hash_div_eq() {
    let n = eval_int(
        r#"
        my $h = +{n => 20}
        $h->{n} /= 4
        $h->{n}
        "#,
    );
    assert_eq!(n, 5);
}

#[test]
fn arrow_hash_mod_eq() {
    let n = eval_int(
        r#"
        my $h = +{n => 25}
        $h->{n} %= 7
        $h->{n}
        "#,
    );
    assert_eq!(n, 4);
}

#[test]
fn arrow_hash_pow_eq() {
    let n = eval_int(
        r#"
        my $h = +{n => 5}
        $h->{n} **= 2
        $h->{n}
        "#,
    );
    assert_eq!(n, 25);
}

// ── bitwise ──────────────────────────────────────────────────────────────────

#[test]
fn arrow_hash_shl_eq() {
    let n = eval_int(
        r#"
        my $h = +{n => 1}
        $h->{n} <<= 4
        $h->{n}
        "#,
    );
    assert_eq!(n, 16);
}

#[test]
fn arrow_hash_shr_eq() {
    let n = eval_int(
        r#"
        my $h = +{n => 64}
        $h->{n} >>= 3
        $h->{n}
        "#,
    );
    assert_eq!(n, 8);
}

#[test]
fn arrow_hash_or_eq() {
    let n = eval_int(
        r#"
        my $h = +{flags => 0b0101}
        $h->{flags} |= 0b1010
        $h->{flags}
        "#,
    );
    assert_eq!(n, 0b1111);
}

#[test]
fn arrow_hash_and_eq() {
    let n = eval_int(
        r#"
        my $h = +{flags => 0b1111}
        $h->{flags} &= 0b1010
        $h->{flags}
        "#,
    );
    assert_eq!(n, 0b1010);
}

#[test]
fn arrow_hash_xor_eq() {
    let n = eval_int(
        r#"
        my $h = +{flags => 0b1100}
        $h->{flags} ^= 0b1010
        $h->{flags}
        "#,
    );
    assert_eq!(n, 0b0110);
}

// ── string concat ────────────────────────────────────────────────────────────

#[test]
fn arrow_hash_concat_eq() {
    let s = eval_string(
        r#"
        my $h = +{msg => "hello"}
        $h->{msg} .= ", world"
        $h->{msg}
        "#,
    );
    assert_eq!(s, "hello, world");
}

// ── defined-or / logical-or ──────────────────────────────────────────────────

#[test]
fn arrow_hash_defined_or_eq_initializes_undef() {
    let n = eval_int(
        r#"
        my $h = +{}
        $h->{counter} //= 99
        $h->{counter}
        "#,
    );
    assert_eq!(n, 99);
}

#[test]
fn arrow_hash_defined_or_eq_keeps_existing_zero() {
    // 0 is defined → //= must NOT overwrite.
    let n = eval_int(
        r#"
        my $h = +{counter => 0}
        $h->{counter} //= 99
        $h->{counter}
        "#,
    );
    assert_eq!(n, 0);
}

#[test]
fn arrow_hash_logical_or_eq_overwrites_falsy_zero() {
    // 0 is falsy → ||= overwrites.
    let n = eval_int(
        r#"
        my $h = +{counter => 0}
        $h->{counter} ||= 99
        $h->{counter}
        "#,
    );
    assert_eq!(n, 99);
}

// ── multi-call still uses each compound op safely (no caller-frame corruption) ──

#[test]
fn arrow_hash_div_eq_in_multi_call_expression() {
    // The original `Pop` bug surfaced as wrong arithmetic when the same
    // function called multiple times in one expression mutated a hashref
    // via compound-assign. Pin /= here.
    let n = eval_int(
        r#"
        fn FOO::halve($h) { $h->{v} /= 2; $h->{v} }
        my $h = +{v => 256}
        FOO::halve($h) + FOO::halve($h) + FOO::halve($h)
        "#,
    );
    // 128 + 64 + 32 = 224
    assert_eq!(n, 224);
}

#[test]
fn arrow_hash_concat_eq_in_multi_call_expression() {
    let s = eval_string(
        r#"
        fn FOO::append_x($h) { $h->{s} .= "x"; $h->{s} }
        my $h = +{s => ""}
        FOO::append_x($h) . FOO::append_x($h) . FOO::append_x($h)
        "#,
    );
    // After each call: "x", then "xx", then "xxx". Concat = "x" . "xx" . "xxx" = "xxxxxx"
    assert_eq!(s, "xxxxxx");
}

// ── nested arrow path: `$h->{a}{b} OP= …` ───────────────────────────────────

#[test]
fn nested_arrow_hash_compound_assign() {
    let n = eval_int(
        r#"
        my $h = +{outer => +{}}
        $h->{outer}{inner} = 0
        $h->{outer}{inner} += 7
        $h->{outer}{inner} *= 3
        $h->{outer}{inner}
        "#,
    );
    // 0 + 7 = 7, then 7 * 3 = 21
    assert_eq!(n, 21);
}
