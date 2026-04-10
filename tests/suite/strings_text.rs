use crate::common::*;

#[test]
fn qq_backslash_dollar_in_eval_string_is_literal_sigil() {
    // CPAN JSON::PP-style: `eval qq/my \$x = 42/` must compile as `my $x`, not outer `$x`.
    assert_eq!(
        eval_int(r#"no strict 'vars'; eval qq/my \$x = 42/; $x"#),
        42
    );
}

#[test]
fn string_operations() {
    assert_eq!(eval_string(r#"uc("hello")"#), "HELLO");
    assert_eq!(eval_string(r#"lc("HELLO")"#), "hello");
    assert_eq!(eval_string(r#"ucfirst("hello")"#), "Hello");
    assert_eq!(eval_string(r#"lcfirst("Hello")"#), "hello");
    assert_eq!(eval_int(r#"length("hello")"#), 5);
    assert_eq!(eval_string(r#"substr("hello", 1, 3)"#), "ell");
    assert_eq!(eval_int(r#"index("hello world", "world")"#), 6);
    assert_eq!(eval_int(r#"rindex("abcbc", "bc")"#), 3);
}

#[test]
fn index_with_start_position() {
    assert_eq!(eval_int(r#"index("hello world", "l", 4)"#), 9);
}

#[test]
fn substr_negative_offset() {
    assert_eq!(eval_string(r#"substr("abcde", -2)"#), "de");
}

#[test]
fn substr_replacement() {
    assert_eq!(
        eval_string(r#"my $s = "hello"; substr($s, 0, 2, "XX"); $s"#),
        "XXllo"
    );
}

#[test]
fn qw_word_list() {
    assert_eq!(eval_string(r#"join(",", qw(a bb ccc))"#), "a,bb,ccc");
}

#[test]
fn chomp_chop() {
    assert_eq!(eval_string(r#"my $s = "hi\n"; chomp $s; $s"#), "hi");
    assert_eq!(eval_string(r#"my $s = "ab"; chop $s"#), "b");
    assert_eq!(eval_string(r#"my $s = "ab"; chop $s; $s"#), "a");
}

#[test]
fn sprintf_basic() {
    assert_eq!(eval_string(r#"sprintf("%d", 42)"#), "42");
    assert_eq!(eval_string(r#"sprintf("%d-%s", 7, "z")"#), "7-z");
}

#[test]
fn sprintf_zero_padding() {
    assert_eq!(eval_string(r#"sprintf("%04d", 7)"#), "0007");
}

#[test]
fn sprintf_float_rounding() {
    assert_eq!(eval_string(r#"sprintf("%.0f", 3.7)"#), "4");
}

#[test]
fn sqrt_builtin() {
    assert_eq!(eval_int("sqrt(25)"), 5);
}

#[test]
fn string_concatenation() {
    assert_eq!(eval_string(r#""hello" . " " . "world""#), "hello world");
}

#[test]
fn string_repetition() {
    assert_eq!(eval_string(r#""ab" x 3"#), "ababab");
}

#[test]
fn string_comparison() {
    assert_eq!(eval_int(r#""abc" eq "abc""#), 1);
    assert_eq!(eval_int(r#""abc" ne "def""#), 1);
    assert_eq!(eval_int(r#""abc" lt "def""#), 1);
}

#[test]
fn length_of_array() {
    assert_eq!(eval_int("my @a = (1,2,3); length @a"), 3);
}

#[test]
fn file_test_exists() {
    assert_eq!(eval_int("-e '.'"), 1);
    assert_eq!(eval_int("-e '/nonexistent_path_xyz_12345'"), 0);
}

#[test]
fn string_interpolation_hash_access() {
    assert_eq!(eval_string(r#"my %h = (x => 42); "$h{x}""#), "42");
}

#[test]
fn string_interpolation_array_access() {
    assert_eq!(eval_string(r#"my @a = (10, 20, 30); "$a[1]""#), "20");
}

#[test]
fn string_interpolation_regexp_captures() {
    assert_eq!(
        eval_string(r#"my $s = "a-b"; $s =~ /(.)-(.)/; "1=$1 2=$2""#),
        "1=a 2=b"
    );
    assert_eq!(
        eval_string(r#"my $s = "abcdefghij"; $s =~ /(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)/; "$10""#),
        "j"
    );
}

#[test]
fn split_join() {
    assert_eq!(eval_string(r#"join("-", split(",", "a,b,c"))"#), "a-b-c");
}

#[test]
fn split_with_regex_pattern_delimiter() {
    assert_eq!(eval_string(r#"join("-", split(/,/, "a,b,c"))"#), "a-b-c");
}

#[test]
fn split_with_limit() {
    assert_eq!(
        eval_string(r#"join("-", split(",", "a,b,c,d", 2))"#),
        "a-b,c,d"
    );
}

#[test]
fn join_empty_list() {
    assert_eq!(eval_string(r#"join(",", ())"#), "");
}

#[test]
fn reverse_string() {
    assert_eq!(eval_string(r#"reverse("abc")"#), "cba");
}
