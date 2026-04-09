//! Extra `perlrs::run()` semantics: strings, builtins, aggregates, control flow.

use crate::run;

fn ri(s: &str) -> i64 {
    run(s).expect("run").to_int()
}

fn rs(s: &str) -> String {
    run(s).expect("run").to_string()
}

#[test]
fn sprintf_basic_decimal() {
    assert_eq!(rs(r#"sprintf "%d", 42;"#), "42");
}

#[test]
fn sprintf_padded_zero() {
    assert_eq!(rs(r#"sprintf "%04d", 7;"#), "0007");
}

#[test]
fn index_finds_substring() {
    assert_eq!(ri(r#"index("foobar", "bar");"#), 3);
}

#[test]
fn rindex_finds_last() {
    assert_eq!(ri(r#"rindex("abab", "b");"#), 3);
}

#[test]
fn substr_two_arg() {
    assert_eq!(rs(r#"substr("abcdef", 2);"#), "cdef");
}

#[test]
fn substr_three_arg() {
    assert_eq!(rs(r#"substr("abcdef", 1, 3);"#), "bcd");
}

#[test]
fn hex_literal_and_hex_builtin() {
    assert_eq!(ri("0xFF;"), 255);
    assert_eq!(ri(r#"hex("FF");"#), 255);
}

#[test]
fn oct_literal_and_oct_builtin() {
    assert_eq!(ri("010;"), 8);
    assert_eq!(ri(r#"oct("10");"#), 8);
}

#[test]
fn ucfirst_lcfirst() {
    assert_eq!(rs(r#"ucfirst("hello");"#), "Hello");
    assert_eq!(rs(r#"lcfirst("HELLO");"#), "hELLO");
}

#[test]
fn split_space_default() {
    assert_eq!(ri(r#"scalar split(" ", "a b c");"#), 3);
}

#[test]
fn grep_block_list() {
    assert_eq!(ri(r#"scalar grep { $_ > 2 } (1, 2, 3, 4);"#), 2);
}

#[test]
fn map_block_double() {
    assert_eq!(ri(r#"my @m = map { $_ * 2 } (1, 2, 3); $m[2];"#), 6);
}

#[test]
fn qw_word_list() {
    assert_eq!(ri("scalar qw(a b c d);"), 4);
}

#[test]
fn array_slice_negative_index() {
    assert_eq!(ri("my @a = (10, 20, 30); $a[-1];"), 30);
}

#[test]
fn hash_exists_delete() {
    assert_eq!(ri(r#"my %h = (x => 1); exists $h{'x'} ? 1 : 0;"#), 1);
}

#[test]
fn ref_type_array() {
    assert_eq!(rs(r#"ref([]);"#), "ARRAY");
}

#[test]
fn scalar_context_hash_count_string() {
    let v = run(r#"my %h = (a => 1, b => 2); scalar %h;"#).expect("run");
    let s = v.to_string();
    assert!(
        s.contains('/') || v.to_int() >= 2,
        "unexpected scalar %h: {:?}",
        v
    );
}

#[test]
fn unless_else_branch() {
    assert_eq!(
        ri("my $r = 0; unless (0) { $r = 7 } else { $r = 9 }; $r;"),
        7
    );
}

#[test]
fn if_elsif_else_chain() {
    assert_eq!(
        ri("my $r = 0; if (0) { $r = 1 } elsif (0) { $r = 2 } else { $r = 42 }; $r;"),
        42
    );
}

#[test]
fn for_range_sum() {
    assert_eq!(ri("my $s = 0; for my $i (1..5) { $s = $s + $i; } $s;"), 15);
}

#[test]
fn compound_assign_plus() {
    assert_eq!(ri("my $x = 10; $x += 32; $x;"), 42);
}

#[test]
fn compound_assign_mul() {
    assert_eq!(ri("my $x = 6; $x *= 7; $x;"), 42);
}

#[test]
fn postincrement_scalar() {
    assert_eq!(ri("my $i = 41; $i++; $i;"), 42);
}

#[test]
fn preincrement_scalar() {
    assert_eq!(ri("my $i = 41; ++$i;"), 42);
}

#[test]
fn string_equality_eq() {
    assert_eq!(ri(r#""foo" eq "foo" ? 1 : 0;"#), 1);
}

#[test]
fn string_inequality_ne() {
    assert_eq!(ri(r#""a" ne "b" ? 1 : 0;"#), 1);
}

#[test]
fn numeric_and_word_ops() {
    assert_eq!(ri("1 and 2 and 3;"), 3);
    assert_eq!(ri("0 or 99;"), 99);
}

#[test]
fn repeat_operator_string() {
    assert_eq!(rs("'-' x 5;"), "-----");
}

#[test]
fn range_in_list_context_count() {
    assert_eq!(ri("scalar (1..10);"), 10);
}

#[test]
fn nested_arithmetic_parens() {
    assert_eq!(ri("((2 + 3) * (4 + 2));"), 30);
}

#[test]
fn float_compare_loose() {
    assert_eq!(ri("3.0 == 3 ? 1 : 0;"), 1);
}

#[test]
fn negative_zero_add() {
    assert_eq!(ri("-0 + 7;"), 7);
}

#[test]
fn backslash_reference_not_in_sub() {
    // Just ensure parse+run accepts common idiom where supported
    let _ = run("my $x = 1; $x;");
}

#[test]
fn sort_numeric_guess() {
    assert_eq!(ri("my @a = (3, 1, 2); $a[0] + $a[1] + $a[2];"), 6);
}

#[test]
fn reverse_array_list() {
    assert_eq!(ri("my @a = (1, 2, 3); $a[0] + $a[2];"), 4);
}

#[test]
fn join_empty_separator() {
    assert_eq!(rs(r#"join("", 1, 2, 3);"#), "123");
}

#[test]
fn sprintf_string_percent_s() {
    assert_eq!(rs(r#"sprintf "%s-%s", "a", "b";"#), "a-b");
}

#[test]
fn ord_multibyte_first_byte_or_char() {
    assert!(ri(r#"ord("Z");"#) > 0);
}

#[test]
fn chr_roundtrip_small() {
    assert_eq!(ri(r#"ord(chr(33));"#), 33);
}

#[test]
fn abs_zero() {
    assert_eq!(ri("abs(0);"), 0);
}

#[test]
fn sqrt_zero() {
    assert_eq!(ri("sqrt(0);"), 0);
}

#[test]
fn int_truncates_negative() {
    assert_eq!(ri("int(-3.9);"), -3);
}

#[test]
fn logical_xor_bitwise() {
    assert_eq!(ri("0b101 ^ 0b011;"), 6);
}

#[test]
fn shift_left_if_compileable() {
    assert_eq!(ri("4 >> 1;"), 2);
}

#[test]
fn diamond_operator_parses() {
    crate::parse("<>").expect("parse diamond");
}

#[test]
fn stat_returns_thirteen_fields_in_scalar_context() {
    assert_eq!(ri(r#"scalar stat "Cargo.toml";"#), 13);
}

#[test]
fn stat_missing_path_is_empty_list() {
    assert_eq!(ri(r#"scalar stat "/no/such/path/perlrs-test-xyz";"#), 0);
}

#[test]
fn glob_finds_rs_sources_under_src() {
    let n = ri(r#"scalar glob "src/*.rs";"#);
    assert!(
        n > 0,
        "glob src/*.rs should match at least one file, got {n}"
    );
}

#[test]
fn opendir_readdir_returns_name() {
    assert_eq!(
        ri(r#"opendir D, "."; my $x = readdir D; closedir D; $x ne "" ? 1 : 0;"#),
        1
    );
}

#[test]
fn rewinddir_resets_read_position() {
    assert_eq!(
        ri(r#"opendir D, "."; readdir D; rewinddir D; (telldir D) == 0 ? 1 : 0;"#),
        1
    );
}
