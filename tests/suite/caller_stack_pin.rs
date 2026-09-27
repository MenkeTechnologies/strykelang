//! caller(N) stack-walking. Each expectation is stock perl's answer for the
//! same program (BUG-248 / BUG-249, fixed): the package and line are those of
//! the *call site*, the sub name is fully qualified, `caller(N)` in list
//! context has perl's 11 fields (the bare `caller` form 3), scalar context is
//! the package, and a level past the outermost frame — including top level —
//! is the empty list.

use crate::common::*;

// ── shape ─────────────────────────────────────────────────────────

#[test]
fn caller_zero_returns_eleven_fields() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            len(@c)
        }
        Demo::CS::leaf() == 11 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bare_caller_returns_three_fields() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller;
            len(@c)
        }
        Demo::CS::leaf() == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_zero_first_field_is_calling_package() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            $c[0]
        }
        # Called from top-level code in `main`.
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
        my $f = Demo::CS::leaf();
        defined($f) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── the line is the call site, not the `caller` site ─────────────

#[test]
fn caller_line_is_the_call_site() {
    let code = r#"
        fn Demo::CS::leaf() {
            my @c = caller(0);
            $c[2]
        }
        fn Demo::CS::mid() {
            my $x = 0;
            Demo::CS::leaf()
        }
        Demo::CS::mid()
    "#;
    // Line 8 of the program is the `Demo::CS::leaf()` call inside `mid`.
    assert_eq!(eval_int(code), 8);
}

// ── the package is the calling code's package ────────────────────

#[test]
fn caller_package_is_the_calling_subs_package() {
    let code = r#"
        fn Demo::CSPkg::leaf() {
            my @c = caller(0);
            $c[0]
        }
        fn Demo::CSPkg::mid() {
            Demo::CSPkg::leaf()
        }
        Demo::CSPkg::mid() eq "Demo::CSPkg" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── no frame: the empty list ─────────────────────────────────────

#[test]
fn caller_at_top_level_is_empty() {
    let code = r#"
        my @c = caller(0);
        len(@c) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caller_past_stack_depth_is_empty() {
    let code = r#"
        fn Demo::CSP::probe() {
            my @c = caller(99);
            len(@c)
        }
        Demo::CSP::probe()
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn caller_negative_n_is_empty() {
    let code = r#"
        fn Demo::CSN::leaf() {
            my @c = caller(-1);
            len(@c)
        }
        Demo::CSN::leaf()
    "#;
    assert_eq!(eval_int(code), 0);
}

// ── walking up ───────────────────────────────────────────────────

#[test]
fn caller_n_walks_one_frame_per_level() {
    let code = r#"
        fn Demo::CSD::leaf() {
            my @past = caller(3);
            join(",", (caller(0))[3], (caller(1))[3], (caller(2))[3], scalar(@past))
        }
        fn Demo::CSD::mid() { Demo::CSD::leaf() }
        fn Demo::CSD::top() { Demo::CSD::mid() }
        Demo::CSD::top()
    "#;
    assert_eq!(
        eval_string(code),
        "Demo::CSD::leaf,Demo::CSD::mid,Demo::CSD::top,0"
    );
}

// ── scalar context ───────────────────────────────────────────────

#[test]
fn caller_scalar_context_is_package() {
    let code = r#"
        fn Demo::CSC::leaf() {
            scalar(caller(0))
        }
        fn Demo::CSC::mid() { Demo::CSC::leaf() }
        Demo::CSC::mid()
    "#;
    assert_eq!(eval_string(code), "Demo::CSC");
}

// ── closures, eval, blocks ───────────────────────────────────────

#[test]
fn caller_inside_closure_names_anon_sub() {
    let code = r#"
        my $c = sub {
            my @c = caller(0);
            $c[3]
        };
        $c->()
    "#;
    assert_eq!(eval_string(code), "main::__ANON__");
}

#[test]
fn caller_inside_eval_block_is_eval_frame() {
    let code = r#"
        my $name;
        eval {
            my @c = caller(0);
            $name = $c[3];
        };
        $name
    "#;
    assert_eq!(eval_string(code), "(eval)");
}

#[test]
fn caller_inside_map_block() {
    let code = r#"
        fn Demo::CSM::f() {
            my @lines = map { my @c = caller(0); $c[3] } (1, 2, 3);
            join(",", @lines)
        }
        Demo::CSM::f()
    "#;
    // A map block is not a call frame: frame 0 stays the enclosing sub.
    assert_eq!(eval_string(code), "Demo::CSM::f,Demo::CSM::f,Demo::CSM::f");
}

#[test]
fn caller_inside_grep_block_at_top_level_is_empty() {
    let code = r#"
        my @r = grep { my @c = caller(0); len(@c) == 0 } (1, 2, 3);
        len(@r)
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn caller_inside_recursive_fn_one_frame_per_call() {
    let code = r#"
        fn Demo::CSR::rec($n) {
            return 0 if $n <= 0;
            my $depth = 0;
            $depth++ while defined((caller($depth))[3]);
            $depth + Demo::CSR::rec($n - 1)
        }
        # rec(3) sees 1 frame, rec(2) 2, rec(1) 3: 1+2+3 = 6.
        Demo::CSR::rec(3)
    "#;
    assert_eq!(eval_int(code), 6);
}

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

#[test]
fn caller_first_four_fields_join() {
    let code = r#"
        fn Demo::CSJ::leaf() {
            my @c = caller(0);
            join("|", $c[0], $c[3])
        }
        Demo::CSJ::leaf()
    "#;
    assert_eq!(eval_string(code), "main|Demo::CSJ::leaf");
}

#[test]
fn caller_called_twice_returns_same_values() {
    let code = r#"
        fn Demo::CSE::leaf() {
            my @a = caller(0);
            my @b = caller(0);
            ($a[0] eq $b[0] && $a[1] eq $b[1] && $a[2] == $b[2]) ? 1 : 0
        }
        Demo::CSE::leaf()
    "#;
    assert_eq!(eval_int(code), 1);
}
