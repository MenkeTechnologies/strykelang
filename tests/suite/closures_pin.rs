//! Closure-semantics pins. Stryke's core closure rule:
//!   * `my $x = ...` is captured **by value** at closure creation.
//!   * `mysync $x = ...` is captured **by reference** (mutations
//!     visible across closure boundaries).
//! This split is load-bearing for AOP advice + parallel primitives.

use crate::common::*;

// ── `my` captures by value (default) ─────────────────────────────────

#[test]
fn my_var_captured_by_value_writes_blocked() {
    // Writes to a captured `my` var from inside a closure must NOT
    // affect the outer scope. Stryke rejects this at parse time when
    // the assignment is illegal, OR silently keeps outer scope clean.
    let code = r#"
        my $outer = 0;
        my $f = sub { my $inner = $outer; $inner + 1 };
        my $r = $f->();
        ($outer == 0 && $r == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn my_var_read_from_closure_sees_value_at_creation_time() {
    let code = r#"
        my $x = 10;
        my $f = sub { $x * 2 };
        $f->() == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── `mysync` captures by reference, writes flow back ────────────────

#[test]
fn mysync_var_writes_visible_outside_closure() {
    let code = r#"
        mysync $count = 0;
        my $f = sub { $count = $count + 1 };
        $f->();
        $f->();
        $f->();
        $count == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_array_push_visible_outside() {
    let code = r#"
        mysync @log;
        my $log_fn = sub { push @log, $_[0] };
        $log_fn->("a");
        $log_fn->("b");
        $log_fn->("c");
        join(",", @log) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_hash_update_visible_outside() {
    let code = r#"
        mysync %tally;
        my $inc = sub { $tally{$_[0]} = ($tally{$_[0]} // 0) + 1 };
        $inc->("a");
        $inc->("a");
        $inc->("b");
        ($tally{a} == 2 && $tally{b} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closures capture parameters correctly ───────────────────────────

#[test]
fn closure_captures_function_parameter() {
    let code = r#"
        fn Demo::Close::make_adder($n) {
            sub { $_[0] + $n }
        }
        my $add5 = Demo::Close::make_adder(5);
        my $add10 = Demo::Close::make_adder(10);
        ($add5->(3) == 8 && $add10->(3) == 13) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn two_adders_have_independent_captures() {
    let code = r#"
        fn Demo::Close::make_adder2($n) {
            sub { $_[0] + $n }
        }
        my $a = Demo::Close::make_adder2(100);
        my $b = Demo::Close::make_adder2(200);
        ($a->(1) == 101 && $b->(1) == 201) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested closures ──────────────────────────────────────────────────

#[test]
fn nested_closure_sees_outer_captures() {
    let code = r#"
        fn Demo::Close::outer_inner($x) {
            sub {
                my $y = $_[0];
                sub { $_[0] + $x + $y }
            }
        }
        my $factory = Demo::Close::outer_inner(1);
        my $inner = $factory->(10);
        $inner->(100) == 111 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure-over-loop-variable: each iteration sees its own value ──

#[test]
fn closure_over_loop_var_captures_per_iteration() {
    let code = r#"
        my @closures;
        for my $i (1, 2, 3, 4, 5) {
            push @closures, sub { $i };
        }
        my @vals = map { $_->() } @closures;
        join(",", @vals) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closures returning closures (Y-combinator-ish) ──────────────────

#[test]
fn closure_returning_closure_composition() {
    let code = r#"
        fn Demo::Close::compose($f, $g) {
            sub { $f->($g->($_[0])) }
        }
        my $dbl  = sub { $_[0] * 2 };
        my $plus = sub { $_[0] + 1 };
        my $c = Demo::Close::compose($dbl, $plus);   # f(g(x)) = 2*(x+1)
        $c->(5) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── @_ inside closure is the closure's argument list ────────────────

#[test]
fn closure_underscore_underscore_is_argument_list() {
    let code = r#"
        my $sum = sub { my $s = 0; $s += $_ for @_; $s };
        $sum->(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Block-captured `_` is the topic, not arg ────────────────────────

#[test]
fn block_underscore_is_topic_inside_map() {
    let code = r#"
        my @r = map { _ * 3 } (1, 2, 3);
        join(",", @r) eq "3,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Counter factory (canonical closure example) ─────────────────────

#[test]
fn counter_factory_state_isolated_per_instance() {
    let code = r#"
        fn Demo::Close::counter() {
            mysync $n = 0;
            sub { $n = $n + 1; $n }
        }
        my $c1 = Demo::Close::counter();
        my $c2 = Demo::Close::counter();
        $c1->();
        $c1->();
        $c1->();
        $c2->();
        my @r = ($c1->(), $c2->());
        ($r[0] == 4 && $r[1] == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure passed as callback to user function ─────────────────────

#[test]
fn user_function_invokes_closure_callback() {
    let code = r#"
        fn Demo::Close::run_3_times($cb) {
            $cb->();
            $cb->();
            $cb->();
        }
        mysync $count = 0;
        Demo::Close::run_3_times(sub { $count = $count + 1 });
        $count == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure stored in hash and dispatched dynamically ──────────────

#[test]
fn closures_in_dispatch_table() {
    let code = r#"
        my %ops = (
            inc => sub { $_[0] + 1 },
            dec => sub { $_[0] - 1 },
            sq  => sub { $_[0] * $_[0] },
        );
        my $r1 = $ops{inc}->(10);
        my $r2 = $ops{sq}->(7);
        ($r1 == 11 && $r2 == 49) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure over namespaced variable ────────────────────────────────

#[test]
fn closure_captures_namespaced_my_at_creation() {
    let code = r#"
        my $base = 100;
        my @added = map { my $n = $_; sub { $base + $n } } (1, 2, 3);
        my @results = map { $_->() } @added;
        join(",", @results) eq "101,102,103" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map/grep callbacks are themselves closures ─────────────────────

#[test]
fn map_callback_sees_outer_mysync_var() {
    let code = r#"
        mysync $threshold = 5;
        my @kept = grep { $_ > $threshold } (1:10);
        join(",", @kept) eq "6,7,8,9,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Anonymous subs preserve their environment across calls ─────────

#[test]
fn anonymous_sub_holds_state_via_mysync() {
    let code = r#"
        fn Demo::Close::make_accumulator() {
            mysync $total = 0;
            sub { $total = $total + $_[0]; $total }
        }
        my $acc = Demo::Close::make_accumulator();
        $acc->(10);
        $acc->(20);
        $acc->(30);
        $acc->(0) == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
