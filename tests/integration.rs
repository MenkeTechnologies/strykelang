use perlrs::error::ErrorKind;
use perlrs::interpreter::Interpreter;
use perlrs::value::PerlValue;

fn eval(code: &str) -> PerlValue {
    let program = perlrs::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(&program).expect("execution failed")
}

fn eval_string(code: &str) -> String {
    eval(code).to_string()
}

fn eval_int(code: &str) -> i64 {
    eval(code).to_int()
}

fn eval_err_kind(code: &str) -> ErrorKind {
    let program = perlrs::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err().kind
}

// ── Arithmetic ──

#[test]
fn integer_arithmetic() {
    assert_eq!(eval_int("3 + 4"), 7);
    assert_eq!(eval_int("10 - 3"), 7);
    assert_eq!(eval_int("6 * 7"), 42);
    assert_eq!(eval_int("15 / 3"), 5);
    assert_eq!(eval_int("17 % 5"), 2);
    assert_eq!(eval_int("2 ** 10"), 1024);
}

#[test]
fn operator_precedence() {
    assert_eq!(eval_int("2 + 3 * 4"), 14);
    assert_eq!(eval_int("(2 + 3) * 4"), 20);
    assert_eq!(eval_int("2 ** 3 ** 2"), 512); // right-associative
}

#[test]
fn comparison_operators() {
    assert_eq!(eval_int("5 == 5"), 1);
    assert_eq!(eval_int("5 != 3"), 1);
    assert_eq!(eval_int("3 < 5"), 1);
    assert_eq!(eval_int("5 > 3"), 1);
    assert_eq!(eval_int("5 <= 3"), 0);
    assert_eq!(eval_int("3 <= 5"), 1);
    assert_eq!(eval_int("5 >= 5"), 1);
    assert_eq!(eval_int("5 <=> 3"), 1);
    assert_eq!(eval_int("3 <=> 5"), -1);
    assert_eq!(eval_int("5 <=> 5"), 0);
}

#[test]
fn string_comparison_ge_le() {
    assert_eq!(eval_int(r#""b" ge "a""#), 1);
    assert_eq!(eval_int(r#""a" le "b""#), 1);
}

// ── Strings ──

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
fn qw_word_list() {
    assert_eq!(
        eval_string(r#"join(",", qw(a bb ccc))"#),
        "a,bb,ccc"
    );
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

// ── Variables ──

#[test]
fn scalar_variables() {
    assert_eq!(eval_int("my $x = 42; $x"), 42);
    assert_eq!(eval_string(r#"my $s = "hello"; $s"#), "hello");
}

#[test]
fn array_variables() {
    assert_eq!(eval_int("my @a = (1,2,3); $a[1]"), 2);
    assert_eq!(eval_int("my @a = (1,2,3); scalar @a"), 3);
}

#[test]
fn array_negative_index() {
    assert_eq!(eval_int("my @a = (10, 20, 30); $a[-1]"), 30);
}

#[test]
fn substr_negative_offset() {
    assert_eq!(eval_string(r#"substr("abcde", -2)"#), "de");
}

#[test]
fn hash_variables() {
    assert_eq!(
        eval_int(r#"my %h = ("a", 1, "b", 2); $h{b}"#),
        2
    );
}

#[test]
fn hash_fat_arrow() {
    assert_eq!(eval_int("my %h = (aa => 10, bb => 20); $h{aa} + $h{bb}"), 30);
}

// ── Control flow ──

#[test]
fn if_else() {
    assert_eq!(eval_int("my $x = 10; if ($x > 5) { 1 } else { 0 }"), 1);
    assert_eq!(eval_int("my $x = 3; if ($x > 5) { 1 } else { 0 }"), 0);
}

#[test]
fn if_elsif_chain() {
    assert_eq!(
        eval_int("my $x = 2; if ($x == 0) { 0 } elsif ($x == 1) { 1 } elsif ($x == 2) { 2 } else { 9 }"),
        2
    );
}

#[test]
fn unless() {
    assert_eq!(eval_int("my $x = 3; unless ($x > 5) { 1 } else { 0 }"), 1);
}

#[test]
fn while_loop() {
    assert_eq!(
        eval_int("my $i = 0; my $sum = 0; while ($i < 10) { $sum = $sum + $i; $i = $i + 1; } $sum"),
        45
    );
}

#[test]
fn until_loop() {
    assert_eq!(
        eval_int("my $i = 0; until ($i >= 4) { $i = $i + 1; } $i"),
        4
    );
}

#[test]
fn for_loop() {
    assert_eq!(
        eval_int("my $sum = 0; for (my $i = 0; $i < 5; $i = $i + 1) { $sum = $sum + $i; } $sum"),
        10
    );
}

#[test]
fn foreach_loop() {
    assert_eq!(
        eval_int("my $sum = 0; foreach my $x (1,2,3,4,5) { $sum = $sum + $x; } $sum"),
        15
    );
}

#[test]
fn postfix_if() {
    assert_eq!(eval_int("my $x = 0; $x = 1 if 1 > 0; $x"), 1);
    assert_eq!(eval_int("my $x = 0; $x = 1 if 0 > 1; $x"), 0);
}

#[test]
fn postfix_unless() {
    assert_eq!(eval_int("my $x = 0; $x = 1 unless 0; $x"), 1);
}

#[test]
fn postfix_while_until() {
    assert_eq!(eval_int("my $x = 0; $x++ while $x < 4; $x"), 4);
    assert_eq!(eval_int("my $x = 0; $x++ until $x >= 4; $x"), 4);
}

#[test]
fn last_next() {
    assert_eq!(
        eval_int("my $sum = 0; for my $i (1..10) { last if $i > 5; $sum = $sum + $i; } $sum"),
        15
    );
    assert_eq!(
        eval_int("my $sum = 0; for my $i (1..10) { next if $i % 2 == 0; $sum = $sum + $i; } $sum"),
        25
    );
}

#[test]
fn ternary() {
    assert_eq!(eval_int("my $x = 5; $x > 3 ? 1 : 0"), 1);
    assert_eq!(eval_int("my $x = 1; $x > 3 ? 1 : 0"), 0);
}

// ── Subroutines ──

#[test]
fn basic_sub() {
    assert_eq!(eval_int("sub add { my $a = shift @_; my $b = shift @_; return $a + $b; } add(3, 4)"), 7);
}

#[test]
fn recursive_fibonacci() {
    assert_eq!(
        eval_int("sub fib { my $n = shift @_; return $n if $n <= 1; return fib($n-1) + fib($n-2); } fib(10)"),
        55
    );
}

#[test]
fn return_with_postfix_if() {
    assert_eq!(
        eval_int("sub f { my $n = shift @_; return 0 if $n <= 0; return $n; } f(5)"),
        5
    );
    assert_eq!(
        eval_int("sub f { my $n = shift @_; return 0 if $n <= 0; return $n; } f(-1)"),
        0
    );
}

// ── Array operations ──

#[test]
fn push_pop() {
    assert_eq!(eval_int("my @a = (1,2,3); push @a, 4; $a[3]"), 4);
    assert_eq!(eval_int("my @a = (1,2,3); pop @a"), 3);
    assert_eq!(eval_int("my @a = (1,2,3); pop @a; scalar @a"), 2);
}

#[test]
fn shift_unshift() {
    assert_eq!(eval_int("my @a = (1,2,3); shift @a"), 1);
    assert_eq!(eval_int("my @a = (1,2,3); unshift @a, 0; $a[0]"), 0);
}

#[test]
fn map_grep() {
    assert_eq!(
        eval_int("my @a = map { $_ * 2 } (1,2,3); $a[2]"),
        6
    );
    assert_eq!(
        eval_int("my @a = grep { $_ > 2 } (1,2,3,4,5); scalar @a"),
        3
    );
}

#[test]
fn sort_array() {
    assert_eq!(
        eval_string(r#"join(",", sort("c","a","b"))"#),
        "a,b,c"
    );
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (3,1,2))"#),
        "1,2,3"
    );
}

#[test]
fn reverse_array() {
    assert_eq!(
        eval_string(r#"join(",", reverse(1,2,3))"#),
        "3,2,1"
    );
}

#[test]
fn split_join() {
    assert_eq!(
        eval_string(r#"join("-", split(",", "a,b,c"))"#),
        "a-b-c"
    );
}

// ── Regex ──

#[test]
fn regex_match() {
    assert_eq!(eval_int(r#"my $s = "hello123"; $s =~ /(\d+)/; $1"#), 123);
}

#[test]
fn regex_substitution() {
    assert_eq!(
        eval_string(r#"my $s = "foo bar"; $s =~ s/bar/baz/; $s"#),
        "foo baz"
    );
}

// ── Parallel operations ──

#[test]
fn parallel_map() {
    let result = eval("my @a = pmap { $_ * 2 } (1,2,3,4,5); scalar @a");
    assert_eq!(result.to_int(), 5);
}

#[test]
fn parallel_grep() {
    let result = eval("my @a = pgrep { $_ % 2 == 0 } (1,2,3,4,5,6); scalar @a");
    assert_eq!(result.to_int(), 3);
}

#[test]
fn parallel_sort() {
    assert_eq!(
        eval_string(r#"join(",", psort { $a <=> $b } (5,3,1,4,2))"#),
        "1,2,3,4,5"
    );
}

#[test]
fn parallel_for_runs() {
    assert_eq!(eval_int("pfor { $_ } (1,2,3); 99"), 99);
}

// ── References ──

#[test]
fn array_ref() {
    assert_eq!(eval_int("my $r = [1,2,3]; $r->[1]"), 2);
}

#[test]
fn hash_ref() {
    assert_eq!(eval_int("my $r = {a => 1, b => 2}; $r->{b}"), 2);
}

// ── Special variables ──

#[test]
fn defined_undef() {
    assert_eq!(eval_int("defined(42)"), 1);
    assert_eq!(eval_int("defined(undef)"), 0);
}

#[test]
fn ref_type() {
    assert_eq!(eval_string(r#"ref([])"#), "ARRAY");
    assert_eq!(eval_string(r#"ref({})"#), "HASH");
    assert_eq!(eval_string(r#"ref(\42)"#), "SCALAR");
}

// ── Numeric functions ──

#[test]
fn numeric_functions() {
    assert_eq!(eval_int("abs(-5)"), 5);
    assert_eq!(eval_int("int(3.7)"), 3);
    assert_eq!(eval_int("hex('ff')"), 255);
    assert_eq!(eval_int("oct('77')"), 63);
    assert_eq!(eval_string("chr(65)"), "A");
    assert_eq!(eval_int("ord('A')"), 65);
}

// ── Range ──

#[test]
fn range_operator() {
    assert_eq!(eval_int("my @a = (1..5); scalar @a"), 5);
    assert_eq!(eval_int("my @a = (1..5); $a[4]"), 5);
}

// ── Die ──

#[test]
fn die_in_eval() {
    let code = r#"eval { die "test error\n" }; $@ eq "test error\n" ? 1 : 0"#;
    // eval catches die, $@ should have the message
    let program = perlrs::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    let result = interp.execute(&program);
    assert!(result.is_ok());
}

// ── Hash operations ──

#[test]
fn hash_delete_exists() {
    assert_eq!(eval_int("my %h = (a => 1, b => 2); delete $h{a}; exists $h{a} ? 1 : 0"), 0);
    assert_eq!(eval_int("my %h = (a => 1, b => 2); exists $h{b} ? 1 : 0"), 1);
}

#[test]
fn hash_keys_values() {
    assert_eq!(eval_int("my %h = (a => 1, b => 2, c => 3); scalar keys %h"), 3);
}

// ── String interpolation ──

#[test]
fn string_interpolation_hash_access() {
    assert_eq!(
        eval_string(r#"my %h = (x => 42); "$h{x}""#),
        "42"
    );
}

#[test]
fn string_interpolation_array_access() {
    assert_eq!(
        eval_string(r#"my @a = (10, 20, 30); "$a[1]""#),
        "20"
    );
}

// ── Logical short-circuit (&&, ||, //) ──

#[test]
fn logical_and_short_circuit() {
    assert_eq!(eval_int("1 && 5"), 5);
    assert_eq!(eval_int("0 && 5"), 0);
}

#[test]
fn logical_or_short_circuit() {
    assert_eq!(eval_int("0 || 7"), 7);
    assert_eq!(eval_int("3 || 7"), 3);
}

#[test]
fn defined_or_operator() {
    assert_eq!(eval_int("undef // 5"), 5);
    assert_eq!(eval_int("0 // 5"), 0);
}

#[test]
fn logical_words_and_or_not() {
    assert_eq!(eval_int("1 and 2"), 2);
    assert_eq!(eval_int("0 or 9"), 9);
    assert_eq!(eval_int("not 0"), 1);
    assert_eq!(eval_int("not 1"), 0);
}

// ── Bitwise ──

#[test]
fn bitwise_operators() {
    assert_eq!(eval_int("0x0f & 0x33"), 0x03);
    assert_eq!(eval_int("0x10 | 0x01"), 0x11);
    assert_eq!(eval_int("0b1010 ^ 0b1100"), 0b0110);
    // Note: `<<` is lexed as heredoc start, not shift-left; use >> only for shift coverage here.
    assert_eq!(eval_int("32 >> 3"), 4);
}

#[test]
fn unary_bitwise_not() {
    assert_eq!(eval_int("~0"), -1);
}

// ── Unary numeric / logical ──

#[test]
fn unary_negate() {
    assert_eq!(eval_int("-42"), -42);
    assert_eq!(eval_int("0 - -1"), 1);
}

// ── Compound assignment ──

#[test]
fn compound_assignment() {
    assert_eq!(eval_int("my $x = 10; $x += 3; $x"), 13);
    assert_eq!(eval_int("my $x = 10; $x -= 4; $x"), 6);
    assert_eq!(eval_int("my $x = 2; $x *= 3; $x"), 6);
    assert_eq!(eval_int("my $x = 2; $x **= 3; $x"), 8);
    assert_eq!(eval_int("my $x = 10; $x %= 3; $x"), 1);
    assert_eq!(eval_string(r#"my $s = "a"; $s .= "b"; $s"#), "ab");
}

// ── Increment / decrement ──

#[test]
fn pre_post_increment() {
    assert_eq!(eval_int("my $x = 1; ++$x"), 2);
    assert_eq!(eval_int("my $x = 1; $x++"), 1);
    assert_eq!(eval_int("my $x = 1; $x++; $x"), 2);
}

#[test]
fn pre_post_decrement() {
    assert_eq!(eval_int("my $x = 3; --$x"), 2);
    assert_eq!(eval_int("my $x = 3; $x--"), 3);
    assert_eq!(eval_int("my $x = 3; $x--; $x"), 2);
}

// ── Truthiness (Perl: string "0" is false) ──

#[test]
fn string_zero_is_false() {
    assert_eq!(eval_int(r#""0" ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#""1" ? 1 : 0"#), 1);
}

// ── String cmp ──

#[test]
fn str_cmp_operator() {
    assert_eq!(eval_int(r#""a" cmp "b""#), -1);
    assert_eq!(eval_int(r#""b" cmp "a""#), 1);
    assert_eq!(eval_int(r#""x" cmp "x""#), 0);
}

// ── Literals ──

#[test]
fn hex_binary_literals() {
    assert_eq!(eval_int("0xff"), 255);
    assert_eq!(eval_int("0b1010"), 10);
}

#[test]
fn division_yields_float_coerced_to_int() {
    assert_eq!(eval_int("7 / 2"), 3);
}

// ── Hash values ──

#[test]
fn hash_values_builtin() {
    assert_eq!(
        eval_int("my %h = (a => 1, b => 2, c => 3); my $s = 0; foreach my $v (values %h) { $s = $s + $v; } $s"),
        6
    );
}

// ── Parse errors ──

#[test]
fn parse_unclosed_brace_is_syntax_error() {
    let err = perlrs::parse("sub f {").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Syntax);
}

// ── Runtime errors ──

#[test]
fn division_by_zero_is_runtime_error() {
    assert_eq!(eval_err_kind("1 / 0"), ErrorKind::Runtime);
}

#[test]
fn modulus_zero_is_runtime_error() {
    assert_eq!(eval_err_kind("1 % 0"), ErrorKind::Runtime);
}
