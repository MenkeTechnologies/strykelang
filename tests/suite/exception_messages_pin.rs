//! Exception-message shape pins. `error_handling_pin.rs` covers
//! die/eval mechanics; this file pins the actual error-MESSAGE format
//! and class-dispatch idioms.

use crate::common::*;

// ── die "msg\n" → $@ holds literal message ──────────────────────────

#[test]
fn die_with_newline_terminator_preserves_message() {
    let code = r#"
        eval { die "expected_msg\n" };
        $@ eq "expected_msg\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn die_without_newline_appends_location_suffix() {
    let code = r#"
        eval { die "no_newline" };
        # Should start with "no_newline" but have " at <file> line <N>." appended.
        (index($@, "no_newline") == 0 && index($@, " at ") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die in nested function preserves the originating line ──────────

#[test]
fn die_in_nested_function_reports_source_line() {
    let code = r#"
        fn Demo::Exc::inner() { die "from_inner" }
        fn Demo::Exc::outer() { Demo::Exc::inner() }
        eval { Demo::Exc::outer() };
        # Message should mention "from_inner" + a line reference.
        (index($@, "from_inner") == 0 && index($@, "line") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Custom error class dispatch via ref + isa-like checks ──────────

#[test]
fn custom_error_hashref_with_class_field() {
    let code = r#"
        eval { die +{ class => "Demo::ParseError", line => 42, msg => "syntax" } };
        my $err = $@;
        (ref($err) =~ /HASH/
            && $err->{class} eq "Demo::ParseError"
            && $err->{line} == 42
            && $err->{msg} eq "syntax") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn error_class_dispatch_pattern() {
    let code = r#"
        fn Demo::Exc::handle($err) {
            if (ref($err) eq "HASH") {
                if ($err->{class} eq "Demo::IOError")    { return "io" }
                if ($err->{class} eq "Demo::ParseError") { return "parse" }
            }
            return "unknown"
        }
        my $r1 = Demo::Exc::handle(+{ class => "Demo::IOError",    msg => "x" });
        my $r2 = Demo::Exc::handle(+{ class => "Demo::ParseError", msg => "y" });
        my $r3 = Demo::Exc::handle("plain string");
        ($r1 eq "io" && $r2 eq "parse" && $r3 eq "unknown") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die in eval-string propagates the message ───────────────────────

#[test]
fn die_in_eval_string_caught_via_dollar_at() {
    let code = r#"
        eval "die \"from_string\\n\"";
        $@ eq "from_string\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── eval-string parse error sets $@ ─────────────────────────────────

#[test]
fn eval_string_parse_error_sets_dollar_at() {
    let code = r#"
        eval "this is not valid )))";
        $@ ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── undef value used as object errors clearly ───────────────────────

#[test]
fn undef_method_call_errors_under_eval() {
    let code = r#"
        my $obj;
        eval { $obj->some_method };
        # Some non-empty error message.
        $@ ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die after partial state mutation: rollback by caller ───────────

#[test]
fn caller_can_rollback_after_partial_mutation_via_die() {
    let code = r#"
        my @log;
        eval {
            push @log, "started";
            push @log, "step1";
            die "midway\n";
            push @log, "never_reached";
        };
        ($@ eq "midway\n"
            && len(@log) == 2
            && $log[1] eq "step1") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── try multiple times, accumulate errors ──────────────────────────

#[test]
fn collect_errors_across_attempts() {
    let code = r#"
        my @errors;
        for my $i (1, 2, 3) {
            eval {
                die "attempt_${i}_failed\n" if $i != 2;
            };
            push @errors, $@ if $@;
        }
        (len(@errors) == 2
            && $errors[0] eq "attempt_1_failed\n"
            && $errors[1] eq "attempt_3_failed\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chained errors: rethrow with context ───────────────────────────

#[test]
fn rethrow_with_extra_context() {
    let code = r#"
        eval {
            eval { die "root_cause\n" };
            if ($@) {
                die "wrapped: $@";
            }
        };
        # $@ should contain "wrapped: " prefix and "root_cause".
        (index($@, "wrapped: ") == 0 && index($@, "root_cause") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Wrapping die with arrayref payload (multi-arg "details") ──────

#[test]
fn arrayref_payload_decoded_in_handler() {
    let code = r#"
        fn Demo::Exc::http_error($status, $url, $msg) {
            die [$status, $url, $msg]
        }
        eval { Demo::Exc::http_error(404, "/api/users", "not found") };
        my $err = $@;
        (ref($err) =~ /ARRAY/
            && $err->[0] == 404
            && $err->[1] eq "/api/users"
            && $err->[2] eq "not found") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Error survives through map / grep ───────────────────────────────

#[test]
fn die_inside_map_caught_at_outer_eval() {
    let code = r#"
        my $stopped_at;
        eval {
            my @r = map {
                $stopped_at = $_;
                die "stop\n" if $_ == 3;
                $_ * 2
            } (1, 2, 3, 4, 5);
        };
        ($stopped_at == 3 && $@ eq "stop\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── No-error path: $@ stays empty ──────────────────────────────────

#[test]
fn dollar_at_empty_after_successful_eval() {
    let code = r#"
        eval { 1 + 1 };
        $@ eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Common idiom: "die ... unless ..." for guards ──────────────────

#[test]
fn die_unless_guard_pattern() {
    let code = r#"
        fn Demo::Exc::require_positive($n) {
            die "negative_or_zero\n" unless $n > 0;
            return $n * 2;
        }
        my @r;
        my @errs;
        for my $v (5, -1, 10, 0, 3) {
            my $ok = eval { Demo::Exc::require_positive($v) };
            if ($@) {
                push @errs, $v;
            } else {
                push @r, $ok;
            }
        }
        (len(@r) == 3
            && join(",", @r) eq "10,20,6"
            && len(@errs) == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── die in BEGIN-like phase (top-level) caught by main eval ───────

#[test]
fn top_level_die_caught_by_program_eval() {
    let code = r#"
        my $r = eval {
            die "early\n" if 1;
            42
        };
        (!defined($r) && $@ eq "early\n") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Custom-class object pattern via hashref ────────────────────────

#[test]
fn custom_error_class_via_blessed_hashref_pattern() {
    // Stryke doesn't have `bless` semantics here, but the pattern of
    // hashref-with-class-key works fine as an exception class.
    let code = r#"
        fn Demo::Exc::make_error($class, $msg) {
            +{ class => $class, msg => $msg, ts => time() }
        }
        eval {
            die Demo::Exc::make_error("NotFound", "user 42 missing")
        };
        ($@->{class} eq "NotFound" && $@->{msg} eq "user 42 missing") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
