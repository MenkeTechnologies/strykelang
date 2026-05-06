//! Behavior-pinning batch U (2026-05-05): Functional, Data Structures, Scoping.
//!
//! This batch pins higher-order functional utilities (Clojure-style) and
//! core container structures like Heaps and Sets.

use crate::common::*;

// ── Functional Programming Utilities ───────────────────────────────────────

#[test]
fn functional_juxt_composes_functions() {
    let code = r#"
        my $f = juxt(sub { $_[0] + 1 }, sub { $_[0] * 2 });
        my @res = $f->(10);
        join(",", @res)
    "#;
    assert_eq!(eval_string(code), "11,20");
}

#[test]
fn functional_fnil_replaces_undef() {
    let code = r#"
        my $add_with_default = fnil(sub { $_[0] + $_[1] }, 0, 100);
        # replace 1st arg if undef (but it's not), replace 2nd if undef
        my $v1 = $add_with_default->(5, undef);   # 5 + 100
        my $v2 = $add_with_default->(undef, 20);  # 0 + 20
        "$v1,$v2"
    "#;
    assert_eq!(eval_string(code), "105,20");
}

#[test]
fn functional_constantly_returns_fixed_value() {
    let code = r#"
        my $c = constantly(42);
        $c->(1, 2, 3)
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn functional_complement_negates_boolean() {
    let code = r#"
        my $is_even = sub { $_[0] % 2 == 0 };
        my $is_odd = complement($is_even);
        $is_odd->(3) . "," . $is_odd->(4)
    "#;
    assert_eq!(eval_string(code), "1,0");
}

#[test]
fn functional_iterate_lazy_sequence() {
    let code = r#"
        # Use pipeline to lazily take from infinite sequence
        my $it = iterate(sub { $_[0] * 2 }, 1);
        my @res = $it |> take 5 |> collect;
        join(",", @res)
    "#;
    assert_eq!(eval_string(code), "1,2,4,8,16");
}

#[test]
fn functional_memoize_caches_results() {
    let code = r#"
        mysync $calls = 0;
        my $f = memoize(sub { $calls++; $_[0] * 2 });
        $f->(10);
        $f->(10);
        $f->(10);
        $calls
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn functional_curry_partial_application() {
    let code = r#"
        my $add = sub { $_[0] + $_[1] };
        my $curried = curry($add, 2);
        my $add5 = $curried->(5);
        $add5->(10)
    "#;
    assert_eq!(eval_int(code), 15);
}

// ── Data Structures ────────────────────────────────────────────────────────

#[test]
fn heap_min_priority_queue() {
    let code = r#"
        my $h = heap { $a <=> $b };
        $h->push(50);
        $h->push(10);
        $h->push(30);
        $h->push(5);
        $h->push(20);
        my @res;
        # Use numification for length
        while (0+$h > 0) {
            push @res, $h->pop();
        }
        join(",", @res)
    "#;
    assert_eq!(eval_string(code), "5,10,20,30,50");
}

#[test]
fn heap_peek_and_len() {
    let code = r#"
        my $h = heap { $a <=> $b };
        $h->push(10);
        $h->push(5);
        $h->push(20);
        my $l1 = 0+$h;
        my $p = $h->peek;
        my $l2 = 0+$h;
        "$l1,$p,$l2"
    "#;
    assert_eq!(eval_string(code), "3,5,3");
}

#[test]
fn set_basic_operations() {
    let code = r#"
        my $s = set(1, 2, 3);
        # Sets are immutable; use operators
        $s = $s | set(4, 2);
        my $s2 = $s & set(2, 3, 5);
        my $c1 = $s->has(4);
        my $c2 = $s2->has(1);
        my $len = $s->len;
        "$c1,$c2,$len"
    "#;
    assert_eq!(eval_string(code), "1,0,4");
}

#[test]
fn deque_double_ended_queue() {
    let code = r#"
        my $d = deque();
        $d->push_back(10);
        $d->push_back(20);
        $d->push_back(30);
        $d->push_front(5);
        my $v1 = $d->pop_front(); # 5
        my $v2 = $d->pop_back();  # 30
        "$v1,$v2," . $d->len
    "#;
    assert_eq!(eval_string(code), "5,30,2");
}

// ── Scoping & Closures ──────────────────────────────────────────────────────

#[test]
fn closure_capture_mutable_mysync_var() {
    let code = r#"
        mysync $count = 0;
        my $inc = sub { $count++ };
        $inc->();
        $inc->();
        $count
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn closure_independent_instances_with_mysync() {
    let code = r#"
        fn make_counter($start) {
            mysync $c = $start;
            return sub { $c++ };
        }
        my $c1 = make_counter(10);
        my $c2 = make_counter(20);
        $c1->();
        $c2->();
        $c1->() . "," . $c2->()
    "#;
    assert_eq!(eval_string(code), "11,21");
}

#[test]
fn scope_loop_variable_isolation() {
    let code = r#"
        my @subs;
        for my $i (1..3) {
            push @subs, sub { $i };
        }
        join(",", map { $_->() } @subs)
    "#;
    assert_eq!(eval_string(code), "1,2,3");
}
