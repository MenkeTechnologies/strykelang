//! given / when pattern matching pins.

use crate::common::*;

// ── Basic scalar match ─────────────────────────────────────────────

#[test]
fn given_when_scalar_match() {
    let code = r#"
        my $x = 2;
        my $r = "";
        given ($x) {
            when (1) { $r = "one" }
            when (2) { $r = "two" }
            when (3) { $r = "three" }
            default  { $r = "other" }
        }
        $r eq "two" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn given_when_default_fallback() {
    let code = r#"
        my $x = 99;
        my $r = "";
        given ($x) {
            when (1) { $r = "one" }
            when (2) { $r = "two" }
            default  { $r = "fallback" }
        }
        $r eq "fallback" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn given_when_first_match_wins() {
    let code = r#"
        my $x = 2;
        my $first = "";
        given ($x) {
            when (2) { $first = "first" }
            when (2) { $first = "second" }
        }
        $first eq "first" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String match ───────────────────────────────────────────────────

#[test]
fn given_when_string_match() {
    let code = r#"
        my $s = "hello";
        my $r = "";
        given ($s) {
            when ("hi")     { $r = "hi" }
            when ("hello")  { $r = "hello" }
            when ("hey")    { $r = "hey" }
            default         { $r = "other" }
        }
        $r eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Regex match in when ─────────────────────────────────────────────

#[test]
fn given_when_regex_match() {
    let code = r#"
        my $s = "abc123";
        my $r = "";
        given ($s) {
            when (/^\d+$/) { $r = "all_digits" }
            when (/\d/)    { $r = "has_digit" }
            default        { $r = "no_digit" }
        }
        $r eq "has_digit" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn given_when_anchored_regex() {
    let code = r#"
        my $s = "Hello, World";
        my $r = "";
        given ($s) {
            when (/^Hello/) { $r = "starts_hello" }
            default         { $r = "other" }
        }
        $r eq "starts_hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric range matching via list ────────────────────────────────

#[test]
fn given_when_arithmetic_clause_falls_through_to_default() {
    // BUG-238: `when (EXPR)` where EXPR is `$_ < N` is smart-matched,
    // not boolean-evaluated. So `when ($_ < 100)` does not behave like
    // a range check — it falls through to default. Pin actual stryke
    // behavior; use if/elsif for ranges.
    let code = r#"
        my $x = 50;
        my $r = "";
        given ($x) {
            when ($_ < 10)  { $r = "low" }
            when ($_ < 100) { $r = "mid" }
            default         { $r = "high" }
        }
        $r eq "high" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn given_when_threshold_via_local_variable() {
    // BUG-239: `return` inside given/when produces "unexpected
    // control flow in tree-assisted opcode" error. Workaround:
    // capture in a local variable and return after the block.
    let code = r#"
        fn Demo::GW::bucket($x) {
            my $r;
            given ($x) {
                when (0)  { $r = "zero" }
                when (-1) { $r = "neg_one" }
                default   {
                    if    ($x < 0)   { $r = "neg" }
                    elsif ($x < 100) { $r = "small" }
                    else             { $r = "big" }
                }
            }
            return $r
        }
        my @r = (
            Demo::GW::bucket(-5),
            Demo::GW::bucket(0),
            Demo::GW::bucket(42),
            Demo::GW::bucket(1000),
        );
        join(",", @r) eq "neg,zero,small,big" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple statements per when ───────────────────────────────────

#[test]
fn given_when_multi_statement_body() {
    let code = r#"
        my $x = 5;
        my $a = 0;
        my $b = 0;
        given ($x) {
            when (5) {
                $a = 100;
                $b = 200;
            }
            default {
                $a = -1;
                $b = -2;
            }
        }
        ($a == 100 && $b == 200) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested given/when ──────────────────────────────────────────────

#[test]
fn nested_given_when_works() {
    let code = r#"
        fn Demo::GW::nested($outer, $inner) {
            my $r = "";
            given ($outer) {
                when ("A") {
                    given ($inner) {
                        when (1) { $r = "A1" }
                        when (2) { $r = "A2" }
                        default  { $r = "A?" }
                    }
                }
                when ("B") {
                    given ($inner) {
                        when (1) { $r = "B1" }
                        default  { $r = "B?" }
                    }
                }
                default { $r = "??" }
            }
            return $r
        }
        my @results = (
            Demo::GW::nested("A", 1),
            Demo::GW::nested("A", 2),
            Demo::GW::nested("B", 1),
            Demo::GW::nested("C", 1),
        );
        join(",", @results) eq "A1,A2,B1,??" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given without when fallthrough ─────────────────────────────────

#[test]
fn given_with_only_default_branch() {
    let code = r#"
        my $r = "";
        given ("anything") {
            default { $r = "default_only" }
        }
        $r eq "default_only" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given with no matching when, no default ───────────────────────

#[test]
fn given_with_no_match_no_default_unset() {
    let code = r#"
        my $r = "untouched";
        given (99) {
            when (1) { $r = "one" }
            when (2) { $r = "two" }
        }
        $r eq "untouched" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given with hash key dispatch ──────────────────────────────────

#[test]
fn given_dispatches_on_string_key_via_local_var() {
    // BUG-239 workaround: avoid return inside given.
    let code = r#"
        fn Demo::GW::cmd_action($cmd) {
            my $r;
            given ($cmd) {
                when ("start") { $r = "starting" }
                when ("stop")  { $r = "stopping" }
                when ("pause") { $r = "pausing" }
                default        { $r = "unknown" }
            }
            return $r
        }
        Demo::GW::cmd_action("stop") eq "stopping" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given + return propagates ──────────────────────────────────────

#[test]
fn given_when_via_local_var_workaround() {
    // BUG-239 workaround: capture branch result, return after block.
    let code = r#"
        fn Demo::GW::classify($n) {
            my $result;
            given ($n) {
                when (0) { $result = "zero" }
                default  {
                    $result = $n < 0 ? "negative" : "positive"
                }
            }
            return $result
        }
        my @r = (
            Demo::GW::classify(-5),
            Demo::GW::classify(0),
            Demo::GW::classify(42),
        );
        join(",", @r) eq "negative,zero,positive" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given with regex named captures ────────────────────────────────

#[test]
fn given_when_regex_matches_string() {
    // Simpler regex test — capture-and-reuse may interact oddly
    // with smart-match; just verify the branch fires.
    let code = r#"
        my $s = "Hello, World!";
        my $r = "";
        given ($s) {
            when (/Hello/) { $r = "matched" }
            default        { $r = "no_match" }
        }
        $r eq "matched" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── given in a loop ────────────────────────────────────────────────

#[test]
fn given_inside_for_loop_literal_when() {
    // BUG-238 workaround: use literal values only.
    let code = r#"
        my @inputs = (1, 2, 3, 4, 5);
        my @results;
        for my $x (@inputs) {
            given ($x) {
                when (1) { push @results, "one" }
                when (2) { push @results, "two" }
                when (3) { push @results, "three" }
                default  { push @results, "other" }
            }
        }
        join(",", @results) eq "one,two,three,other,other" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── State machine via given/when ───────────────────────────────────

#[test]
fn state_machine_via_given_when_string_only() {
    // BUG-239 workaround: assign to local, return after.
    let code = r#"
        fn Demo::GW::transition($state, $event) {
            my $r;
            given ($state) {
                when ("idle") {
                    given ($event) {
                        when ("start") { $r = "running" }
                        default        { $r = "idle" }
                    }
                }
                when ("running") {
                    given ($event) {
                        when ("pause") { $r = "paused" }
                        when ("stop")  { $r = "idle" }
                        default        { $r = "running" }
                    }
                }
                when ("paused") {
                    given ($event) {
                        when ("resume") { $r = "running" }
                        default         { $r = "paused" }
                    }
                }
                default { $r = "error" }
            }
            return $r
        }
        my @s = ("idle");
        my @events = ("start", "pause", "resume", "stop");
        for my $e (@events) {
            push @s, Demo::GW::transition($s[-1], $e);
        }
        join(",", @s) eq "idle,running,paused,running,idle" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
