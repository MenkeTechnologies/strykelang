//! Behavior-pinning batch R (2026-05-05): coderef-in-block-position. Any
//! per-element list operator (`grep`, `map`, `sort`, `first`, `any`, `all`,
//! `none`, `take_while`, `drop_while`, `reject`, `partition`, `min_by`,
//! `max_by`) accepts a coderef-shaped expression where a `{ BLOCK }`
//! would otherwise be required, including in `|>` pipe-forward stages.
//! Threading (`~>`) is intentionally excluded — whitespace-delimited
//! stages can't disambiguate `~> @l grep $f` from two stages. `--compat`
//! mode preserves Perl `grep EXPR, LIST` truthiness semantics.
//!
//! These tests pin the runtime-dispatch contract:
//!   1. If the EXPR result is a `CodeRef`, call it with the element(s)
//!      as positional args. The call result drives filter/map/cmp.
//!   2. Otherwise, evaluate as a normal value (regex match against `$_`,
//!      truthiness for grep, mapped value for map, comparator int for
//!      sort).

use crate::common::*;

// ── grep with coderef (Phase 1: VM dispatch in Op::GrepWithExpr) ────────────

#[test]
fn grep_with_scalar_coderef_calls_predicate_per_element() {
    // Pre-fix: `grep $f, @l` filtered by truthiness of `$f` (always true for
    // a coderef → kept all 5). Now: dispatches `$f($_)` per element.
    let s = eval_string(
        r#"my $f = fn ($x) { $x > 2 };
           my @l = (1, 2, 3, 4, 5);
           join(",", grep $f, @l)"#,
    );
    assert_eq!(s, "3,4,5");
}

#[test]
fn grep_with_coderef_via_hash_slot_dispatches() {
    // The expression in block-position can be any coderef-yielding form,
    // not just a scalar var. Hash-slot deref is the common HOF idiom.
    let s = eval_string(
        r#"my %ops = (big => fn ($x) { $x > 3 });
           my @l = (1, 2, 3, 4, 5);
           join(",", grep $ops{big}, @l)"#,
    );
    assert_eq!(s, "4,5");
}

#[test]
fn grep_with_lambda_using_topic_works_via_coderef_call() {
    // The lambda reads `$_` (set via set_closure_args inside call_sub)
    // rather than the named param. Verifies _ propagates.
    let s = eval_string(
        r#"my $f = fn { $_ > 2 };
           my @l = (1, 2, 3, 4, 5);
           join(",", grep $f, @l)"#,
    );
    assert_eq!(s, "3,4,5");
}

// ── map with coderef ────────────────────────────────────────────────────────

#[test]
fn map_with_scalar_coderef_applies_per_element() {
    // Pre-fix: `map $f, @l` evaluated `$f` per iter → 5 copies of the
    // stringified coderef. Now: calls `$f($_)` and collects results.
    let s = eval_string(
        r#"my $double = fn ($x) { $x * 2 };
           my @l = (1, 2, 3);
           join(",", map $double, @l)"#,
    );
    assert_eq!(s, "2,4,6");
}

#[test]
fn map_with_coderef_returning_pair_flattens() {
    // `map { (a, b) }` returns flat list. Same for coderef call returning
    // a list — `map_flatten_outputs` handles both paths uniformly.
    let s = eval_string(
        r#"my $pair = fn ($x) { ($x, $x * 10) };
           my @l = (1, 2);
           join(",", map $pair, @l)"#,
    );
    assert_eq!(s, "1,10,2,20");
}

// ── pipe-forward (Phase 3: parser routes |> grep $f to GrepExprComma) ──────

#[test]
fn pipe_forward_grep_with_coderef_dispatches() {
    // `|> grep $f` previously synthesized a block `{ $f }` and went through
    // Op::GrepWithBlock — which doesn't dispatch coderefs. Now routes
    // through GrepExprComma so the dispatch fires.
    let s = eval_string(
        r#"my $is_big = fn ($x) { $x > 2 };
           my @l = (1, 2, 3, 4, 5);
           join(",", @l |> grep $is_big)"#,
    );
    assert_eq!(s, "3,4,5");
}

#[test]
fn pipe_forward_map_with_coderef_dispatches() {
    let s = eval_string(
        r#"my $sq = fn ($x) { $x * $x };
           my @l = (1, 2, 3);
           join(",", @l |> map $sq)"#,
    );
    assert_eq!(s, "1,4,9");
}

#[test]
fn pipe_forward_grep_with_bareword_still_lifts_to_topic_call() {
    // `|> grep is_big` should still desugar to `grep { is_big($_) } @l` —
    // not be mistaken for a coderef-call. Ensures GrepExprComma's lift
    // path was preserved when we rerouted the pipe-rhs.
    let s = eval_string(
        r#"sub is_big { $_[0] > 2 }
           my @l = (1, 2, 3, 4, 5);
           join(",", @l |> grep is_big)"#,
    );
    assert_eq!(s, "3,4,5");
}

// ── sort with coderef + positional ($a, $b) ────────────────────────────────

#[test]
fn sort_with_lambda_receives_positional_a_b() {
    // Pre-fix: `call_sub(&sub, vec![], ...)` left positional params undef,
    // so `fn ($a, $b) { ... }` always saw undef => Equal. Now passes
    // `($a, $b)` as args.
    let s = eval_string(
        r#"my $cmp = fn ($a, $b) { $b <=> $a };
           join(",", sort $cmp (3, 1, 4, 1, 5))"#,
    );
    assert_eq!(s, "5,4,3,1,1");
}

#[test]
fn sort_with_perl_style_block_using_a_b_globals_still_works() {
    // Block form reads `$a`/`$b` as package globals via set_sort_pair.
    // Verifies we didn't break the legacy path.
    let s = eval_string(r#"join(",", sort { $a <=> $b } (3, 1, 4, 1, 5))"#);
    assert_eq!(s, "1,1,3,4,5");
}

#[test]
fn sort_pipe_forward_with_coderef_dispatches() {
    let s = eval_string(
        r#"my $cmp = fn ($a, $b) { $b <=> $a };
           my @l = (3, 1, 4, 1, 5);
           join(",", @l |> sort $cmp)"#,
    );
    assert_eq!(s, "5,4,3,1,1");
}

// ── tier-2 builtins (Phase 4) ──────────────────────────────────────────────

#[test]
fn first_with_no_paren_coderef_returns_first_match() {
    // Pre-fix: parser required `first(...)` parens or `first { } LIST`.
    // Now: `first $f LIST` works too.
    assert_eq!(
        eval_int(
            r#"my $is_big = fn ($x) { $x > 3 };
               first $is_big, (1, 2, 3, 4, 5)"#
        ),
        4
    );
}

#[test]
fn any_all_none_with_no_paren_coderef_dispatches() {
    // Pre-fix: any_all_none called sub with empty args → lambda saw undef.
    let s = eval_string(
        r#"my $is_big = fn ($x) { $x > 3 };
           my @l = (1, 2, 3, 4, 5);
           "any=" . (any $is_big, @l) . ",all=" . (all $is_big, @l) . ",none=" . (none $is_big, @l)"#,
    );
    assert_eq!(s, "any=1,all=0,none=0");
}

#[test]
fn take_while_with_coderef_lambda_stops_at_first_failure() {
    // Pre-fix: `list_higher_order_block_builtin_exec` used
    // `exec_block(&sub.body)` which set $_ but didn't bind positional
    // params. Lambda saw undef and `take_while $f, @l` returned all
    // elements unconditionally. Now uses call_sub with positional args.
    let s = eval_string(
        r#"my $is_small = fn ($x) { $x < 4 };
           my @l = (1, 2, 3, 4, 5);
           join(",", take_while $is_small, @l)"#,
    );
    assert_eq!(s, "1,2,3");
}

#[test]
fn drop_while_with_coderef_lambda_returns_tail() {
    let s = eval_string(
        r#"my $is_small = fn ($x) { $x < 4 };
           my @l = (1, 2, 3, 4, 5);
           join(",", drop_while $is_small, @l)"#,
    );
    assert_eq!(s, "4,5");
}

#[test]
fn reject_with_coderef_lambda_inverts_grep() {
    let s = eval_string(
        r#"my $is_big = fn ($x) { $x > 2 };
           my @l = (1, 2, 3, 4, 5);
           join(",", reject $is_big, @l)"#,
    );
    assert_eq!(s, "1,2");
}

#[test]
fn min_by_with_coderef_lambda_uses_positional() {
    // `min_by` uses key function; the absolute-value example flips
    // ordering vs. raw min, so this is a real regression catcher.
    assert_eq!(
        eval_int(
            r#"my $abs = fn ($x) { $x < 0 ? -$x : $x };
               min_by $abs, (-5, -1, 3)"#
        ),
        -1
    );
}

// ── --compat mode preserves Perl truthiness semantics ──────────────────────
//
// Not pinned as an integration test: `set_compat_mode` mutates a shared
// `AtomicBool` (`strykelang/lib.rs:125`) read by every concurrent test. A
// `#[test]` that flips it races every dispatch-sensitive test in this file.
// The negative case is verified via CLI smoke test:
//   stryke --compat -e 'my $f = fn ($x) { $x > 2 };
//                       my @l = (1, 2, 3, 4, 5);
//                       print join(",", grep $f, @l)'
// expected output: 1,2,3,4,5  (Perl truthiness; coderef is always truthy)
// The matching guards are at `vm.rs::map_with_expr_common`,
// `vm.rs::Op::GrepWithExpr`, `vm_helper.rs::ExprKind::GrepExprComma|MapExprComma`
// — all of them check `if !crate::compat_mode()` before dispatching.

// ── Regression: non-coderef EXPR forms still work ──────────────────────────

#[test]
fn grep_regex_form_still_matches_topic() {
    // `grep /pat/, @l` desugars to `grep { $_ =~ /pat/ } @l`. A regex
    // value is not a coderef, so dispatch must skip and fall to the
    // existing `as_regex()` branch.
    let s = eval_string(
        r#"my @words = ("apple", "banana", "cherry");
           join(",", grep /a/, @words)"#,
    );
    assert_eq!(s, "apple,banana");
}

#[test]
fn grep_inline_expr_truthiness_still_works() {
    // `grep $_ > 3, @l` evaluates the comparison per element. Result is
    // an integer, not a coderef — dispatch must not interfere.
    let s = eval_string(
        r#"my @l = (1, 2, 3, 4, 5);
           join(",", grep $_ > 3, @l)"#,
    );
    assert_eq!(s, "4,5");
}

#[test]
fn grep_block_form_with_explicit_coderef_call_still_works() {
    // The block form `grep { $f($_) } @l` was already canonical. The
    // coderef-dispatch we added applies to EXPR-form only; the block's
    // body is user-controlled and must not be auto-dispatched even if
    // the block returns a coderef.
    let s = eval_string(
        r#"my $f = fn ($x) { $x > 2 };
           my @l = (1, 2, 3, 4, 5);
           join(",", grep { $f->($_) } @l)"#,
    );
    assert_eq!(s, "3,4,5");
}

// ── Positional arg passthrough: $_ / $_0 / $_1 ─────────────────────────────

#[test]
fn map_lambda_can_use_underscore_zero_alias_for_topic() {
    // `set_closure_args(&args)` sets $_ to args[0] AND $_0 to args[0].
    // So `fn { _0 * 10 }` is equivalent to `fn { _ * 10 }` for 1-arg
    // dispatch (map/grep/first/etc).
    let s = eval_string(
        r#"my $f = fn { _0 * 10 };
           join(",", map $f, (1, 2, 3))"#,
    );
    assert_eq!(s, "10,20,30");
}

#[test]
fn sort_lambda_can_use_underscore_zero_and_one_for_a_b() {
    // 2-arg dispatch sets $_ = $_0 = a, $_1 = b. Verifies both numeric
    // shortcut forms work as comparator args.
    let s = eval_string(
        r#"my $cmp = fn { _0 <=> _1 };
           join(",", sort $cmp (3, 1, 4))"#,
    );
    assert_eq!(s, "1,3,4");
}

#[test]
fn map_lambda_underscore_one_is_undef_for_single_arg_dispatch() {
    // map dispatches with 1 arg. `_1` reads the (nonexistent) second
    // positional → undef. `(_1 // 99)` uses defined-or to expose it.
    let s = eval_string(
        r#"my $f = fn { _ + (_1 // 99) };
           join(",", map $f, (1, 2, 3))"#,
    );
    assert_eq!(s, "100,101,102");
}

// ── Indexed topic-chain ascent: `_<N` ≡ `_<<<...<` (N chevrons) ────────────

#[test]
fn indexed_ascent_one_equals_chevron_form() {
    // `$_<1` ≡ `$_<` — depth 1 (one frame up).
    let s = eval_string(
        r#"my $r = "";
           ~> 100..100 map {
               ~> 10..10 map {
                   $r = ($_<1 == $_<) ? "eq" : "neq"
               }
           };
           $r"#,
    );
    assert_eq!(s, "eq");
}

#[test]
fn indexed_ascent_three_equals_three_chevrons() {
    // `$_<3` ≡ `$_<<<` — three frames up. Both should resolve to the
    // outermost map's iteration value (1).
    let s = eval_string(
        r#"my $r = "";
           ~> 1..1 map {
               ~> 10..10 map {
                   ~> 100..100 map {
                       ~> 1000..1000 map {
                           $r = ($_<3 == $_<<<) ? "eq:" . $_<3 : "neq"
                       }
                   }
               }
           };
           $r"#,
    );
    // depth 0 = 1000, depth 1 = 100, depth 2 = 10, depth 3 = 1.
    assert_eq!(s, "eq:1");
}

#[test]
fn indexed_ascent_depth_five_reachable_after_cap_bump() {
    // Cap is now 5; `$_<5` ≡ `$_<<<<<` should resolve to the same value
    // (whatever ends up at level 5 after 5 nested shifts).
    let s = eval_string(
        r#"my $r = "";
           ~> 1..1 map {
               ~> 10..10 map {
                   ~> 100..100 map {
                       ~> 1000..1000 map {
                           ~> 10000..10000 map {
                               $r = ($_<5 == $_<<<<<) ? "match" : "mismatch"
                           }
                       }
                   }
               }
           };
           $r"#,
    );
    assert_eq!(s, "match");
}

#[test]
fn indexed_ascent_string_interpolation_picks_up_n() {
    // `"$_<2"` inside a double-quoted string interpolates as the depth-2
    // ref, not as `$_<` followed by literal "2". This requires the parser's
    // string-interpolation path (parser.rs:15338+) to mirror the lexer.
    let s = eval_string(
        r#"my $r = "";
           ~> 1..1 map {
               ~> 10..10 map {
                   ~> 100..100 map {
                       $r = "$_<2"
                   }
               }
           };
           $r"#,
    );
    // depth 0 = 100, depth 1 = 10, depth 2 = 1.
    assert_eq!(s, "1");
}

#[test]
fn indexed_ascent_for_positional_alias() {
    // `_M<N` is also rewritten — `_0<` and `_0<1` should agree, since `_0`
    // aliases the topic. Verifies the lexer handles `_<digits><digits>`
    // patterns where the leading digits are the positional and the trailing
    // ones are the indexed ascent.
    let s = eval_string(
        r#"my $r = "";
           ~> 100..100 map {
               ~> 10..10 map {
                   $r = ($_0<1 == $_0<) ? "match" : "mismatch"
               }
           };
           $r"#,
    );
    assert_eq!(s, "match");
}
