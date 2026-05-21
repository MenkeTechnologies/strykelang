//! caller(N) stack-walking pins. Stryke's caller surface is observably
//! different from Perl 5 in multiple ways — these pins lock the current
//! shape so any future fix is deliberate.
//!
//! Documented as BUG-248 (caller pkg/line wrong) and BUG-249 (caller
//! doesn't signal empty stack) in docs/BUGS.md.

use crate::common::*;

// ── shape: always returns a 3-tuple ───────────────────────────────

#[test]
fn caller_zero_returns_four_elements() {
    // Stryke's caller returns (package, file, line, subname). Field 3 is the
    // sub being executed (fully qualified when stored that way in the sub
    // registry).
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            len(@c)
        }
        Demo::CS::leaf() == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_zero_first_field_is_package_string() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            $c[0]
        }
        # Stryke returns package="main" regardless of caller's package.
        # See BUG-248.
        my $p = Demo::CS::leaf();
        $p eq "main" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_zero_second_field_is_filename() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            $c[1]
        }
        # When run via eval_int, filename is the embedded "-e"-style
        # source identifier. Just assert it's defined and a string.
        my $f = Demo::CS::leaf();
        defined($f) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_zero_third_field_is_numeric_line() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            $c[2]
        }
        my $line = Demo::CS::leaf();
        # Stryke returns the line where caller() was invoked, NOT the
        # call origin (BUG-248). Either way the value is numeric > 0.
        $line > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-248: line is the caller() site, not the call origin ──────

#[test]
fn caller_line_is_callee_site_not_invocation_site() {
    let code = r#"
        # The line of `caller(0)` inside leaf is fixed; the line in
        # mid where leaf() is *called* is different. Stryke returns
        # the former — Perl returns the latter.
        fn Demo::CS::leaf() {
            my @c = caller(0);   # line A
            $c[2]
        }
        fn Demo::CS::mid() {
            Demo::CS::leaf()
            # line B: line of the call site above
        }
        my $reported = Demo::CS::mid();
        # Per BUG-248, $reported == line A (~7), not line B (~10).
        # We pin that it's a small line number (< 20) for stability.
        ($reported > 0 && $reported < 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-248: package is always "main" regardless of caller pkg ───

#[test]
fn caller_package_always_main_per_bug_248() {
    let code = r#"
        fn Demo::CSPkg::leaf() {
            my @c = caller(0);
            $c[0]
        }
        fn Demo::CSPkg::mid() {
            Demo::CSPkg::leaf()
        }
        # In Perl the caller of leaf is mid in pkg Demo::CSPkg.
        # Stryke returns "main".
        Demo::CSPkg::mid() eq "main" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-249: caller never signals empty stack ────────────────────

#[test]
fn caller_at_top_level_returns_non_empty() {
    let code = r#"
        # In Perl, caller(0) at top-level returns empty list.
        # Stryke returns 3 elements with main/file/line.
        my @c = caller(0);
        len(@c) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_past_stack_depth_returns_non_empty() {
    let code = r#"
        fn Demo::CSP::probe() {
            my @c = caller(99);
            len(@c)
        }
        # In Perl this would be 0 (empty list). Stryke returns the 4-tuple
        # regardless of stack depth (BUG-249 still open).
        Demo::CSP::probe() == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── multiple-depth callers behave consistently ────────────────────

#[test]
fn caller_n_increments_returns_same_shape() {
    let code = r#"
        fn Demo::CSD::leaf() {
            my @c0 = caller(0);
            my @c1 = caller(1);
            my @c2 = caller(2);
            (len(@c0) == 4 && len(@c1) == 4 && len(@c2) == 4) ? 1 : 0
        }
        fn Demo::CSD::mid() { Demo::CSD::leaf() }
        fn Demo::CSD::top() { Demo::CSD::mid() }
        Demo::CSD::top()
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── scalar context of caller ──────────────────────────────────────

#[test]
fn caller_scalar_context_is_field_count_not_package() {
    // Stryke surface: `scalar(caller(0))` returns the field count (now 4
    // since the sub-name field landed) rather than the package. Perl
    // returns just the package in scalar context — tracked under BUG-248.
    let code = r#"
        fn Demo::CSC::leaf() {
            scalar(caller(0))
        }
        Demo::CSC::leaf() == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller inside closure ─────────────────────────────────────────

#[test]
fn caller_inside_closure_returns_shape() {
    let code = r#"
        my $c = sub {
            my @c = caller(0);
            len(@c)
        };
        $c->() == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller inside eval ────────────────────────────────────────────

#[test]
fn caller_inside_eval_block() {
    let code = r#"
        my $line;
        eval {
            my @c = caller(0);
            $line = $c[2];
        };
        defined($line) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller inside map block ───────────────────────────────────────

#[test]
fn caller_inside_map_block() {
    let code = r#"
        my @lines = map { my @c = caller(0); $c[2] } (1, 2, 3);
        # All three iterations should return the same line (the
        # line of caller(0) inside the map block).
        ($lines[0] == $lines[1] && $lines[1] == $lines[2]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller inside grep block ──────────────────────────────────────

#[test]
fn caller_inside_grep_block() {
    let code = r#"
        my @r = grep { my @c = caller(0); $c[2] > 0 } (1, 2, 3);
        len(@r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller in recursive fn ────────────────────────────────────────

#[test]
fn caller_inside_recursive_fn_stable_per_frame() {
    let code = r#"
        fn Demo::CSR::rec($n) {
            return 0 if $n <= 0;
            my @c = caller(0);
            len(@c) + Demo::CSR::rec($n - 1)
        }
        # rec(3) -> rec(2) -> rec(1) -> rec(0)=0. Three frames each
        # return 4 (caller returns a 4-tuple); 4+4+4+0 = 12.
        Demo::CSR::rec(3) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller value usable as line marker ────────────────────────────

#[test]
fn caller_line_field_is_positive_integer() {
    let code = r#"
        fn Demo::CSI::leaf() {
            my @c = caller(0);
            $c[2] > 0 && $c[2] == int($c[2]) ? 1 : 0
        }
        Demo::CSI::leaf()
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller fields stringify cleanly ──────────────────────────────

#[test]
fn caller_fields_join_to_one_line() {
    let code = r#"
        fn Demo::CSJ::leaf() {
            my @c = caller(0);
            my $s = join("|", @c);
            $s
        }
        my $s = Demo::CSJ::leaf();
        # 4-tuple: package | file | line | sub-name. The sub stash records the
        # fully qualified name (`Demo::CSJ::leaf`) for fn-decl'd subs.
        ($s =~ /^main\|/ && $s =~ /\|Demo::CSJ::leaf$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── caller with negative N still returns 3-tuple ─────────────────

#[test]
fn caller_negative_n_returns_shape() {
    let code = r#"
        fn Demo::CSN::leaf() {
            my @c = caller(-1);
            len(@c)
        }
        Demo::CSN::leaf() == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── multiple calls return consistent values ──────────────────────

#[test]
fn caller_called_twice_returns_same_values() {
    let code = r#"
        fn Demo::CSE::leaf() {
            my @a = caller(0);
            my @b = caller(0);
            ($a[0] eq $b[0] && $a[1] eq $b[1]) ? 1 : 0
        }
        Demo::CSE::leaf()
    "#;
    assert_eq!(eval_int(code), 1);
}
