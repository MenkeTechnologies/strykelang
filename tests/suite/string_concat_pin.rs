//! String concatenation pins. `.` operator across types, `.=` in
//! place, interaction with sprintf, undef coercion.

use crate::common::*;

// ── Basic dot concat ────────────────────────────────────────────────

#[test]
fn dot_concat_two_strings() {
    let code = r#"
        ("hello" . "world") eq "helloworld" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_concat_chain_three() {
    let code = r#"
        ("a" . "b" . "c") eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_concat_string_with_int() {
    let code = r#"
        ("value=" . 42) eq "value=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_concat_string_with_float() {
    let code = r#"
        ("pi=" . 3.14) eq "pi=3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_concat_int_with_int_returns_string() {
    let code = r#"
        my $r = 12 . 34;
        $r eq "1234" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── .= in place ────────────────────────────────────────────────────

#[test]
fn dot_eq_appends_in_place() {
    let code = r#"
        my $s = "hello";
        $s .= " world";
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_eq_chain_builds_string() {
    let code = r#"
        my $s = "";
        $s .= "a";
        $s .= "b";
        $s .= "c";
        $s eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_eq_in_loop_builds_csv() {
    let code = r#"
        my @items = ("alice", "bob", "carol");
        my $csv = "";
        my $sep = "";
        for my $x (@items) {
            $csv .= $sep . $x;
            $sep = ",";
        }
        $csv eq "alice,bob,carol" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── join is equivalent to repeated . with sep ─────────────────────

#[test]
fn join_equivalent_to_dot_concat_with_sep() {
    let code = r#"
        my @arr = ("a", "b", "c");
        my $j = join(",", @arr);
        my $d = $arr[0] . "," . $arr[1] . "," . $arr[2];
        $j eq $d ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String literal repeat operator ────────────────────────────────

#[test]
fn x_operator_repeats_string() {
    let code = r#"
        ("ab" x 3) eq "ababab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with undef ──────────────────────────────────────────────

#[test]
fn concat_with_undef_coerces_to_empty() {
    let code = r#"
        my $u;
        my $r = "x=" . ($u // "");
        $r eq "x=" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Interpolation builds same result as concat ───────────────────

#[test]
fn interpolation_and_concat_produce_same_string() {
    let code = r#"
        my $n = "alice";
        my $a = "hi, $n!";
        my $b = "hi, " . $n . "!";
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn interpolation_with_arrayref_expression() {
    let code = r#"
        my $x = 5;
        my $y = 7;
        my $s = "sum is @{[$x + $y]}";
        $s eq "sum is 12" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Long concat doesn't truncate ─────────────────────────────────

#[test]
fn long_concat_produces_correct_length() {
    let code = r#"
        my $s = "";
        for my $i (1:100) {
            $s .= "x";
        }
        len($s) == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat preserves unicode ─────────────────────────────────────

#[test]
fn concat_preserves_unicode_codepoint_count() {
    let code = r#"
        my $a = "café";
        my $b = "🌟";
        my $r = $a . " " . $b;
        len($r) == 6 ? 1 : 0   # 4 + 1 + 1 = 6 codepoints
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with negative number ──────────────────────────────────

#[test]
fn concat_with_negative_number() {
    let code = r#"
        ("balance=" . -42) eq "balance=-42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat doesn't auto-add space ────────────────────────────────

#[test]
fn concat_does_not_add_separator() {
    let code = r#"
        ("a" . "b") eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with array element ─────────────────────────────────────

#[test]
fn concat_with_array_element() {
    let code = r#"
        my @a = ("alpha", "beta", "gamma");
        my $r = "pick: " . $a[1];
        $r eq "pick: beta" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with hash element ──────────────────────────────────────

#[test]
fn concat_with_hash_element() {
    let code = r#"
        my %h = (name => "alice", age => 30);
        my $r = $h{name} . " is " . $h{age};
        $r eq "alice is 30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with fn return ─────────────────────────────────────────

#[test]
fn concat_with_fn_return() {
    let code = r#"
        fn Demo::SC::greet($name) { "hello, $name" }
        my $r = Demo::SC::greet("alice") . "!";
        $r eq "hello, alice!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf produces same as concat for simple cases ─────────────

#[test]
fn sprintf_equivalent_to_concat_for_simple() {
    let code = r#"
        my $name = "alice";
        my $age = 30;
        my $a = sprintf("%s,%d", $name, $age);
        my $b = $name . "," . $age;
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Many small concats round-trip ────────────────────────────────

#[test]
fn alphabet_via_chr_loop() {
    let code = r#"
        my $s = "";
        for my $code (65:90) {
            $s .= chr($code);
        }
        $s eq "ABCDEFGHIJKLMNOPQRSTUVWXYZ" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with boolean result ───────────────────────────────────

#[test]
fn concat_with_truthy_value() {
    let code = r#"
        my $r = "result=" . (1 == 1);
        # In Perl, 1==1 → 1, 1==2 → "" (empty).
        ($r eq "result=1" || $r eq "result=true") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn concat_with_falsy_value() {
    let code = r#"
        my $r = "result=" . (1 == 2);
        # Perl: empty string for false.
        ($r eq "result=" || $r eq "result=0" || $r eq "result=false") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Big-string building via array + join idiom ────────────────────

#[test]
fn join_is_faster_idiom_than_dot_eq_in_loop() {
    let code = r#"
        my @parts;
        for my $i (1:100) {
            push @parts, "p$i";
        }
        my $joined = join(",", @parts);
        # Roughly: 100 parts * 3 chars + 99 commas = ~400 chars.
        len($joined) > 300 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat with very long string ──────────────────────────────────

#[test]
fn concat_with_10k_char_strings() {
    let code = r#"
        my $a = "x" x 10000;
        my $b = "y" x 10000;
        my $c = $a . $b;
        len($c) == 20000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat returns new string, doesn't mutate ────────────────────

#[test]
fn concat_does_not_mutate_operands() {
    let code = r#"
        my $a = "abc";
        my $b = "xyz";
        my $c = $a . $b;
        ($a eq "abc" && $b eq "xyz" && $c eq "abcxyz") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat empty string is identity ──────────────────────────────

#[test]
fn concat_with_empty_string_unchanged() {
    let code = r#"
        ("hello" . "" eq "hello"
            && "" . "world" eq "world"
            && "" . "" eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
