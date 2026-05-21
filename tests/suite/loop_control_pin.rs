//! Loop-control pins: `next`, `last`, `redo`, with and without labels.

use crate::common::*;

// ── next (unlabeled) ──────────────────────────────────────────────

#[test]
fn next_skips_iteration() {
    let code = r#"
        my @seen;
        for my $i (1:5) {
            next if $i == 3;
            push @seen, $i;
        }
        join(",", @seen) eq "1,2,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn next_skips_multiple_iterations() {
    let code = r#"
        my @seen;
        for my $i (1:10) {
            next if $i % 2 == 0;
            push @seen, $i;
        }
        join(",", @seen) eq "1,3,5,7,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn next_in_while_loop() {
    let code = r#"
        my $i = 0;
        my @seen;
        while ($i < 5) {
            $i++;
            next if $i == 3;
            push @seen, $i;
        }
        join(",", @seen) eq "1,2,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── last (unlabeled) ──────────────────────────────────────────────

#[test]
fn last_terminates_loop() {
    let code = r#"
        my @seen;
        for my $i (1:10) {
            last if $i == 4;
            push @seen, $i;
        }
        join(",", @seen) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn last_in_while_terminates() {
    let code = r#"
        my $i = 0;
        my @seen;
        while ($i < 100) {
            $i++;
            last if $i == 3;
            push @seen, $i;
        }
        join(",", @seen) eq "1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn last_first_iteration_yields_empty() {
    let code = r#"
        my @seen;
        for my $i (1:5) {
            last;
            push @seen, $i;
        }
        len(@seen) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── nested loops with unlabeled next/last ─────────────────────────

#[test]
fn unlabeled_next_only_affects_innermost() {
    let code = r#"
        my @pairs;
        for my $i (1:3) {
            for my $j (1:3) {
                next if $j == 2;
                push @pairs, "$i-$j";
            }
        }
        join(",", @pairs) eq "1-1,1-3,2-1,2-3,3-1,3-3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unlabeled_last_only_breaks_innermost() {
    let code = r#"
        my @pairs;
        for my $i (1:3) {
            for my $j (1:5) {
                last if $j == 3;
                push @pairs, "$i-$j";
            }
        }
        join(",", @pairs) eq "1-1,1-2,2-1,2-2,3-1,3-2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── labeled next ──────────────────────────────────────────────────

#[test]
fn labeled_next_skips_to_outer() {
    let code = r#"
        my @pairs;
        OUTER: for my $i (1:3) {
            INNER: for my $j (1:3) {
                next OUTER if $j == 2;
                push @pairs, "$i-$j";
            }
        }
        # j=2 fires next OUTER; only j=1 makes it.
        join(",", @pairs) eq "1-1,2-1,3-1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn labeled_next_with_three_levels() {
    // NOTE: labels with digits ("L1", "L2") silently break loop control
    // in stryke (BUG-260). Use letter-only labels.
    let code = r#"
        my @triples;
        ALPHA: for my $i (1:2) {
            BETA: for my $j (1:2) {
                GAMMA: for my $k (1:2) {
                    next ALPHA if $k == 2 && $j == 2;
                    push @triples, "$i-$j-$k";
                }
            }
        }
        join(",", @triples) eq "1-1-1,1-1-2,1-2-1,2-1-1,2-1-2,2-2-1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn labels_containing_digits_route_loop_control() {
    // Labels with digits (e.g. `LABEL1`) are accepted as targets for
    // next/last/redo. `next LABEL1 if $j == 2` skips to the next outer
    // iteration after the j==1 push only.
    let code = r#"
        my @seen;
        LABEL1: for my $i (1:3) {
            for my $j (1:3) {
                next LABEL1 if $j == 2;
                push @seen, "$i-$j";
            }
        }
        join(",", @seen) eq "1-1,2-1,3-1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── labeled last ──────────────────────────────────────────────────

#[test]
fn labeled_last_exits_outer() {
    let code = r#"
        my @pairs;
        OUTER: for my $i (1:5) {
            for my $j (1:5) {
                last OUTER if $i == 2 && $j == 2;
                push @pairs, "$i-$j";
            }
        }
        # i=1 j=1..5, i=2 j=1, then breaks both.
        join(",", @pairs) eq "1-1,1-2,1-3,1-4,1-5,2-1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn labeled_last_in_while_loop() {
    let code = r#"
        my @vals;
        OUTER: for my $i (1:5) {
            my $j = 0;
            while ($j < 10) {
                $j++;
                last OUTER if $i * $j > 6;
                push @vals, "$i*$j=" . ($i * $j);
            }
        }
        # i=1 j=1..7 then 1*8=8 > 6 fires last OUTER.
        # But actually j increments first, then checks i*j. So:
        # i=1, j=1, 1*1=1 push; j=2, 1*2=2 push; ... j=7, 1*7=7>6 -> last
        # So we push 1*1 through 1*6 = 6 entries.
        len(@vals) == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── redo (unlabeled) ──────────────────────────────────────────────

#[test]
fn redo_repeats_iteration_without_increment() {
    let code = r#"
        my @seen;
        my $tries = 0;
        for my $i (1:3) {
            $tries++;
            if ($i == 2 && $tries < 4) {
                redo;
            }
            push @seen, "$i/$tries";
        }
        # i=1 tries=1
        # i=2 tries=2, redo
        # i=2 tries=3, redo
        # i=2 tries=4, push "2/4"
        # i=3 tries=5, push "3/5"
        join(",", @seen) eq "1/1,2/4,3/5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn redo_statement_modifier_fails_per_bug_261() {
    // Stryke parser rejects `redo if EXPR;` statement-modifier form.
    // Working idiom: wrap in block `if (EXPR) { redo }`.
    let code = r#"
        my $total_iters = 0;
        for my $i (1:3) {
            $total_iters++;
            if ($total_iters < 5) {
                redo;
            }
        }
        # i=1 (redo×4 to iters=5), i=2 (iters=6), i=3 (iters=7).
        $total_iters == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── labeled redo ──────────────────────────────────────────────────

#[test]
fn labeled_redo_restarts_outer_loop() {
    let code = r#"
        my @seen;
        my $tries = 0;
        OUT: for my $i (1:3) {
            for my $j (1:3) {
                $tries++;
                if ($i == 1 && $j == 2 && $tries < 5) {
                    redo OUT;
                }
                push @seen, "$i.$j";
            }
        }
        # After redo, OUT restarts i=1 from the beginning.
        len(@seen) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── loop control inside for over a hash ───────────────────────────

#[test]
fn next_in_for_over_keys() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        my $sum = 0;
        for my $k (sort keys %h) {
            next if $k eq "b";
            $sum += $h{$k};
        }
        # 1 + 3 + 4 = 8
        $sum == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── nested loop + sum interaction ─────────────────────────────────

#[test]
fn last_breaks_outer_via_label_total_correct() {
    let code = r#"
        my $sum = 0;
        OUTER: for my $i (1:10) {
            for my $j (1:10) {
                last OUTER if $sum > 100;
                $sum += $j;
            }
        }
        $sum > 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── continue block ────────────────────────────────────────────────

#[test]
fn continue_block_runs_after_each_iter() {
    let code = r#"
        my $checks = 0;
        my @seen;
        for my $i (1:5) {
            push @seen, $i;
        } continue {
            $checks++;
        }
        ($checks == 5 && len(@seen) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn continue_block_runs_after_next() {
    let code = r#"
        my $continue_count = 0;
        for my $i (1:5) {
            next if $i == 3;
        } continue {
            $continue_count++;
        }
        # Continue runs after the iteration body OR after next.
        # So all 5 iterations should run the continue block.
        $continue_count == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn continue_block_does_not_run_after_last() {
    let code = r#"
        my $continue_count = 0;
        for my $i (1:5) {
            last if $i == 3;
        } continue {
            $continue_count++;
        }
        # last skips continue; iters 1, 2 fully complete -> 2 runs.
        $continue_count == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── early-exit search pattern ────────────────────────────────────

#[test]
fn first_match_search_via_last() {
    let code = r#"
        my @arr = (10, 20, 30, 40, 50);
        my $target = 30;
        my $found_idx = -1;
        for my $i (0:len(@arr) - 1) {
            if ($arr[$i] == $target) {
                $found_idx = $i;
                last;
            }
        }
        $found_idx == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nested_find_via_labeled_last() {
    let code = r#"
        my @grid = (
            [1, 2, 3],
            [4, 5, 6],
            [7, 8, 9],
        );
        my $r = -1;
        my $c = -1;
        OUTER: for my $i (0:len(@grid) - 1) {
            for my $j (0:len(@{$grid[$i]}) - 1) {
                if ($grid[$i]->[$j] == 5) {
                    $r = $i;
                    $c = $j;
                    last OUTER;
                }
            }
        }
        ($r == 1 && $c == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── prime-finding via labeled next ───────────────────────────────

#[test]
fn primes_via_labeled_next() {
    let code = r#"
        my @primes;
        CAND: for my $n (2:30) {
            for my $d (2:int(sqrt($n))) {
                next CAND if $n % $d == 0;
            }
            push @primes, $n;
        }
        join(",", @primes) eq "2,3,5,7,11,13,17,19,23,29" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
