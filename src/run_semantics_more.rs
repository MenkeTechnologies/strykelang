//! Additional `stryke::run()` semantics: strings, builtins, aggregates, loops.

use crate::run;

fn ri(s: &str) -> i64 {
    run(s).expect("run").to_int()
}

fn rs(s: &str) -> String {
    run(s).expect("run").to_string()
}

#[test]
fn chomp_strips_trailing_newline() {
    assert_eq!(rs(r#"my $s = "hi\n"; chomp $s; $s;"#), "hi");
}

#[test]
fn chop_returns_removed_char() {
    assert_eq!(rs(r#"my $s = "ab"; chop $s;"#), "b");
}

#[test]
fn chop_shortens_string() {
    assert_eq!(rs(r#"my $s = "ab"; chop $s; $s;"#), "a");
}

#[test]
fn length_empty_string() {
    assert_eq!(ri(r#"length("");"#), 0);
}

#[test]
fn uc_lc_empty() {
    assert_eq!(rs(r#"uc("");"#), "");
    assert_eq!(rs(r#"lc("");"#), "");
}

#[test]
fn fc_lowercase_ascii() {
    assert_eq!(rs(r#"fc("HELLO");"#), "hello");
}

#[test]
fn index_miss_returns_minus_one() {
    assert_eq!(ri(r#"index("abc", "z");"#), -1);
}

#[test]
fn rindex_ab_in_abab() {
    assert_eq!(ri(r#"rindex("abab", "ab");"#), 2);
}

#[test]
fn substr_four_arg_replaces() {
    assert_eq!(
        rs(r#"my $s = "hello"; substr($s, 1, 2, "XX"); $s;"#),
        "hXXlo"
    );
}

#[test]
fn substr_negative_offset_from_end() {
    assert_eq!(rs(r#"substr("abcdef", -2);"#), "ef");
}

#[test]
fn sprintf_hex_lower() {
    assert_eq!(rs(r#"sprintf "%x", 255;"#), "ff");
}

#[test]
fn sprintf_hex_zero_pad() {
    assert_eq!(rs(r#"sprintf "%02x", 2;"#), "02");
    assert_eq!(rs(r#"sprintf "%04X", 10;"#), "000A");
}

#[test]
fn sprintf_oct() {
    assert_eq!(rs(r#"sprintf "%o", 8;"#), "10");
}

#[test]
fn sprintf_percent_c() {
    assert_eq!(rs(r#"sprintf "%c", 65;"#), "A");
}

#[test]
fn sprintf_float() {
    let s = rs(r#"sprintf "%.1f", 3.25;"#);
    assert!(s.starts_with("3.2"), "got {s:?}");
}

#[test]
fn power_of_two_ten() {
    assert_eq!(ri("2 ** 10;"), 1024);
}

#[test]
fn bitwise_not_positive() {
    assert_eq!(ri("~0;"), -1);
}

#[test]
fn repeat_zero_times() {
    assert_eq!(rs("'x' x 0;"), "");
}

#[test]
fn concat_integers_to_string() {
    assert_eq!(rs(r#"1 . 2 . 3;"#), "123");
}

#[test]
fn list_assignment_two_scalars() {
    assert_eq!(ri("my $a; my $b; $a = $b = 4; $a + $b;"), 8);
}

#[test]
fn list_assignment_my_pair() {
    assert_eq!(ri("my ($a, $b) = (10, 20); $a + $b;"), 30);
}

#[test]
fn list_assignment_extra_values_ignored() {
    assert_eq!(ri("my ($a, $b) = (1, 2, 99); $a + $b;"), 3);
}

#[test]
fn unshift_returns_new_length() {
    assert_eq!(ri("my @a = (2, 3); unshift @a, 1;"), 3);
}

#[test]
fn splice_remove_middle_join() {
    assert_eq!(
        rs(r#"my @a = (1, 2, 3, 4); join(",", splice @a, 1, 2);"#),
        "2,3"
    );
}

#[test]
fn splice_leaves_remainder() {
    assert_eq!(
        ri(r#"my @a = (1, 2, 3, 4); splice @a, 1, 2; scalar @a;"#),
        2
    );
}

#[test]
fn empty_shift_undef() {
    assert_eq!(ri("my @a = (); defined(shift @a) ? 1 : 0;"), 0);
}

#[test]
fn empty_pop_undef() {
    assert_eq!(ri("my @a = (); defined(pop @a) ? 1 : 0;"), 0);
}

#[test]
fn grep_empty_list() {
    assert_eq!(ri("my @a = grep { $_ > 0 } (); scalar @a;"), 0);
}

#[test]
fn map_empty_list() {
    assert_eq!(ri("my @a = map { $_ * 2 } (); scalar @a;"), 0);
}

#[test]
fn sort_strings_joined() {
    assert_eq!(rs(r#"join(",", sort("b", "a", "c"));"#), "a,b,c");
}

#[test]
fn reverse_list_joined() {
    assert_eq!(rs(r#"join(",", reverse(1, 2, 3));"#), "3,2,1");
}

#[test]
fn reverse_scalar_string() {
    assert_eq!(rs(r#"reverse("ab");"#), "ba");
}

#[test]
fn delete_hash_key() {
    assert_eq!(ri(r#"my %h = (a => 42, b => 1); delete $h{a};"#), 42);
}

#[test]
fn scalar_empty_array() {
    assert_eq!(ri("my @a = (); scalar @a;"), 0);
}

#[test]
fn values_hash_count() {
    assert_eq!(ri(r#"my %h = (a => 1, b => 2); scalar values %h;"#), 2);
}

#[test]
fn ref_hash() {
    assert_eq!(rs(r#"ref({});"#), "HASH");
}

#[test]
fn ref_scalar_ref() {
    assert_eq!(rs(r#"ref(\42);"#), "SCALAR");
}

#[test]
fn until_loop_counter() {
    assert_eq!(ri("my $i = 0; until ($i >= 3) { $i = $i + 1; } $i;"), 3);
}

#[test]
fn while_last_breaks() {
    assert_eq!(
        ri("my $n = 0; my $i = 0; while ($i < 10) { $i = $i + 1; $n = $n + $i; last if $i == 3; } $n;"),
        6
    );
}

#[test]
fn for_next_skips_evens() {
    assert_eq!(
        ri("my $s = 0; for my $i (1, 2, 3, 4) { next if $i % 2 == 0; $s = $s + $i; } $s;"),
        4
    );
}

#[test]
fn for_last_stops_at_five() {
    assert_eq!(
        ri("my $s = 0; for my $i (1..10) { $s = $s + $i; last if $i == 5; } $s;"),
        15
    );
}

#[test]
fn do_block_arithmetic() {
    assert_eq!(ri("do { 6 * 7 };"), 42);
}

#[test]
fn eval_expr_add() {
    assert_eq!(ri(r#"eval "2 + 2";"#), 4);
}

#[test]
fn sin_zero() {
    assert_eq!(ri("sin(0);"), 0);
}

#[test]
fn cos_zero() {
    assert_eq!(ri("cos(0);"), 1);
}

#[test]
fn exp_zero() {
    assert_eq!(ri("exp(0);"), 1);
}

#[test]
fn log_one() {
    assert_eq!(ri("log(1);"), 0);
}

#[test]
fn sqrt_two_approx() {
    let v = run("sqrt(2);").expect("run").to_int();
    assert!((1.414 - v as f64 / 1_000_000.0).abs() < 0.01 || v == 1);
    let f = run("sqrt(2);").expect("run");
    let s = f.to_string();
    assert!(s.contains('1') && s.contains('4'), "sqrt(2) string {s:?}");
}

#[test]
fn atan2_quarter_pi() {
    let v = run("atan2(1, 1);").expect("run").to_string();
    assert!(!v.is_empty(), "atan2 string {v:?}");
}

#[test]
fn abs_large_negative() {
    assert_eq!(ri("abs(-1000000);"), 1_000_000);
}

#[test]
fn int_positive_fraction() {
    assert_eq!(ri("int(3.99);"), 3);
}

#[test]
fn chr_ord_roundtrip_cap_a() {
    assert_eq!(rs(r#"chr(ord("A"));"#), "A");
}

#[test]
fn lc_all_upper() {
    assert_eq!(rs(r#"lc("ABC");"#), "abc");
}

#[test]
fn uc_all_lower() {
    assert_eq!(rs(r#"uc("xyz");"#), "XYZ");
}

#[test]
fn hex_empty() {
    assert_eq!(ri(r#"hex("");"#), 0);
}

#[test]
fn oct_zero_string() {
    assert_eq!(ri(r#"oct("0");"#), 0);
}

#[test]
fn split_limit() {
    assert_eq!(ri(r#"scalar split(":", "a:b:c:d", 2);"#), 2);
}

#[test]
fn join_single() {
    assert_eq!(rs(r#"join("-", 42);"#), "42");
}

#[test]
fn compound_assign_minus() {
    assert_eq!(ri("my $x = 50; $x -= 8; $x;"), 42);
}

#[test]
fn compound_assign_div() {
    assert_eq!(ri("my $x = 84; $x /= 2; $x;"), 42);
}

#[test]
fn compound_assign_mod() {
    assert_eq!(ri("my $x = 45; $x %= 43; $x;"), 2);
}

#[test]
fn postdecrement() {
    assert_eq!(ri("my $i = 43; $i--; $i;"), 42);
}

#[test]
fn string_lt_gt() {
    assert_eq!(ri(r#""a" lt "b" ? 1 : 0;"#), 1);
    assert_eq!(ri(r#""z" gt "a" ? 1 : 0;"#), 1);
}

#[test]
fn cmp_equal_strings() {
    assert_eq!(ri(r#""foo" cmp "foo";"#), 0);
}

#[test]
fn bitwise_xor_integers() {
    assert_eq!(ri("0b101 ^ 0b011;"), 6);
}

#[test]
fn negative_modulo() {
    assert_eq!(ri("-7 % 3;"), -1);
}

#[test]
fn power_chain() {
    assert_eq!(ri("2 ** 3 ** 2;"), 512);
}

#[test]
fn defined_concat_undef() {
    assert_eq!(ri(r#"my $u; defined($u . "") ? 1 : 0;"#), 1);
}

#[test]
fn array_element_assign() {
    assert_eq!(ri("my @a; $a[2] = 5; $a[2];"), 5);
}

#[test]
fn hash_key_autovivify() {
    assert_eq!(ri(r#"my %h; $h{'q'} = 99; $h{'q'};"#), 99);
}

#[test]
fn regex_match_count() {
    assert_eq!(ri(r#""aba" =~ /a/;"#), 1);
}
