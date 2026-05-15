//! Subroutine call-surface pins. Focus on the call site:
//! arg passing, default values, slurpy @_, splat, mutual recursion.

use crate::common::*;

// ── Positional args via @_ ──────────────────────────────────────────

#[test]
fn fn_reads_args_via_at_underscore() {
    let code = r#"
        fn Demo::Sub::two_args { $_[0] + $_[1] }
        Demo::Sub::two_args(10, 20)
    "#;
    assert_eq!(eval_int(code), 30);
}

#[test]
fn fn_reads_args_via_named_params() {
    let code = r#"
        fn Demo::Sub::named($x, $y) { $x * $y }
        Demo::Sub::named(6, 7)
    "#;
    assert_eq!(eval_int(code), 42);
}

// ── Slurpy @rest ───────────────────────────────────────────────────

#[test]
fn fn_with_slurpy_array_after_named_params() {
    let code = r#"
        fn Demo::Sub::slurpy($first, @rest) {
            len(@rest)
        }
        Demo::Sub::slurpy(1, 2, 3, 4, 5)
    "#;
    assert_eq!(eval_int(code), 4);
}

// ── Recursion ──────────────────────────────────────────────────────

#[test]
fn recursive_factorial() {
    let code = r#"
        fn Demo::Sub::fact($n) {
            $n <= 1 ? 1 : $n * Demo::Sub::fact($n - 1)
        }
        Demo::Sub::fact(7)
    "#;
    assert_eq!(eval_int(code), 5040);
}

#[test]
fn recursive_fibonacci() {
    let code = r#"
        fn Demo::Sub::fib($n) {
            $n < 2 ? $n : Demo::Sub::fib($n - 1) + Demo::Sub::fib($n - 2)
        }
        Demo::Sub::fib(10)
    "#;
    assert_eq!(eval_int(code), 55);
}

// ── Mutual recursion ───────────────────────────────────────────────

#[test]
fn mutual_recursion_is_even_is_odd() {
    let code = r#"
        fn Demo::Sub::is_even($n) {
            $n == 0 ? 1 : Demo::Sub::is_odd($n - 1)
        }
        fn Demo::Sub::is_odd($n) {
            $n == 0 ? 0 : Demo::Sub::is_even($n - 1)
        }
        my @r = (
            Demo::Sub::is_even(0),
            Demo::Sub::is_even(1),
            Demo::Sub::is_even(2),
            Demo::Sub::is_even(7),
        );
        join(",", @r) eq "1,0,1,0" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple return values via array ───────────────────────────────

#[test]
fn fn_returns_multiple_values_via_list() {
    let code = r#"
        fn Demo::Sub::divmod($a, $b) {
            (int($a / $b), $a % $b)
        }
        my ($q, $r) = Demo::Sub::divmod(17, 5);
        ($q == 3 && $r == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Fn returns hashref ─────────────────────────────────────────────

#[test]
fn fn_returns_hashref() {
    let code = r#"
        fn Demo::Sub::make_user($name, $age) {
            +{ name => $name, age => $age }
        }
        my $u = Demo::Sub::make_user("alice", 30);
        ($u->{name} eq "alice" && $u->{age} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Fn returns arrayref ────────────────────────────────────────────

#[test]
fn fn_returns_arrayref() {
    let code = r#"
        fn Demo::Sub::make_pair($a, $b) {
            [$a, $b]
        }
        my $p = Demo::Sub::make_pair(10, 20);
        ($p->[0] == 10 && $p->[1] == 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Calling with too few args ──────────────────────────────────────

#[test]
fn fn_called_with_fewer_args_undefs_missing() {
    let code = r#"
        fn Demo::Sub::three($a, $b, $c) {
            defined($c) ? "all" : "missing"
        }
        Demo::Sub::three(1, 2) eq "missing" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Calling with extra args (Perl: extras land in @_) ─────────────

#[test]
fn fn_called_with_extra_args_ignored_safely() {
    let code = r#"
        fn Demo::Sub::two_only($a, $b) {
            $a + $b
        }
        Demo::Sub::two_only(10, 20, 30, 40) == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Splat operator: @arr into call ────────────────────────────────

#[test]
fn splat_array_into_call() {
    let code = r#"
        fn Demo::Sub::sum_three($a, $b, $c) {
            $a + $b + $c
        }
        my @nums = (10, 20, 30);
        Demo::Sub::sum_three(@nums) == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Splat hashref into call ───────────────────────────────────────

#[test]
fn pass_hashref_to_fn() {
    let code = r#"
        fn Demo::Sub::use_opts($opts) {
            $opts->{count} * $opts->{factor}
        }
        my $opts = +{ count => 7, factor => 3 };
        Demo::Sub::use_opts($opts) == 21 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty function body returns undef ─────────────────────────────

#[test]
fn empty_fn_body_returns_undef() {
    let code = r#"
        fn Demo::Sub::empty() { }
        my $r = Demo::Sub::empty();
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Forward references work ───────────────────────────────────────

#[test]
fn forward_reference_works() {
    let code = r#"
        fn Demo::Sub::caller() {
            Demo::Sub::callee()   # defined below
        }
        fn Demo::Sub::callee() {
            "forward"
        }
        Demo::Sub::caller() eq "forward" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── return early in if/else branch ────────────────────────────────

#[test]
fn return_early_in_if_branch() {
    let code = r#"
        fn Demo::Sub::pos_neg($n) {
            return "pos" if $n > 0;
            return "neg" if $n < 0;
            "zero"
        }
        my @r = (
            Demo::Sub::pos_neg(5),
            Demo::Sub::pos_neg(-3),
            Demo::Sub::pos_neg(0),
        );
        join(",", @r) eq "pos,neg,zero" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Variadic via @_ ────────────────────────────────────────────────

#[test]
fn variadic_via_at_underscore_only() {
    let code = r#"
        fn Demo::Sub::sumall {
            my $s = 0;
            $s += $_ for @_;
            $s
        }
        Demo::Sub::sumall(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested function calls in expression ────────────────────────────

#[test]
fn nested_fn_calls_in_expression() {
    let code = r#"
        fn Demo::Sub::dbl($n) { $n * 2 }
        fn Demo::Sub::inc($n) { $n + 1 }
        Demo::Sub::dbl(Demo::Sub::inc(Demo::Sub::dbl(5))) == 22 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map / grep using user function name as callback ───────────────

#[test]
fn user_fn_via_map_block() {
    let code = r#"
        fn Demo::Sub::times3($n) { $n * 3 }
        my @r = map { Demo::Sub::times3($_) } (1, 2, 3);
        join(",", @r) eq "3,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die-and-catch across call boundary ────────────────────────────

#[test]
fn die_in_callee_caught_by_caller_eval() {
    let code = r#"
        fn Demo::Sub::risky() { die "from_risky\n" }
        eval { Demo::Sub::risky() };
        $@ eq "from_risky\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Deep recursion limit smoke test ───────────────────────────────

#[test]
fn deep_recursion_within_safe_depth() {
    let code = r#"
        fn Demo::Sub::countdown($n) {
            return $n if $n <= 0;
            Demo::Sub::countdown($n - 1)
        }
        Demo::Sub::countdown(200) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Return from inside loop returns from fn ───────────────────────

#[test]
fn return_inside_loop_returns_from_fn() {
    let code = r#"
        fn Demo::Sub::find_first_even() {
            for my $i (1, 3, 5, 6, 7) {
                return $i if $i % 2 == 0;
            }
            -1
        }
        Demo::Sub::find_first_even() == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── No explicit return: last expr value ───────────────────────────

#[test]
fn last_expression_is_return_value() {
    let code = r#"
        fn Demo::Sub::compute($a, $b) {
            my $x = $a + $b;
            my $y = $x * 2;
            $y - 1   # last expr
        }
        Demo::Sub::compute(3, 4) == 13 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
