//! Control-flow pins. last/next/redo/unless/until/do-while/given.

use crate::common::*;

// ── last (break) ─────────────────────────────────────────────────────

#[test]
fn last_breaks_out_of_for_loop() {
    let code = r#"
        my $found = -1;
        for my $i (1:100) {
            if ($i == 42) {
                $found = $i;
                last;
            }
        }
        $found == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn last_breaks_only_innermost_loop() {
    let code = r#"
        my $count = 0;
        for my $i (1:3) {
            for my $j (1:5) {
                last if $j == 3;
                $count++;
            }
        }
        # 3 outer × 2 inner-before-last = 6.
        $count == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── next (continue) ──────────────────────────────────────────────────

#[test]
fn next_skips_rest_of_iteration() {
    let code = r#"
        my @kept;
        for my $i (1:10) {
            next if $i % 2 == 0;
            push @kept, $i;
        }
        join(",", @kept) eq "1,3,5,7,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn next_in_nested_loop_skips_inner_only() {
    let code = r#"
        my @visits;
        for my $i (1, 2) {
            for my $j (1, 2, 3) {
                next if $j == 2;
                push @visits, "${i}_${j}";
            }
        }
        join(",", @visits) eq "1_1,1_3,2_1,2_3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── while loop ──────────────────────────────────────────────────────

#[test]
fn while_loop_runs_until_condition_false() {
    let code = r#"
        my $i = 0;
        my $sum = 0;
        while ($i < 10) {
            $sum += $i;
            $i++;
        }
        $sum == 45 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn while_with_last() {
    let code = r#"
        my $i = 0;
        while (1) {
            $i++;
            last if $i >= 7;
        }
        $i == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── do-while ────────────────────────────────────────────────────────

#[test]
fn do_while_runs_at_least_once() {
    let code = r#"
        my $count = 0;
        do {
            $count++;
        } while (0);   # condition false: still runs once.
        $count == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── until ───────────────────────────────────────────────────────────

#[test]
fn until_inverts_while_condition() {
    let code = r#"
        my $i = 0;
        until ($i >= 5) {
            $i++;
        }
        $i == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── unless ──────────────────────────────────────────────────────────

#[test]
fn unless_inverts_if_condition() {
    let code = r#"
        my $r = "fallback";
        unless (0) {
            $r = "ran";
        }
        $r eq "ran" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unless_does_not_run_when_truthy() {
    let code = r#"
        my $r = "fallback";
        unless (1) {
            $r = "ran";
        }
        $r eq "fallback" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn postfix_unless() {
    let code = r#"
        my @r;
        for my $i (1:10) {
            push @r, $i unless $i % 3 == 0;
        }
        join(",", @r) eq "1,2,4,5,7,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── postfix modifiers: if / unless / while / for ────────────────────

#[test]
fn postfix_if_modifier() {
    let code = r#"
        my @r;
        for my $i (1:10) {
            push @r, $i if $i > 7;
        }
        join(",", @r) eq "8,9,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn postfix_for_modifier() {
    let code = r#"
        my @r;
        push @r, $_ for (1, 3, 5);
        join(",", @r) eq "1,3,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ternary nesting ─────────────────────────────────────────────────

#[test]
fn nested_ternary() {
    let code = r#"
        my $x = 50;
        my $bucket = $x < 25 ? "low"
                   : $x < 75 ? "mid"
                   :           "high";
        $bucket eq "mid" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ternary_in_argument_position() {
    let code = r#"
        my @r;
        push @r, $_ % 2 == 0 ? "even" : "odd" for (1, 2, 3, 4);
        join(",", @r) eq "odd,even,odd,even" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── for-loop variations ─────────────────────────────────────────────

#[test]
fn for_loop_c_style_with_three_clauses() {
    let code = r#"
        my $sum = 0;
        for (my $i = 0; $i < 10; $i++) {
            $sum += $i;
        }
        $sum == 45 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_loop_over_list_literal() {
    let code = r#"
        my @log;
        for my $name ("alice", "bob", "carol") {
            push @log, "hello, $name";
        }
        join("|", @log) eq "hello, alice|hello, bob|hello, carol" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_loop_over_hash_keys() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my $sum = 0;
        for my $k (sort { _0 cmp _1 } keys %h) {
            $sum += $h{$k};
        }
        $sum == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Block as expression (last value returns) ───────────────────────

#[test]
fn block_expression_returns_last_value() {
    let code = r#"
        my $r = do {
            my $x = 10;
            my $y = 20;
            $x + $y
        };
        $r == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── if/elsif/else chain ─────────────────────────────────────────────

#[test]
fn if_elsif_else_chain_picks_first_match() {
    let code = r#"
        fn Demo::CF::bucketize($n) {
            if    ($n < 10)  { "small"  }
            elsif ($n < 100) { "medium" }
            else             { "large"  }
        }
        my @r = map { Demo::CF::bucketize($_) } (1, 50, 500);
        join(",", @r) eq "small,medium,large" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── early return from function ──────────────────────────────────────

#[test]
fn early_return_exits_function() {
    let code = r#"
        fn Demo::CF::safe_div($a, $b) {
            return undef if $b == 0;
            return $a / $b;
        }
        my $r1 = Demo::CF::safe_div(10, 2);
        my $r2 = Demo::CF::safe_div(10, 0);
        ($r1 == 5 && !defined($r2)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Combination: postfix while + last ───────────────────────────────

#[test]
fn while_with_break_at_threshold() {
    let code = r#"
        my $sum = 0;
        my $i = 0;
        while ($i < 1000) {
            $sum += $i;
            last if $sum > 100;
            $i++;
        }
        # Triangular sum up to N reaches 100+ around N=14 (1+2+...+14=105).
        $sum > 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Loop labels (LABEL: for) ────────────────────────────────────────

#[test]
fn labeled_loop_with_last_label() {
    let code = r#"
        my $found_at = "";
        OUTER: for my $i (1:5) {
            for my $j (1:5) {
                if ($i == 3 && $j == 4) {
                    $found_at = "${i},${j}";
                    last OUTER;
                }
            }
        }
        $found_at eq "3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
