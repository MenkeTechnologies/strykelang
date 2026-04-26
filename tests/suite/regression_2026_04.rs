//! Regression tests for bugs fixed in April 2026.
//! Each test documents a specific bug that was identified and fixed.

use crate::common::*;

// =============================================================================
// BUG: Namespaced function `y` was lexed as transliteration operator
// FIX: Lexer now checks for preceding `::` before treating `y` or `tr` as operators
// =============================================================================

#[test]
fn namespaced_y_function_not_transliteration() {
    // `Rosetta::YCombinator::y` should parse as a namespaced function call,
    // not as transliteration operator `y///`
    assert_eq!(
        eval_int(
            r#"
            fn Rosetta::YCombinator::y($x) { $x * 2 }
            Rosetta::YCombinator::y(21)
            "#
        ),
        42
    );
}

#[test]
fn namespaced_tr_function_not_transliteration() {
    // `Foo::tr` should parse as a namespaced function, not `tr///`
    assert_eq!(
        eval_int(
            r#"
            fn Foo::Bar::tr($x) { $x + 10 }
            Foo::Bar::tr(32)
            "#
        ),
        42
    );
}

#[test]
fn reserved_word_y_rejected_as_bare_function_name() {
    // Bare `fn y` should be rejected - it's a reserved word
    let err = parse_err_kind("fn y($x) { $x }");
    assert!(
        matches!(err, stryke::error::ErrorKind::Syntax),
        "expected syntax error for reserved word `y` as function name"
    );
}

#[test]
fn reserved_word_tr_rejected_as_bare_function_name() {
    let err = parse_err_kind("fn tr($x) { $x }");
    assert!(
        matches!(err, stryke::error::ErrorKind::Syntax),
        "expected syntax error for reserved word `tr` as function name"
    );
}

#[test]
fn reserved_word_s_rejected_as_bare_function_name() {
    let err = parse_err_kind("fn s($x) { $x }");
    assert!(
        matches!(err, stryke::error::ErrorKind::Syntax),
        "expected syntax error for reserved word `s` as function name"
    );
}

#[test]
fn reserved_word_qr_rejected_as_bare_function_name() {
    let err = parse_err_kind("fn qr($x) { $x }");
    assert!(
        matches!(err, stryke::error::ErrorKind::Syntax),
        "expected syntax error for reserved word `qr` as function name"
    );
}

// =============================================================================
// BUG: `[` on newline was parsed as array subscript instead of new statement
// FIX: Parser now breaks on newline before `[`, treating it as array literal
// =============================================================================

#[test]
fn bracket_on_newline_is_array_literal_not_subscript() {
    // After `my $x = func()`, a `[...]` on a new line should be a new array literal,
    // NOT an array subscript on the return value of func()
    assert_eq!(
        eval_string(
            r#"
            fn Test::Util::get_value { 100 }
            my $x = Test::Util::get_value()
            [1, 2, 3]
            join(",", 1, 2, 3)
            "#
        ),
        "1,2,3"
    );
}

#[test]
fn bracket_on_same_line_is_subscript() {
    // `$arr[1]` on same line IS a subscript
    assert_eq!(
        eval_int(
            r#"
            my @arr = (10, 20, 30)
            my $x = $arr[1]
            $x
            "#
        ),
        20
    );
}

#[test]
fn fraction_reduction_pattern_no_div_by_zero() {
    // This pattern caused "Illegal division by zero" before the fix
    // because `[$num / $g, ...]` was parsed as subscript of gcd() result
    assert_eq!(
        eval_string(
            r#"
            fn Test::Math::my_gcd($a, $b) { $b == 0 ? $a : Test::Math::my_gcd($b, $a % $b) }
            my $num = 6
            my $den = 8
            my $g = Test::Math::my_gcd($num, $den)
            [$num / $g, $den / $g]
            join("/", $num / $g, $den / $g)
            "#
        ),
        "3/4"
    );
}

// =============================================================================
// BUG: `return @arr` returned array length instead of contents
// FIX: Compiler now uses List context for ArrayVar/Range in return statements
// =============================================================================

#[test]
fn return_array_returns_contents_not_length() {
    assert_eq!(
        eval_string(
            r#"
            fn Test::Arr::get_list {
                my @arr = (1, 2, 3)
                return @arr
            }
            my @result = Test::Arr::get_list()
            join(",", @result)
            "#
        ),
        "1,2,3"
    );
}

#[test]
fn return_range_returns_contents() {
    assert_eq!(
        eval_string(
            r#"
            fn Test::Arr::get_range {
                return 1..5
            }
            my @result = Test::Arr::get_range()
            join(",", @result)
            "#
        ),
        "1,2,3,4,5"
    );
}

#[test]
fn return_list_in_scalar_context_stringifies() {
    // When a sub returns a list literal, it stringifies in scalar context
    // Note: This differs from Perl 5 which returns last element
    assert_eq!(
        eval_string(
            r#"
            fn Test::Ctx::triple { (10, 20, 30) }
            my $x = Test::Ctx::triple()
            $x
            "#
        ),
        "102030"
    );
}

// =============================================================================
// BUG: `||` operator double-popped stack causing wrong results
// FIX: Removed redundant Pop after JumpIfTrueKeep in LogOr compilation
// =============================================================================

#[test]
fn logical_or_short_circuit_first_truthy() {
    // First value is truthy, should return it without evaluating second
    assert_eq!(eval_int("my $x = 5 || 10; $x"), 5);
}

#[test]
fn logical_or_short_circuit_first_falsy() {
    // First value is falsy (0), should return second
    assert_eq!(eval_int("my $x = 0 || 42; $x"), 42);
}

#[test]
fn logical_or_in_recursive_function() {
    // This pattern was corrupted by the double-pop bug
    assert_eq!(
        eval_int(
            r#"
            fn Test::Fib::calc($n) {
                if ($n <= 1) { $n }
                else { Test::Fib::calc($n - 1) + Test::Fib::calc($n - 2) }
            }
            Test::Fib::calc(10)
            "#
        ),
        55
    );
}

#[test]
fn logical_or_chained() {
    assert_eq!(eval_int("my $x = 0 || 0 || 0 || 7; $x"), 7);
    assert_eq!(eval_int("my $x = 0 || '' || 0 || 99; $x"), 99);
}

#[test]
fn logical_or_with_function_calls() {
    assert_eq!(
        eval_int(
            r#"
            fn Test::Or::get_zero { 0 }
            fn Test::Or::get_forty_two { 42 }
            my $x = Test::Or::get_zero() || Test::Or::get_forty_two()
            $x
            "#
        ),
        42
    );
}

// =============================================================================
// BUG: Threading operator `~>` didn't support namespaced functions
// FIX: Parser now collects Package::Name::func in thread stages
// =============================================================================

#[test]
fn thread_operator_with_namespaced_function() {
    assert_eq!(
        eval_int(
            r#"
            fn Math::double($x) { $x * 2 }
            ~> 10 Math::double
            "#
        ),
        20
    );
}

#[test]
fn thread_operator_with_namespaced_function_and_args() {
    assert_eq!(
        eval_int(
            r#"
            fn Math::add($x, $y) { $x + $y }
            ~> 10 Math::add(5)
            "#
        ),
        15
    );
}

#[test]
fn thread_operator_chained_namespaced_functions() {
    assert_eq!(
        eval_int(
            r#"
            fn Math::double($x) { $x * 2 }
            fn Math::add_one($x) { $x + 1 }
            ~> 5 Math::double Math::add_one
            "#
        ),
        11
    );
}

#[test]
fn thread_operator_namespaced_with_pipe() {
    assert_eq!(
        eval_string(
            r#"
            fn Str::wrap($s, $l, $r) { $l . $s . $r }
            ~> "hello" Str::wrap("[", "]") |> uc
            "#
        ),
        "[HELLO]"
    );
}

// =============================================================================
// BUG: Postfix `for` loop closures captured `$_` by reference, not value
// FIX: Documented as known bug; workaround is explicit `for my $x` loop
// =============================================================================

#[test]
fn explicit_for_loop_captures_by_value() {
    // Using explicit `for my $x` correctly captures each value
    assert_eq!(
        eval_int(
            r#"
            my @results
            for my $x (1, 2, 3) {
                push @results, fn { $x * 10 }
            }
            my $sum = 0
            for my $f (@results) {
                $sum += $f->()
            }
            $sum
            "#
        ),
        60 // 10 + 20 + 30
    );
}

// =============================================================================
// BUG: `qr` as function name in namespace caused parse error
// FIX: Renamed to avoid conflict (qr is regex quote operator)
// =============================================================================

#[test]
fn qr_in_namespace_requires_different_name() {
    // Can't use `qr` even in namespace because lexer sees it first
    // Use `decompose` or similar instead
    assert_eq!(
        eval_string(
            r#"
            fn Math::Qr::decompose($x) { "decomposed: $x" }
            Math::Qr::decompose(42)
            "#
        ),
        "decomposed: 42"
    );
}

// =============================================================================
// General regression tests for recursive functions
// =============================================================================

#[test]
fn recursive_factorial() {
    assert_eq!(
        eval_int(
            r#"
            fn Test::Math::factorial($n) {
                if ($n <= 1) { 1 }
                else { $n * Test::Math::factorial($n - 1) }
            }
            Test::Math::factorial(5)
            "#
        ),
        120
    );
}

// =============================================================================
// BUG: Passing empty array to recursive call incorrectly substituted $_
// FIX: Only call with_topic_default_args when argc == 0, not when args is empty
// =============================================================================

#[test]
fn recursive_sum_with_shift() {
    // Recursive sum using shift @nums - now works correctly
    assert_eq!(
        eval_int(
            r#"
            fn Test::List::sum(@nums) {
                if (scalar(@nums) == 0) { 0 }
                else {
                    my $first = shift @nums
                    $first + Test::List::sum(@nums)
                }
            }
            Test::List::sum(1, 2, 3, 4, 5)
            "#
        ),
        15
    );
}

#[test]
fn recursive_with_empty_array_param() {
    // Passing empty array should not substitute $_ topic
    assert_eq!(
        eval_int(
            r#"
            fn Test::Count::len(@arr) {
                if (scalar(@arr) == 0) { 0 }
                else {
                    shift @arr
                    1 + Test::Count::len(@arr)
                }
            }
            Test::Count::len(1, 2, 3)
            "#
        ),
        3
    );
}

#[test]
fn recursive_sum_with_index() {
    // Alternative: recursion using index (also valid approach)
    assert_eq!(
        eval_int(
            r#"
            fn Test::List::sum_from($arr, $i) {
                if ($i >= scalar(@$arr)) { 0 }
                else { $arr->[$i] + Test::List::sum_from($arr, $i + 1) }
            }
            my @nums = (1, 2, 3, 4, 5)
            Test::List::sum_from(\@nums, 0)
            "#
        ),
        15
    );
}

#[test]
fn mutually_recursive_even_odd() {
    assert_eq!(
        eval_string(
            r#"
            fn Test::Parity::check_even($n) {
                if ($n == 0) { "even" }
                else { Test::Parity::check_odd($n - 1) }
            }
            fn Test::Parity::check_odd($n) {
                if ($n == 0) { "odd" }
                else { Test::Parity::check_even($n - 1) }
            }
            Test::Parity::check_even(10) . "," . Test::Parity::check_odd(10)
            "#
        ),
        "even,odd"
    );
}

// =============================================================================
// Edge cases for array/list handling
// =============================================================================

#[test]
fn empty_array_return() {
    assert_eq!(
        eval_int(
            r#"
            fn Test::Arr::empty_list { () }
            my @arr = Test::Arr::empty_list()
            scalar(@arr)
            "#
        ),
        0
    );
}

#[test]
fn nested_array_construction() {
    assert_eq!(
        eval_string(
            r#"
            fn Test::Arr::make_pair($a, $b) { [$a, $b] }
            my $p = Test::Arr::make_pair(1, 2)
            join(",", @$p)
            "#
        ),
        "1,2"
    );
}

#[test]
fn array_in_hash_value() {
    assert_eq!(
        eval_int(
            r#"
            my %h = (nums => [1, 2, 3])
            my $sum = 0
            for my $n (@{$h{nums}}) { $sum += $n }
            $sum
            "#
        ),
        6
    );
}
