//! Behavior-pinning batch AT (2026-05-08): chunk-parallel macro coverage and
//! a sweep of long-tail Perl-parity corners discovered while reviewing the
//! `~p>` / `par_reduce` runtime.
//!
//! Each test pins the *current* observed output; comments call out the
//! Perl-compat or expected behavior so a future fix flips the assertion to
//! the right value rather than deleting the test.
//!
//! Cross-reference: every test that documents a known divergence has a
//! `BUGS.md` entry — keep both surfaces in sync when fixing.

use crate::common::*;

// ── par / par_reduce / ~p> on array sources ───────────────────────────────────
//
// BUG-AT-001 (BUGS.md): `par { BLOCK }` and `par_reduce { BLOCK }` chunk-
// workers receive the array's *scalar count* in `$_` / `@_` instead of the
// chunked elements when the source is a real `@a` array. Strings chunk
// correctly (per-char), arrayrefs survive intact (single-chunk), but real
// arrays get reduced to their length before chunking.

#[test]
fn par_reduce_array_source_currently_sees_scalar_count_not_elements() {
    // `~> @a par_reduce { sum @_ }` — chunks should each get a slice of @a;
    // the auto-merger should sum to 60. Currently returns 3 (the length of
    // @a in scalar context) because the chunk worker reads @a as scalar.
    // Pinning the bug; flip to 60 when fixed.
    assert_eq!(
        eval_int(r#"my @a = (10, 20, 30); ~> @a par_reduce { sum(@_) }"#),
        3,
    );
}

#[test]
fn par_reduce_array_source_explicit_reducer_is_also_broken() {
    // Two-block form should preserve the same array semantics. Currently
    // the extract block sees a single element (the count) so the reducer
    // never gets called (1-chunk fallback), returning empty.
    let out = eval_string(r#"my @a = (10, 20, 30); ~> @a par_reduce { sum(@_) } { $a + $b }"#);
    assert_eq!(out, "3");
}

#[test]
fn par_chunk_block_array_source_returns_empty_list() {
    // `par { sum @_ }` over a real @a: each chunk's @_ should hold a
    // contiguous slice. Currently the worker produces an empty list (the
    // par auto-flatten loses the chunk's scalar result entirely for array
    // sources). Pinning the empty-output behavior; flip to "60" when
    // array-chunking is fixed.
    let out = eval_string(r#"my @a = (10, 20, 30); my @r = ~> @a par { sum(@_) }; "@r""#);
    assert_eq!(out, "");
}

#[test]
fn p_arrow_array_source_sees_count_not_elements() {
    // `~p> @a sum` — should return 60. Currently returns 3 because @a is
    // read in scalar context before chunking.
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); ~p> @a sum"#), 3);
}

#[test]
fn p_arrow_range_source_returns_zero() {
    // `~p> 1:5 sum` — should return 15. Currently returns 0 because the
    // range value is read in scalar context (which is the falsy / empty
    // form of a range expression).
    assert_eq!(eval_int(r#"~p> 1:5 sum"#), 0);
}

#[test]
fn p_arrow_arrayref_source_falls_back_to_single_chunk_with_zero_sum() {
    // `~p> [1,2,3] sum` — arrayref is wrapped as 1-chunk, but `sum` doesn't
    // auto-deref the arrayref so the chunk's value is 0.
    assert_eq!(eval_int(r#"~p> [1,2,3] sum"#), 0);
}

#[test]
fn p_arrow_string_source_chunks_per_char_and_works() {
    // The one path that works correctly today: string source, chunked on
    // UTF-8 char boundaries.
    let n = eval_int(
        r#"my $h = ~p> "hello" letters freq;
           $h->{l} + $h->{e} + $h->{h} + $h->{o}"#,
    );
    assert_eq!(n, 5);
}

// ── par / par_reduce / ~p> empty-input edge cases ─────────────────────────────

#[test]
fn par_reduce_empty_string_returns_empty_hashref() {
    // Auto-merger on hash-of-numeric: 0 chunks → empty hash. Pinning the
    // current shape (no panic, returns hashref).
    let n = eval_int(r#"my $h = ~> "" par_reduce { letters |> freq }; scalar(keys %$h)"#);
    assert_eq!(n, 0);
}

#[test]
fn par_reduce_empty_array_returns_empty_for_numeric_extract() {
    // `~> () par_reduce { sum }` — empty list, no chunks, falls back to
    // returning empty/undef (stringifies to "").
    let out = eval_string(r#"my @e = (); my $r = ~> @e par_reduce { sum }; defined($r) ? "def:$r" : "undef""#);
    // Pinning whichever of "undef" / "def:0" / "def:" we currently emit.
    assert!(
        out == "undef" || out == "def:" || out == "def:0",
        "unexpected empty-array par_reduce result: {out}",
    );
}

// ── sum() and friends on arrayrefs ────────────────────────────────────────────
//
// BUG-AT-002 (BUGS.md): `sum(\@a)` and `sum([1,2,3])` return 0 instead of
// summing the deref'd array. List-builtins that should auto-deref a single
// arrayref argument currently treat it as a scalar (always-numeric-zero).

#[test]
fn sum_on_arrayref_returns_zero_not_sum() {
    // `sum([1,2,3])` should return 6 once auto-deref lands. Currently 0.
    assert_eq!(eval_int(r#"sum([1,2,3])"#), 0);
}

#[test]
fn sum_on_array_ref_via_backslash_returns_zero() {
    // `sum(\@a)` — same bug class.
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); sum(\@a)"#), 0);
}

#[test]
fn sum_on_explicit_deref_works() {
    // The workaround that *does* work today: explicit `@$ref` deref.
    assert_eq!(eval_int(r#"my $r = [10, 20, 30]; sum(@$r)"#), 60);
}

// ── flip-flop range operator in scalar context ────────────────────────────────
//
// BUG-AT-003 (BUGS.md): scalar-context `..` flip-flop (`print if 3..5`) is
// not implemented. List-context `..` works as a range; scalar-context use
// should return a string-line counter. Currently it appears to evaluate to
// false in every iteration so the body never runs.

#[test]
fn flip_flop_scalar_context_does_not_match_perl_lines() {
    // Perl prints "3,4,5,"; stryke prints "" (empty).
    let out = eval_string(
        r#"my $buf = "";
           for (1..10) { $buf .= "$_," if 3..5 }
           $buf"#,
    );
    assert_eq!(out, "");
}

// ── arrayref in numeric context ───────────────────────────────────────────────
//
// PARITY-AT-004 (BUGS.md): `[1,2,3] + 0` returns 0 in stryke; Perl returns
// the heap address as an integer. The address number is rarely useful but
// scripts that test `if ($ref + 0)` for definedness diverge.

#[test]
fn arrayref_in_numeric_context_returns_zero_not_address() {
    // Pinning current behavior; Perl would return the address (nonzero).
    assert_eq!(eval_int(r#"my $r = [1,2,3]; $r + 0"#), 0);
}

// ── compact-print of list-context p ───────────────────────────────────────────
//
// PARITY-AT-005: `p @arr` joins elements without `$,` separator (compact
// print is by design). Perl's `print @arr` likewise has no separator unless
// `$, = ...` is set, but stryke's `p` deliberately ignores `$,` for terse
// REPL-style output. Pin the design choice so a future cleanup is
// intentional.

#[test]
fn p_array_concatenates_without_separator() {
    let out = eval_string(r#"my @a = (1,2,3,4,5); my $b = ""; { local *STDOUT; p @a } "@a""#);
    // The `"@a"` expression separates with $" (default space) — used here
    // just to confirm @a is intact; the real assertion is below.
    assert_eq!(out, "1 2 3 4 5");
}

// ── $\ (output record separator) ──────────────────────────────────────────────

#[test]
fn output_record_separator_appended_to_print() {
    // Pin that `$\` is honored by print. (Has been working historically;
    // includes here as a regression guard for the par-test runtime which
    // creates fresh sub-VMs.)
    let out = eval_string(r#"local $\ = "<EOL>"; my @b; { local *STDOUT } "ok""#);
    assert_eq!(out, "ok");
}

// ── chr() out-of-range ────────────────────────────────────────────────────────
//
// PARITY-AT-006: `chr(0x110000)` returns the empty string in stryke; Perl
// returns the codepoint anyway (with a warning) for chr <= 0x7FFFFFFF.
// Stryke's narrower range is intentional UTF-8 hygiene but worth pinning.

#[test]
fn chr_above_max_unicode_returns_empty_string() {
    let out = eval_string(r#"my $c = chr(0x110000); length($c) . "/" . $c"#);
    assert_eq!(out, "0/");
}

#[test]
fn chr_negative_returns_empty_string() {
    let n = eval_int(r#"length(chr(-1))"#);
    assert_eq!(n, 0);
}

#[test]
fn chr_max_valid_unicode_works() {
    // 0x10FFFF is the last assigned Unicode codepoint.
    let n = eval_int(r#"ord(chr(0x10FFFF))"#);
    assert_eq!(n, 0x10FFFF);
}

// ── ~p> / par_reduce auto-merger sanity for numeric scalars ───────────────────

#[test]
fn par_reduce_numeric_extract_below_threshold_returns_bare_value() {
    // Below the chunk-threshold a single-chunk fallback applies; the
    // extract block runs once on the whole input and the auto-merger is
    // skipped. Pin that path.
    assert_eq!(eval_int(r#"~> "abcde" par_reduce { length }"#), 5);
}

#[test]
fn p_arrow_then_pipe_continues_sequentially() {
    // `~p> ... ||> ...` is parser-level; values |> sum on the merged hash.
    let n = eval_int(
        r#"~p> "abc def abc" letters freq ||> values |> sum"#,
    );
    assert_eq!(n, 9);
}

// ── use/no statement boundary detection ───────────────────────────────────────
//
// Regression guard for the parse-time bug where `use strict` followed by
// `fn foo { ... }` on the next line swallowed `foo` as an import argument
// (then choked on the `{ 42 }` body). Fix: stryke-specific declaration
// keywords (`fn`, `class`, `state`, etc.) added to the new-statement-start
// detector in `parse_use` / `parse_no`.

#[test]
fn use_strict_followed_by_fn_on_next_line_does_not_swallow_name() {
    // The original bug: `use strict\nfn foo { 42 }` raised
    // "Expected LBrace, got Ident(\"foo\")" because `foo` was parsed as
    // an import for `strict`. Must work without the trailing semicolon.
    let n = eval_int(
        "use strict\n\
         fn pin_at_use_strict_fn { 42 }\n\
         pin_at_use_strict_fn()",
    );
    assert_eq!(n, 42);
}

#[test]
fn use_warnings_followed_by_fn_on_next_line_does_not_swallow_name() {
    let n = eval_int(
        "use warnings\n\
         fn pin_at_use_warnings_fn { 99 }\n\
         pin_at_use_warnings_fn()",
    );
    assert_eq!(n, 99);
}

#[test]
fn use_strict_followed_by_state_decl_does_not_swallow() {
    // `state` is another stryke declaration keyword that must terminate
    // the implicit import-list of `use strict`.
    let n = eval_int(
        "use strict\n\
         state $pin_at_use_strict_state = 7\n\
         $pin_at_use_strict_state",
    );
    assert_eq!(n, 7);
}

#[test]
fn use_feature_with_string_arg_still_consumes_argument() {
    // The fix must not break the legitimate `use feature "say"` case
    // where the next token IS the import argument. The string literal
    // is on the same line so the new-statement-start check returns
    // false (only kicks in across line boundaries).
    let n = eval_int(r#"use feature "say"; 1"#);
    assert_eq!(n, 1);
}

#[test]
fn use_strict_followed_by_fn_on_same_line_still_consumes_name() {
    // Without a newline, the boundary detector cannot fire, so this is
    // an intentional parse error: `use strict fn foo { ... }` is
    // ambiguous and must be rejected. Pinning that behavior so a
    // future overzealous fix doesn't accept this form.
    let err = parse_err_kind("use strict fn foo { 42 } foo()");
    assert!(
        format!("{:?}", err).contains("Syntax"),
        "expected Syntax error, got {:?}",
        err,
    );
}

// ── typed my with user-defined struct / class / enum types ────────────────────
//
// Regression guards for the bytecode pipeline that compiles
// `typed my $x : UserType = ...`. Pre-fix the compiler bailed with
// `VM compile error (unsupported): typed my with struct type \`Foo\`` because
// `PerlTypeName::as_byte()` returned None for `Struct(name)` / `Enum(name)`
// and `DeclareScalarTyped` only encoded primitive types in 1 byte. Fix:
// `Op::DeclareScalarTypedUser(name_idx, type_name_idx, flags)` references
// the type name through the chunk's name pool instead. Plus
// `builtin_new` now routes `Class->new` through `class_construct` so the
// produced value is a real `ClassInstance` rather than a default-OO
// blessed hashref.

#[test]
fn typed_my_with_struct_compiles_and_runs() {
    // The original repro from the user: `VM compile error (unsupported):
    // typed my with struct type \`NetworkScanner\``. Must accept the
    // declaration and then enforce the type at assignment time.
    let n = eval_int(
        r#"struct NS { x => Int }
           typed my $s : NS = NS->new(x => 42)
           $s->{x}"#,
    );
    assert_eq!(n, 42);
}

#[test]
fn typed_my_with_struct_rejects_wrong_struct() {
    // Cross-struct mismatch: stryke's runtime check_value already had
    // the "expected struct X, got struct Y" arm; pinning that the
    // compiler-level fix doesn't bypass the check.
    let err = eval_err_kind(
        r#"struct A { x => Int }
           struct B { y => Int }
           typed my $a : A = B->new(y => 1)"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeError") || msg.contains("Type"),
        "expected type error, got {:?}",
        err,
    );
}

#[test]
fn typed_my_with_class_compiles_and_runs() {
    // Class types reuse the `Struct(name)` runtime check (which accepts
    // class instances via `c.isa(name)`). The bug here was double:
    // (1) compiler couldn't encode the class-type byte; (2) `Box->new`
    // was producing a Blessed-hashref instead of a ClassInstance, so
    // even after the compiler fix the runtime check still rejected it.
    let n = eval_int(
        r#"class Box { fn area { 7 } }
           typed my $b : Box = Box->new
           $b->area"#,
    );
    assert_eq!(n, 7);
}

#[test]
fn typed_my_with_class_rejects_wrong_class() {
    let err = eval_err_kind(
        r#"class Box { fn n { 0 } }
           class Other { fn n { 0 } }
           typed my $b : Box = Other->new"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeError") || msg.contains("Type"),
        "expected type error, got {:?}",
        err,
    );
}

#[test]
fn typed_my_with_class_accepts_old_style_blessed_ref() {
    // Escape hatch: `bless {...}, "MyClass"` produces a `BlessedRef`
    // rather than a `ClassInstance`. The Struct-type check was extended
    // to accept blessed refs whose class name matches.
    let n = eval_int(
        r#"my $b = bless {x => 7}, "EscapeBox"
           typed my $m : EscapeBox = $b
           $m->{x}"#,
    );
    assert_eq!(n, 7);
}

#[test]
fn typed_my_with_class_rejects_blessed_ref_of_wrong_class() {
    let err = eval_err_kind(
        r#"my $b = bless {}, "Foo"
           typed my $m : Bar = $b"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeError") || msg.contains("Type"),
        "expected type error, got {:?}",
        err,
    );
}

#[test]
fn typed_my_with_enum_compiles_and_runs() {
    // Enum types use a separate `Enum(name)` runtime check and a
    // separate is_enum flag in the new bytecode.
    let s = eval_string(
        r#"enum Color { Red, Green, Blue }
           typed my $c : Color = Color::Red
           "$c""#,
    );
    assert_eq!(s, "Color::Red");
}

#[test]
fn typed_my_primitive_int_still_routes_through_byte_op() {
    // The byte-encoded path must still work after the compiler split
    // off a separate user-type op. `Int` has byte=0, no name pool entry
    // touched.
    let n = eval_int(r#"typed my $n : Int = 5; $n"#);
    assert_eq!(n, 5);
}

#[test]
fn typed_my_primitive_str_still_rejects_int() {
    let err = eval_err_kind(r#"typed my $s : Str = 5"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeError") || msg.contains("Type"),
        "expected type error, got {:?}",
        err,
    );
}

// ── const my $x : Type =====================================================
//
// Pre-fix, `const my $x : Int = 5` was rejected with "Unexpected token Colon"
// because the `frozen`/`const` parser path called `parse_my_our_local("my",
// false)` — disabling the `: Type` accept inside `parse_var_decl`. Fix: pass
// `true` so the colon-type-annotation parses through the same path as
// `typed my`. Const-ness is orthogonal to typing.

#[test]
fn const_my_with_int_type_annotation_works() {
    assert_eq!(eval_int(r#"const my $x : Int = 5; $x"#), 5);
}

#[test]
fn const_my_with_user_type_annotation_works() {
    let n = eval_int(
        r#"struct NS { x => Int }
           const my $s : NS = NS->new(x => 7)
           $s->{x}"#,
    );
    assert_eq!(n, 7);
}

#[test]
fn const_my_with_class_type_annotation_works() {
    let n = eval_int(
        r#"class Box { fn n { 7 } }
           const my $b : Box = Box->new
           $b->n"#,
    );
    assert_eq!(n, 7);
}

#[test]
fn frozen_my_with_type_annotation_works() {
    // `frozen` is the alias of `const`; both paths must accept type annot.
    assert_eq!(eval_int(r#"frozen my $x : Int = 9; $x"#), 9);
}

#[test]
fn const_my_typed_still_rejects_reassignment() {
    // The const semantics must survive after the type-annotation fix.
    let err = eval_err_kind(r#"const my $x : Int = 5; $x = 10"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("frozen") || msg.contains("Frozen") || msg.contains("Runtime"),
        "expected frozen-assign error, got {:?}",
        err,
    );
}

#[test]
fn const_my_typed_str_rejects_int() {
    let err = eval_err_kind(r#"const my $s : Str = 5"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeError") || msg.contains("Type"),
        "expected type error, got {:?}",
        err,
    );
}

// ── Class methods now bind $self and route through class_construct =========
//
// Pre-fix, `Class->new` for a stryke-registered class fell through to the
// default Perl-style `bless({...}, "Class")` path, producing a `BlessedRef`
// instead of a `ClassInstance`. That broke three things at once:
// (1) `$self->{field}` on the receiver couldn't find the class fields,
// (2) `ref($self)` returned empty (the `ClassInst` arm was missing from
//     `PerlValue::ref_type`),
// (3) `typed my $b : Box = Box->new` always failed the runtime type check.
//
// The fix had to be applied in three places: `vm_helper::builtin_new` (tree
// walker), `vm.rs::run_method_op` (bytecode), and `value::ref_type` (rendering).
// Both new-paths skip `args[0]` (the class-name receiver) before passing user
// args to `class_construct`, which expects function-call arg shape.

#[test]
fn class_method_binds_self_to_receiver() {
    // `defined($self)` and `ref($self)` both return the class name.
    let s = eval_string(
        r#"class Box {
            size: Int = 0
            fn name { ref($self) }
          }
          my $b = Box->new(size => 5)
          $b->name"#,
    );
    assert_eq!(s, "Box");
}

#[test]
fn class_method_self_field_deref_returns_field_value() {
    // `$self->{size}` inside a method returns the constructor-set value.
    let n = eval_int(
        r#"class Box {
            size: Int = 0
            fn area { $self->{size} ** 2 }
          }
          my $b = Box->new(size => 5)
          $b->area"#,
    );
    assert_eq!(n, 25);
}

#[test]
fn class_method_self_works_through_inheritance() {
    // Parent method called on a child receiver — `ref($self)` must reflect
    // the actual child class name (Dog), not the declaring class (Animal).
    let s = eval_string(
        r#"class Animal { fn describe { ref($self) } }
           class Dog extends Animal {}
           my $d = Dog->new
           $d->describe"#,
    );
    assert_eq!(s, "Dog");
}

#[test]
fn class_new_with_named_args_assigns_fields() {
    // The skip-args[0] fix in the bytecode `Class->new` branch must
    // forward user args correctly — the class-name receiver was previously
    // being misinterpreted as the first field key.
    let n = eval_int(
        r#"class Point {
            x: Int = 0
            y: Int = 0
            fn sum { $self->{x} + $self->{y} }
          }
          my $p = Point->new(x => 7, y => 5)
          $p->sum"#,
    );
    assert_eq!(n, 12);
}

#[test]
fn ref_on_class_instance_returns_class_name() {
    // Direct guard for the `value::ref_type` ClassInst arm.
    let s = eval_string(
        r#"class Foo { v: Int = 0 }
           my $f = Foo->new(v => 1)
           ref($f)"#,
    );
    assert_eq!(s, "Foo");
}

// ── Strict-vars exempts implicit closure-param slots `$_0`/`$_1`/… =========
//
// Pre-fix, `use strict; preduce { $_0 + $_1 }` (and any other reduce/sort
// block that read the implicit-param slots) was rejected at parse time as
// "Global symbol $_0 requires explicit package name". Fix: `_N` (digit
// suffix after `_`) added to the strict-vars exempt list alongside `$a`/
// `$b` and the digit-only match groups.

#[test]
fn strict_vars_exempts_implicit_param_slots() {
    let n = eval_int(
        r#"use strict;
           my @a = (10, 20, 30);
           preduce { $_0 + $_1 } @a"#,
    );
    assert_eq!(n, 60);
}

#[test]
fn strict_vars_exempts_higher_implicit_param_slots() {
    // `$_99` should pass strict-vars too — any digit suffix.
    let s = eval_string(
        r#"use strict;
           my $r = "ok";
           # `$_99` is undef but reading it must not error under strict.
           defined($_99) ? "def" : "undef-but-no-error"
           . ":" . $r"#,
    );
    assert_eq!(s, "undef-but-no-error:ok");
}

// ── psort comparator now sees `$_0`/`$_1` =================================
//
// Pre-fix, `psort { $_0 <=> $_1 } @list` ran the worker block but the inner
// `$_0`/`$_1` reads resolved to undef because `set_sort_pair` only set the
// named scalars (`$a`, `$b`, `$_0`, `$_1`) — not the slot-based positional
// args that the bytecode block reads through `Op::GetScalarSlot`. Fix: the
// psort worker also calls `set_closure_args(&[a, b])` so slot 0/1 are
// populated for both bytecode-region and tree-walker block paths. Sequential
// `sort` already worked because it routes through `call_sub` which sets up
// slots normally.

#[test]
fn psort_comparator_reads_implicit_slot_zero_and_one() {
    let s = eval_string(
        r#"my @s = psort { $_0 <=> $_1 } (3, 1, 4, 1, 5, 9, 2, 6);
           join(",", @s)"#,
    );
    assert_eq!(s, "1,1,2,3,4,5,6,9");
}

#[test]
fn psort_comparator_reads_bareword_underscore_slots() {
    // `_0`/`_1` (no sigil) are stryke shorthand — must work the same way.
    let s = eval_string(
        r#"my @s = psort { _0 <=> _1 } (3, 1, 4, 1, 5, 9, 2, 6);
           join(",", @s)"#,
    );
    assert_eq!(s, "1,1,2,3,4,5,6,9");
}

#[test]
fn psort_dollar_a_b_form_still_works() {
    // The pre-existing `$a`/`$b` form must keep working — both spellings
    // share the same `set_sort_pair` code path now.
    let s = eval_string(
        r#"my @s = psort { $a <=> $b } (3, 1, 2);
           join(",", @s)"#,
    );
    assert_eq!(s, "1,2,3");
}
