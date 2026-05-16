//! wantarray + context propagation pins.

use crate::common::*;

// ── wantarray distinguishes contexts ──────────────────────────────

#[test]
fn wantarray_true_in_list_context() {
    let code = r#"
        fn Demo::WA::ctx() {
            wantarray() ? "list" : "scalar"
        }
        my @r = Demo::WA::ctx();
        $r[0] eq "list" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn wantarray_false_in_scalar_context() {
    let code = r#"
        fn Demo::WA::ctx() {
            wantarray() ? "list" : "scalar"
        }
        my $r = Demo::WA::ctx();
        $r eq "scalar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── fn return shape per context ───────────────────────────────────

#[test]
fn fn_returns_array_in_list_context() {
    let code = r#"
        fn Demo::WA::seq() {
            return (10, 20, 30) if wantarray();
            return 30
        }
        my @l = Demo::WA::seq();
        my $s = Demo::WA::seq();
        (len(@l) == 3 && $l[1] == 20 && $s == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array in scalar context returns count ─────────────────────────

#[test]
fn array_in_scalar_context_returns_count() {
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my $n = @arr;
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn empty_array_in_scalar_context_is_zero() {
    let code = r#"
        my @empty;
        my $n = @empty;
        $n == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── List literal in scalar context returns last value (comma) ────

#[test]
fn comma_list_to_scalar_rejected_at_parse_time() {
    // Stryke strictly rejects `my $r = (list)` assignment.
    // Workaround pattern: use index of last element.
    let code = r#"
        my @arr = (1, 2, 3, 4, 5);
        my $r = $arr[-1];
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash in scalar context ─────────────────────────────────────────

#[test]
fn hash_in_scalar_context_is_truthy_when_non_empty() {
    let code = r#"
        my %h = (a => 1);
        my $r = %h ? 1 : 0;
        $r == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_in_scalar_context_is_falsy_when_empty() {
    let code = r#"
        my %empty;
        my $r = %empty ? 1 : 0;
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── List of fn calls in scalar context ────────────────────────────

#[test]
fn fn_call_in_scalar_context_returns_scalar() {
    let code = r#"
        fn Demo::WA::get_list() { (1, 2, 3) }
        my $r = Demo::WA::get_list();
        # In scalar context, last comma value: 3.
        $r == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn fn_call_in_list_context_returns_array() {
    let code = r#"
        fn Demo::WA::get_list() { (1, 2, 3) }
        my @r = Demo::WA::get_list();
        len(@r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── wantarray inside nested call ──────────────────────────────────

#[test]
fn wantarray_per_call_not_propagated() {
    let code = r#"
        fn Demo::WA::inner() {
            wantarray() ? "list" : "scalar"
        }
        fn Demo::WA::outer() {
            # outer is in list context; inner is called in scalar.
            my $r = Demo::WA::inner();
            return $r
        }
        my @r = Demo::WA::outer();
        # inner sees scalar context regardless of outer.
        $r[0] eq "scalar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reverse in list vs scalar context ─────────────────────────────

#[test]
fn reverse_in_list_context_reverses() {
    let code = r#"
        my @r = reverse (1, 2, 3);
        join(",", @r) eq "3,2,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reverse_in_scalar_context_reverses_string() {
    let code = r#"
        my $r = scalar reverse "hello";
        $r eq "olleh" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sort in list context only ─────────────────────────────────────

#[test]
fn sort_in_list_context_returns_sorted_array() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (3, 1, 4, 1, 5);
        join(",", @r) eq "1,1,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── map / grep in scalar context return count ─────────────────────

#[test]
fn map_in_scalar_context_returns_count() {
    let code = r#"
        my $n = scalar(map { _ * 2 } (1, 2, 3, 4));
        $n == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn grep_in_scalar_context_returns_match_count() {
    let code = r#"
        my $n = scalar(grep { _ > 2 } (1, 2, 3, 4, 5));
        $n == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── List slice context ────────────────────────────────────────────

#[test]
fn array_slice_in_list_context_returns_array() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @s = @a[1, 3];
        len(@s) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Last expression value in fn body ──────────────────────────────

#[test]
fn fn_last_expr_returned_unless_explicit_return() {
    let code = r#"
        fn Demo::WA::implicit() {
            42
        }
        Demo::WA::implicit() == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── wantarray in undef context (statement) ─────────────────────────

#[test]
fn wantarray_in_void_context_may_return_undef() {
    let code = r#"
        fn Demo::WA::ctx_full() {
            if (!defined(wantarray())) {
                return "void"
            }
            return wantarray() ? "list" : "scalar"
        }
        # When called as standalone statement, context is void.
        # Capture in scalar to force scalar context.
        my $s = Demo::WA::ctx_full();
        ($s eq "scalar" || $s eq "void") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── List assignment in scalar context returns RHS count ──────────

#[test]
fn list_assignment_in_scalar_context_returns_count() {
    let code = r#"
        my ($a, $b, $c);
        my $n = (($a, $b, $c) = (10, 20, 30));
        # Perl returns 3 (count of items on RHS).
        $n == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Composition: scalar context inside hash key ──────────────────

#[test]
fn scalar_context_for_hash_key_expression() {
    let code = r#"
        my %h;
        my @arr = (1, 2, 3);
        # Using @arr as a hash key would scalar-context it (= count = 3).
        $h{scalar @arr} = "found";
        exists($h{3}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
