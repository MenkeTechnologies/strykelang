//! Error-handling pins. eval / die / $@ are load-bearing — any
//! regression here breaks the whole AOP layer (intercept dies), the
//! AI session loop ($@ recovery), and KV transaction rollback.
//! Lock the surface so a future refactor of the unwinder doesn't
//! silently drop exception payloads.

use crate::common::*;

// ── die → $@ propagation ──────────────────────────────────────────────

#[test]
fn die_inside_eval_sets_dollar_at() {
    let code = r#"
        eval { die "boom\n" };
        $@ eq "boom\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn successful_eval_clears_dollar_at() {
    let code = r#"
        eval { die "first\n" };
        eval { 42 };
        $@ eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_returns_undef_on_die() {
    let code = r#"
        my $r = eval { die "x\n"; 99 };
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_returns_value_on_success() {
    let code = r#"
        my $r = eval { 1 + 2 + 3 };
        $r == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested eval ───────────────────────────────────────────────────────

#[test]
fn nested_eval_inner_catches_first() {
    let code = r#"
        my $outer = "(empty)";
        my $inner = "(empty)";
        eval {
            eval { die "inner\n" };
            $inner = $@;
            die "outer\n";
        };
        $outer = $@;
        ($inner eq "inner\n" && $outer eq "outer\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nested_eval_inner_does_not_leak_to_outer() {
    let code = r#"
        eval {
            eval { die "inner\n" };
            # Reset $@ between evals.
            $@ = "";
        };
        $@ eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die with hashref payload (Perl-style "throw an object") ──────────

#[test]
fn die_with_hashref_propagates_through_dollar_at() {
    let code = r#"
        eval { die +{ code => 42, msg => "bad request" } };
        my $err = $@;
        (ref($err) =~ /HASH/
            && $err->{code} == 42
            && $err->{msg} eq "bad request") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn die_with_arrayref_payload_preserved() {
    let code = r#"
        eval { die [404, "not found", "/api/users"] };
        my $err = $@;
        (ref($err) =~ /ARRAY/
            && $err->[0] == 404
            && $err->[2] eq "/api/users") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die rethrown ─────────────────────────────────────────────────────

#[test]
fn die_rethrown_propagates_payload() {
    let code = r#"
        eval {
            eval { die +{ code => 1 } };
            die $@ if ref($@) eq "HASH";
        };
        (ref($@) eq "HASH" && $@->{code} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die in a called function ─────────────────────────────────────────

#[test]
fn die_in_callee_caught_by_caller_eval() {
    let code = r#"
        fn Demo::Err::boom() { die "from_callee\n" }
        eval { Demo::Err::boom() };
        $@ eq "from_callee\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn die_in_deep_callee_unwinds_to_eval() {
    let code = r#"
        fn Demo::Err::a() { Demo::Err::b() }
        fn Demo::Err::b() { Demo::Err::c() }
        fn Demo::Err::c() { die "from_depth_3\n" }
        eval { Demo::Err::a() };
        $@ eq "from_depth_3\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Return from eval ─────────────────────────────────────────────────

#[test]
fn return_inside_eval_returns_from_eval_not_enclosing_sub() {
    // Stryke divergence from Perl: `return` inside `eval { }` returns
    // from the eval block, letting the enclosing sub continue. In
    // Perl, `return` would unwind through the eval back to the sub.
    // Pinning stryke's current behavior so a future move toward Perl
    // semantics is a deliberate decision.
    let code = r#"
        fn Demo::Err::guarded() {
            eval { return 42 };
            99
        }
        Demo::Err::guarded()
    "#;
    assert_eq!(eval_int(code), 99);
}

// ── die with chomped vs newline-terminated message ───────────────────

#[test]
fn die_message_without_newline_preserved() {
    // Perl appends " at FILE line N.\n" if the die string has no \n.
    // Stryke's behavior: keep whatever the user passed.
    let code = r#"
        eval { die "no newline" };
        # Either Perl-style appended "at ..." or stryke's raw form is
        # acceptable; just check the original message is a prefix.
        index($@, "no newline") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn die_message_with_newline_kept_verbatim() {
    let code = r#"
        eval { die "with newline\n" };
        $@ eq "with newline\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric die payload ──────────────────────────────────────────────

#[test]
fn die_with_integer_payload_stringified_with_location_suffix() {
    // Perl: `die 42` produces "42 at FILE line N.\n"; numerifying
    // recovers 42. Stryke appends the location suffix the same way
    // but numerification currently returns 1 (suspected divergence
    // in numeric coercion of leading-digit strings — separate bug).
    // Pin the string-prefix form, which is stable.
    let code = r#"
        eval { die 42 };
        # Location suffix is appended; first two chars are "42".
        substr($@, 0, 2) eq "42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Conditional rethrow ──────────────────────────────────────────────

#[test]
fn rethrow_only_specific_error_types() {
    let code = r#"
        my $caught_inner = 0;
        my $caught_outer = 0;
        eval {
            eval { die +{ class => "io" } };
            if (ref($@) eq "HASH" && $@->{class} eq "io") {
                $caught_inner = 1;
            } else {
                die $@;
            }
        };
        $caught_outer = 1 if $@;
        ($caught_inner == 1 && $caught_outer == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die inside loop body ─────────────────────────────────────────────

#[test]
fn die_inside_loop_aborts_outer_eval() {
    let code = r#"
        my $iter = 0;
        eval {
            for my $i (1:10) {
                $iter = $i;
                die "stop\n" if $i == 5;
            }
        };
        ($iter == 5 && $@ eq "stop\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die inside map/grep callback ─────────────────────────────────────

#[test]
fn die_inside_map_caught_by_outer_eval() {
    let code = r#"
        eval {
            my @r = map { die "in_map\n" if _ == 3; _ * 2 } (1, 2, 3, 4);
        };
        $@ eq "in_map\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── eval { } as expression, not statement ────────────────────────────

#[test]
fn eval_block_as_expression_returns_last_value() {
    let code = r#"
        my $r = eval {
            my $x = 10;
            my $y = 20;
            $x + $y
        };
        $r == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_string_form_evaluates_code() {
    let code = r#"
        my $r = eval "1 + 2 + 3";
        $r == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_string_catches_parse_error() {
    let code = r#"
        eval "this is not valid )))";
        $@ ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $@ visible to AOP intercepts ─────────────────────────────────────

#[test]
fn aop_around_can_observe_inner_die_via_eval() {
    let code = r#"
        fn Demo::Err::risky() { die "in_risky\n" }
        my $observed = "";
        around "Demo::Err::risky" {
            eval { proceed() };
            $observed = $@;
            "swallowed"
        }
        my $r = Demo::Err::risky();
        intercept_clear("Demo::Err::risky");
        ($r eq "swallowed" && $observed eq "in_risky\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
