use crate::common::*;

#[test]
fn double_quoted_string_interpolates_double_dollar_as_pid() {
    let s = eval_string(r#""we got $$""#);
    assert!(
        s.starts_with("we got "),
        "expected PID interpolation, got {s:?}"
    );
    let pid_str = s.trim_start_matches("we got ");
    assert!(
        pid_str.chars().all(|c| c.is_ascii_digit()),
        "expected digits after 'we got ', got {s:?}"
    );
    assert_ne!(pid_str, "$$", "literal $$ should not appear");
}

#[test]
fn double_quoted_interpolates_dollar_caret_vars_and_punctuation_specials() {
    let o = eval_string(r#""x$^O""#);
    assert!(o.starts_with('x') && o.len() > 2, "expected $^O, got {o:?}");
    let bang = eval_string(r#""a$!b""#);
    assert_eq!(bang.chars().next(), Some('a'));
    assert_eq!(bang.chars().last(), Some('b'));
}

#[test]
fn double_quoted_whitespace_after_dollar_before_name() {
    assert_eq!(eval_string(r#"my $b = 42; "a$ b""#), "a42");
}

#[test]
fn qq_bracket_interpolates_at_plus_after_match() {
    // `@+` last-match offsets; array interpolates with `$"` (may be empty in perlrs → `"11"`).
    let s = eval_string(r#"$_ = "ab"; /(.)/; qq[@+]"#);
    assert!(
        s == "11" || s == "1 1",
        "expected @+ interpolation, got {s:?}"
    );
}

#[test]
fn double_quoted_array_interpolation_uses_list_separator_dollar_quote() {
    assert_eq!(eval_string(r#"my @a = (1,2,3); "<@a>""#), "<1 2 3>");
    assert_eq!(
        eval_string(r#"my @a = (1,2,3); $" = ":"; "<@a>""#),
        "<1:2:3>"
    );
}

#[test]
fn dollar_hash_array_last_index_reads_special_var() {
    assert_eq!(eval_string(r#"my @x = (10,20,30); "$#x""#), "2");
    assert_eq!(eval_string(r#"my @x = (); "$#x""#), "-1");
}

#[test]
fn double_quoted_dollar_only_whitespace_before_close_quote_is_parse_error() {
    assert!(perlrs::parse(r#"my $x = "a$ ""#).is_err());
}

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
fn double_quoted_at_array_joins_with_list_separator() {
    assert_eq!(eval_string(r#"my @a = qw(x y z); $" = ","; "@a""#), "x,y,z");
}

#[test]
fn double_quoted_array_slice_joins_with_list_separator() {
    assert_eq!(
        eval_string(r#"my @a = qw(a b c d); $" = ","; "@a[1..2]""#),
        "b,c"
    );
}

#[test]
fn double_quoted_array_slice_dollar_hash_last_index() {
    assert_eq!(
        eval_string(r#"my @a = qw(x y z); $" = ","; "@a[1..$#a]""#),
        "y,z"
    );
}

#[test]
fn double_quoted_at_f_joins_like_other_arrays() {
    assert_eq!(eval_string(r#"my @F = qw(p q r); $" = "|"; "@F""#), "p|q|r");
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
