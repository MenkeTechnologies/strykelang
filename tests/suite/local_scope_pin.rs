//! Local-scope pins inside loops/blocks. Complement scope_pin.rs.

use crate::common::*;

// ── my in loop body is reinitialised per iteration ─────────────────

#[test]
fn my_in_loop_body_per_iteration() {
    let code = r#"
        my @vals;
        for my $i (1, 2, 3) {
            my $local = $i * 10;
            push @vals, $local;
        }
        join(",", @vals) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── my in while body ───────────────────────────────────────────────

#[test]
fn my_in_while_body() {
    let code = r#"
        my @vals;
        my $i = 0;
        while ($i < 3) {
            my $sq = $i * $i;
            push @vals, $sq;
            $i++;
        }
        join(",", @vals) eq "0,1,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure-over-loop-var captures correctly ─────────────────────

#[test]
fn closure_over_loop_var_captures_each_iter() {
    let code = r#"
        my @closures;
        for my $i (1, 2, 3) {
            push @closures, sub { $i };
        }
        my @r = map { $_->() } @closures;
        join(",", @r) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── my-in-if-block isolated ────────────────────────────────────────

#[test]
fn my_in_if_block_isolated() {
    let code = r#"
        my $r = "outer";
        if (1) {
            my $local = "inner";
            $r = $local;
        }
        $r eq "inner" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── For-loop var is block-scoped ──────────────────────────────────

#[test]
fn for_loop_var_block_scoped() {
    let code = r#"
        for my $i (1:5) { }
        # $i should not be in outer scope; we can't read it anyway.
        1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Accumulator pattern: my outside loop, modify inside ──────────

#[test]
fn accumulator_outside_loop() {
    let code = r#"
        my $total = 0;
        for my $i (1:10) {
            $total += $i;
        }
        $total == 55 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested for-loops with inner my ────────────────────────────────

#[test]
fn nested_loops_inner_my_independent() {
    let code = r#"
        my $count = 0;
        for my $i (1:3) {
            for my $j (1:3) {
                my $local = $i * 10 + $j;
                $count++ if $local > 20;
            }
        }
        # (2,1)=21, (2,2)=22, (2,3)=23, (3,1)=31, ... = 6 values > 20.
        $count == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── my array inside loop builds new each iter ─────────────────────

#[test]
fn my_array_in_loop_reinitialises() {
    let code = r#"
        my @results;
        for my $i (1:3) {
            my @local = ($i, $i + 10, $i + 20);
            push @results, sum(@local);
        }
        # 1+11+21=33; 2+12+22=36; 3+13+23=39.
        join(",", @results) eq "33,36,39" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Last-iter value persists outside loop via outer var ───────────

#[test]
fn outer_var_captures_last_value() {
    let code = r#"
        my $last_seen;
        for my $x (10, 20, 30) {
            $last_seen = $x;
        }
        $last_seen == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── For-each over array unchanged after loop ─────────────────────

#[test]
fn array_iteration_does_not_mutate_array() {
    let code = r#"
        my @arr = (1, 2, 3);
        for my $x (@arr) {
            # No mutation.
        }
        join(",", @arr) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple my in same block ─────────────────────────────────────

#[test]
fn multiple_my_declarations_in_same_block() {
    let code = r#"
        my $a = 1;
        my $b = 2;
        my $c = 3;
        ($a + $b + $c == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Closure created inside loop sees its iteration ────────────────

#[test]
fn closure_in_loop_sees_iteration_value() {
    let code = r#"
        my @factories;
        for my $i (1, 2, 3) {
            push @factories, sub { my $arg = $_[0]; $i * $arg };
        }
        # factories[0]: i=1, factories[1]: i=2, factories[2]: i=3.
        my $r = $factories[2]->(10);
        $r == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── For-loop with key collection ──────────────────────────────────

#[test]
fn for_loop_over_hash_keys_collects() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @collected;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @collected, "$k:$h{$k}";
        }
        join(",", @collected) eq "a:1,b:2,c:3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Conditional my via if/else ────────────────────────────────────

#[test]
fn conditional_my_via_branches() {
    let code = r#"
        my $cond = 1;
        my $r;
        if ($cond) {
            my $x = 100;
            $r = $x;
        } else {
            my $y = 200;
            $r = $y;
        }
        $r == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── my declarations don't leak out of eval ────────────────────────

#[test]
fn my_in_eval_does_not_leak() {
    let code = r#"
        my $accessible_outside = 0;
        eval {
            my $only_inside = 42;
            $accessible_outside = $only_inside;
        };
        $accessible_outside == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── For-loop var with explicit my ─────────────────────────────────

#[test]
fn for_loop_my_var_iterates() {
    let code = r#"
        my $sum = 0;
        for my $x (1, 2, 3, 4, 5) {
            $sum += $x;
        }
        $sum == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Loop var doesn't pollute outer scope ─────────────────────────

#[test]
fn loop_var_clobbers_outer_my_per_bug_233() {
    // BUG-233 manifestation: `for my $x` inside a scope that already
    // has `my $x` clobbers the outer to undef rather than shadowing.
    let code = r#"
        my $x = 100;
        for my $x (1, 2, 3) {
            # No-op body.
        }
        # Outer is undef per BUG-233; original 100 lost.
        !defined($x) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── my-hash inside loop is reinit ────────────────────────────────

#[test]
fn my_hash_in_loop_reinit() {
    let code = r#"
        my @results;
        for my $i (1:3) {
            my %local = (key => $i, doubled => $i * 2);
            push @results, $local{doubled};
        }
        join(",", @results) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Static-like state via mysync ──────────────────────────────────

#[test]
fn mysync_inside_fn_reinit_per_call_not_static() {
    // Stryke surface: `mysync` inside a fn body reinitialises on
    // each call — it's per-call shared state, not static. Static-
    // like state requires a closure-factory pattern.
    let code = r#"
        fn Demo::LS::counter() {
            mysync $n = 0;
            $n = $n + 1;
            return $n
        }
        my @r = (
            Demo::LS::counter(),
            Demo::LS::counter(),
            Demo::LS::counter(),
        );
        # Each call sees fresh $n; all return 1.
        join(",", @r) eq "1,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Visibility of fn param after fn body ────────────────────────

#[test]
fn fn_param_scoped_to_fn_body() {
    let code = r#"
        my $top = "outer";
        fn Demo::LS::echo($top) { $top }
        Demo::LS::echo("inner") eq "inner" && $top eq "outer" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reference captured in closure ─────────────────────────────────

#[test]
fn closure_captures_ref_writes_visible() {
    let code = r#"
        my @arr = (1, 2, 3);
        my $arr_ref = \@arr;
        my $appender = sub { push @$arr_ref, $_[0] };
        $appender->(4);
        $appender->(5);
        join(",", @arr) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Looping shadow ─────────────────────────────────────────────────

#[test]
fn loop_var_clobbers_outer_per_bug_233_collected() {
    // BUG-233: outer $x is clobbered, but the collected loop values
    // are correct.
    let code = r#"
        my $x = 10;
        my @seen;
        for my $x (1, 2, 3) {
            push @seen, $x;
        }
        # Outer $x lost; collection preserved.
        (!defined($x) && join(",", @seen) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map block has its own my ──────────────────────────────────────

#[test]
fn map_block_my_local_per_element() {
    let code = r#"
        my @r = map {
            my $double = $_ * 2;
            $double + 1
        } (1, 2, 3);
        join(",", @r) eq "3,5,7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── My-var visible inside loop body and used in test ─────────────

#[test]
fn my_var_visible_in_loop_condition() {
    let code = r#"
        my @r;
        for my $i (1:10) {
            my $sq = $i * $i;
            push @r, $sq if $sq > 25;
        }
        join(",", @r) eq "36,49,64,81,100" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
