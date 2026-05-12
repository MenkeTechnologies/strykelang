//! Behavior-pinning batch H (2026-05-04): traits/roles, abstract/final classes,
//! field types, regex extras, sprintf star widths, NaN comparisons, ENV +
//! filehandle behavior, ARGV.

use crate::common::*;

// ── trait + impl ─────────────────────────────────────────────────────────────

#[test]
fn trait_provides_method_to_class() {
    assert_eq!(
        eval_string(
            r#"trait Speak { fn say_hi { "hi" } }
                       class Cat impl Speak { }
                       Cat()->say_hi"#
        ),
        "hi"
    );
}

#[test]
fn role_keyword_is_not_supported_today() {
    // Stryke uses `trait`, not `role`. Pin the parse-time rejection.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"role Speak { fn say_hi { "hi" } }
           class Cat impl Speak { }
           Cat()->say_hi"#,
    );
    assert!(
        matches!(
            kind,
            ErrorKind::Syntax | ErrorKind::Runtime | ErrorKind::UndefinedSubroutine
        ),
        "expected error, got {:?}",
        kind
    );
}

#[test]
fn class_with_two_traits_inherits_methods_from_each() {
    assert_eq!(
        eval_string(
            r#"trait Speak { fn say_hi { "hi" } }
               trait Walk { fn step { "step" } }
               class Person impl Speak, Walk { name: Str = "anon" }
               my $p = Person();
               $p->say_hi . "/" . $p->step"#
        ),
        "hi/step"
    );
}

#[test]
fn trait_with_field_is_parse_error_today() {
    // BUG-046: traits cannot declare fields ("Expected `fn` in trait
    // definition"). Pin until/if traits are extended to support state.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(
        r#"trait Counter { count: Int = 0; fn inc { $self->count($self->count + 1); $self } }"#,
    );
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

// ── abstract / final classes ─────────────────────────────────────────────────

#[test]
fn abstract_class_cannot_be_instantiated() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"abstract class A { fn doit { "a" } }
           A()"#,
    );
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

#[test]
fn final_class_cannot_be_extended() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"final class D { fn doit { 2 } }
           class E extends D { fn other { 3 } }"#,
    );
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax
        ),
        "expected error, got {:?}",
        kind
    );
}

#[test]
fn final_class_methods_invoke_normally() {
    assert_eq!(eval_int(r#"final class D { fn doit { 2 } } D()->doit"#), 2);
}

// ── Field types: Int / Str / Float / Bool / Array / Hash / Ref / Any ────────

#[test]
fn class_field_int_type_check_rejects_string() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"class Counter { value: Int = 0 } Counter(value => "abc")"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn class_field_int_type_check_accepts_int() {
    assert_eq!(
        eval_int(r#"class Counter { value: Int = 0 } Counter(value => 42)->value"#),
        42
    );
}

#[test]
fn class_field_array_type_accepts_arrayref_default() {
    // `Array` is the supported keyword (not `ARRAY` or `ArrayRef`).
    assert_eq!(
        eval_string(r#"class S { items: Array = [] } my $s = S(); ref(\@{$s->items})"#),
        "ARRAY"
    );
}

#[test]
fn class_field_array_uppercase_keyword_does_not_match_array_default_today() {
    // BUG-047: writing `items: ARRAY = []` produces "expected ARRAY, got
    // ARRAY" because the type name `ARRAY` is parsed as a Struct(name) and
    // the runtime tag mismatches it.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"class S { items: ARRAY = [] } S()"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn class_field_arrayref_keyword_does_not_match_array_default_today() {
    // BUG-047b: same for `ArrayRef`/`HashRef` — they look like Moose-isms but
    // stryke uses `Array`/`Hash`.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"class S { items: ArrayRef = [] } S()"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn typed_param_with_int_rejects_float() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"fn typed_add($x: Int, $y: Int) { $x + $y } typed_add(2.5, 3)"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn typed_param_with_str_rejects_int() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"fn greet($name: Str) { "hi $name" } greet(42)"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn typed_param_with_bool_accepts_one_and_zero() {
    assert_eq!(
        eval_string(r#"fn yorn($b: Bool) { $b ? "Y" : "N" } yorn(1) . "/" . yorn(0)"#),
        "Y/N"
    );
}

// ── ref() on stryke-native class instance returns the class name (BUG-048 FIXED) ──

#[test]
fn ref_of_stryke_class_instance_returns_class_name() {
    // BUG-048 (now FIXED): `ref($obj)` for a stryke `class C { ... }`
    // instance returned the empty string instead of the class name. The
    // `ClassInst` arm was missing from `StrykeValue::ref_type` — added it
    // alongside the `StructInst` / `EnumInst` arms so `ref()` emits
    // `def.name` for class instances too.
    let out = eval_string(
        r#"class C { v: Int = 0 } my $c = C(v => 5);
           ref($c) . "|" . ($c->isa("C") ? "Y" : "N")"#,
    );
    assert_eq!(out, "C|Y");
}

#[test]
fn ref_of_blessed_hashref_returns_class_name() {
    // For comparison: bless does set ref correctly.
    assert_eq!(
        eval_string(r#"my $h = bless { v => 0 }, "H"; ref($h)"#),
        "H"
    );
}

// ── PCRE2 doesn't support `(?{ code })` embedded blocks ──────────────────────

#[test]
fn embedded_code_in_regex_is_rejected_today() {
    // PARITY-017: PCRE2 (stryke's regex engine) does not implement Perl's
    // `(?{...})` extension.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#""abc" =~ /a(?{ "side" })b/"#);
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Regex | ErrorKind::Syntax
        ),
        "expected error, got {:?}",
        kind
    );
}

#[test]
fn regex_recursion_via_question_r_works() {
    // `(?R)` recursive patterns are supported.
    assert_eq!(
        eval_int(r#""abc(def(ghi)jkl)mno" =~ /\((?:[^()]|(?R))*\)/ ? 1 : 0"#),
        1
    );
}

#[test]
fn regex_conditional_pattern_works() {
    assert_eq!(eval_int(r#""ab1" =~ /(a)b(?(1)\d|x)/ ? 1 : 0"#), 1);
}

#[test]
fn regex_atomic_group_prevents_backtrack() {
    // `(?>a+)` consumes greedily without giving back, so `(?>a+)ab` cannot
    // match "aaab".
    assert_eq!(eval_int(r#""aaab" =~ /(?>a+)ab/ ? 1 : 0"#), 0);
    // Plain `a+ab` does match (greedy + backtrack).
    assert_eq!(eval_int(r#""aaab" =~ /a+ab/ ? 1 : 0"#), 1);
}

// ── sprintf star-width and dynamic precision ────────────────────────────────

#[test]
fn sprintf_star_width_consumes_an_arg() {
    // BUG-049 FIXED: `%*d` consumes the width as an arg.
    assert_eq!(eval_string(r#"sprintf("%*d", 5, 42)"#), "   42");
    // Negative width turns into left-alignment.
    assert_eq!(eval_string(r#"sprintf("%*d|", -5, 42)"#), "42   |");
}

#[test]
fn sprintf_star_precision_consumes_an_arg() {
    assert_eq!(eval_string(r#"sprintf("%.*f", 3, 3.14159)"#), "3.142");
}

#[test]
fn sprintf_star_width_and_precision_combined() {
    assert_eq!(
        eval_string(r#"sprintf("%*.*f", 8, 3, 3.14159)"#),
        "   3.142"
    );
}

// ── %s with width and truncation ────────────────────────────────────────────

#[test]
fn sprintf_width_and_max_precision_on_string() {
    // %5.3s: pad to width 5, max length 3.
    assert_eq!(eval_string(r#"sprintf("%5.3s", "abcdefg")"#), "  abc");
}

// ── Banker's rounding for `%.0f` ────────────────────────────────────────────

#[test]
fn sprintf_zero_decimal_uses_bankers_rounding() {
    // 0.5 → 0, 1.5 → 2, 2.5 → 2, 3.5 → 4 — round-half-to-even.
    assert_eq!(eval_string(r#"sprintf("%.0f", 0.5)"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.0f", 1.5)"#), "2");
    assert_eq!(eval_string(r#"sprintf("%.0f", 2.5)"#), "2");
    assert_eq!(eval_string(r#"sprintf("%.0f", 3.5)"#), "4");
}

// ── NaN and zero comparisons ────────────────────────────────────────────────

#[test]
fn nan_does_not_equal_itself() {
    assert_eq!(
        eval_string(r#"my $n = sqrt(-1); $n == $n ? "eq" : "ne""#),
        "ne"
    );
}

#[test]
fn nan_is_not_equal_to_itself() {
    assert_eq!(eval_int(r#"my $n = sqrt(-1); $n != $n ? 1 : 0"#), 1);
}

#[test]
fn negative_zero_equals_positive_zero() {
    assert_eq!(eval_string(r#"0.0 == -0.0 ? "eq" : "ne""#), "eq");
}

// ── %ENV access ─────────────────────────────────────────────────────────────

#[test]
fn env_hash_ref_type_is_hash() {
    assert_eq!(eval_string(r#"ref \%ENV"#), "HASH");
}

#[test]
fn env_missing_var_neither_exists_nor_defined() {
    assert_eq!(
        eval_string(
            r#"my $name = "STRYKE_DEFINITELY_NOT_SET_BY_ANY_TEST_xx";
               (exists $ENV{$name} ? "Y" : "N") . "/" . (defined $ENV{$name} ? "Y" : "N")"#
        ),
        "N/N"
    );
}

// ── printf with too few or too many args ─────────────────────────────────────

#[test]
fn printf_with_too_few_args_pads_with_zero() {
    assert_eq!(eval_string(r#"sprintf("%d %d", 1)"#), "1 0");
}

#[test]
fn printf_with_too_many_args_drops_extras() {
    assert_eq!(eval_string(r#"sprintf("%d %d", 1, 2, 3)"#), "1 2");
}

// ── Static method via `fn Self.NAME` is callable as `Pkg::NAME(...)` ────────

#[test]
fn static_method_callable_via_pkg_double_colon() {
    assert_eq!(
        eval_int(
            r#"class P { x: Int = 0; fn Self.new_at($cls, $val) { P(x => $val) } }
               my $p = P::new_at(undef, 42);
               $p->x"#
        ),
        42
    );
}

// ── Class method that mutates and chains via $self ──────────────────────────

#[test]
fn class_method_chain_mutates_internal_state() {
    assert_eq!(
        eval_int(
            r#"class Counter { value: Int = 0; fn bump { $self->value($self->value + 1); $self } }
               my $c = Counter();
               $c->bump->bump->bump->bump;
               $c->value"#
        ),
        4
    );
}

// ── Class method with `Array` field accessor read-write ─────────────────────

#[test]
fn array_field_accessor_returns_arrayref() {
    // Field declared as `Array` is stored as an arrayref under the hood, so
    // the accessor returns an arrayref the caller dereferences.
    assert_eq!(
        eval_string(
            r#"class S { items: Array = [] }
               my $s = S();
               push @{$s->items}, 1;
               push @{$s->items}, 2;
               push @{$s->items}, 3;
               join(",", @{$s->items})"#
        ),
        "1,2,3"
    );
}

// ── Numeric computation: i64 boundaries ─────────────────────────────────────

#[test]
fn pow_two_to_thirty_two_minus_one() {
    assert_eq!(eval_int("2**32 - 1"), 4_294_967_295);
}

#[test]
fn pow_two_to_sixty_two_is_max_safe_int_squared_form() {
    // Pin the exact integer (still in i64 range).
    assert_eq!(eval_int("2**62"), 4_611_686_018_427_387_904);
}

// ── ARGV processing ─────────────────────────────────────────────────────────

#[test]
fn argv_is_empty_when_invoked_via_lib_eval() {
    assert_eq!(eval_int(r#"scalar @ARGV"#), 0);
}

// ── shift @ARGV idiom ────────────────────────────────────────────────────────

#[test]
fn shift_at_argv_returns_undef_when_empty() {
    assert_eq!(eval_int(r#"defined(shift @ARGV) ? 1 : 0"#), 0);
}

// ── Trait method visibility & ordering ───────────────────────────────────────

#[test]
fn trait_method_lookup_yields_trait_implementation() {
    // No conflict — trait method exposed directly through the impl'ing class.
    assert_eq!(
        eval_int(
            r#"trait T { fn answer { 42 } }
                    class K impl T { }
                    K()->answer"#
        ),
        42
    );
}

#[test]
fn class_method_overrides_trait_method() {
    // Local fn definition wins over the trait's same-named method.
    assert_eq!(
        eval_string(
            r#"trait T { fn name { "trait" } }
                       class K impl T { fn name { "class" } }
                       K()->name"#
        ),
        "class"
    );
}
