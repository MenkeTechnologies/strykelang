//! Behavior-pinning batch F (2026-05-04): higher-order functions, method
//! dispatch (can/DOES), nested data, range op, pos(), pattern match,
//! @ISA dynamics, closure call-site bugs.

use crate::common::*;

// ── can() and DOES() ─────────────────────────────────────────────────────────

#[test]
fn can_returns_truthy_for_existing_method() {
    assert_eq!(
        eval_int(
            r#"package Cat; sub new { bless {}, shift } sub meow { "meow" }
               package main;
               Cat->new->can("meow") ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn can_returns_falsy_for_missing_method() {
    assert_eq!(
        eval_int(
            r#"package Cat; sub new { bless {}, shift } sub meow { "meow" }
               package main;
               Cat->new->can("bark") ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn can_returns_coderef_but_invocation_returns_undef_today() {
    // BUG-036: `$obj->can("method")` returns a CODE ref, but invoking that
    // ref with the object as receiver yields undef instead of the method's
    // return value. Direct method call works fine.
    let out = eval_string(
        r#"package Cat; sub new { bless {}, shift } sub meow { "meow!" }
           package main;
           my $c = Cat->new;
           my $m = $c->can("meow");
           my $direct  = $c->meow;
           my $via_can = $m->($c);
           "ref=" . ref($m) . " direct=$direct via_can=" . (defined $via_can ? $via_can : "U")"#,
    );
    assert_eq!(out, "ref=CODE direct=meow! via_can=U");
}

#[test]
fn does_returns_true_for_inherited_class() {
    assert_eq!(
        eval_int(
            r#"package Animal; sub new { bless {}, shift }
               package Cat; our @ISA = ("Animal"); sub new { bless {}, shift }
               package main;
               Cat->new->DOES("Animal") ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn does_returns_false_for_unrelated_class() {
    assert_eq!(
        eval_int(
            r#"package Cat; our @ISA = (); sub new { bless {}, shift }
               package main;
               Cat->new->DOES("Plant") ? 1 : 0"#
        ),
        0
    );
}

// ── HOFs (closures, partial application, sub-returning-sub) ─────────────────

#[test]
fn anon_sub_returns_anon_sub_chain() {
    assert_eq!(
        eval_int(r#"my $f = sub { my $n = shift; sub { $n + shift } };
                    my $g = $f->(10);
                    $g->(5)"#),
        15
    );
}

#[test]
fn fn_factory_captures_outer_param() {
    assert_eq!(
        eval_int(r#"fn maker($n) { sub { $n + shift } }
                    my $f = maker(100);
                    $f->(7)"#),
        107
    );
}

#[test]
fn partial_application_via_closure_works() {
    assert_eq!(
        eval_int(
            r#"fn myadd3($x, $y, $z) { $x + $y + $z }
               my $bound = sub { myadd3(10, @_) };
               $bound->(20, 30)"#
        ),
        60
    );
}

// ── BUG-037 — Closure-wrapped coderef call with `(SCALAR, @ARRAY)` mis-routes
//
// Pinned at the failing value (4). Direct calls and same closure with element
// access (`$arr[0]`) work; only the flatten form inside the closure fails.

#[test]
fn closure_calling_sigfn_via_coderef_with_array_arg_breaks_today() {
    // `sub { $f->($first, @rest) }` where $f is `\&fn_with_sig` should pass
    // both args. Today the array gets numified to its count.
    assert_eq!(
        eval_int(
            r#"fn myadd($x, $y) { $x + $y }
               my $f = \&myadd;
               my $g = sub { my $first = shift; my @rest = @_; $f->($first, @rest) };
               $g->(3, 4)"#
        ),
        4
    );
}

#[test]
fn closure_calling_sigfn_via_coderef_with_indexed_arg_works() {
    assert_eq!(
        eval_int(
            r#"fn myadd($x, $y) { $x + $y }
               my $f = \&myadd;
               my $g = sub { my $first = shift; my @rest = @_; $f->($first, $rest[0]) };
               $g->(3, 4)"#
        ),
        7
    );
}

#[test]
fn direct_call_inside_closure_works() {
    // No coderef — just calling the fn by name.
    assert_eq!(
        eval_int(
            r#"fn myadd($x, $y) { $x + $y }
               my $g = sub { my $first = shift; my @rest = @_; myadd($first, @rest) };
               $g->(3, 4)"#
        ),
        7
    );
}

#[test]
fn top_level_coderef_call_with_scalar_then_array_works() {
    assert_eq!(
        eval_int(
            r#"fn myadd($x, $y) { $x + $y }
               my $f = \&myadd;
               my @r = (4);
               $f->(3, @r)"#
        ),
        7
    );
}

// ── Pattern match (algebraic) ────────────────────────────────────────────────

#[test]
fn match_string_dispatches_on_value() {
    assert_eq!(
        eval_string(
            r#"match("hi") { "hi" => "greet", "bye" => "farewell", _ => "?" }"#
        ),
        "greet"
    );
}

#[test]
fn match_arrayref_falls_through_to_underscore_default() {
    assert_eq!(
        eval_string(
            r#"match([1,2,3]) {
                 [] => "empty",
                 [1] => "one",
                 [1,2] => "two",
                 _ => "many"
               }"#
        ),
        "many"
    );
}

// ── Nested data structures ──────────────────────────────────────────────────

#[test]
fn aoa_index_chain() {
    assert_eq!(
        eval_string(r#"my @aoa = ([1,2], [3,4]); $aoa[1][0] . "/" . $aoa[0][1]"#),
        "3/2"
    );
}

#[test]
fn hoh_double_key_chain() {
    assert_eq!(
        eval_string(
            r#"my %hoh = (a => {x=>1, y=>2}, b => {x=>10, y=>20});
               $hoh{a}{y} . "/" . $hoh{b}{x}"#
        ),
        "2/10"
    );
}

#[test]
fn aoh_index_then_key() {
    assert_eq!(
        eval_string(
            r#"my @aoh = ({n=>"a", v=>1}, {n=>"b", v=>2}); $aoh[1]{n}"#
        ),
        "b"
    );
}

// ── pos() reports the position after each /g match ──────────────────────────

#[test]
fn pos_advances_with_each_g_match() {
    let out = eval_string(
        r#"my $s = "abcabc";
           my $log = "";
           while ($s =~ /a/g) { $log .= pos($s) . "," }
           $log"#,
    );
    assert_eq!(out, "1,4,");
}

#[test]
fn pos_outside_while_loop_is_undef_today() {
    // BUG-038: a single `/g` match outside a `while` loop should leave
    // `pos($s) == 1`. Stryke leaves it undef. The while-loop form (above)
    // still produces correct positions.
    assert_eq!(
        eval_int(r#"my $s = "abc"; $s =~ /a/g; defined(pos($s)) ? 1 : 0"#),
        0
    );
}

// ── Range operator: alphabetic, mixed, reverse ──────────────────────────────

#[test]
fn alpha_range_a_to_e() {
    assert_eq!(
        eval_string(r#"join(",", "a".."e")"#),
        "a,b,c,d,e"
    );
}

#[test]
fn mixed_alphanum_range_increments_numeric_part() {
    assert_eq!(
        eval_string(r#"join(",", "a1".."a5")"#),
        "a1,a2,a3,a4,a5"
    );
}

#[test]
fn reverse_of_numeric_range_descends() {
    assert_eq!(
        eval_string(r#"join(",", reverse(1..5))"#),
        "5,4,3,2,1"
    );
}

// ── @ISA modifications at runtime ───────────────────────────────────────────

#[test]
fn at_isa_modification_after_object_creation_picks_up_new_methods() {
    let out = eval_string(
        r#"package Cat; sub new { bless {}, shift } sub speak { "meow" }
           package Dog; sub bark { "woof" }
           package Pet; our @ISA = ("Cat");
           package main;
           my $p = Pet->new;
           my $a = $p->speak;
           push @Pet::ISA, "Dog";
           my $b = $p->bark;
           "$a/$b""#,
    );
    assert_eq!(out, "meow/woof");
}

// ── reset / study are accepted ───────────────────────────────────────────────

#[test]
fn study_is_accepted_and_does_not_change_match() {
    assert_eq!(
        eval_int(r#"study "abc"; "abc" =~ /b/ ? 1 : 0"#),
        1
    );
}

#[test]
fn reset_does_not_panic() {
    // Just verify the keyword parses + runs.
    assert_eq!(eval_string(r#"reset; "OK""#), "OK");
}

// ── Scalar context coercion ──────────────────────────────────────────────────

#[test]
fn scalar_of_empty_array_is_zero() {
    assert_eq!(eval_int("my @a = (); scalar @a"), 0);
}

#[test]
fn scalar_localtime_zero_is_epoch_string() {
    let s = eval_string("scalar localtime(0)");
    assert!(
        s.contains("1969") || s.contains("1970"),
        "expected epoch timestamp, got {:?}",
        s
    );
}

// ── map with multi-element block return ─────────────────────────────────────

#[test]
fn map_block_returning_two_elements_doubles_array_size() {
    assert_eq!(
        eval_string(r#"my @r = map { ($_, $_ * 2) } 1..3; "@r""#),
        "1 2 2 4 3 6"
    );
}

// ── grep returns the elements that match ────────────────────────────────────

#[test]
fn grep_returns_selected_elements() {
    assert_eq!(
        eval_string(r#"my @r = grep { $_ % 2 == 0 } 1..10; "@r""#),
        "2 4 6 8 10"
    );
}

// ── Sub composition by hand (curry/compose builtins are reserved names) ─────

#[test]
fn manual_curry_via_nested_sub_works() {
    // Use `$_[0]`-indexed access in the inner sub to sidestep BUG-037.
    assert_eq!(
        eval_int(
            r#"fn myadd($x, $y) { $x + $y }
               my $f = \&myadd;
               my $c = sub { my $a = shift; sub { $f->($a, $_[0]) } };
               $c->(3)->(4)"#
        ),
        7
    );
}

#[test]
fn closure_calling_coderef_with_at_underscore_flattens_to_count_today() {
    // BUG-037 broader: any closure that calls a coderef with `@_` as
    // argument passes `scalar(@_)` instead of flattening. So
    // `sub { $f->(@_) }->(5)` returns `$f->(1)`, not `$f->(5)`.
    assert_eq!(
        eval_int(
            r#"sub mydbl { my $x = shift; $x * 2 }
               my $f = \&mydbl;
               my $h = sub { $f->(@_) };
               $h->(5)"#
        ),
        2
    );
}

#[test]
fn direct_named_call_with_at_underscore_inside_closure_works() {
    // The same body works when the call is by name, not via coderef.
    assert_eq!(
        eval_int(
            r#"sub mydbl { my $x = shift; $x * 2 }
               sub myinc1 { my $x = shift; $x + 1 }
               my $h = sub { mydbl(myinc1(@_)) };
               $h->(5)"#
        ),
        12
    );
}
