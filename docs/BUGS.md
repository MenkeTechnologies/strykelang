# BUGS.md — Known parity gaps and surprising behaviors

Captured 2026-05-04 from a behavior-pinning sweep against `stryke v0.11.12` on
macOS aarch64; continuously updated since. Additional behavior pins live in
`tests/suite/behavior_pin_2026_05*.rs` (rolling `_a..z`, `_aa..` batches).
Entries below pair each documented bug with the pinning tests that lock the
*current* output.

When a bug is fixed, update the corresponding test rather than deleting
it — the test then becomes the regression guard. Bugs marked **FIXED**
in the title kept their numeric ID so historical references in commits
and other docs still resolve.

Severity legend:

- `parity` — diverges from Perl 5; intentional or accidental TBD
- `bug` — observably wrong vs documented intent
- `polish` — non-critical UX/error-message issue

## Recently fixed

- **BUG-119** — Serializers (`to_json`, `to_xml`, `to_yaml`, `to_toml`,
  `to_html`, `ddump`) treated stryke `class` / `struct` / `enum`
  instances as opaque scalars and emitted the receiver's `Display`
  stringification (`"Outer(name => x, inner => Inner(v => 7))"`)
  wrapped in the target format. Root cause: serializers worked off the
  raw `PerlValue` tree without any recursive flatten step. Fix: new
  `strykelang/serialize_normalize.rs` module exposes `deep_normalize`
  — recursively converts ClassInstance / StructInstance / EnumInstance
  / nested HashRef / ArrayRef into plain hashref/arrayref shapes the
  existing serializer logic already handles. Hooked at the root of
  `normalize_serialize_root` so every serializer benefits at once,
  plus `builtin_ddump`. Inheritance fields resolve via a thread-local
  `CLASS_DEFS_REGISTRY` populated from `VMHelper::execute` and each
  `ClassDecl` statement. Companion: `$obj->to_hash_rec` (alias
  `to_hash_deep`) gives users an explicit entry point for the same
  flatten on struct and class instances.
  Pin tests:
  `to_hash_rec_flattens_nested_class_instances`,
  `to_hash_rec_alias_to_hash_deep_is_equivalent`,
  `to_hash_shallow_keeps_nested_class_instance`,
  `to_hash_rec_walks_arrayref_of_classes`,
  `to_hash_rec_works_for_struct_too`,
  `to_json_recursive_on_nested_class`,
  `to_json_recursive_on_struct`,
  `ddump_recursive_returns_normalized_string`,
  `to_yaml_recursive_on_class`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-118** — `%$obj` (and `keys %$obj` / `values %$obj`) on a stryke
  `ClassInstance` regressed to "Can't dereference non-reference as hash"
  after BUG-114's fix turned class instances into real `ClassInstance`
  values instead of Perl-style blessed hashrefs. Broke
  `examples/rosetta/t/test_reflection_list_properties.stk` and any
  Perl-style introspection idiom (`for my $k (keys %$self) { ... }`).
  Fix in `vm_helper::dereference` Hash arm: flatten `ClassInstance`
  fields into a fresh `IndexMap` (using `collect_class_fields_full`'s
  inheritance-resolved order), same for `StructInstance` fields, plus
  an unbless step for `BlessedRef` whose payload is a hash so old-style
  Perl OO patterns keep working too.
  Pin tests:
  `keys_percent_deref_on_class_instance_returns_field_names`,
  `values_percent_deref_on_class_instance_returns_field_values`,
  `percent_deref_on_struct_instance_returns_field_map`,
  `percent_deref_on_blessed_hashref_unwraps_inner_hash`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-117** — `(LIST) |> psort { ... }` followed by a newline silently
  swallowed the next statement as the list operand. Root cause: the
  bareword `psort` parser always called
  `parse_assign_expr_list_optional_progress()` after the comparator
  block, with no in-pipe-RHS / newline check — unlike `sort`'s
  block-form which already had a `peek_line() > block_end_line`
  early-out. Without this gate, `my @s = (1,2,3) |> psort { _0 <=>
  _1 }\nmy $n = len(@s)` parsed the second `my $n = ...` as the
  psort list operand and dropped the binding entirely. Fix in
  `parser.rs::psort` arm: mirror sort's gating — record
  `block_end_line` after the block, then if `in_pipe_rhs() &&
  (terminator-token || peek_line > block_end_line)`, switch to
  `pipe_placeholder_list`. Same-line continuations (`|> psort {
  ... } |> rev`) keep working because the next token IS `|>`.
  Pin tests:
  `psort_block_in_pipe_rhs_terminates_at_newline`,
  `psort_block_chain_with_pipe_forward_continues_on_same_line`,
  `psort_block_followed_by_explicit_list_still_works`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-116** — `psort { $_0 <=> $_1 } @list` (and the bareword `_0`/`_1`
  form) silently returned the input unsorted when the comparator block
  read the implicit-param slots. The worker invoked the block via
  `run_block_region` directly (bytecode region) instead of `call_sub`,
  and `set_sort_pair` only populated named scalars (`$a`, `$b`,
  `$_0`, `$_1`) — *not* the slot-based positional args that the
  bytecode reads through `Op::GetScalarSlot`. Fix in `vm.rs`:
  `Op::PSortWithBlock` worker now also calls
  `set_closure_args(&[a, b])` for both the bytecode-region and
  tree-walker fallback paths so slot 0/1 hold the comparator pair.
  Sequential `sort` was unaffected because it routes through
  `call_sub` which sets up slots normally.
  Pin tests: `psort_comparator_reads_implicit_slot_zero_and_one`,
  `psort_comparator_reads_bareword_underscore_slots`,
  `psort_dollar_a_b_form_still_works`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-115** — `use strict; preduce { $_0 + $_1 } @list` (and any
  reduce/sort block that read implicit-param slots) was rejected at
  parse time with "Global symbol $_0 requires explicit package name".
  Root cause: `VMHelper::strict_scalar_exempt` whitelisted `$a`/`$b`
  and digit-only match groups (`$1`, `$2`, …) but missed stryke's
  positional-slot spelling `$_0` / `$_1` / … `$_99`. Fix: added a
  `name.starts_with('_') && rest.all_digits()` arm to the exempt
  predicate. Critical under `--no-interop`, where `$a`/`$b` are
  rejected and `$_0`/`$_1` are the only valid comparator-slot names.
  Pin tests: `strict_vars_exempts_implicit_param_slots`,
  `strict_vars_exempts_higher_implicit_param_slots`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-114 / BUG-048 (FIXED)** — Stryke `class C { ... }` instances
  weren't recognized as class instances by the rest of the runtime. The
  bytecode method dispatcher (`vm.rs::run_method_op`) inlined a copy of
  the default `Class->new` path that produced a `BlessedRef` instead of
  a `ClassInstance`, which cascaded into three visible bugs:
  (1) `$self->{field}` inside instance methods couldn't find class
  fields; (2) `ref($self)` returned the empty string because
  `PerlValue::ref_type` had no `ClassInst` arm; (3) `typed my $b : C =
  C->new` always failed the runtime type check. Fixes:
  - `vm.rs::run_method_op` now checks `class_defs` before the
    Perl-blessed-hashref fallback and routes through `class_construct`
    (skipping `all_args[0]` which holds the class-name receiver).
  - `vm_helper::builtin_new` got the same routing for the tree-walker
    path (initial fix during BUG-111 work).
  - `value::ref_type` learned a `HeapObject::ClassInst(c) =>
    c.def.name` arm so `ref($obj)` returns the class name.
  Pin tests:
  `class_method_binds_self_to_receiver`,
  `class_method_self_field_deref_returns_field_value`,
  `class_method_self_works_through_inheritance`,
  `class_new_with_named_args_assigns_fields`,
  `ref_on_class_instance_returns_class_name`
  in `tests/suite/behavior_pin_2026_05_at.rs`. The pre-existing
  `behavior_pin_2026_05_h::ref_of_stryke_class_instance_returns_class_name`
  flipped from documenting the bug to guarding the fix.
- **BUG-113** — `const my $x : Int = 5` (and `frozen my $x : Type`)
  was rejected at parse time with "Unexpected token Colon". The
  `frozen`/`const` parser branch called `parse_my_our_local("my",
  false)`, which suppressed the type-annotation accept inside
  `parse_var_decl`. Const/frozen-ness is orthogonal to typing, so the
  flag is now `true` for both spellings — `const my $b : Box =
  Box->new`, `frozen my $n : Int = 9`, and the user-type variants all
  work. Pin tests:
  `const_my_with_int_type_annotation_works`,
  `const_my_with_user_type_annotation_works`,
  `const_my_with_class_type_annotation_works`,
  `frozen_my_with_type_annotation_works`,
  `const_my_typed_still_rejects_reassignment`,
  `const_my_typed_str_rejects_int`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-112** — `s docs <TOPIC>` entered the interactive TUI when a
  caller named a specific topic (or when AI wrappers like Gemini's
  exec invoked it with mixed-tty plumbing), blocking until the user
  pressed `q`. Two compounding causes: (1) the TTY check only
  inspected stdout, so wrappers that kept stdout as a tty while
  piping stdin slipped through; (2) even with both ttys, naming a
  topic should be a one-shot lookup (`man pmap` semantics), not a
  book-browser entry point. Fix in `main.rs::run_doc_subcommand`:
  the interactive loop now requires *no positional argument*, plus
  both `stdin().is_terminal()` and `stdout().is_terminal()`, with
  `STRYKE_NO_TTY=1` / `NO_TTY=1` as explicit overrides. Any
  positional `TOPIC` or page number → dump the page and exit 0.
  Bare `s docs` on a real terminal still launches the TUI.
- **BUG-111** — `typed my $x : UserType = ...` where `UserType` is a
  user-defined struct, class, or enum was rejected at compile time
  with `VM compile error (unsupported): typed my with struct type
  \`Foo\``. Root cause: `Op::DeclareScalarTyped` only carried a 1-byte
  type tag and `PerlTypeName::as_byte()` returned None for `Struct(_)`
  / `Enum(_)`. Fix: added `Op::DeclareScalarTypedUser(name_idx,
  type_name_idx, flags)` that resolves the type name through the
  chunk's name pool, plus a `compiler::emit_declare_scalar_typed`
  helper that picks the right op. A second compounding bug —
  `builtin_new` didn't check `class_defs`, so `Class->new` for a
  registered class produced a default-OO blessed-hashref instead of
  a `ClassInstance`, defeating the runtime `check_value` —
  was fixed by routing class lookups through `class_construct`.
  `check_value::Struct(name)` was also extended to accept
  `BlessedRef` whose `class` matches, so old-style `bless {...},
  "MyClass"` round-trips through typed-my. Pin tests:
  `typed_my_with_struct_compiles_and_runs`,
  `typed_my_with_struct_rejects_wrong_struct`,
  `typed_my_with_class_compiles_and_runs`,
  `typed_my_with_class_rejects_wrong_class`,
  `typed_my_with_class_accepts_old_style_blessed_ref`,
  `typed_my_with_class_rejects_blessed_ref_of_wrong_class`,
  `typed_my_with_enum_compiles_and_runs`,
  `typed_my_primitive_int_still_routes_through_byte_op`,
  `typed_my_primitive_str_still_rejects_int`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-110** — `use strict` / `use warnings` / `use VERSION` followed by
  `fn foo { ... }` on the next line no longer swallows `foo` as an
  import argument. The `next_is_new_stmt_keyword` detector in
  `parser.rs` learned the stryke-specific declaration keywords (`fn`,
  `class`, `abstract`, `final`, `trait`, `state`, `mysync`, `oursync`)
  so a fresh-line keyword terminates the implicit import-list. Same-
  line uses (`use strict fn foo { ... }`) stay rejected because the
  detector only fires across line boundaries. Pin tests:
  `use_strict_followed_by_fn_on_next_line_does_not_swallow_name`,
  `use_warnings_followed_by_fn_on_next_line_does_not_swallow_name`,
  `use_strict_followed_by_state_decl_does_not_swallow`,
  `use_feature_with_string_arg_still_consumes_argument`,
  `use_strict_followed_by_fn_on_same_line_still_consumes_name`
  in `tests/suite/behavior_pin_2026_05_at.rs`.
- **BUG-027** — `$#a = N` now resizes `@a` to length `N + 1` (truncates
  if shrinking, pads with `undef` if growing). Routed `#name` writes
  through `VMHelper::set_special_var`, which calls
  `scope.set_array(name, vec_resized)`. Negative values empty the array.
- **BUG-029** — `"$&"` inside double-quoted strings now interpolates the
  match result instead of staying literal. The interpolation parser
  (`parse_interpolated_string`) had explicit branches for the `'`
  (postmatch) and `` ` `` (prematch) regex special vars but missed
  `&` — added it. The `s///` replacement form (BUG-032) is a separate
  interpolation path and remains broken.
- **BUG-107** — `"$Pkg::Var"` interpolation now greedy-matches `::`
  continuations, matching the bare-code path. Multi-segment chains
  (`$A::B::C::x`) work too. Plus a separate fix in the lexer for the
  IPv6-zero-compression trap that was misfiring on 3-or-more-segment
  package paths like `package A::B::C` (the hex-digit-only ident `B`
  followed by `::` looked like an IPv6 address). The trap now skips
  when `ident_start` is preceded by `::`.
- **`smartmatch` array / hash RHS** — `given (X) { when ([list]) }`,
  `when (\@arr)`, and `when (\%hash)` now match Perl's smartmatch
  semantics. `smartmatch_when` previously fell back to string equality;
  extended to recurse over array elements and check hash-key existence.
- **`tie my $x, Class`** — common Perl idiom now parses (was rejected
  with "tie expects $scalar, @array, or %hash, got Ident(\"my\")").
  Parser desugars to `my $x; tie $x, Class` via implicit `StmtGroup`.
  Tied-hash `tie my %h, Class` works end-to-end. Tied-scalar runtime
  FETCH is a separate pre-existing limitation.
- **`or`/`and`/`not` precedence vs `=`** — `EXPR or $err = $@` now parses
  as `EXPR or ($err = $@)` (Perl's documented lowest-precedence
  operators). Stryke previously parsed this as
  `(EXPR or $err) = $@`, surfacing as "Assign to complex lvalue".
  Restructured the precedence chain to put word-ops at the top, with
  `parse_assign_expr` between `parse_not_word` and `parse_ternary`,
  and `parse_pipe_forward` descending into `parse_range` so `..`
  remains reachable.
- **Test framework isolation** — `test_run` no longer calls
  `std::process::exit(1)` from inside the VM (was hostile to
  embedding); it now sets a sticky flag (`interp.test_run_failed`) that
  the CLI driver translates to exit code 1. Test counters
  (`test_pass_count`, `test_fail_count`, `test_skip_count`) moved from
  process-global `static AtomicUsize` to per-`VMHelper` fields so
  runs in the same process don't contaminate each other. The
  `test_pass`/`test_fail`/`test_skip` progress lines now respect
  `interp.suppress_stdout`.
- **`oursync`** — package-global counterpart of `mysync`. Same
  `Arc<Mutex>` backing, but keyed by `Pkg::x` so all packages and
  parallel workers share one cell. The classic Counter pattern
  (`package C; oursync $total = 0; fn bump { $total++ }; fan_cap N
  { C::bump() }`) lands at exactly N. Plain `our` mutated inside a
  parallel block now errors strictly (DESIGN-001 parity with `my`).
- **CaptureCell nesting leak** — closures used to surface
  `SCALAR(0x...)` for outer-scope `$_` after a sequence of
  `fn outer { my $cb = sub { ... }; $cb->(...) } outer()`. Root cause:
  `Scope::capture()` walked all frames and pushed one entry per frame
  for each scalar name, so a name that shadowed itself across frames
  got declared twice during `restore_capture`, nesting a CaptureCell
  inside another. Fix: dedup hash-stored scalars at capture time,
  innermost-first; slot-stored scalars keep outer-first iteration so
  the factory-closure pattern still wins via last-write-wins on slot
  collisions.
- **Topic-variant frame-locality** — user writes to `$_`/`$_<`/`$_N`
  inside a closure used to leak through CaptureCells and clobber outer
  topic state. New `Frame::set_scalar_raw` bypasses the cell write-
  through; `Scope::set_scalar` recognises topic-variant names
  (`is_topic_variant_name`, regex `^_[0-9]*<*$`) and routes them through
  the raw path. Topic variants now follow the same frame-local rule as
  `|param|` block params and `my $x` inside a block.
- **BUG-082** — Lexer now recognises Perl 5.34+'s `0o777` / `0O777`
  octal prefix alongside `0x`, `0b`, and bare-`0`. Underscore
  separators (`0o7_7_7`) supported.
- **BUG-010 / BUG-011** — `return (1, 2, 3)` and `return 1, 2, 3` (no
  parens) both return the full list now — `return` is a list operator
  per Perl semantics. The compiler evaluates the operand in list
  context for list-shaped exprs, and `Op::ReturnValue` coerces to the
  last element when the caller's wantarray context is `Scalar`. Fixes
  every multi-value early-return idiom AND `my $x = sub_returning_
  list()` taking the last element instead of stringifying the array.
- **BUG-090** — `my ($head, @tail) = LIST` (and the canonical
  `my ($self, @args) = @_` sub-arg idiom) now binds `@tail` to the
  *tail* of the list, not the full list. New `Op::GetArrayFromIndex`
  reads `tmp[start..]` for the slurpy decl. Hash variant
  `my ($a, %h) = (...)` builds `%h` from the tail's alternating
  key-value pairs.
- **BUG-009** — `exists $h{x}{y}{z}` and `exists $a[5][0]` now soft-fail
  to 0 at any missing or non-container intermediate level (was: erroring
  with "exists argument is not a HASH/ARRAY reference"). Multi-level
  chains route through `Op::ExistsExpr` + `eval_expr_exists_mode` which
  propagates undef instead of erroring on intermediate derefs. Matches
  Perl 5 across 13 differential cases.
- **BUG-019** — `for (@arr) { $_ *= 10 }` now mutates `@arr` in place.
  Bytecode compiler detects a bare-`@arr` source and emits an
  `Op::SetArrayElem` write-back at the merged step target so both
  normal-completion and `next` paths flush the loop variable's current
  value back to the source array. Named loop vars (`for my $x (@a)`)
  alias too. Non-lvalue sources (ranges, list literals, `keys`) keep
  copy semantics — matching Perl 5 exactly.
- **PARITY-012** — `use overload "+" => sub { ... }` now accepts
  anonymous-sub handlers. Parser promotes the anon body to a synthetic
  top-level `__overload_anon_N` SubDecl; install_use_overload_pairs
  re-binds it under the current package so dispatch resolves
  `Pkg::__overload_anon_N`. Named-sub form (`"+" => "add"`) keeps
  working.
- **PARITY-011** — `CORE::keyword(...)` now parses identically to bare
  `keyword(...)`. Parser strips a leading `CORE::` prefix before the
  keyword-dispatch match, so `CORE::length` produces `ExprKind::Length`,
  `CORE::print` produces `ExprKind::Print`, etc. — same AST as the
  unprefixed forms. Matches Perl 5's documented `CORE::` namespace.
- **PARITY-010** — `vec($s, $offset, $bits) = N` lvalue now works.
  Compiler rewrites the assignment to `$s = vec_set_value(...)`, where
  `vec_set_value` is a new internal 4-arg builtin that returns the
  modified bit-buffer. While fixing the lvalue, the existing 16/32-bit
  `vec` *read* path was also corrected — Perl uses big-endian byte
  order for multi-byte BITS, and zero-pads past-the-end reads (stryke
  previously did neither).
- **PARITY-013** — `length` now respects `use utf8;`. With the pragma
  active, scalar args count Unicode codepoints; without it, UTF-8
  bytes. Raw byte buffers always return byte count. Per-interpreter
  flag (not a process-global static) so concurrent test workers don't
  bleed pragma state.
- **PARITY-016** — Named-unary precedence: `ref $@ eq "E"`,
  `length $s == 3 ? "Y" : "N"`, and similar idioms now parse as
  `(ref $@) eq "E"` / `(length $s) == 3 ? "Y" : "N"` — matching Perl.
  `parse_one_arg_or_default` (every Perl named-unary builtin: `ref`,
  `length`, `lc`, `uc`, `chr`, `ord`, `hex`, `oct`, `int`, `abs`,
  `sqrt`, `sin`, `cos`, `exp`, `log`, etc.) parses the bare argument
  at named-unary precedence (shift-level) instead of full assignment-
  expression precedence. List-op `rev` got its own arg path so
  `rev 1..3` still parses as `rev(1..3)`.
- **PARITY-015** — `"Inf"` / `"Infinity"` / `"NaN"` strings (case-
  insensitive, optional leading sign) now numify to actual float
  specials. `format_float` also prints `Inf` / `-Inf` / `NaN` (Perl's
  capitalization) instead of libc's lowercase default. Covers
  `"Inf" + 1`, `9 ** 9 ** 9`, `sqrt(-1)`, `log(0)`, `log(-1)`, and
  `0 ** -1`, all matching Perl 5.42 exactly.
- **BUG-025, BUG-050** — `$SIG{__WARN__}` / `$SIG{__DIE__}` handlers now
  fire (commit 3669fb30a3).
- **BUG-017, BUG-034, BUG-049, PARITY-006, PARITY-007** — sprintf `+` /
  `#` flags, `*` width and `.*` precision, `%g` shortest-form selection,
  and `%e`/`%E` Perl-style exponent (`1.234568e+04` instead of
  `1.234568e4`) are now all matching Perl exactly across 38 tested
  format specifiers.
- **PARITY-014** — `substr($s, $o, $l) = $rhs` lvalue assignment now
  works. The bytecode compiler recognises an `Assign { target: Substr
  { replacement: None }, value }` shape and rewrites it to the 4-arg
  form `substr($s, $o, $l, $rhs)`. Two-arg, three-arg, negative-offset,
  zero-length insert/append, and the explicit 4-arg form all match
  Perl across 8 differential cases.
- **PARITY-005** — `%` now uses Perl-style floored division so the
  result has the sign of the divisor (or is zero). New helper
  `value::perl_mod_i64` wraps the snap. Float operands are truncated
  to int first, matching Perl. The compound `$x %= rhs` form picks up
  the same fix; the previously-buggy `rem_euclid` site (always-non-
  negative, neither Rust's nor Perl's) is gone.
- **PARITY-004** — division-by-zero (and modulus-by-zero) now raise
  `ErrorKind::DivisionByZero` instead of `ErrorKind::Runtime`. The
  user-visible message ("Illegal division by zero" / "Illegal modulus
  zero") is unchanged. Lets `try`/`catch` and lib-API consumers match
  the kind specifically.
- **PARITY-003** — `use bigint;` (and `use bignum;` / `use bigrat;`)
  now activates BigInt promotion for `**`, `+`, `-`, `*`, and `*=`.
  Same numeric-promotion path as `--compat` but gated by the source-
  level pragma. Bare `2 ** 64` (without `use bigint;` and without
  `--compat`) continues to fall back to `f64`, matching Perl's
  default. Note: `use bigint; ... no bigint;` still cancels because
  pragmas are processed at compile time globally; full lexical scoping
  would require a separate effort.
- **PARITY-002** — `(my $copy = $orig) =~ s///` and the matching
  `tr///` form now bind the substitution to `$copy` (the freshly
  declared variable inside the parens), leaving `$orig` untouched.
  `assign_value` learned to recognize `MyExpr` as an lvalue and write
  through to the declared variable's name slot without re-running the
  initializer.
- **PARITY-001** — magic string increment (`"b"++ → "c"`, `"Az"++ →
  "Ba"`, `"zz"++ → "aaa"`, `""++ → "1"`, `"a9"++ → "b0"`). Decrement
  has no magic counterpart in Perl 5 and stays numeric. Pure-digit
  and mixed (e.g. `"9a"`) strings continue to fall back to numeric
  increment. Wired through every VM increment op (`PostInc`,
  `PostIncSlot`, `PreIncSlot`, `PreIncSlotVoid`) via a shared
  `perl_inc` helper.
- **BUG-057, BUG-079, BUG-080, PARITY-008, PARITY-009** — sprintf `%a` /
  `%A` (C99 hex-float), `%n` (write byte-count through scalar ref),
  `%p` (deterministic placeholder), `%v...` (vectorize per-byte through
  inner conversion, `.`-joined), and `%N$X` positional args. All match
  Perl modulo the `%p` design choice (stryke uses `0x...` placeholder
  rather than live SV addresses).

## High-impact bugs (worth fixing first)

These break common Perl idioms across the codebase:

| ID | Summary |
|----|---------|
| BUG-037 | Closure-captured coderefs called with `@_` flatten to scalar count |
| BUG-089 | Closures capture outer-scope `my` vars by value — outer counter idiom broken |
| BUG-090 | `my ($head, @tail) = LIST` slurps full LIST into `@tail` |
| BUG-095 | `my ($cb, @rest) = @_` slurps full `@_` into `@rest` (same root as BUG-090) |
| BUG-101 | `my ($x) = @arr` returns scalar count instead of first element |
| BUG-010 | `return (1, 2, 3)` collapses to last comma operand |
| BUG-011 | `my $s = list_sub()` concatenates instead of taking last element |
| BUG-018 | `local $/; <$fh>` does not enable slurp mode |
| BUG-019 | `for (@a) { $_ *= 10 }` does not alias array element for mutation |

These compound: BUG-095 breaks every `($cb, @rest) = @_; $cb->(@rest)`
pattern, BUG-089 breaks every state-tracking closure, and BUG-037 breaks
every coderef-call-with-array-arg. Together they make most functional-
style libraries unusable until fixed.

## BUG-120 — `cosine_distance` with a zero-length vector operand returns **1** — **`polish`**

When either argument has Euclidean norm ~0 (`cosine_similarity` is undefined),
`builtin_cosine_distance` clamps to **1** (maximum distance). That matches the
Rust guard `na < 1e-15 || nb < 1e-15` but differs from ecosystems that propagate
NaN instead of a finite sentinel.

Pin test: `cosine_distance_zero_operand_is_unit_bx` in
`tests/suite/behavior_pin_2026_05_bx.rs`.

## BUG-121 — `median_absolute_deviation` uses `sorted[len/2]` as the central value — **`bug`**

For even-sized samples the implementation takes `vals[vals.len() / 2]` after
sorting rather than the mean of the two middle order statistics (the usual
definition of the sample median). The subsequent median of absolute deviations
is therefore skewed whenever the even-length middle pair straddles a wide gap.

Example: `median_absolute_deviation(1, 2, 100, 101)` returns **98** because
the code treats the “median” of the data as **100**; the conventional median
would be **51**, yielding a much smaller MAD (~49.5).

Pin test: `median_absolute_deviation_even_n_spread_bx` in
`tests/suite/behavior_pin_2026_05_bx.rs`.

## BUG-122 — `js_divergence` / `js_div` vs `jensen_shannon_div` disagree (nats vs bits) — **`bug`**

`js_divergence` (in `math_wolfram3.rs`) builds KL terms with **natural**
logarithms. `jensen_shannon_div` is wired to `kullback_jensen_div`
(`math_wolfram40.rs`), which uses **log2** in each KL term. The two therefore
differ by a factor of **`ln 2`** for the same distributions even though docs
refer to both as Jensen–Shannon-style quantities.

Illustrative non-uniform pair (pinned numerically):

- `sprintf("%.12f", jensen_shannon_div(...)) → "0.031596722287"`
- `sprintf("%.12f", js_div(...))          → "0.021901178968"`

Pin tests: `jensen_shannon_div_triple_bx`, `js_divergence_triple_nats_bx` in
`tests/suite/behavior_pin_2026_05_bx.rs`.

## BUG-123 — `chi_squared_distance` vs `chisquare_metric` differ by a factor of **2** — **`bug`**

Both walk the elementwise \(\sum_i (p_i-q_i)^2/(p_i+q_i)\)
pattern, but `chi_squared_distance` (`math_wolfram4.rs`) multiplies by
**`0.5`** while `chisquare_metric` (`math_wolfram40.rs`) omits it. Names
give no indication which convention applies.

Pins: `chisquare_metric_axis_pair_by`, `chisquare_metric_equals_twice_chi_squared_distance_by`
in `tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-124 — `csiszar_phi_div` is \(\sum_i q_i \ln(p_i/q_i) = -\mathrm{KL}(Q\|P)\), not an unsigned ϕ-form — **`bug`**

Rust comment claims “Csiszár ϕ-divergence: \(\sum q \, \phi(p/q)\)” with the
usual convex \(\phi\) so the sum is \(\mathrm{KL}(P\|Q)\) nonnegative. The
implementation instead accumulates **`q_i * ln(p_i/q_i)`**, which yields
**\(-\mathrm{KL}(Q\|P)\)** and can surface **negative floats** whenever
\(Q\neq P\).

Pin test: `csiszar_phi_div_coin_pair_by` in
`tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-125 — `relative_entropy_kl` measures KL in **bits**; `kl_divergence` / `kl_div` use **nats** — **`bug`**

`builtin_relative_entropy_kl` (`math_wolfram40.rs`) uses `(p/q).log2()`.
The older `builtin_kl_divergence` path (`math_wolfram3.rs`) uses `.ln()`
throughout. Multiply the former by \(\ln 2\) to reproduce the latter for the
same \(P,Q\).

Pin tests: `relative_entropy_kl_uses_bits_by`,
`relative_entropy_kl_times_ln2_matches_kl_div_by` in
`tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-126 — Entropy/share builtins read only **`args.first()`**, dropping comma-arg tails — **`bug`**

Many helpers flatten **one** positional argument (`arg_to_vec(&args[0])` or read
via `args.first()` as a lone arrayref/scalar). Supplying probabilities or values
**as Perl variads** (`f(p1, p2, p3)`, no square brackets) therefore keeps only the
leading scalar and ignores the comma-separated tails. Pass a single **array ref**
explicitly (`f([ p1, p2, … ])`) to aggregate the intended list today.

Demonstrated builtins (non-exhaustive):

| Builtin | Pins |
|---------|------|
| `joint_entropy_step` | `joint_entropy_four_uniform_coin_bits_array_bz`, `joint_entropy_variadic_trailing_probs_ignored_tail_bz` |
| `herfindahl_hirschman`, `hhi` | `herfindahl_hirschman_normalized_quarter_shares_array_bz`, `hhi_variadic_trailing_shares_use_first_squared_only_tail_bz` |
| `gini_impurity` | `gini_impurity_three_class_normalized_array_bz`, `gini_impurity_variadic_first_probability_only_tail_bz` |
| `entropy_bits` | `entropy_bits_four_coin_array_equals_two_tail_bz`, `entropy_bits_variadic_degenerate_after_truncation_tail_bz` |
| `log_sum_exp`, `lse` | `log_sum_exp_array_maximum_dominated_stable_bz`, `log_sum_exp_variadic_first_scalar_only_tail_bz` |
| `lorenz_curve_points` | `lorenz_curve_points_sorted_three_in_array_ca`, `lorenz_curve_points_variadic_truncated_tail_ca` |
| `grade_up` | `grade_up_permutation_three_ca`, `grade_up_variadic_first_element_only_ca` |
| `grade_down` | `grade_down_permutation_three_ca`, `grade_down_variadic_first_scalar_only_ca` |
| `npv` | `npv_array_discounts_four_uniform_periods_ce`, `npv_variadic_second_bucket_only_counts_lead_outflow_ce` |
| `irr` | `irr_array_newton_positive_rate_ce`, `irr_variadic_first_flow_only_interprets_second_as_guess_ce`, `irr_satisfies_npv_near_zero_residual_ce` |
| `payback_period` | `payback_requires_array_bucket_second_arg_ce` (variadic commas miss the **`args[1]`** array bucket → **`undef`**) |
| `discounted_payback` | `discounted_payback_requires_array_middle_bucket_ce` (same **`args[1]`** coupling) |
| `resistance_parallel` | `resistance_parallel_three_resistors_array_cf`, `resistance_parallel_variadic_ignores_trailing_cf` |
| `resistance_series` | `resistance_series_array_sum_cf`, `resistance_series_variadic_first_only_cf` |
| `capacitance_parallel` / `capacitance_series` | **`capacitance_parallel_series_array_buckets_cf`** (`arg_to_vec` on **`args.first()`** only) |
| `inductance_parallel` / `inductance_series` | **`inductance_parallel_formula_matches_reciprocal_cf`**, **`inductance_series_linear_sum_cf`** |
| `charcodes_to_string` | **`charcodes_to_string_array_round_trip_hi_cg`**, **`charcodes_to_string_variadic_second_codepoint_dropped_tail_cg`** |
| `squared` / `sq` | **`squared_three_ch`**, **`squared_variadic_second_operand_ignored_ch`**, **`sq_alias_matches_squared_ch`** |
| `cubed` / `cb` | **`cubed_two_ch`**, **`cubed_variadic_second_operand_ignored_ch`**, **`cb_alias_matches_cubed_ch`** |
| `uniq` | **`uniq_variadic_deduplicates_neighbors_ch`**, **`uniq_single_array_bucket_treated_as_atom_ch`** |
| `sum` / `sum0` / `product` | see **BUG-140** |


Pins documenting **tail truncation** split across **`tests/suite/behavior_pin_2026_05_bz.rs`**,
**`behavior_pin_2026_05_ca.rs`** (Lorenz + `grade_*`), **`behavior_pin_2026_05_ce.rs`** (NPV/IRR + paybacks), and **`behavior_pin_2026_05_cf.rs`**
(passive **R/L/C** ladders). Companion geo/string pins live in **`behavior_pin_2026_05_cg.rs`**
(geohashes, projections, kernels, AES/Simon graph helpers).

**`behavior_pin_2026_05_ca.rs`** also pins assorted ML helpers (`confusion_counts`, `mcc`,
`hinge_loss`, …) strictly for reproducible floats — **not** tail-drop cases.

List / stats companion pins: **`tests/suite/behavior_pin_2026_05_ch.rs`** (also **`chain_from`**
**`ARRAYREF`** pitfall — **BUG-142**), **`behavior_pin_2026_05_ci.rs`** (streaming / `to_list` traps — **BUG-143** … **BUG-146**),
and **`behavior_pin_2026_05_cj.rs`** (list glue + **`permutations([...])`** — **BUG-147**, **`concat`** — **BUG-148**),
**`behavior_pin_2026_05_ck.rs`** (**`without([...], LIST)`** — **BUG-149**; multiset order — **BUG-150**),
**`behavior_pin_2026_05_cl.rs`** (**BUG-151** … **BUG-155** — clamp / strings / `hamming` / `substr` / **`reverse([...])`**),
**`behavior_pin_2026_05_cm.rs`** (**`seq`** / **`crc32`** multi-arg — **BUG-156**, **BUG-157**),
**`behavior_pin_2026_05_cn.rs`** (**`parse_int("0xff")`** — **BUG-158**; **`transpose`** nested AoA — **BUG-159**; regex helper arg order — **BUG-160**),
**`behavior_pin_2026_05_co.rs`** (**`percentile`** / **`quantile`** conventions — **BUG-161**; **`take`** / **`product`** **`ARRAYREF`** buckets cross-ref **BUG-143** / **BUG-140**),
**`behavior_pin_2026_05_cp.rs`** (scalar planar **`chebyshev` / `slope` / `midpoint`** vs vector distances — **BUG-162**),
**`behavior_pin_2026_05_cq.rs`** (**`running_reduce`** + **`$a`/`$b`** — **BUG-163**; **`uri_resolve` / `uri_normalize`** byte vectors — **BUG-164**),
**`behavior_pin_2026_05_cr.rs`** (**`string_take_while` / `string_drop_while`** charset-prefix semantics — **BUG-165**; **`nth`** on **`ARRAYREF`** — **BUG-166**; **`gcd` / `lcm`** two-operand only — **BUG-167**),
**`behavior_pin_2026_05_cs.rs`** (**`hamming`** vs **`hamming_distance`** — **BUG-168**; **`matrix_transpose`** — cross-ref **BUG-159** / variadic transpose),
**`behavior_pin_2026_05_ct.rs`** (**`hhi` / `herfindahl_hirschman`** share vector — **BUG-169**),
**`behavior_pin_2026_05_cu.rs`** (**`moving_average` / `batch` / `chunk_n`** arity — **BUG-170**),
**`behavior_pin_2026_05_cv.rs`** (**`ml_binary_cross_entropy`** open interval — **BUG-171**),
**`behavior_pin_2026_05_cw.rs`** (**`jaccard_similarity`** string-set collapse on vector args — **BUG-172**; **`mode([…])`** bracket
operand — **BUG-173**),
**`behavior_pin_2026_05_cx.rs`** (**`windowed` / `chunked`** — **BUG-174**; **`trimmed_mean`** — **BUG-175**; **`base_convert`**
two-arg numeric — **BUG-176**),
**`behavior_pin_2026_05_cy.rs`** (**`graph_density`** — **BUG-177**; **`transpose`** vs **`matrix_transpose`** — **BUG-178**),
**`behavior_pin_2026_05_cz.rs`** (**`pmt`** arg order — **BUG-179**; **`format_percent`** — **BUG-180**),
**`behavior_pin_2026_05_da.rs`** (**`anova_oneway`** nested AoA — **BUG-181**; **`trapz` / `simpson`** second operand — **BUG-182**),
**`behavior_pin_2026_05_db.rs`**: **BUG-183** (search/bounds needle-first), **BUG-184** (`dice_coefficient` strings), **BUG-185** (`winsorize` percent-first),
**`behavior_pin_2026_05_dc.rs`**: **BUG-186** (`unzip` vs row pairs), **BUG-187** (`clamp_list` inverted bounds panic).

## BUG-127 — `iota_range` ignores arguments after the first — **`polish`**

`builtin_iota_range` consumes only \(N\) from `args[0]`. Passing `iota_range(5,
99)` (or longer comma tails) parses as Perl variadic call sites normally do but
everything after **`5`** is discarded with no arity error, so callers can
mistakenly believe they threaded multiple ranges.

Pins: `iota_range_zero_until_n_exclusive_cb`,
`iota_range_trailing_numeric_args_ignored_matches_five_only_cb` in
`tests/suite/behavior_pin_2026_05_cb.rs`.

## BUG-128 — `lambert_w0` (and **`wright_omega(0)`**) returns **NaN** at **`W(1)`** — **`bug`**

`builtin_lambert_w0` selects the Halley initializer **`ln(x) - ln(ln(x))`** whenever
\(x \ge 1\). Exactly at **`x == 1`**, \(\ln(\ln 1) = \ln 0\) is undefined in IEEE
floating point, polluting **`w`** with **NaN** before the iterations can recover.
Adjacent values (including **`exp(1)`**) still converge normally.

Because **`wright_omega(z)`** is implemented as **`lambert_w0(exp(z))`**, plugging
\(z = 0\) reduces to **`W(1)`** and hits the same NaN (**`Ω` absent** despite the
literature \(\omega(0)=\Omega\) within branch conventions).

Pins (contrast finite principal branch neighbors vs NaN sentinel):

| Case | Pins |
|------|------|
| Working paths | `lambert_w_omega_constant_cc`, `lambert_w_at_exp_two_known_branch_cc`, `lambert_w0_at_e_equals_one_principal_cc`, `lambert_w0_above_one_finite_two_cc`, `wright_omega_exponential_branch_cc` |
| NaN regressions | `lambert_w0_at_exactly_one_is_nan_bug_cc`, `wright_omega_zero_is_nan_bug_cc` |

Batch: **`tests/suite/behavior_pin_2026_05_cc.rs`** (also aggregates many analytic/combinatorial pins unrelated to Lambert).

## BUG-129 — `convolve_*`/`correlate_full`/`kron_product` return **sizes**, not convolution values — **`bug`**

`math_wolfram72.rs` computes only scalar dimensions (`len(a)+len(b)-1`, valid overlap counts,
Kronecker flat cardinality). Callers naming these after textbook convolution expect full
summed outputs (like **`cross_correlation`** already emits).

Pins: `convolve_full_reports_output_length_minus_one_stub_cd`,
`convolve_valid_reports_overlap_extent_stub_cd`, `correlate_full_same_impl_as_conv_stub_cd`,
`kron_product_cardinality_multiplier_stub_cd` in `tests/suite/behavior_pin_2026_05_cd.rs`,
plus **`cross_correlation_sliding_sumdefinition_cd`** for the real sliding-sum variant.

## BUG-130 — `detrend_linear` returns **slope**, not **detrended samples** — **`polish`**

Despite the noun-like name mirroring MATLAB's `detrend`, the builtin returns **`num/den`** from
the single least-squares line fit — a scalar slope estimate only. Users expecting residual series
subtract the fit manually today.

Pin: `detrend_linear_pure_ramp_slope_one_cd` in `tests/suite/behavior_pin_2026_05_cd.rs`.

## BUG-131 — `medfilt_1d` is not a (**2k+1**) sliding-window median filter — **`bug`**

Implementation flattens the entire operand, globally sorts **all samples**, then returns **one**
median of the multiset. There is **no positional windowing** contrary to Rustdoc ("1-D median filter:
median of (**2k+1**)-sized window centred at i").

Pin: `medfilt_one_d_global_sorted_median_cd` in `tests/suite/behavior_pin_2026_05_cd.rs`.

## BUG-132 — **`bs_*` greeks** (`bs_delta`, **`bs_theta`**, **`bs_rho`**) are **call** formulas — **`polish`**

`builtin_bs_delta` returns **`N(d1)`** only — textbook **put \(\Delta\)** is **`N(d1) - 1`** (pins show the
**\(-1\)** parity gap next to **`bs_delta`**). **`bs_theta`** and **`bs_rho`** inline the derivatives of the **call**
price (**`-r · K · e^{-rT} · N(d2)`** curvature terms), **not** the put equivalents (which flip signs on pieces
stemming from \(\partial N(-d\*)/\partial T\) / \(\rho\)).

Pins documenting current call-only Greeks: **`bs_delta_returns_call_delta_cdf_d1_ce`**,
**`bs_put_delta_equals_call_delta_minus_one_ce`**, **`bs_theta_call_style_negative_ce`**, **`bs_rho_call_style_positive_ce`**
in **`tests/suite/behavior_pin_2026_05_ce.rs`**.

## BUG-133 — **`depreciation_double`** ignores the **salvage**/middle operand — **`bug`**

`builtin_depreciation_double` reads **`cost`** (`args[0]`) and **`life`** from **`args[2]`**, skipping **`args[1]`**
entirely. Callers threading **`double_declining(cost, salvage, life)`** like **`depreciation_linear`** silently drop
 **`salvage`**, overstating depreciation relative to accountants' double-declining convention that floor-values against
scrap.

Pins: **`depreciation_double_ignores_salvage_middle_arg_ce`**, **`depreciation_double_middle_arg_does_not_affect_rate_ce`**
in **`tests/suite/behavior_pin_2026_05_ce.rs`**.

## BUG-134 — **`weber_number`** clamps a **missing** \(\sigma\) to **1e-30** — **`bug`**

`builtin_weber_number` computes **`ρ v² L / σ`** with **`σ = max(args[3].unwrap_or(0.0), 1e-30)`**. Omitting \(\sigma\)
therefore divides by **\(10^{-30}\)** rather than returning an arity error — orders of magnitude larger than
reasonable surface-tension values. The companion **`weber_number_step`** defaults **`σ = 0.072`** (`N/m`), which is
the usual water–air ballpark.

Pins: **`weber_number_requires_sigma_fourth_arg_cf`**, **`weber_number_step_matches_definition_with_default_sigma_cf`**,
**`weber_number_omitting_sigma_explodes_via_tiny_denominator_cf`** in **`tests/suite/behavior_pin_2026_05_cf.rs`**.

## BUG-135 — **`dB_voltage`** / **`dB_power`** missing reference becomes **1e-30** → **spurious giant dB** — **`bug`**

Both helpers clamp the reference argument with **`.max(1e-30)`** (`math_wolfram12.rs`). Calling **`dB_voltage(V)`** with
only the numerator sets **`V_in = 1e-30`**, yielding **`20·log10(V / 10⁻³⁰)` ≈ 606 dB** instead of a controlled default
like **1 V** or **`undef`**.

Pins: **`db_voltage_two_reference_cf`**, **`db_power_two_reference_cf`**, **`db_voltage_missing_reference_balloons_cf`**
in **`tests/suite/behavior_pin_2026_05_cf.rs`**.

## BUG-136 — **`geohash_neighbor`** nudges \(\Delta\)lat/\(\Delta\)lon with **tiny isotropic **`2^{-(5·len/2)}`** (\(i32\)**) **step** → **effective no-op at common precisions** — **`bug`**

`builtin_geohash_neighbor` decodes **`s`**, then shifts **lat** / **lon** by **one magnitude** (**`step = 1 /
2^{(\texttt{len} \cdot 5 / 2)}`** in Rust integer division) every direction. Typical **~6-character** hashes use a **sub-cell**
**\(\Delta\)** versus the **child-bit** quantization of **`geohash_encode`** — perturbations Round-trip inside the **same**
base-32 string (**`geohash_neighbor_cardinals_are_identity_at_precision_six_cg`**). Applying the **same \(\Delta\)**
to **latitude** and **longitude** also ignores customary **North–South** vs **East–West** bin anisotropy. **`match dir.as_str()`**
fall-through assigns **\((0, 0)\)** for unknown direction tokens (**`geohash_neighbor_unknown_direction_leaves_hash_unchanged_cg`**)
instead of an error.

Pins: **`geohash_neighbor_cardinals_are_identity_at_precision_six_cg`**, **`geohash_neighbor_unknown_direction_leaves_hash_unchanged_cg`**
in **`tests/suite/behavior_pin_2026_05_cg.rs`**.

## BUG-137 — **`box_blur_kernel`** first argument is **half-width radius `r`**, output side **`2r+1`** — **`polish`**

`builtin_box_blur_kernel` computes **`n = 2·r + 1`** from `args.first()` as an integer **radius** (`math_wolfram14.rs`). Callers
supplying **`box_blur_kernel(7)`** expecting a **\(7\times7\)** stencil actually materialize a **\(15\times15\)** (**`2·7+1`**) kernel.
The entry value is **`1 / n²`** (uniform norm).

Pin: **`box_blur_kernel_radius_three_is_seven_squared_weights_cg`** in **`tests/suite/behavior_pin_2026_05_cg.rs`**.

## BUG-138 — **`clamp` call-shape heuristic** vs **`clamp_list(LIST...)`** — **`polish`**

`builtin_clamp` is documented as **`clamp MIN, MAX, LIST...`** (and pipeline-friendly `LIST |>
clamp MIN, MAX`). When callers pass **`([v1, v2, ...], lo, hi)`** expecting per-element clamping
like other languages, the implementation still treats **`args[0]`** / **`args[1]`** as **min/max
scalars** (with **`args[0].to_number()`** taking the **first list element** as the min) and only
the **third argument** expands into the value list. Result: silent mis-clamps (single scalar
return) instead of a tuple. Use **`clamp_list(lo, hi, ...)`** for the **`lo, hi` first** layout today.

Pins: **`clamp_wrong_shape_list_first_reads_min_from_first_element_ch`**,
**`clamp_min_max_then_values_tuple_ch`**, **`clamp_list_explicit_vector_form_ch`**
in **`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-139 — **`normalize`** docs mention **`OUT_MIN, OUT_MAX, LIST`**; implementation always **`0..1`** — **`polish`**

Rustdoc on **`builtin_normalize`** sketches a **`normalize OUT_MIN, OUT_MAX, LIST`** form. The body
fixes **`out_min`** / **`out_max`** at **`0.0` / `1.0`** and flattens **all** positional arguments into
the sample multiset, so leading “range” operands become ordinary data rows.

Pin: **`normalize_extra_leading_scalars_folded_into_source_strip_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-140 — **`sum` / `sum0` / `product`** skip **`ARRAYREF`** innards for a lone **`[...]`** operand — **`bug`**

`list_builtins::sum`, `sum0`, `product` only recurse when **`as_array_vec()`** succeeds (dense
 heap **`HeapObject::Array`**). A typical inline **`sum([10,11])`** / **`product([6,7])`** arrayref
hits the **`else`** arm and **`to_number()`** the container as a single scalar (**`0`** today),
rather than iterating elements. Prefer **`sum(10, 11)`**, **`sum_list([10,11])`** (pinned), or **`sum
@ary`** after materializing **`@ary`** without the boxed-ref ambiguity.

Pins: **`sum_single_inline_array_yields_zero_bug_ch`**, **`sum_list_reads_array_contents_ch`**,
**`product_single_inline_array_discards_interior_bug_ch`**, **`sum_variadic_two_addends_ch`**,
**`product_variadic_two_factors_ch`** in **`tests/suite/behavior_pin_2026_05_ch.rs`** ( **`sum0`**
empty path: **`sum0_empty_is_zero_ch`** ).

## BUG-141 — **`frequencies` / string operands** — one scalar ⇒ one hash key (**`polish`**)

Flattening treats a **`Str`** Perl value as a **single countable item**, so **`frequencies("aab")`**
returns **`{"aab" => 1}`** unless the string is first split into graphemes (**`chars(...)`** /
**`split("", ...)`**). Not a hashing bug once element cardinality is understood, but differs from
“count characters” intuition.

Pins: **`frequencies_whole_string_counts_as_one_key_ch`**, **`frequencies_chars_aab_two_keys_ch`**,
**`pfrequencies_matches_frequencies_large_multiset_parallel_path_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-142 — **`chain_from([[...],[...]])`** leaves inner **`ARRAYREF`** buckets as opaque atoms — **`bug`**

`builtin_chain_from` does `flatten_args` then **`item.to_list()`** per segment. **`PerlValue::to_list`**
only expands **`HeapObject::Array`** (`Array` storages); a typical literal inner **`[..., ...]`**
is stored as **`ArrayRef`** (RW handle), whose **`to_list`** arm falls through **`_ ⇒
vec![self.clone()]`**. A single outer array argument **`([[1,2],[3]])`** therefore concatenates **four**
**list-valued slots** instead of draining their elements. Spreading the same buckets as Perl variadic
arguments (**`chain_from([1,2],[3],[4])`**) already worked.

Pins: **`chain_from_variadic_top_level_lists_concat_ch`**,
**`chain_from_single_outer_arrayref_leaves_inner_lists_unmerged_bug_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-143 — **`PerlValue::to_list` + iterator plumbing** treat many **`ARRAYREF`** / “one arg” shapes as **atoms** — **`bug` / `polish`**

- **`HeapObject::ArrayRef`** (typical literal **`[ … ]`**) falls through **`PerlValue::to_list`’s `_` arm** and becomes a **single opaque cell** instead of cloning the inner vector (unlike **`HeapObject::Array`**). Any helper that only calls **`to_list()`** (rather than **`map_flatten_outputs`**) mis-counts operands: pinned for **`head`** / **`tail`** / **`drop`** / **`take`** with **`head([1,2,3], 2)`**.
- Streaming builtins that special-case “one non-iterator argument” still route through **`into_pull_iter`**: that path also uses **`to_list`**, so **`ARRAYREF` sources** expose **one streamed item** (breaks **`chunk(2, [...])`** expectations). Variadic / iterator call shapes work today — e.g. **`chunk(2, range(1, 5))`**, **`dedup(1, 1, 2)`**.
- **`enumerate`**, **`dedup`**, **`chunk`**: when passed a **single** list argument, the implementation wraps **`PerlValue::array(args.to_vec())`** for the pull source, so **`enumerate([a,b])`** yields **one** indexed row **`[0, list]`** (the whole list as the item) rather than per-element indices (contrast **`enumerate(range(1, 3))`**).
- **`PerlIterator::collect_all` on `CycleIterator` is intentionally `vec![]`** (infinite source guard), but **`flatten_args` / `map_flatten_outputs` call `collect_all`** for iterators — so compositions like **`take_n(6, cycle([1, 2, 3]))`** materialize **`()`** today.

Pins throughout **`tests/suite/behavior_pin_2026_05_ci.rs`** (file module doc enumerates the **`_ci`** suffix names).

## BUG-144 — **`transpose([[row1],[row2]])` does *not* transpose an AoA** — **`polish`**

`builtin_transpose` only ingests **top-level actuals** whose **`.as_array_ref()`** succeeds — one nested bracket form **`([[1,2],[3,4]])`** is parsed as **one row** whose columns are the **inner row refs**, not a 2×2 matrix. Use **`transpose`** with **multiple row operands** (**`transpose([1, 2], [3, 4])`**).

Pins: **`transpose_single_nested_outer_array_clusters_rows_bug_ci`**, **`transpose_two_row_arguments_column_major_ci`**.

## BUG-145 — **`unzip_pairs(zip(...))`** shreds pair rows because **`flatten_args` deep-merges** tuple innards — **`bug`**

`zip` already returns an array of pair rows, but **`builtin_unzip_pairs` calls `flatten_args`**, and each **dense inner array** expands to **raw scalars**, so the unzip walk pairs **`(1,9), (2), (8, undef)`** style garbage. Pass an explicit pair list (**`unzip_pairs([[1, 9], [2, 8]])`**) or rebuild pairs without an intermediate **`zip`** unless / until **`flatten_args` stops peeling pair innards**.

Pins: **`unzip_pairs_explicit_pair_rows_ci`**, **`unzip_pairs_after_zip_over_flattens_to_scalars_bug_ci`**.

## BUG-146 — **`take_n(_, cycle(...))` is vacuous**: **`CycleIterator::collect_all` → `[]` under `flatten_args`** — **`bug`**

**`flatten_args`** expands iterators via **`map_flatten_outputs`**, which invokes **`PerlIterator::collect_all`**. **Infinite `cycle` iterators return an empty snapshot** (“do not eagerly loop forever”), leaving **`take_n`** with **no input elements**, so stringify is **`()`** today.

Pin: **`take_n_cycle_iterator_yields_empty_today_bug_ci`**.

## BUG-147 — **`permutations([...])`** (one argument) vacates: first slot **`to_int` → 0** — **`polish`**

**`permutations N, LIST`** is documented as taking a numeric **\(N\)** first. A **single** bracket
array actual **`permutations([1, 2, 3])`** still parses as one argument; **`PerlValue::to_int`** on an
**`ARRAYREF` is `0`**, the implementation treats **`n == 0`**, and returns **`()`** instead of
**full-list permutations**. Call **`permutations(scalar(@xs), \@xs)`** / **`permutations(len(\@xs), \@xs)`**
or the explicit **`permutations(3, [1, 2, 3])`** shape.

Pins: **`permutations_k_equals_list_length_three_cj`**, **`permutations_single_arrayref_numifies_to_zero_empty_bug_cj`**
in **`tests/suite/behavior_pin_2026_05_cj.rs`**.

## BUG-148 — **`concat` / `chain`** on **`ARRAYREF` operands** streams **one cell per argument** — **`polish`**

**`builtin_concat`** wraps each actual in **`into_pull_iter`**. A plain **`[...]`** value is an **`ARRAYREF`**
whose iterator surfaces **the whole list as one pulled item**, not element-by-element. Stringifying the
concat iterator therefore looks like **one bucket per argument** — e.g. **`([1, 2], [3], [4, 5])`** —
whereas **`chain_from([1, 2], [3], [4, 5])`** flattens top-level list slots today.

Pins: **`concat_iterator_one_bucket_per_arrayref_arg_cj`**, **`chain_from_three_lists_eager_flat_cj`**
in **`tests/suite/behavior_pin_2026_05_cj.rs`**.

## BUG-149 — **`without([...], LIST)`** does not subtract members: filter compares **ref display string** — **`bug`**

**`builtin_without`** takes **`drop = args.first()`** and drops list elements where **`v.to_string() ==
drop.to_string()`**. When **`drop`** is an **`ARRAYREF`**, **`drop.to_string()`** is the opaque
**`ARRAY(0x…)`** banner — no list element stringifies the same way, so **nothing is removed** and the
tail list is returned intact. To drop values present in another collection, flatten to scalars /
use a predicate loop / multiset helper instead of passing **`[…]`** as the selector.

Pins: **`without_scalar_filters_by_string_equality_ck`**, **`without_arrayref_first_compare_ref_display_no_drops_bug_ck`**
in **`tests/suite/behavior_pin_2026_05_ck.rs`**.

## BUG-150 — **`multiset_intersection` / `multiset_difference` (and multiset union)** emit **HashMap iteration order** — **`polish`**

**`math_wolfram10.rs`** multiset builtins walk **`HashMap`** / **`HashSet`** keys when building the result
vector. **`stringify(...)`** order is therefore **non-deterministic** run-to-run. Sort explicitly when
stable output matters (**`sort { $a cmp $b } multiset_intersection(...)`**).

Pins: **`multiset_difference_sorted_join_counts_ck`**, **`multiset_intersection_sorted_join_counts_ck`**
in **`tests/suite/behavior_pin_2026_05_ck.rs`** (sorted joins; unsorted shapes are intentionally not pinned).

## BUG-151 — **`clamp` three-scalar Perl order **`clamp($x,$min,$max)`** is mis-read as **`clamp($min,$max,@list)`** — **`polish`**

**`builtin_clamp`** treats **three** operands as **`min, max, first list value`** when the flattened
tail after the first two args is **non-empty** (even for a **single** trailing scalar). So
**`clamp(11, 0, 10)`** becomes **min=11**, **max=0**, values **`[10]`**, and **`10 < 11`** clamps to
**`11`** instead of **`10`**. For scalars, use **`clamp(0, 10, 11)`** (stryke **min,max,value** order)
or **`clamp_list`**.

Pins: **`clamp_scalar_inside_range_cl`**, **`clamp_value_min_max_order_misread_as_min_max_list_bug_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-152 — **`reverse($scalar)`** path-dependent string: **tail/assign** reverse; **`join("", …)`** does not — **`bug` / `polish`**

For a **string scalar** **`$s`**, **`reverse($s)`** as a **statement tail** or **`my $t = reverse($s); $t`** stringifies **`cba`**, but **`join("", reverse($s))`** stays **`abc`** today — list flattening / topic context treats the operand differently than assignment / return-value stringification.

Pins: **`reverse_scalar_tail_expr_stringifies_reversed_cl`**, **`reverse_scalar_after_let_binding_reversed_cl`**, **`reverse_scalar_join_list_context_stays_forward_bug_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**. (**`reverse_str`** remains the explicit grapheme reversal helper.)

## BUG-153 — bare **`hamming`** is the **DSP window**, not **string Hamming distance** — **`polish`**

Dispatch maps **`"hamming"`** to **`window_hamming`**. For **edit distance** on two strings, use
**`hamming_distance`** or **`hamming_distance_str`**.

Pins: **`hamming_distance_bit_flip_one_cl`** in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-154 — **`substr` on UTF‑8 uses **byte** offsets (not grapheme indices)** — **`polish`**

**`substr("αβγ", 1, 1)`** lands **inside** the first UTF-8 codepoint and returns **empty**; stepping
**two** bytes from the start (**`substr(..., 0, 2)`**) yields **`α`**. Use **`char_at` /
`graphemes` + indexing** when you mean **character** positions.

Pins: **`substr_ascii_slice_cl`**, **`substr_utf8_byte_window_one_grapheme_cl`**, **`substr_utf8_one_byte_mid_codepoint_empty_bug_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-155 — **`reverse([...])`** does not reverse **inner** elements (single **`ARRAYREF`** actual) — **`polish`**

Like **`uniq([…])`** / iterator bucket pitfalls, a **single** bracket array passed to **`reverse`**
is not **`map_flatten_outputs`**’d into a variadic list — **`stringify(reverse([1, 2, 3]))`** stays
**`[1, 2, 3]`**. Use **`reverse_list`**, **`reverse(1,2,3)`**, or **`reverse @{ $aref }`**-style
flattening when porting Perl.

Pins: **`reverse_variadic_three_ints_cl`**, **`reverse_single_inline_arrayref_identity_shape_cl`**, **`reverse_list_drains_bracket_list_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-156 — **`seq` is not Bash/Raku numeric `seq FIRST LAST` — only first arg is used** — **`polish`**

**`builtin_seq`** documents **`seq COLL`** — it turns one collection into a list (and **`UNDEF`**
when empty). **`seq(2, 5)`** therefore only inspects **`2`** (stringifies as **`"2"`**), not a range;
use **`range(2, 5)`** for inclusive integer steps.

Pin: **`seq_two_args_only_first_used_bug_cm`** in **`tests/suite/behavior_pin_2026_05_cm.rs`**.

## BUG-157 — **`crc32`** ignores **all bytes after `args.first()`** (extra operands silent) — **`polish`**

Implementation hashes only **`perl_scalar_as_bytes(args[0])`**. **`crc32("a","b")`** is identical to
**`crc32("a")`**, not **`crc32("ab")`**, so “split string” call sites silently diverge from intuition.

Pin: **`crc32_separate_args_differs_from_concat_bug_cm`** in **`tests/suite/behavior_pin_2026_05_cm.rs`**.

## BUG-158 — **`parse_int("0xff")` without an explicit radix is not hex** — **`polish`**

**`parse_int`** only interprets a leading **`0x`** when the second-argument radix is **`16`**. A
literal **`parse_int("0xff")`** numifies **`0`** and stops (**`0`**, not **`255`**). Use
**`parse_int("ff", 16)`** (or **`hex` / `sprintf`**) for hex byte strings.

Pin: **`parse_int_zero_x_without_radix_is_zero_bug_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**.

## BUG-159 — **`transpose`** treats a **single** nested AoA as **one row** (use variadic rows or **`matrix_transpose`**) — **`polish`**

**`transpose`** is documented as variadic rows: **`transpose(@row_a, @row_b, …)`**. Passing **one**
value that is itself an AoA (**`transpose([[1,2],[3,4]])`**) flattens only the **outer** wrapper: the
implementation iterates **`args`**, not **`args[0].rows`**, so you get a **1×2** “row of rowrefs” and a
column-major shuffle — not a **2×2** transpose. **`matrix_transpose([[1,2],[3,4]])`** matches the
usual matrix expectation.

Pins: **`transpose_variadic_rows_cn`**, **`transpose_single_nested_aoa_columns_wrapped_bug_cn`**,
**`matrix_transpose_nested_aoa_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**, and **`matrix_transpose_nested_two_by_two_cs`** in **`tests/suite/behavior_pin_2026_05_cs.rs`**.

## BUG-160 — **`count_regex_matches`** argument order differs from **`split_regex` / `match_all` / `replace_regex`** — **`polish`**

**`count_regex_matches(STR, PATTERN)`** puts the **haystack first**. The other regex helpers in the
same family take **pattern-first** call sites: **`split_regex(PAT, STR)`**, **`match_all(PAT, STR)`**,
**`replace_regex(PAT, REPL, STR)`**. Easy to permute arguments when mixing builtins in one script.

Pins: **`count_regex_matches_digits_cn`**, **`split_regex_csv_cn`**, **`match_all_digit_pattern_first_cn`**,
**`replace_regex_global_digits_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**.

## BUG-161 — **`percentile`** vs **`quantile`**: **percent scale** (0–100) **and** operand order differs — **`polish`**

**`builtin_percentile`** takes **`(P, LIST...)`** — the **probability mass** is **`args.first()`**, clamped to
**`[0, 100]`**, and the sample is **`args[1..]`**. **`builtin_quantile`** takes **`(LIST..., P)`** — **all but the
last** argument are data values, and **`P`** is **`args.last()`** in the **`[0, 1]`** interval with linear
interpolation between sorted neighbors.

So **`percentile(0.5, DATA)`** is **not** “half”; it is the **0.5th percentile** (bottom bucket after rounding).
The median in **`percentile`** units is **`percentile(50, DATA)`**. **`quantile(DATA, 0.5)`** is the usual **`0.5`**
quantile (**median**); the swapped call **`quantile(0.5, DATA)`** accidentally quantiles the scalar **`0.5`**
with default/leftover semantics and does **not** match **`quantile(DATA, 0.5)`**.

Pins: **`percentile_fifty_median_co`**, **`percentile_fraction_is_percent_units_not_quantile_bug_co`**,
**`quantile_half_matches_intuition_co`**, **`quantile_probability_first_arg_is_not_list_plus_p_bug_co`**,
**`percentile_zero_and_hundred_extrema_co`** in **`tests/suite/behavior_pin_2026_05_co.rs`**.

## BUG-162 — Planar **`chebyshev_distance` / `slope` / `midpoint`** are **four-scalar** APIs; vector distances differ — **`polish`**

**`chebyshev_distance`** is **`(x1, y1, x2, y2)`** on the Euclidean plane. Two bracket “point” operands
(**`chebyshev_distance([0, 0], [3, 4])`**) are not unpacked into coordinates — the call numifies the
container values and can return **`0`** instead of **`max(|Δx|, |Δy|)`**.

**`slope`** and **`midpoint`** use the same **four-numeric-actual** shape **(`x1`, `y1`, `x2`, `y2`)**.
Feeding two lists intended as paired samples does not compute a linear regression slope; it repartitions
scalars and can yield **`inf`** when the effective **Δx** clamps to zero.

Prefer **`distance` / `manhattan_distance` / `euclidean_distance`** (two vector operands) for
coordinate-array workflows; use the scalar planar builtins only when you truly mean a two-point planar
construction.

Pins: **`chebyshev_distance_four_scalars_cp`**, **`chebyshev_two_vectors_coerces_to_zero_bug_cp`**,
**`slope_four_coordinates_cp`**, **`slope_with_two_vector_args_vertical_line_inf_bug_cp`**,
**`midpoint_four_coordinates_cp`** in **`tests/suite/behavior_pin_2026_05_cp.rs`**.

## BUG-163 — **`running_reduce { $a + $b }`** does not see comparator scalars (zeros after first) — **`bug`**

**`builtin_running_reduce`** invokes the reducer via **`call_sub`** on successive prefix tails, but the
block’s **`$a` / `$b`** (or implicit sort-style bindings) are not populated for that code path the way
**`$_0` / `$_1`** slots are for **`preduce`**-family workers. With **`$a + $b`**, the second operand is
**`undef`**, so the running output collapses to **`0`** after the first element.

**`running_reduce { $_0 + $_1 }`** (or an explicit two-argument **`sub`**) matches the intended fold.

Pins: **`running_reduce_implicit_slot_add_cq`**, **`running_reduce_dollar_ab_zeros_after_first_bug_cq`** in
**`tests/suite/behavior_pin_2026_05_cq.rs`**.

## BUG-164 — **`uri_resolve` / `uri_normalize`** take **numeric byte vectors**, not **URI strings** — **`bug`**

Both helpers feed **`b81_to_bytes`**, which expands the first argument with **`arg_to_vec`** and then casts
each Perl value with **`to_number() as u8`**. Ordinary **`"http://…"`** strings therefore do not become
UTF-8 bytes — they stringify as a lump scalar that **`arg_to_vec`** does not split into octets — and
classification / “change counts” bear no relation to RFC 3986 on strings.

Pass an explicit byte array (e.g. **`[104, 116, 116, 112, …]`** for **`http…`**) if you need the
current implementation’s behaviour; do not assume **`uri_resolve(STR)`** performs reference resolution.

Pins: **`uri_resolve_byte_vector_absolute_uri_cq`**, **`uri_resolve_plain_string_misclassified_relative_bug_cq`**,
**`uri_normalize_counts_upper_bytes_cq`** in **`tests/suite/behavior_pin_2026_05_cq.rs`**.

## BUG-165 — **`string_take_while` / `string_drop_while`** filter a **leading prefix** against an **allowed-char set**, not a Perl predicate — **`polish`**

Both builtins (`math_wolfram11.rs`: **`builtin_string_take_while`**, **`builtin_string_drop_while`**) treat the
second operand as a string of characters to match from the start of the first string (greedy charset scan).
They are **not** list-style **`take_while { … }`** callback filters; passing a code ref or expecting regex-like
behaviour will not work.

Pins: **`string_take_while_charset_prefix_not_predicate_cr`**, **`string_drop_while_charset_prefix_not_predicate_cr`**
in **`tests/suite/behavior_pin_2026_05_cr.rs`**.

## BUG-166 — **`nth(N, ARRAYREF)`** often returns **`undef`** because **`to_list`** does not unpack **`ArrayRef`** — **`bug`**

**`builtin_nth`** falls back to **`v.to_list()`** for non-iterators. **`PerlValue::to_list`** expands
**`HeapObject::Array`** but **`HeapObject::ArrayRef`** hits the default arm and becomes a **one-element list**
containing the ref itself, so any positive index reads **`undef`**. **`nth(N, range(...))`** still works
because **`range`** yields an iterator.

Pins: **`nth_zero_indexed_from_range_iterator_cr`**, **`nth_inline_arrayref_undef_bug_cr`** in **`tests/suite/behavior_pin_2026_05_cr.rs`**.

## BUG-167 — **`gcd`** and **`lcm`** use **only `args[0]`** and **`args[1]`**; further operands are ignored — **`polish`**

**`builtin_gcd`** / **`builtin_lcm`** implement pairwise binary GCD/LCM on the first two arguments. Variadic tails
silently drop. **`gcd(12, 18, 35)`** returns **`6`** ( **`gcd(12, 18)`** ) instead of **`1`**; **`lcm(4, 6, 10)`**
returns **`12`** instead of **`60`**.

Pins: **`gcd_trailing_operands_ignored_two_arg_only_cr`**, **`lcm_trailing_operands_ignored_two_arg_only_cr`** in
**`tests/suite/behavior_pin_2026_05_cr.rs`**.

## BUG-168 — Bare **`hamming`** is the **DSP window**; **string Hamming distance** is **`hamming_distance`** — **`polish`**

**`window_hamming`** is exported under the bare name **`hamming`** (`builtins.rs` dispatch shares the alias with
**`window_hamming`**). The unrelated string metric lives only on **`hamming_distance`**, which routes to
**`builtin_hamming`** (characterwise mismatch count, equal lengths). Feeding two bitstrings into **`hamming(...)`**
does **not** compare them — it builds a window whose size comes from **`args[0].to_int()`** after string→number
coercion, producing window coefficients unrelated to the second “argument”.

Use **`hamming_distance($a, $b)`** for edit counts; use **`hamming($n)`** or **`window_hamming($n)`** for the taper
vector.

Pins: **`dsp_hamming_window_four_stringify_cs`**, **`string_hamming_distance_bitstrings_cs`** in
**`tests/suite/behavior_pin_2026_05_cs.rs`**.

## BUG-169 — **`hhi` / `herfindahl_hirschman`** ingests **`arg_to_vec(args[0])` only** — variadic tails are not market shares — **`polish`**

**`builtin_herfindahl_hirschman`** builds the share list exclusively from the **first** actual argument
(**`math_wolfram8.rs`**). A natural call **`hhi(0.3, 0.3, 0.4)`** therefore uses **only** **`0.3`** (one firm with
100 % share → **HHI = 0.09**), not three competing shares (**0.34** when passed as **`hhi([0.3, 0.3, 0.4])`**).

Pass a **single** arrayref / list bucket for the full share vector.

Pins: **`herfindahl_three_shares_array_ct`**, **`herfindahl_variadic_uses_first_share_only_bug_ct`** in
**`tests/suite/behavior_pin_2026_05_ct.rs`**.

## BUG-170 — **`moving_average`**, **`batch`**, **`chunk_n`**, **`group_of_n`** take **size/window first**, then data — reversed args misuse **`to_int(first)`** — **`polish`**

These builtins all read **`n = args[0].to_int().max(1)`** ( **`builtins_extended.rs`** moving-average path; **`builtins.rs`** **`builtin_batch`**
for **`chunk_n` / `group_of_n`** ) and treat **`flatten_args(args[1..])`** as the series. The first operand is therefore **not** “the list”.

- **`moving_average([1,2,3], 5)`**: for a leading **`ARRAYREF`**, **`to_int()`** is **`0`** → **`max(1)`** forces window **`1`**, so the
  implementation averages **only** the tail scalar **`5`** (output **`5`**) instead of signalling a **`(LIST, WINDOW)`** swap.
  Correct: **`moving_average(3, 1, 2, 3, …)`** or **`moving_average(3, \@xs)`**.
- **`chunk_n([1,2,3,4], 2)`**: first-arg array **`to_int()`** is **length `4`**; the tail is **`[2]`**, so **`batch`** emits a single
  chunk of **`[2]`** — stringify **`[2]`**, not **`([1,2], [3, 4])`**.

Pins: **`moving_average_window_first_three_cu`**, **`moving_average_arrayref_first_tail_only_bug_cu`**, **`chunk_n_size_first_cu`**,
**`chunk_n_list_first_yields_single_tail_chunk_bug_cu`** in **`tests/suite/behavior_pin_2026_05_cu.rs`**.

## BUG-171 — **`ml_binary_cross_entropy(Y, P)`** returns **`inf`** when **`P ≤ 0`** or **`P ≥ 1`** — **`polish`**

**`builtin_ml_binary_cross_entropy`** (**`math_wolfram45.rs`**) guards **`ln P`** / **`ln(1−P)`** by rejecting **`p <= 0`** or **`p >= 1`**
with **`inf`**, so “certain” probabilities (**`1`**, **`0`**) are not admissible even though the analytic limit is finite on one
branch. Use **`P`** in **`(0, 1)`** (e.g. **`1 - ε`**) near the boundary.

Pins: **`ml_binary_cross_entropy_interior_cv`**, **`ml_binary_cross_entropy_prob_one_is_inf_bug_cv`** in **`tests/suite/behavior_pin_2026_05_cv.rs`**.

## BUG-172 — **`jaccard_similarity(A, B)`** on numeric vectors uses **stringified element sets** — **`polish`**

**`builtin_jaccard_similarity`** (**`builtins.rs`**) builds **`HashSet<String>`** from **`flatten_args`** over each side. Any multiset / order /
multiplicity information is lost: e.g. **`[1, 0, 1]`** and **`[0, 1, 1]`** both become **`{"0", "1"}`**, so the coefficient is **`1`**
instead of the multiset Jaccard one would expect for binary masks. For multiset-aware similarity, use primitives that compare aligned
vectors (or build explicit count maps). **`jaccard_index`** follows the same string-set pattern on **`arg_to_vec`** elements.

Pins: **`jaccard_similarity_binary_masks_collapse_to_unit_bug_cw`**, **`jaccard_similarity_unique_elements_matches_index_cw`** (contrast)
in **`tests/suite/behavior_pin_2026_05_cw.rs`**.

## BUG-173 — **`mode([…])`** (single bracket list operand) does **not** return the element-wise mode — **`bug`**

**`builtin_mode`** (**`builtins.rs`**) uses **`flatten_args`**. **Observed:** **`mode([1, 2, 2, 3])`** **`stringify`** as **`[1, 2, 2, 3]`**
(the bracket list echoed), while **`mode(1, 2, 2, 3)`** correctly yields **`2`**. Prefer variadic arguments or **`mode_val([1, 2,
2, 3])`** when the population lives in one array.

Pins: **`mode_variadic_vs_single_arrayref_bug_cw`**, **`mode_val_arrayref_finds_modal_cw`** in **`tests/suite/behavior_pin_2026_05_cw.rs`**.

## BUG-174 — **`windowed` / `chunked`** treat a **bracket list** **`[LIST], N`** as a **single** list cell — **`polish`**

**`windowed_with_want`** / chunked sibling (**`list_builtins.rs`**) split **`args[..len−1]`** into raw **`PerlValue`** cells without
**`flatten_args`** / **`to_list()`**. A **tuple** **`(1, 2, 3, 4)`** (or comma-arg tails) supplies **four** scalar slots, but **`[1, 2,
3, 4]`** is **one** slot whose length is **`1`**, so **`N > len`** and the list result is empty (**`windowed`**) or a single outer chunk
(**`chunked`**). Prefer **`windowed((…), N)`** (or **`LIST |> windowed(N)`** per compiler message) when the list is one grouped value.

Pins: **`windowed_tuple_two_overlap_three_windows_cx`**, **`windowed_bracket_array_yields_empty_bug_cx`**, **`chunked_tuple_pairs_cx`**,
**`chunked_bracket_array_single_outer_chunk_bug_cx`** in **`tests/suite/behavior_pin_2026_05_cx.rs`**.

## BUG-175 — **`trimmed_mean`** first operand **`ARRAY` → `to_number()`** is **length**, not an error — **`bug`**

**`builtin_trimmed_mean`** (**`builtins.rs`**) reads **`pct = args.first().to_number()`**. For an **`ARRAY`**, **`to_number`** is the
element **count**. **`trimmed_mean([1, 2, 3, 4, 100], 20)`** therefore uses **`pct = 5`** (not **`20` %**) and **`collect_numbers([20])`**
so the “sample” is **[20]** only — output **`20`** instead of a trimmed mean of the five originals. Correct surface: **`trimmed_mean(20,
1, 2, 3, 4, 100)`** or **`trimmed_mean(20, [1, 2, 3, 4, 100])`** (percent **first**, **`0–100`** scale).

Pins: **`trimmed_mean_twenty_percent_trim_cx`**, **`trimmed_mean_list_first_yields_mean_of_tail_only_bug_cx`** in
**`tests/suite/behavior_pin_2026_05_cx.rs`**.

## BUG-176 — **`base_convert(N, FROM)`** (two-arg numeric) numifies to **`"…"`** then parses in **`FROM`** radix — **`polish`**

**`builtin_base_convert`** (**`builtins_extended.rs`**) takes **`args[0].to_string()`**, **`args[1]`** as **source** radix, **`args[2]`** as **target**
radix (default **10**). **`base_convert(255, 16)`** therefore parses **`"255"`** as a **base-16** literal (**`0x255 = 597`**), not “decimal **255**
converted to hex”. Safe pattern: **`base_convert("255", 10, 16)`** (string + explicit **from**/**to**).

Pins: **`base_convert_decimal_string_to_hex_cx`**, **`base_convert_two_arg_numeric_parses_string_in_from_radix_bug_cx`** in
**`tests/suite/behavior_pin_2026_05_cx.rs`**.

## BUG-177 — **`graph_density`** expects an **adjacency list**, not **`(|V|, |E|)`** scalars — **`polish`**

**`builtin_graph_density`** (**`math_wolfram13.rs`**) calls **`parse_adj_list`** on **`args.first()`** only. **`graph_density(4, 3)`** does **not**
compute **3 / C(4, 2)**; the second argument is ignored and the numeric **`4`** is not a valid graph shell, so the density collapses to **0**
(**`n < 2`** guard or empty parse). Pass **Adjacency lists** like **`[[1], [0, 2], [1]]`**.

Pins: **`graph_density_three_node_path_cy`**, **`graph_density_spurious_numeric_pair_yields_zero_bug_cy`** in
**`tests/suite/behavior_pin_2026_05_cy.rs`**.

## BUG-178 — **`transpose`** on a **2×2 AoA** is **not** **`matrix_transpose`** — **`polish`**

For **`[[1, 2], [3, 4]]`**, **`matrix_transpose`** flips rows/columns to **`[[1, 3], [2, 4]]`**, but **`transpose([[1, 2], [3, 4]])`**
**`stringify`** as **`([[1, 2]], [[3, 4]])`** (pairs of row buckets), not the numeric adjoint layout. For linear-algebra transpose of
numeric matrices, prefer **`matrix_transpose`** (cross-ref **BUG-159** nested **`transpose`** pins where applicable).

Pins: **`matrix_transpose_two_by_two_cy`**, **`transpose_list_of_row_refs_not_matrix_transpose_bug_cy`** in
**`tests/suite/behavior_pin_2026_05_cy.rs`**.

## BUG-179 — **`pmt`** argument order is **`RATE, NPER, PV`** — **`polish`**

**`builtin_pmt`** (**`builtins_extended.rs`**) reads **`rate = args[0]`**, **`nper = args[1]`**, **`pv = args[2]`**. **`pmt(10000,
0.05/12, 360)`** is wrong if the first slot was meant to be principal: **`rate = 10000`** yields absurd payments. Excel-compatible order is
**rate → periods → present value**.

Pins: **`pmt_monthly_loan_standard_order_cz`**, **`pmt_principal_first_slot_absurd_payment_bug_cz`** in
**`tests/suite/behavior_pin_2026_05_cz.rs`**.

## BUG-180 — **`format_percent(x)`** prints **`x`** + **`%`**, not **`100·x`** from a **(0, 1)** probability — **`polish`**

**`builtin_format_percent`** (**`builtins.rs`**) uses **`format!("{:.*}%", places, x)`**. A fraction like **`0.125`** becomes **`"0.1%"`** (default
one decimal), not **`"12.5%"`**. Pass **already-percent values** (e.g. **`12.5`**) or pre-scale.

Pins: **`format_percent_appends_raw_value_cz`**, **`format_percent_unit_fraction_not_scaled_bug_cz`** in
**`tests/suite/behavior_pin_2026_05_cz.rs`**.

## BUG-181 — **`anova_oneway([[...],[...]])`** nests **one** group — **`polish`**

**`builtin_anova_oneway`** flattens **comma-separated arguments** into independent sample groups. A **single** outer arrayref
**`[[1, 2, 3], [2, 3, 4]]`** is still **one** operand → **one** merged group, so the implementation reports **`anova: need at least 2 groups`**
instead of a shape/type error. The intended call is variadic **`anova_oneway([1, 2, 3], [2, 3, 4])`** (or equivalent comma
arguments).

Pins: **`anova_oneway_variadic_two_groups_da`**, **`anova_oneway_nested_aoa_error_message_da`** in
**`tests/suite/behavior_pin_2026_05_da.rs`**.

## BUG-182 — **`trapz(YS, …)`** / **`simpson(YS, …)`** second slot is **`dx`**, not **`XS`** — **`polish`**

**`builtin_trapz`** / **`builtin_simpson`** treat **`args[0]`** as the **Y** sample vector and **`args[1]`** as optional
**`dx`** (scalar spacing). Passing **`trapz([x0,x1,…], [y0,y1,…])`** (NumPy-style paired abscissa/ordinate arrays) does **not**
integrate against the X ordinate — the second array **numifies** to a scalar step (**0** when it does not look like a single
number), yielding a **0** area with no arity error.

Pins: **`trapz_simpson_evenly_spaced_y_with_dx_one_da`**, **`trapz_two_array_operands_second_becomes_dx_zero_da`** in
**`tests/suite/behavior_pin_2026_05_da.rs`**.

## BUG-183 — **`binary_search` / `lower_bound` / `upper_bound` / `equal_range`** take **needle first** — **`polish`**

These builtins read **`args[0]`** as the **target scalar** and treat **`args[1..]`** (flattened) as the sorted list. The call
**`binary_search([1, 3, 5], 5)`** uses the **array** as the numeric target (via **`to_number`**) and **`5`** alone as the list —
yielding **not found** / bogus bounds — instead of a type error. Correct: **`binary_search(5, [1, 3, 5, 7])`**, **`lower_bound(5,
…)`**, etc.

Pins: **`binary_search_lower_upper_correct_needle_first_db`**, **`binary_search_swapped_args_not_found_db`**, **`lower_bound_swapped_args_returns_zero_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-184 — **`dice_coefficient`** (and **`overlap_coefficient`**) on **strings** are **single-token sets** — **`polish`**

**`arg_to_vec("abc")`** is **one** cell (`"abc"`), not per-character grams. **`dice_coefficient("abc", "abd")`** compares **`{abc}`** vs **`{abd}`**
(intersection **0**), not character bigrams / multiset overlap. Pass explicit lists (e.g. **`split(//, $s)`** or codepoint lists) when
character-level Dice is intended.

Pins: **`dice_coefficient_strings_singleton_tokens_db`**, **`dice_coefficient_numeric_lists_expected_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-185 — **`winsorize(PCT, DATA…)`** — **percent first** — **`polish`**

**`builtin_winsorize`** (**`builtins_extended.rs`**) uses **`args[0]`** as **`pct`** and **`flatten_args(args[1..])`** as the samples. **`winsorize([1,…], 10)`**
interprets the **array** as **`pct`** (after **`to_number`**) and **`10`** alone as the dataset — a silent garbage path. Correct:
**`winsorize(10, 1, 2, …)`** or **`winsorize(10, [ … ])`**.

Pins: **`winsorize_percent_first_bracket_list_db`**, **`winsorize_array_first_yields_scalar_noise_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-186 — **`unzip`** with one nested **`[[a,b],[c,d]]`** mis-pairs columns — **`polish`**

**`builtin_unzip`** (**`builtins.rs`**) calls **`flatten_args`** on **`args`**, yielding **two** outer cells for **`[[1, 10], [2, 20]]`**, then walks that list pairwise as if it were a **flat** zipper of scalars — **`1`** with **`10`** land in the **A** column, **`[2, 20]`**’s string/int cells never participate as intended. Use **`unzip(1, 10, 2, 20)`** / **`unzip_pairs([[1, 10], [2, 20]])`** for pair rows.

Pins: **`zip_interleave_unzip_flat_dc`**, **`unzip_nested_aof_pairs_mispairs_bug_dc`** in **`tests/suite/behavior_pin_2026_05_dc.rs`**.

## BUG-187 — **`clamp_list`** **Rust-panics** when **`lo > hi`** — **`bug`**

**`builtin_clamp_list`** forwards to **`f64::clamp`**, which **`panic!`s** when **`min > max`**. Example: **`stryke -e 'clamp_list(5,0,1)'`** aborts the process instead of raising **`PerlError`**. Valid calls use **`lo ≤ hi`**.

No stable integration pin (subprocess abort); reproduction is the one-liner above.

## PARITY-001 — Magic string increment is not implemented — **FIXED**

`++` on a string operand numifies the operand to 0 then increments. Perl 5
performs magic string increment (`"b"++ → "c"`, `"Az"++ → "Ba"`, `"zz"++ → "aaa"`).

```sh
$ stryke -e 'my $x = "b"; $x++; print $x'
1                       # stryke
# perl prints: c

$ stryke -e 'my $x = "Az"; $x++; print $x'
1                       # stryke
# perl prints: Ba

$ stryke --compat -e 'my $x = "b"; $x++; print $x'
1                       # stryke (still numeric even with --compat)
# perl prints: c
```

Tests: `postfix_inc_on_alpha_string_yields_one`,
`postfix_inc_on_alphanumeric_string_yields_one`.

Severity: **parity**. `--compat` is the obvious place for the magic form to
work; today it does not. Fix would live near the postfix-inc lowering on
strings in `interpreter.rs` / `vm.rs`.


## PARITY-002 — `(my $copy = $orig) =~ s///` / `=~ tr///` does not bind to the copy — **FIXED**

The classic Perl copy-and-substitute idiom is a no-op in stryke: both `s///`
and `tr///` leave both vars equal to the original.

```sh
$ stryke -e 'my $s = "abc"; (my $t = $s) =~ s/a/X/; print "$s/$t"'
abc/abc                 # stryke
# perl prints: abc/Xbc

$ stryke -e 'my $s = "abc"; (my $t = $s) =~ tr/a-z/A-Z/; print "$s/$t"'
abc/abc                 # stryke
# perl prints: abc/ABC
```

Tests: `copy_on_bind_substitute_does_not_mutate`,
`copy_on_bind_tr_does_not_mutate`, plus the pair of `explicit_copy_then_*`
sanity checks proving the explicit form works.

Severity: **bug**. Affects readability of common idiomatic Perl. Likely
parsing/lvalue-shaping in `parser.rs` for the parenthesized-decl-as-lvalue
case feeding `=~`.


## PARITY-003 — `2 ** 64` falls back to float instead of bigint — **FIXED** (`use bigint;`)

```sh
$ stryke -e 'print 2 ** 64'
1.84467440737096e+19    # stryke
# perl prints (with use bigint;): 18446744073709551616
```

Tests: `pow_2_64_uses_float_notation`.

Severity: **parity**. The repo has a `bigint_compat.rs` test suite, so this
is a known limitation rather than oversight. Worth pinning so the float
formatter doesn't change shape silently.


## PARITY-004 — Division by zero surfaces as `ErrorKind::Runtime`, not `DivisionByZero` — **FIXED**

`ErrorKind::DivisionByZero` exists as a variant in `error.rs:17`, but the
1/0 path raises a `Runtime` error with message `"Illegal division by zero"`.

Tests: `division_by_zero_is_runtime_error_today`.

Severity: **bug** (low-impact). The variant exists; routing the `/` and `%`
ops to it would let users catch division specifically.


## BUG-001 — `clamp` direct-vs-piped heuristic misroutes single-value pipe

`clamp` uses a runtime heuristic to distinguish `clamp(MIN, MAX, LIST...)`
from a pipe-style call where the LHS is inserted at `args[0]`. The heuristic
checks "if `args[2..]` expands to multiple items, treat the first two as
min/max" (`builtins.rs:6738`). When exactly one value is passed, both call
shapes have identical arity, so the pipe form is decoded as the direct form
and clamps the *min* and *max* against the lone value:

```sh
$ stryke -e 'print clamp(0, 10, 15)'
10                      # direct, correct

$ stryke -e 'print 15 |> clamp(0, 10)'
15                      # piped, wrong — should be 10
```

Tests: not yet pinned (requires deciding which behavior is canonical
before locking it). Suggested fix: distinguish via call site (parser knows
whether it lowered a `|>`) rather than via runtime arity heuristic.

Severity: **bug**. Pipe-friendliness is a feature stryke ships, so a
broken pipe form for a documented builtin is high-visibility.


## POLISH-001 — Builtin-redefinition error tells user to use `fn` when they already did

When a `fn` declaration shadows a stryke builtin, the rejection message
reads:

> `id` is a stryke builtin and cannot be redefined (this is not Perl 5;
> use `fn` not `sub`, or pass --compat)

…but the user typed `fn` already. The message should branch on the
keyword observed:

- if `sub` — keep current text
- if `fn` — drop the "use `fn` not `sub`" half; only suggest `--compat`

Tests: `redefining_builtin_id_is_rejected`,
`redefining_builtin_squared_is_rejected` (these only assert that an error
is raised, not the wording, so they survive a wording fix).

Severity: **polish**.


## POLISH-002 — `++` on a non-lvalue reports `PostfixOp on non-scalar`

```sh
$ stryke -e '("b"++)'
VM compile error (unsupported): PostfixOp on non-scalar at -e line 0.
```

The operand is a scalar; the issue is that it is not assignable. A more
accurate message would be "Can't modify constant string in postfix ++"
(matches Perl 5 phrasing) or "postfix ++ requires an lvalue".

Severity: **polish**.


## PARITY-005 — Modulo follows sign-of-dividend, not sign-of-divisor — **FIXED**

Stryke uses Rust/C semantics for `%`:

```sh
$ stryke -e 'print -7 % 3, " / ", 7 % -3'
-1 / 1
$ perl   -e 'print -7 % 3, " / ", 7 % -3'
2 / -2
```

Tests: `mod_negative_dividend_positive_divisor_returns_negative`,
`mod_positive_dividend_negative_divisor_returns_positive`.

Severity: **parity**. Worth deciding whether this is intentional (faster /
matches Rust ergonomics) or a parity bug.


## PARITY-006 — `sprintf "%g"` falls back to `%f` formatting — **FIXED**

```sh
$ stryke -e 'printf "%g\n", 0.0001; printf "%g\n", 1234567'
0.000100
1234567.000000
$ perl   -e 'printf "%g\n", 0.0001; printf "%g\n", 1234567'
0.0001
1.23457e+06
```

Tests: `sprintf_g_format_uses_fixed_decimal_today`.

Severity: **parity**. `%g`'s job is shortest-of-`%e`-or-`%f`; stryke just
uses `%f`.


## PARITY-007 — `sprintf "%e"` omits the `+` sign and zero-pad on the exponent — **FIXED**

```sh
$ stryke -e 'printf "%e\n", 12345.6789'
1.234568e4
$ perl   -e 'printf "%e\n", 12345.6789'
1.234568e+04
```

Tests: `sprintf_e_format_omits_plus_and_zero_pad_today`.

Severity: **parity**.


## PARITY-008 — `sprintf "%v..."` not implemented — **FIXED**

`%v` is supposed to interpret the argument as a version string (one byte
per dot-separated component). Stryke emits the raw value followed by the
literal letter that was supposed to be the format suffix:

```sh
$ stryke -e 'print sprintf("%vd", "1.2.3")'
1.2.3d
$ perl   -e 'print sprintf("%vd", "1.2.3")'
49.46.50.46.51
```

Tests: `sprintf_v_format_emits_literal_today`.

Severity: **parity** (rarely used).


## PARITY-009 — `sprintf` positional `%N$s` not implemented — **FIXED**

```sh
$ stryke -e 'print sprintf("%2\$s %1\$s", "world", "hello")'
worlds hellos
$ perl   -e 'print sprintf("%2\$s %1\$s", "world", "hello")'
hello world
```

Tests: `sprintf_positional_arg_emits_literal_today`.

Severity: **parity** (i18n-relevant).


## PARITY-010 — `vec($s, $offset, $bits) = N` rejected as complex lvalue — **FIXED**

The bytecode compiler now rewrites `vec($s, $o, $b) = $rhs` into
`$s = vec_set_value($s, $o, $b, $rhs)`, where `vec_set_value` is a new
internal 4-arg builtin that returns the modified bit-buffer. The
existing supported-lvalue paths (`$s` plain scalar, `$arr[i]`, `$h{k}`,
etc.) handle the assignment from there.

While fixing the lvalue, the existing `vec` *read* impl was also
corrected: Perl uses **big-endian** byte order for multi-byte BITS (16 /
32) and zero-pads past the end of the string. Stryke previously read
little-endian and returned 0 on out-of-range reads.

Tests: `vec_lvalue_byte_assignment`, `vec_read_8_bit`,
`vec_lvalue_16_bit_big_endian`, `vec_lvalue_32_bit_round_trip`,
`vec_read_zero_pads_past_end`.

Known limitation: writes that produce non-UTF-8 bytes (e.g.,
`vec($s, 7, 1) = 1` → byte 0x80) round-trip through `PerlValue::bytes`,
but downstream `substr` / `ord` on those byte values still apply
UTF-8/Latin-1 decoding, which can corrupt single-byte indexing. This is
the same string-vs-bytes interaction that affects `pack` output.

Severity: **parity** (FIXED for the documented lvalue case).


## PARITY-011 — `CORE::*` namespace not available — **FIXED**

The parser now strips a leading `CORE::` prefix from any qualified
identifier just before the keyword-dispatch match, so `CORE::length`,
`CORE::print`, `CORE::abs`, `CORE::ord`, `CORE::chr`, `CORE::int`,
`CORE::uc`, `CORE::lc`, `CORE::scalar`, `CORE::sort`, `CORE::printf`,
etc. all parse identically to the bare keyword. Matches Perl 5's
documented `CORE::` namespace, which routes through to the built-in
implementation.

Tests: `core_prefix_routes_to_builtin_keyword` (8 builtins),
`core_prefix_works_inside_print_arg`.

Severity: **parity** (FIXED).


## PARITY-012 — `use overload "+" => sub { ... }` rejects anonymous-sub handlers — **FIXED**

`expr_to_overload_sub` now recognises `ExprKind::CodeRef { params, body }`
(an anonymous `sub { ... }` block) in the value position of a `use
overload` pair. The parser promotes the anon body to a synthetic top-
level `SubDecl` named `__overload_anon_N` (incrementing counter), and
the overload-pair value records that synthetic name. At install time
(`Interpreter::install_use_overload_pairs`), if the value starts with
`__overload_anon_`, the synthetic sub is re-bound under the current
package as `Pkg::__overload_anon_N` so the operator-dispatch lookup
(`Pkg::sub_short`) at runtime resolves.

Tests: `use_overload_accepts_anonymous_subroutine_for_op_plus`,
`use_overload_accepts_anonymous_subroutine_for_stringify`,
`use_overload_named_handler_still_works` (backward compat).

Severity: **parity** (FIXED).


## BUG-002 — Blessed arrayrefs stringify with `HASH` tag

```sh
$ stryke -e 'my $o = bless [1,2,3], "Bar"; print "$o\n"; print ref($o)'
Bar=HASH(0x...)
Bar
$ perl   -e 'my $o = bless [1,2,3], "Bar"; print "$o\n"; print ref($o)'
Bar=ARRAY(0x559abc...)
Bar
```

`ref()` correctly returns `Bar`; only the stringification is wrong (always
`HASH`). The `0x...` literal placeholder is intentional (stryke does not
expose live addresses).

Tests: `bless_arrayref_stringifies_with_hash_tag_today`.

Severity: **bug**.


## BUG-003 — `$self->SUPER::method` overflows the stack inside `class extends`

```sh
$ stryke -e '
class Animal { fn speak { "generic" } }
class Dog extends Animal { fn speak { "woof+" . $self->SUPER::speak } }
say Dog()->speak;'
thread 'main' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

The Perl-5-style `our @ISA = (...)` + `$self->SUPER::speak` form works
correctly (see `perl5_super_call_through_isa_works`). The bug is specific
to the native-class MRO path.

Tests: `class_extends_overrides_parent_method` (works without SUPER),
`perl5_super_call_through_isa_works` (the path that does work).

Severity: **bug**. Almost any non-trivial class hierarchy will need
`SUPER::`; without it, `extends` is half-broken.


## BUG-004 — Pipe `|>` with arrayref LHS does not auto-dereference

```sh
$ stryke -e 'my @a = (1..5); print @a |> sum'
15                                  # @-array LHS works
$ stryke -e 'print [1..5] |> sum'
0                                   # arrayref LHS broken
$ stryke -e 'my @r = [1..5] |> map { _ * 2 }; print scalar @r, ":", $r[0]'
1:0                                 # one iteration with _ = the arrayref
```

Either of two fixes is reasonable: auto-deref arrayref LHS into a list, or
reject arrayref LHS at parse time so the user is forced to write `@$ref |>`.

Tests: `pipe_with_arrayref_into_sum_returns_zero_today`,
`pipe_with_arrayref_through_map_returns_single_zero_today`,
`pipe_with_array_var_through_map_and_sum` (the form that works).

Severity: **bug**. Arrayrefs are the natural unit of data flow in stryke
(every pipe-friendly builtin returns one), so a broken pipe entry-point
for arrayrefs is high-visibility.


## BUG-005 — `(caller(N))[3]` (subroutine name) is undef

```sh
$ stryke -e '
sub g { my @c = caller(0); join("|", map { defined $_ ? $_ : "(undef)" } @c[0..3]) }
sub fnx { g() }
say fnx();'
main|-e|2|(undef)
```

Index 3 of the `caller()` list should be the fully-qualified subroutine
name (`main::g`). Stryke currently leaves it undef.

Tests: `caller_zero_omits_subroutine_name_today`.

Severity: **bug**. Breaks any logger / decorator that prints the calling
sub's name.


## BUG-006 — `chomp @array` does not behave as documented

```sh
$ stryke -e 'my @s = ("a\n", "b\n"); chomp @s; print join("|", @s)'
2
$ perl   -e 'my @s = ("a\n", "b\n"); chomp @s; print join("|", @s)'
a|b
```

The number `2` is the count of items in `@s` (or the chomp count, which
would be `2` regardless). Whether the array is mutated is unclear from this
output alone — needs a focused investigation. Pinning is deferred until the
behavior is understood.

Severity: **bug** (pending root-cause analysis).


## BUG-007 — `Util->greet(...)` of a `Self.greet($name)` static method passes class as first arg

```sh
$ stryke -e '
class Util { fn Self.greet($name) { "hi, $name" } }
say Util->greet("world");'
hi, Util
```

`Util->greet("world")` should either be rejected (this is a static method,
call it as `Util.greet("world")`) or strip the class name from the front
of `@_` before binding. Today the user gets a silent argument shift.

Tests: `arrow_invoke_of_static_method_passes_class_as_first_arg_today`.

Severity: **bug**.


## BUG-008 — `%h{KEYS}` kv-slice returns the full hash — **FIXED**

`%h{KEYS}` is Perl 5.20+'s key-value hash slice — returns a flat list
of (key, value, key, value, …) pairs for just the requested keys, NOT
the whole hash. New AST variant `ExprKind::HashKvSlice { hash, keys }`
parses `%h{...}` (lexer feeds `Token::HashVar(h)` followed by
`Token::LBrace`). The bytecode compiler emits `LoadConst(key) ;
LoadConst(key) ; GetHashElem(h)` per key (or `compile_expr ; Dup ;
GetHashElem` for non-literal keys), then `MakeArray(2 * total_pairs)`
to build the flat key-value list.

Tests: `kv_slice_returns_subset_with_key_value_pairs` (was
`kv_slice_returns_full_hash_today`),
`kv_slice_into_array_yields_alternating_key_value_pairs`.

Severity: **bug** (FIXED — Perl 5.20+ syntax, common destructuring).



## BUG-009 — `exists $h{x}{y}` errors when `$h{x}` is missing — **FIXED**

The deepest exists test now soft-fails to false at any missing or
non-container intermediate. `exists_arrow_hash_element` and
`exists_arrow_array_element` both return `Ok(false)` when the container
is undef or any non-ref scalar (instead of erroring). The bytecode
compiler routes multi-level deref chains (`exists $h{x}{y}{z}`,
`exists $a[5][0]`, etc.) through `Op::ExistsExpr` so the chain walk
runs through `eval_exists_operand` + the new
`eval_expr_exists_mode` helper, which propagates undef instead of
erroring on intermediate `ArrowDeref` evaluations.

Differential-tested against Perl 5.42 across 13 cases including
two-level, three-level, hash-then-array, array-then-array,
present-then-missing, scalar-as-intermediate.

Tests: `exists_on_missing_intermediate_returns_false` (was
`_errors_today`), `exists_on_present_chain_returns_true`,
`exists_on_three_level_missing_returns_false`,
`exists_through_array_chain_soft_fails`,
`exists_through_non_ref_intermediate_returns_false`.

Severity: **bug** (FIXED).


## POLISH-003 — `say BAREWORD()->method()` parses BAREWORD as a filehandle

```sh
$ stryke -e 'class C { fn m2($x) { $x * 2 } } say C()->m2(5)'
print on unopened filehandle C at -e line 1.
$ stryke -e 'class C { fn m2($x) { $x * 2 } } say(C()->m2(5))'
10
```

Workaround: parenthesize the argument to `say`. The error message at
least names the offending bareword, which helps; a smarter
"is-this-a-class?" check could give a friendlier hint.

Severity: **polish**.


## POLISH-004 — Class method named `m` is parsed as the regex-match operator

```sh
$ stryke -e 'class C { fn m($x, $y) { $x + $y } }'
Expected method name, got Regex("$x, $y", "", '(') at -e line 1.
```

The lexer sees `m(` after `fn` and commits to the regex-match form. A
post-`fn` lookahead would resolve this. Workaround: name the method
something other than `m` (or `s`, `tr`, `y`, `qr`, `q`, `qq`, `qw`).

Severity: **polish**.


## PARITY-013 — `length` ignores `use utf8` and always returns byte count — **FIXED**

`length` now consults the per-interpreter `utf8_pragma` flag set by
`use utf8;` / `no utf8;`. With the pragma on, scalar args count Unicode
codepoints (`s.chars().count()`); without it, they count UTF-8 bytes
(`s.len()`). Raw byte buffers (`as_bytes_arc`) always return byte count,
matching Perl's `bytes::length` semantics. The VM `BuiltinId::Length`
reads `self.interp.utf8_pragma`. The flag is per-interpreter (not a
process-global static), so concurrent test workers don't bleed pragma
state.

Tests: `length_returns_byte_count_for_unicode_string`,
`length_with_use_utf8_returns_char_count` (covers `héllo` → 5,
`日本語` → 3, `café` → 4 with the pragma; bytes without it).

Severity: **parity** (FIXED).


## PARITY-014 — `substr($s, $off, $len) = $rep` lvalue not supported — **FIXED**

```sh
$ stryke -e 'my $s = "Hello"; substr($s, 0, 1) = "J"; print $s'
VM compile error (unsupported): Assign to complex lvalue at -e line 0.
```

Workaround: 4-arg `substr($s, $off, $len, $rep)` is fully supported and
returns the replaced segment. The lvalue form is a Perl idiom that needs
VM lowering work.

Tests: `substr_lvalue_assignment_not_supported_today`,
`substr_four_arg_replaces_in_place_and_returns_old` (the workaround).

Severity: **parity**.


## BUG-010 — `return (LIST)` collapses to last comma operand — **FIXED**

The bytecode compiler now compiles the operand of `return` in **list
context** for any list-shaped expression (`ExprKind::List`,
`ExprKind::Range`, `ExprKind::ArrayVar`, `ExprKind::HashVar`,
`ExprKind::HashSlice` / `HashKvSlice` / `ArraySlice` / `SliceRange`),
matching Perl's list-operator semantics for `return`. The previous
"compile in scalar context to give last element" comment was the wrong
shape — Perl's rule is "return propagates the caller's wantarray
context", and the **caller** decides whether to coerce to scalar.

Caller-side scalar coercion happens at `Op::ReturnValue`: if
`self.interp.wantarray_kind` is `Scalar` and the returned value is a
list/array, take the last element. That makes `my $x = sub_returning
_list()` yield the last element (Perl wantarray semantics) — also fixes
BUG-011 in the same dispatch.

Parser: `parse_return` was extended to accept a comma-list operand —
Perl's `return` is a list operator, so `return 1, 2, 3` (no parens)
returns the full list (1, 2, 3). Stops at postfix-statement-modifier
keywords (`if`, `unless`, etc.) so `return 1, 2, 3 if 1` still parses
correctly.

Tests: `explicit_return_paren_list_returns_full_list` (was
`_collapses_to_last_today`), `explicit_return_with_bare_commas_returns_full_list`,
`return_array_var_passes_through_full_list`,
`sub_return_list_in_scalar_context_yields_last_element`.

Severity: **bug** (FIXED — affected every multi-value early-return
pattern).


## BUG-011 — `my $s = list_returning_sub()` concatenates instead of taking last — **FIXED**

Fixed alongside BUG-010. `Op::ReturnValue` now coerces the returned
value to its last element when the caller's wantarray context is
`Scalar`, matching Perl's wantarray semantics. `my $s = xs()` and
`scalar xs()` now agree.

Tests: `list_returning_sub_in_scalar_context_yields_last` (was
`_concatenates_today`), `return_list_in_scalar_context_yields_last_element`
(was `_stringifies`), `list_in_scalar_context_via_scalar_keyword_takes_last`.

Severity: **bug** (FIXED).


## BUG-012 — `each %hash` always returns an empty list

```sh
$ stryke -e 'my %h = (a=>1); my @kv = each %h; print scalar @kv'
0
$ perl   -e 'my %h = (a=>1); my @kv = each %h; print scalar @kv'
2
```

The companion `while (my ($k, $v) = each %h)` form is rejected at VM
lowering with "my/our/state/local in expression context with multiple or
non-scalar decls". `keys`/`values` work correctly, so iteration is
possible — just not in the `each` style.

Tests: `each_returns_empty_list_today`,
`while_my_pair_each_rejected_at_runtime_today`.

Severity: **bug**. Standard hash iterator; many libraries use it.


## BUG-013 — Backticks in list context return one big string instead of one-string-per-line

```sh
$ stryke -e 'my @lines = `printf "a\nb\nc\n"`; print scalar @lines'
1
$ perl   -e 'my @lines = `printf "a\nb\nc\n"`; print scalar @lines'
3
```

Tests: `backticks_list_context_returns_single_string_today`,
`backticks_scalar_context_returns_full_string` (the form that works).

Severity: **bug**. Most shell-glue idioms break.


## BUG-014 — `$ENV{X} = ...` not propagated to subprocesses

```sh
$ stryke -e '$ENV{STRYKE_X} = "hi"; system "env | grep STRYKE_X"'
                       # (no output)
$ perl   -e '$ENV{STRYKE_X} = "hi"; system "env | grep STRYKE_X"'
STRYKE_X=hi
```

The variable is visible from inside stryke (`$ENV{STRYKE_X}` reads
`"hi"`), but child processes do not see it. Inherited environment
variables (HOME, PATH, …) are passed through normally.

Tests: `env_set_visible_within_stryke`,
`env_set_not_propagated_to_subprocess_today`.

Severity: **bug**. Commonly used to pass config to wrapped shell calls.


## BUG-015 — Reference `==` always returns true (placeholder address)

Stryke deliberately stringifies refs as `KIND(0x...)` with a literal
placeholder rather than a live address (this is a documented design
choice). The numeric form of a ref is therefore always 0, and `==` between
any two refs is always true:

```sh
$ stryke -e 'my @a; my @b; print \@a == \@b ? "eq" : "ne"'
eq
$ stryke -e 'my @a; print 0 + \@a'
0
```

Tests: `ref_numeric_value_is_zero_today`.

Severity: **bug**. The fix is either to expose live addresses (loses the
deterministic-output property) or to compare refs by identity for `==`
without going through numification.


## BUG-016 — `m//g` in list context concatenates captures per match

```sh
$ stryke -e 'my @m = "a1 b2 c3" =~ /(\w)(\d)/g; print scalar @m, " / @m"'
3 / a1 b2 c3
$ perl   -e 'my @m = "a1 b2 c3" =~ /(\w)(\d)/g; print scalar @m, " / @m"'
6 / a 1 b 2 c 3
```

stryke returns 3 elements (one per match, with captures concatenated);
Perl returns 6 (each capture as its own element).

Tests: `regex_g_flag_returns_full_matches_today`.

Severity: **bug**. Idiomatic capture extraction breaks.


## BUG-017 — `sprintf "%+d"` ignores the `+` flag — **FIXED**

```sh
$ stryke -e 'print sprintf("%+5d", 3)'
   3
$ perl   -e 'print sprintf("%+5d", 3)'
   +3
```

Tests: `printf_plus_flag_ignored_today`.

Severity: **bug** (low impact). Affects readable signed output.


## PARITY-015 — `"Inf"` and `"NaN"` strings numify to 0 — **FIXED**

`parse_number` now recognises `Inf` / `Infinity` / `NaN` (case-insensitive,
optional leading `+` / `-`) at the start of `value::parse_number` before the
regular numeric tokenizer runs. `format_float` also short-circuits NaN /
±Infinity to print `"NaN"` / `"Inf"` / `"-Inf"` instead of the libc lowercase
default — matching Perl across `9 ** 9 ** 9`, `sqrt(-1)`, `log(0)`, `log(-1)`,
and `0 ** -1`.

Tests: `numeric_inf_string_becomes_infinity` (covering `"Inf"`, `"inf"`,
`"Infinity"`, `"-Inf"`, `"+Inf"`, `"NaN"`, `"nan"`),
`numeric_overflow_yields_inf`, `sqrt_negative_yields_nan`,
`log_zero_is_negative_infinity`, `log_negative_one_is_nan`,
`zero_to_negative_one_is_inf`.

Severity: **parity** (FIXED).


## BUG-018 — `local $/` does not enable slurp mode

```sh
$ stryke -e 'open my $fh, "<", "/etc/hosts"; local $/; my $x = <$fh>; print length($x)'
1                       # stryke
$ perl   -e 'open my $fh, "<", "/etc/hosts"; local $/; my $x = <$fh>; print length($x)'
357                     # whole file (whatever its size is)
```

Reading via `<$fh>` ignores `$/` undef and stops at the first newline (or
even after one byte for some inputs). The `-0777` slurp flag is also
broken on stdin.

Tests: `open_then_slurp_with_undef_separator_reads_only_first_line_today`.

Severity: **bug**. Slurping a file is one of Perl's most common idioms.


## BUG-019 — `for (@arr) { $_ ... }` does not alias array elements — **FIXED**

The bytecode compiler (`StmtKind::Foreach` in compiler.rs) now detects a
bare-`@arr` source list and emits an `Op::SetArrayElem` write-back step
at the end of each iteration. Mutations to `$_` (or a named loop var)
through the body propagate back to the source array. Approach: at the
merged `step_ip` target (where both normal-completion and `next` paths
converge), push the loop var, push the counter, then emit
`SetArrayElem(arr_name)` — using the cached counter and var slots so
nested foreach loops don't poison the slot resolution. Aliasing only
fires when the source is `ExprKind::ArrayVar(name)`; ranges, list
literals, and `keys`/`values` keep copy semantics, matching Perl 5.

Tests: `for_dollar_underscore_aliases_array_element` (was
`_does_not_alias_..._today`), `for_named_loop_var_aliases_array_element`,
`for_alias_respects_last_and_next`,
`for_alias_only_for_simple_array_source`, `for_index_assignment_works`.

Severity: **bug** (FIXED — affects every in-place mutation idiom).


## BUG-020 — `$\`` (pre-match) does not parse outside string interpolation

```sh
$ stryke -e '"hello world" =~ /world/; my $p = $`; print "[$p]"'
Expected variable name after $ at -e line 1.
```

Workaround: `use English; my $p = $PREMATCH;` — that does parse and
captures correctly.

Tests: `premuf_via_english_alias_works`.

Severity: **bug** (low impact; rare idiom).


## BUG-021 — Scalar-ref to arrayref unwrap fails

```sh
$ stryke -e 'my $x = [1,2,3]; my $r = \$x; print $$r->[0]'
Can't use arrow deref on non-array-ref at -e line 1.
$ perl   -e 'my $x = [1,2,3]; my $r = \$x; print $$r->[0]'
1
```

Same with `${$r}->[0]`. The double-deref to reach an arrayref through a
scalar-ref intermediary is rejected.

Tests: `scalar_ref_to_arrayref_unwrap_fails_today`.

Severity: **bug**.


## BUG-022 — `weaken` runs but `isweak` always returns 0

```sh
$ stryke -e 'my $a = [1]; my $b = $a; weaken($b); print isweak($b) ? "weak" : "strong"'
strong
$ perl -MScalar::Util=weaken,isweak -e '...'
weak
```

Tests: `weaken_does_not_make_isweak_true_today`.

Severity: **bug**. Weak-ref semantics are needed for cycle-breaking; if
`weaken` is a no-op then long-lived parent/child structures will leak.


## BUG-023 — Autovivification of nested hash/array fails

```sh
$ stryke -e 'my %h; $h{k}[0] = "first"; print "@{$h{k}}"'
Can't assign to arrow array deref on non-array-ref at -e line 1.
$ perl   -e 'my %h; $h{k}[0] = "first"; print "@{$h{k}}"'
first
```

Workaround: pre-allocate the inner ref:
`$h{k} = []; $h{k}[0] = "first";`.

Tests: `autoviv_hash_then_array_index_fails_today`.

Severity: **bug**. Autovivification is a major Perl ergonomic feature.


## BUG-024 — `given/when` fails inside subs and with arrayref patterns

Two related failures, both raise "unexpected control flow in tree-assisted
opcode":

```sh
# 1. arrayref smart-match
$ stryke -e 'use feature "switch"; my $x = 3;
             given ($x) { when ([1..5]) { print "low" } default { print "?" } }'
unexpected control flow in tree-assisted opcode

# 2. given/when wrapped in a sub
$ stryke -e 'use feature "switch";
             sub g { my $x = $_[0]; given ($x) { when ("hi") { return "M" } default { return "N" } } }
             print g("hi")'
unexpected control flow in tree-assisted opcode
```

Top-level `given/when` with scalar literals or `/regex/` works fine.

Tests: `given_when_arrayref_range_fails_today`,
`given_when_inside_sub_fails_today`.

Severity: **bug**. The sub-wrapped form is the way most code uses
given/when.


## BUG-025 — `$SIG{__WARN__}` handler is not invoked — **FIXED**

**FIXED** in commit (pending) — the bytecode VM `warn` op now routes
through `$SIG{__WARN__}` when a coderef is installed. Recursion guard:
the slot is temporarily cleared during dispatch so a handler that itself
calls `warn` falls back to stderr instead of looping.

Original report: variable is assignable and reads back as a CODE ref,
but `warn` did not route through it.

Tests: `sig_warn_assignment_succeeds` (assignment side, unchanged),
`sig_warn_handler_runs_on_warn`,
`sig_warn_handler_receives_message_with_newline`,
`sig_warn_handler_recursion_guard_prevents_loop`.

Severity: **bug**. Affects logging libraries and test frameworks that
intercept warnings.


## BUG-026 — `$s x= N` compound assignment is rejected

```sh
$ stryke -e 'my $s = "ab"; $s x= 3'
Unexpected token Assign at -e line 1.
```

Workaround: `$s = $s x N`.

Tests: `x_compound_assign_is_parse_error_today`,
`x_compound_workaround_works`.

Severity: **bug** (parse-time; small surface).


## BUG-027 — `$#arr = N` does not change array length — **FIXED**

```sh
$ stryke -e 'my @a = (1..5); $#a = 2; print scalar @a, " / @a"'
3 / 1 2 3
$ perl   -e 'my @a = (1..5); $#a = 2; print scalar @a, " / @a"'
3 / 1 2 3
```

Both truncation (`$#a = $smaller`) and extension (`$#a = $bigger`, fills
with undef) now work. Routed `#name` writes through
`VMHelper::set_special_var` which calls `scope.set_array(name, vec_resized)`.
Negative values (`$#a = -1`) empty the array.

Tests: `dollar_hash_array_truncates_when_assigned`,
`dollar_hash_array_extends_with_undef_when_assigned`,
`dollar_hash_array_negative_one_empties` in
`tests/suite/behavior_pin_2026_05_aq.rs`.

Severity: **parity** (FIXED).


## BUG-028 — `@hash{@array_var}` slice returns empty list

```sh
$ stryke -e 'my %h = (a=>1, b=>2, c=>3); my @v = @h{("a","c")};   print "@v"'
1 3
$ stryke -e 'my %h = (a=>1, b=>2, c=>3); my @ks = ("a","c");
             my @v = @h{@ks}; print "@v"'
                                    # (empty)
```

The literal-list form works; the array-var form does not. The arrayref
deref form (`@h{@$kref}`) is also broken.

Tests: `hash_slice_with_literal_keys_returns_correct_values`,
`hash_slice_with_array_var_keys_returns_empty_today`.

Severity: **bug**.


## BUG-029 — `$&` does not interpolate inside double-quoted strings — **FIXED**

```sh
$ stryke -e '"abXYZcd" =~ /XYZ/; print "[$&]"'
[XYZ]
$ perl   -e '"abXYZcd" =~ /XYZ/; print "[$&]"'
[XYZ]
```

The double-quoted `$&` interpolation now matches the bare-expression read.
`parse_interpolated_string` had explicit branches for `'` (postmatch) and
`` ` `` (prematch) but missed `&` — added it to the same `matches!` arm.

Tests: `match_dollar_amp_captures_whole_match`,
`match_dollar_amp_interpolates_correctly` (formerly
`..._does_not_interpolate_today`), plus
`dollar_amp_interpolates_after_match` and
`dollar_apostrophe_interpolates_postmatch` in
`tests/suite/behavior_pin_2026_05_aq.rs`.

Severity: **parity** (FIXED).


## PARITY-016 — `ref $@ eq "Class"` parses with the wrong precedence — **FIXED**

`parse_one_arg_or_default` (the helper used by every Perl named unary —
`ref`, `length`, `lc`/`uc`, `chr`/`ord`, `hex`/`oct`, `int`/`abs`/`sqrt`,
`sin`/`cos`/`exp`/`log`, etc.) now parses the bare argument at named-unary
precedence (`parse_named_unary_arg`, which stops at shift level) instead
of `parse_one_arg`'s wider assignment-expression precedence. `ref $@ eq
"E"` now parses as `(ref $@) eq "E"`, matching Perl. Same fix lifts
`length $s == 3 ? "Y" : "N"` and similar idioms.

`rev` (a stryke list-operator alias) was migrated off
`parse_one_arg_or_default` to its own list-op-precedence path with an
inline implicit-`$_` default, so `rev 1..3` keeps parsing as
`rev(1..3)`.

Tests: `ref_dollar_at_eq_string_precedence` (was
`_today`).

Severity: **parity** (FIXED).


## BUG-030 — `system()` return value is exit code, not Perl's status word

```sh
$ stryke -e 'my $r = system("false"); print "r=$r ?=$?"'
r=1 ?=256
$ perl   -e 'my $r = system("false"); print "r=$r ?=$?"'
r=256 ?=256
```

`$?` is set correctly (exit-code << 8). Only the *return value* of
`system()` is wrong — it returns the bare exit code instead of the same
value as `$?`.

Tests: `system_false_returns_exit_code_not_status_word_today`,
`system_true_returns_zero_in_both` (where the values happen to coincide).

Severity: **bug**.


## BUG-031 — `system(LIST)` form drops exit code from `$?`

```sh
$ stryke -e 'system("sh", "-c", "exit 7"); print "?=", $?'
?=0
$ stryke -e 'system("sh -c \"exit 7\""); print "?=", $?'
?=1792                  # single-string shell form: correct
$ perl   -e 'system("sh", "-c", "exit 7"); print "?=", $?'
?=1792
```

Tests: `system_list_form_loses_exit_code_today`,
`system_string_form_propagates_exit_code`.

Severity: **bug**.


## BUG-032 — `$&` not interpolated in `s///` replacement string

```sh
$ stryke -e 'my $s = "abc 123"; $s =~ s/(\d+)/$&/g; print $s'
abc $&
$ perl   -e 'my $s = "abc 123"; $s =~ s/(\d+)/$&/g; print $s'
abc 123
```

Numbered captures (`$1`, `$2`, …) DO interpolate in replacements; only
`$&` is broken. (Same root issue as BUG-029 for double-quoted strings.)

Tests: `dollar_amp_not_interpolated_in_replacement_today`,
`captures_dollar_one_dollar_two_work_in_replacement`.

Severity: **bug**.


## BUG-033 — Multiple heredocs on a single line not supported

```sh
$ stryke -e 'print <<A, <<B;
A1
A
B1
B
'
Undefined subroutine &B1 at -e line 5.
```

Stryke consumes the first heredoc body correctly but then parses the
second body as code instead of as the second heredoc's content.
Workaround: split into separate prints.

Tests: `multiple_heredocs_on_same_line_not_supported_today`.

Severity: **bug**.


## BUG-034 — `sprintf "%#x"` / `"%#o"` ignore the `#` flag — **FIXED**

```sh
$ stryke -e 'printf "%#x %#o\n", 255, 8'
ff 10
$ perl   -e 'printf "%#x %#o\n", 255, 8'
0xff 010
```

Tests: `sprintf_hash_flag_does_not_add_prefix_today`,
`sprintf_capital_x_uppercase_hex` (the form that works).

Severity: **bug** (low impact). `%X` and `%x` themselves work; only the
`#` prefix flag is missing.


## BUG-035 — `open "-|", "cmd", "arg"` list form drops the extra args

```sh
$ stryke -e 'open my $fh, "-|", "echo", "hi"; my $l = <$fh>; print "[", $l, "]"'
[
]                       # `echo` ran with no arg, only "\n" came back
$ stryke -e 'open my $fh, "-|", "echo hi"; my $l = <$fh>; print "[", $l, "]"'
[hi
]                       # single-string shell form works
$ perl   -e 'open my $fh, "-|", "echo", "hi"; my $l = <$fh>; print "[", $l, "]"'
[hi
]
```

Tests: `pipe_open_read_string_form_captures_subprocess_stdout`,
`pipe_open_read_list_form_drops_args_today`.

Severity: **bug**. The list form is the safe (no-shell-quoting) idiom and
should be preferred.


## BUG-036 — `$obj->can("method")` returns a coderef that doesn't actually invoke

```sh
$ stryke -e '
package Cat; sub new { bless {}, shift } sub meow { "meow!" }
package main;
my $c = Cat->new;
my $m = $c->can("meow");
print "ref=", ref($m), " direct=", $c->meow, " via=", $m->($c) // "U"'
ref=CODE direct=meow! via=U

$ perl ...
ref=CODE direct=meow! via=meow!
```

`can` correctly returns a CODE reference for an existing method, but
calling that ref with the receiver as the first arg returns undef instead
of running the method body. Direct invocation works.

Tests: `can_returns_coderef_but_invocation_returns_undef_today`,
`can_returns_truthy_for_existing_method`,
`can_returns_falsy_for_missing_method`.

Severity: **bug**. Common idiom: `$obj->can($method) and $obj->$method(...)`
relies on the returned ref actually calling through.


## BUG-037 — Closures pass `@_` as scalar count when invoking a captured coderef

```sh
$ stryke -e '
sub mydbl { my $x = shift; $x * 2 }
my $f = \&mydbl;
my $h = sub { $f->(@_) };
print $h->(5)'
2                       # stryke (= scalar(@_) * 2)
$ perl ...
10                      # perl (= 5 * 2)
```

Inside a closure body, calling a captured coderef with `@_` as argument
flattens `@_` to its element count instead of its contents. The same body
called directly by name (`mydbl(@_)` rather than `$f->(@_)`) works
correctly. Manifests in:

- `compose(f, g)`-style HOFs where the inner closure is `sub { $f->($g->(@_)) }`
- curry/partial application where the outer arg is captured and `@_` carries the rest
- any code that hands a coderef into a higher-order combinator

Tests: `closure_calling_coderef_with_at_underscore_flattens_to_count_today`,
`closure_calling_sigfn_via_coderef_with_array_arg_breaks_today`,
`direct_call_inside_closure_works` (the form that works),
`closure_calling_sigfn_via_coderef_with_indexed_arg_works` (workaround
using `$_[0]` per-index access).

Severity: **bug** (high impact). Most functional-style libraries are
unusable until this is fixed.


## BUG-038 — `pos($s)` returns undef outside the `while (//g)` form

```sh
$ stryke -e 'my $s = "abc"; $s =~ /a/g; print defined(pos($s)) ? "Y" : "N"'
N
$ perl   -e 'my $s = "abc"; $s =~ /a/g; print defined(pos($s)) ? "Y" : "N"'
Y
```

The `while ($s =~ /g)` loop form correctly reports `pos()` at each
iteration; pinning the working form ensures we don't lose it. Stand-alone
`/g` followed by `pos()` returns undef.

Tests: `pos_outside_while_loop_is_undef_today`,
`pos_advances_with_each_g_match`.

Severity: **bug** (low impact).


## BUG-039 — `<*.ext>` angle-bracket glob shorthand not parsed

```sh
$ stryke -e 'my @f = </etc/host*>; print scalar @f'
Unexpected token NumLt at -e line 1.
$ stryke -e 'my @f = glob "/etc/host*"; print scalar @f'
3
```

Workaround: use the `glob` function form, which works correctly.

Tests: `angle_bracket_glob_form_is_parse_error_today`,
`glob_function_form_lists_matches`.

Severity: **bug** (small surface).


## BUG-040 — `tie $var, $class, ...` does not invoke FETCH/STORE

```sh
$ stryke -e '
package T; sub TIESCALAR { my ($cls, $v) = @_; bless \$v, $cls }
sub FETCH { "fetched:" . ${$_[0]} }
sub STORE { ${$_[0]} = $_[1] . "!" }
package main;
my $x; tie $x, "T", "init"; print $x; $x = "new"; print "/", $x'
/new                    # stryke (FETCH never fires)
$ perl ...
fetched:init/fetched:new!
```

`tie` does not error, but neither FETCH nor STORE is called on subsequent
reads/writes; the variable behaves as untied.

Tests: `tie_scalar_fetch_store_not_invoked_today`.

Severity: **bug**. Tied vars are how DBM/file-backed scalars work in
Perl modules.


## BUG-041 — `\@` prototype does not auto-take ref of array argument

```sh
$ stryke -e 'sub f (\@) { sort @{$_[0]} }
            my @a = (3,1,2);
            my @r = f(@a);
            print "@r"'
Can't dereference non-reference as array at -e line 1.
$ perl ...
1 2 3
```

The Perl convention is that `\@` in a prototype causes `f(@a)` to be
silently rewritten as `f(\@a)` so the callee receives a single arrayref
in `$_[0]`. Stryke passes the flattened array elements instead.

Workaround: drop the prototype and have callers pass `\@a` explicitly.

Tests: `backslash_at_prototype_does_not_auto_take_ref_today`.

Severity: **bug**.


## BUG-042 — `delete @array[indices]` (slice form) is rejected

```sh
$ stryke -e 'my @a = (10..15); delete @a[1..3]; print "@a"'
delete requires hash or array element at -e line 1.
$ perl ...
10  14 15               # 11,12,13 → undef
```

Single-element `delete $a[2]` works correctly. The slice variant raises a
runtime error. Workaround: `splice @a, $start, $count`.

Tests: `delete_array_slice_is_rejected_today`,
`splice_workaround_for_array_slice_delete_works`.

Severity: **bug**.


## BUG-043 — `delete @hash{KEYS}` (slice form) is rejected

```sh
$ stryke -e 'my %h = (a=>1, b=>2, c=>3); delete @h{qw(a b)}; print join(",", sort keys %h)'
delete requires hash or array element at -e line 1.
$ perl ...
c
```

Same root cause as BUG-042: only single-element delete is implemented.
Workaround: loop over keys with single-element delete.

Tests: `delete_hash_slice_is_rejected_today`.

Severity: **bug**.


## BUG-044 — AOP `after` block sees `$?` as 0, not the original return value

```sh
$ stryke -e '
fn payload { 42 }
after "payload" { print "got $? "; }
payload();'
got 0
```

The `aop.rs` module's preamble explicitly documents `$?` as the original
return value:

> after  "<glob>" { ... }   # run after; sees $INTERCEPT_MS, $INTERCEPT_US, $? (retval)

Stryke populates the timing variables (`$INTERCEPT_MS`, `$INTERCEPT_US`)
correctly and exposes the sub name in `$INTERCEPT_NAME`, but `$?` is
always 0 inside the after block. Workaround: use `around` with `proceed()`
and inspect the return value directly.

Tests: `aop_after_dollar_question_is_zero_not_return_value_today`,
`aop_intercept_name_visible_in_after` (the parts that work).

Severity: **bug**. Documented behavior diverges from observed.


## PARITY-017 — Embedded code blocks `(?{ ... })` not supported in regex

```sh
$ stryke -e '"abc" =~ /a(?{ "side" })b/'
Invalid regex /a(?{ "side" })b/: PCRE2: error compiling pattern at offset 3: unrecognized character after (? or (?-
```

stryke uses PCRE2, which deliberately omits Perl's `(?{...})` (embedded
code) and `(??{...})` (deferred-eval pattern) extensions because they
require runtime escape into the host language. Recursive patterns
(`(?R)`), conditional patterns (`(?(1)yes|no)`) and atomic groups
(`(?>...)`) all work.

Tests: `embedded_code_in_regex_is_rejected_today`,
`regex_recursion_via_question_r_works`,
`regex_conditional_pattern_works`,
`regex_atomic_group_prevents_backtrack`.

Severity: **parity** (intentional engine choice).


## BUG-046 — `trait` cannot declare fields

```sh
$ stryke -e 'trait Counter { count: Int = 0; fn inc { 1 } }'
Expected `fn` in trait definition at -e line 1.
```

Stryke's `trait` blocks accept only `fn` declarations; fields must live
in the impl'ing class. Moose `role`s by contrast can declare attributes.

Tests: `trait_with_field_is_parse_error_today`.

Severity: **parity / design choice**. Worth deciding whether to keep
trait-as-method-only or extend to attributes.


## BUG-047 — `ARRAY` / `ArrayRef` / `HashRef` field/param types fail to match

```sh
$ stryke -e 'class S { items: ARRAY = [] } S()'
class S field `items`: expected ARRAY, got ARRAY at -e line 1.
$ stryke -e 'class S { items: ArrayRef = [] } S()'
class S field `items`: expected ArrayRef, got ARRAY at -e line 1.
$ stryke -e 'class S { items: Array = [] } S()'
                       # works
```

Stryke's supported type names are `Int`, `Str`, `Float`, `Bool`,
`Array`, `Hash`, `Ref`, `Any` (any unrecognized name becomes
`Struct(name)`, which always type-mismatches the runtime tag for
arrayrefs/hashrefs). Anyone coming from Moose-land will reach for
`ArrayRef`/`HashRef` first and get a confusing error.

Tests: `class_field_array_uppercase_keyword_does_not_match_array_default_today`,
`class_field_arrayref_keyword_does_not_match_array_default_today`,
`class_field_array_type_accepts_arrayref_default` (the form that works).

Severity: **bug** (high friction). Either accept the Moose names as
aliases or improve the error message to say "did you mean `Array`?".


## BUG-048 — `ref()` on stryke-native class instances returns the empty string

```sh
$ stryke -e 'class C { v: Int = 0 } my $c = C(v => 5);
            print "[", ref($c), "]/", $c->isa("C") ? "Y" : "N"'
[]/Y
$ stryke -e 'my $h = bless { v => 0 }, "H"; print ref($h)'
H
```

`isa()` works correctly; the bug is specific to `ref()`. Moose-style
`ref($obj) eq "ClassName"` checks across the codebase silently fail,
which can quietly route data through default branches.

Tests: `ref_of_stryke_class_instance_is_empty_today`,
`ref_of_blessed_hashref_returns_class_name`.

Severity: **bug**.


## BUG-049 — `sprintf` star-width / dynamic-precision (`%*d`, `%.*f`) not implemented — **FIXED**

```sh
$ stryke -e 'print sprintf("%*d", 5, 42)'
5d
$ perl   -e 'print sprintf("%*d", 5, 42)'
   42

$ stryke -e 'print sprintf("%.*f", 3, 3.14159)'
3f
$ perl   -e 'print sprintf("%.*f", 3, 3.14159)'
3.142
```

Stryke leaves the `*` literal in the format and consumes the next arg
for the (now broken) format spec. Workaround: build the format string
dynamically: `sprintf("%${w}d", $n)` works.

Tests: `sprintf_star_width_emits_literal_today`,
`sprintf_star_precision_emits_literal_today`.

Severity: **bug** (low impact; common in column-formatted output).


## PARITY-018 — `printf "%d"` with float overflow saturates instead of wrapping

```sh
$ stryke -e 'printf "%d", 1e20'
9223372036854775807                 # i64::MAX
$ perl   -e 'printf "%d", 1e20'
-1                                  # wraps modulo 2^64
```

Stryke uses Rust's `as i64` which saturates; Perl uses C's `long`-style
cast which wraps. Neither matches a useful "bigint" answer — the value
1e20 simply doesn't fit in 64 bits.

Tests: `printf_d_with_large_float_saturates_to_i64_max_today`.

Severity: **parity** (defined behavior; differs from Perl).


## BUG-050 — `$SIG{__DIE__}` handler is not invoked — **FIXED**

**FIXED** in commit (pending) — `die` now fires `$SIG{__DIE__}` before
propagating the error. The handler can re-`die` to substitute a
different exception (the swapped error reaches `$@` instead of the
original). Recursion guard: the slot is temporarily cleared during
dispatch so a handler's own `die` does not re-enter the handler.

```sh
# After fix:
$ stryke -e '$SIG{__DIE__} = sub { print "trapped: $_[0]" }; eval { die "boom\n" }; print "after err=[$@]"'
trapped: boom
after err=[boom
]
$ stryke -e '$SIG{__DIE__} = sub { die "swap:" . $_[0] }; eval { die "orig\n" }; print "[$@]"'
[swap:orig
]
```

Tests: `sig_handler_assignment_returns_code_ref`,
`sig_die_handler_runs_inside_eval`,
`sig_die_handler_can_swap_error_by_redieing`,
`sig_die_handler_recursion_guard_prevents_loop`.

Severity: **bug**.


## BUG-051 — PerlIO layers in `open` mode strings are rejected

```sh
$ stryke -e 'open my $fh, ">:utf8", "/tmp/x"'
Unknown open mode '>:utf8' at -e line 1.
$ stryke -e 'open my $fh, "<:raw", "/tmp/x"'
Unknown open mode '<:raw' at -e line 1.
```

Workaround: the bare `>` / `<` modes work; data is byte-stream by
default. Programs that need encoding can `Encode::decode("UTF-8", $bytes)`
once the data is read in. (Encode itself is not loaded today either —
see BUG-052.)

Tests: `open_with_utf8_layer_is_rejected_today`.

Severity: **bug**.


## BUG-052 — `prototype("BUILTIN")` returns empty for built-ins

```sh
$ stryke -e 'print "[", prototype("push"), "]"'
[]
$ stryke -e 'print "[", prototype("scalar"), "]"'
[]
$ perl   -e 'print "[", prototype("push"), "]/[", prototype("scalar"), "]"'
[+@]/[$]
```

User-defined subs still report their prototypes correctly:

```sh
$ stryke -e 'sub myf ($) { 1 } print prototype \&myf'
$
```

Tests: `prototype_of_push_is_empty_today`,
`prototype_of_scalar_is_empty_today`,
`prototype_of_user_sub_returns_proto_string`.

Severity: **bug**.


## BUG-053 — `exists &name` (sub existence check) is a parse error

```sh
$ stryke -e 'sub myf { 1 } exists &main::myf'
Unexpected token BitAnd at -e line 1.
$ perl   -e 'sub myf { 1 } print exists &main::myf ? "Y" : "N"'
Y
```

Workaround: `defined &name` works and is functionally equivalent for
declared subs.

Tests: `exists_ampersand_subname_is_parse_error_today`,
`defined_ampersand_subname_works`.

Severity: **bug**.


## BUG-054 — `looks_like_number` not a builtin

Stryke ships many `Scalar::Util` functions as built-ins (`reftype`,
`blessed`, `weaken`, `refaddr`) but `looks_like_number` is missing.

```sh
$ stryke -e 'print looks_like_number("3.14")'
Undefined subroutine &looks_like_number at -e line 1.
```

Workaround: regex-based check, e.g. `m/^-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?$/`.

Tests: `looks_like_number_is_not_a_builtin_today`,
`reftype_unblessed_arrayref_is_array`,
`blessed_returns_class_for_blessed_ref`.

Severity: **bug**.


## BUG-055 — `\U` / `\L` not honored in `s///` replacement

```sh
$ stryke -e 'my $s = "abc def"; $s =~ s/\b(\w)/\U$1/g; print $s'
\Uabc \Udef
$ perl   -e 'my $s = "abc def"; $s =~ s/\b(\w)/\U$1/g; print $s'
Abc Def
```

`\U`/`\L`/`\u`/`\l` work correctly inside double-quoted string
interpolation; only the substitution-replacement path is broken.
Workaround: the `/e` flag with a `uc()`/`lc()` call:

```sh
$ stryke -e 'my $s = "abc def"; $s =~ s/\b(\w)/uc($1)/ge; print $s'
Abc Def
```

Tests: `upper_case_escape_in_substitution_is_literal_today`,
`s_e_flag_with_uc_call_works`,
`upper_case_escape_uppercases_until_e` (the interpolation path that works).

Severity: **bug**.


## BUG-056 — `%-` (named multi-capture hash) keeps only the last match

```sh
$ stryke -e '"abc 123 def 456" =~ /(?<wd>\w+)/g; print join(",", @{$-{wd}})'
456
$ perl   -e '"abc 123 def 456" =~ /(?<wd>\w+)/g; print join(",", @{$-{wd}})'
abc,123,def,456
```

`%+` (single-match named hash) works correctly. `%-` is for accumulating
all `/g` matches; stryke overwrites instead of appending.

Tests: `percent_minus_multi_capture_returns_only_last_today`,
`percent_plus_named_capture_works`.

Severity: **bug**.


## BUG-057 — `sprintf "%a"` (hex-float) not implemented — **FIXED**

```sh
$ stryke -e 'printf "%a", 1.5'
1.5
$ perl   -e 'printf "%a", 1.5'
0x1.8p+0
```

Stryke ignores the `%a` specifier and prints the value with default
formatting. Hex-float output is rare in scripting but used by some
numerical-debugging tooling.

Tests: `sprintf_a_hex_float_emits_decimal_today`.

Severity: **bug** (low impact).


## BUG-058 — `chunk(N, LIST)` returns one arrayref instead of N-sized groups

```sh
$ stryke -e 'my @r = chunk(2, 1..6); print scalar @r'
1
$ stryke -e 'my @r = chunk_n(2, 1..6); print scalar @r'
3
```

The `chunk` builtin behaves as a no-op grouping (single arrayref). The
`chunk_n` builtin does what users probably mean. Either rename `chunk` →
`chunk_n` and add an alias, or fix `chunk` to mean N-sized groups.

Tests: `chunk_alone_returns_one_arrayref_today`,
`chunk_n_groups_into_runs_of_n`,
`chunk_while_groups_consecutive_runs`.

Severity: **bug** (high friction; the conventional name is broken).


## BUG-059 — `partition(sub { ... }, LIST)` returns empty arrays

```sh
$ stryke -e 'my @r = partition(sub { $_ > 3 }, 1..6);
            print "0=[", join(",",@{$r[0]}), "] 1=[", join(",",@{$r[1]}), "]"'
0=[] 1=[]
$ stryke -e 'my @r = partition { _ > 3 } 1..6;
            print "0=[", join(",",@{$r[0]}), "] 1=[", join(",",@{$r[1]}), "]"'
0=[4,5,6] 1=[1,2,3]
```

Stryke's block form (no `sub` keyword) works correctly. The Perl-style
`sub { ... }` form parses but silently returns empty.

Tests: `partition_block_form_splits_into_yes_and_no`,
`partition_sub_form_returns_empty_arrays_today`.

Severity: **bug**.


## BUG-060 — Range flip-flop in scalar context evaluates as a list-range

```sh
$ stryke -e 'for my $i (1..6) { print "$i;" if $i == 2 .. $i == 4 }'
1;3;4;5;6;
$ perl   -e 'for my $i (1..6) { print "$i;" if $i == 2 .. $i == 4 }'
2;3;4;
```

The flip-flop operator (Perl `..` in scalar context) is meant to track a
state machine: false until the left side becomes true (state on, emit a
firing token), true until the right side becomes true (state off). Stryke
evaluates `0 .. 0` as the list-range `(0)` — a non-empty list, therefore
truthy — and `1 .. 0` as the empty descending list.

Workaround: build the state machine manually with a closure-captured flag.

Tests: `range_flip_flop_in_conditional_evaluates_as_list_today`.

Severity: **bug**.


## BUG-061 — `pairs()` returns Pair objects that don't array-deref

```sh
$ stryke -e 'my @r = pairs(a => 1, b => 2); print ref $r[0]'
Pair
$ stryke -e 'my @r = pairs(a => 1, b => 2); my @kv = @{$r[0]}'
Can't dereference non-reference as array at -e line 1.
```

In Perl's `List::Util`, `pairs(...)` returns blessed two-element arrayrefs
that respond to both `->key`/`->value` and `@{$_}` patterns. Stryke's
Pair type only supports the method form.

Tests: `pairs_returns_pair_ref_kind_today`,
`pair_object_does_not_array_deref_today`.

Severity: **bug** (compat).


## BUG-062 — `group_by(sub { ... }, LIST)` parse error

```sh
$ stryke -e 'my %g = group_by(sub { $_ % 2 }, 1..6)'
Expected Comma, got Semicolon at -e line 1.
```

Same root cause as BUG-059 (partition): the `sub { ... }` calling
convention isn't accepted. Block form (`group_by { _ % 2 } 1..6`) parses
but produces a hash with a stringified-arrayref key. No working form
discovered yet.

Tests: `group_by_with_sub_keyword_is_parse_error_today`.

Severity: **bug**.


## BUG-063 — `take(N, LIST)` / `step(N, LIST)` argument order returns empty

```sh
$ stryke -e 'my @r = take(3, 1..10); print "@r"'

$ stryke -e 'my @r = take(qw(a b c d), 2); print "@r"'
a b
```

Stryke's signature is `take(LIST, COUNT)` — list first. The Perl-ish
`take(N, LIST)` ordering returns nothing. `step` has the same shape.

Tests: `take_list_then_count_keeps_first_n`,
`take_n_first_signature_returns_empty_today`,
`take_bareword_with_n_first_returns_empty_today`,
`step_with_n_first_signature_returns_empty_today`.

Severity: **bug** (calling-convention surprise; existing tests show the
list-first form is the contract).


## BUG-064 — `PI` / `TAU` / `E` (uppercase) constants are barewords

```sh
$ stryke -e 'print pi'
3.14159265358979
$ stryke -e 'print PI'
PI
$ stryke -e 'print E'
E
$ stryke -e 'print e'
Unexpected token Eof at -e line 1.
```

`pi` and `tau` work as built-in constants; uppercase aliases are missing
and `e` is a parser-level token-fragment (since `eq`/`each` start with
`e`). Workaround: `exp(1)` for Euler's number.

Tests: `pi_constant_known_value`, `tau_constant_is_two_pi`,
`pi_uppercase_is_not_a_constant_today`, `e_alone_is_parse_error_today`,
`exp_one_yields_e`.

Severity: **bug** (low impact; aliases would resolve it).


## BUG-065 — `head(N, LIST)` returns just `N` instead of first N elements

```sh
$ stryke -e 'my @r = head(qw(a b c d e), 3); print "@r"'
a b c
$ stryke -e 'my @r = head(3, qw(a b c d e)); print "@r"'
3
```

The `(LIST, N)` order is the working contract — same as `take`, `drop`,
`tail`. The `(N, LIST)` form silently returns `(N)`.

Tests: `head_list_then_n_returns_first_n`,
`head_n_first_returns_just_n_today`,
`tail_list_then_count_returns_last_n`.

Severity: **bug** (calling-convention surprise).


## BUG-066 — `pairwise { $a + $b } @a, @b` returns empty list

```sh
$ stryke -e 'my @a = (1,2,3); my @b = (10,20,30);
            my @r = pairwise { $a + $b } @a, @b;
            print scalar @r'
0
$ perl -MList::Util=pairwise -e '...'
3                       # (11, 22, 33)
```

Stryke's `pairwise` builtin parses but produces nothing. Workaround:
manual `map` over indices.

Tests: `pairwise_block_form_returns_empty_today`.

Severity: **bug**.


## BUG-067 — `find_index` not built-in

`find { ... } LIST` works (returns the matching value), but Perl's
`find_index` (returns the index of the first match) is missing.

Tests: `find_index_is_not_a_builtin_today`.

Severity: **bug** (parity gap with `List::Util`).


## BUG-068 — AOP advice cannot mutate `@INTERCEPT_ARGS` or call `proceed(NEW_ARGS)`

```sh
$ stryke -e '
fn greet($name) { "hi $name" }
around "greet" {
  $INTERCEPT_ARGS[0] = uc($INTERCEPT_ARGS[0]);   # ignored
  proceed(uc($INTERCEPT_ARGS[0]));               # also ignored
}
print greet("world")'
hi world
```

Both the in-place mutation of `@INTERCEPT_ARGS` and the explicit-args
form `proceed(LIST)` get dropped — the original args reach the wrapped
sub. This makes around-advice unable to rewrite arguments.

Tests: `intercept_args_array_visible_in_before` (read-only access works),
`intercept_args_mutation_does_not_propagate_today`,
`proceed_with_explicit_args_does_not_override_today`.

Severity: **bug**. Argument-rewriting is a common AOP use case.


## BUG-069 — Multiple `around` advice for the same target does not compose

```sh
$ stryke -e '
fn val { 1 }
around "val" { proceed() + 10 }
around "val" { proceed() * 100 }
print val()'
11                       # only first registration applied
```

Perl-style aspect ordering would either compose both (e.g. 110) or stack
in declaration order. Stryke uses only the first registered around block.

Tests: `multiple_around_advice_does_not_compose_today`,
`multiple_before_and_after_fire_in_order` (the form that does work).

Severity: **bug**.


## BUG-070 — Explicit `return` inside `around` body is rejected by lowering

```sh
$ stryke -e '
fn val { 1 }
around "val" { my $r = proceed(); return $r + 10 }
val()'
AOP around advice body for `val` could not be lowered to bytecode
(likely contains a construct unsupported by block lowering, e.g. a literal `return`);
rewrite the body without it at -e line 3.
```

Implicit final-expression form (`{ proceed() + 10 }`) works. The error
message is helpful and tells the user to rewrite — pinned both forms so
the workaround stays valid if/when the underlying limitation is lifted.

Tests: `explicit_return_in_around_block_is_rejected_today`,
`implicit_final_value_in_around_is_used_as_return`.

Severity: **bug**.


## BUG-071 — `before`-advice `die` does not propagate to the caller's `eval`

```sh
$ stryke -e '
fn payload { print "G " }
before "payload" { print "B "; die "blocked\n" }
eval { payload() };
print "[$@]"'
B G G []        # before ran, original ran twice (?), $@ is empty
```

The `before` block's `die` neither aborts the call nor reaches `$@`
through the surrounding `eval`. Workarounds: handle the early-abort case
inside `before` itself, or move the gate into `around { ... }` and skip
`proceed()`.

Tests: `before_advice_die_does_not_propagate_today`.

Severity: **bug**.


## BUG-072 — `--lint` accepts strict-violating sources that runtime catches

```sh
$ stryke --lint -e 'use strict; $undeclared_xx = 5'
-e compile OK
$ stryke -e 'use strict; $undeclared_xx = 5'
Global symbol "$undeclared_xx" requires explicit package name (did you
forget to declare "my $undeclared_xx"?) at -e line 1.
```

Perl's `perl -c` catches this at compile time. Stryke's `--lint` only
runs through bytecode lowering and doesn't apply the strict-pragma
checker. Workaround: run the script for real (or wrap in `eval` and
inspect `$@`).

Tests: `parse_ok_for_strict_violator_but_runtime_fails`.

Severity: **bug** (the whole purpose of `--lint` is compile-time
verification).


## BUG-003 (expanded) — Three-level Perl-5 ISA + `SUPER::` chain also stack-overflows

The original BUG-003 was filed against stryke-native `class extends` +
`SUPER::`. This iteration confirmed the issue is broader: a three-class
Perl-5-style hierarchy (`our @ISA = ("Parent")`) where each level calls
`$self->SUPER::name` on the way up also overflows the stack:

```sh
$ stryke -e '
package A; sub new { bless {}, shift } sub name { "A" }
package B; our @ISA = ("A"); sub name { my $self = shift; $self->SUPER::name . "B" }
package C; our @ISA = ("B"); sub name { my $self = shift; $self->SUPER::name . "C" }
package main;
print C->new->name'
thread 'main' has overflowed its stack
```

Two-level chains (`A` → `B`) work; three or more crash. Method-resolution
state seems to lose its position cursor on the second hop.

Tests: `perl5_super_one_level_chain_works`,
`perl5_three_level_super_chain_at_least_parses`.

Severity: **bug**. Limits practical class hierarchies.


## BUG-073 — `BUILDARGS` method on a class is never invoked

```sh
$ stryke -e '
class Cat {
  name: Str = "?"
  fn BUILDARGS { print "BUILDARGS "; @_ }
  fn BUILD     { print "BUILD " }
}
Cat(name => "Felix")'
BUILD                       # BUILDARGS missing
```

`BUILD` is invoked correctly. `BUILDARGS` (the Moose-style hook for
preprocessing constructor arguments) is silently skipped. Workaround:
override `Self.new` to do the preprocessing directly.

Tests: `class_buildargs_method_not_invoked_today`,
`class_build_method_runs_at_construction`.

Severity: **bug** (compat with Moose-shaped class libraries).


## BUG-074 — `struct` lacks a `Pkg::new(...)` constructor

```sh
$ stryke -e 'struct Pt { x => Int, y => Int } Pt::new(3, 4)'
Undefined subroutine &Pt::new at -e line 1.
$ stryke -e 'struct Pt { x => Int, y => Int } Pt(3, 4)'
                       # works
```

Use the bareword constructor (`Pt(...)`). The Perl-classic `Pkg::new(...)`
form is only generated for `class` definitions, not `struct`s.

Tests: `struct_does_not_have_pkg_new_today`,
`struct_positional_construction_assigns_fields`.

Severity: **bug** (small surface).


## BUG-075 — `refaddr(\@a)` returns a fresh address per `\@a` evaluation

```sh
$ stryke -e 'my @a; print refaddr(\@a) == refaddr(\@a) ? "eq" : "ne"'
ne
$ perl -MScalar::Util=refaddr -e 'my @a; print refaddr(\@a) == refaddr(\@a) ? "eq" : "ne"'
eq
```

Each `\@a` evaluation creates a new ref-cell; stryke's `refaddr` returns
the cell's address rather than the underlying array's address. Aliased
copies (`my $s = $r`) do share the same refaddr, so propagating a captured
ref still works.

Tests: `refaddr_of_repeated_backslash_at_returns_different_addresses_today`,
`refaddr_of_aliased_scalar_is_same`.

Severity: **bug**. Common idiom for ref-identity tests
(`refaddr(\@a) == refaddr(\@b)`) gives wrong answers.


## BUG-076 — `\N` (numeric backref) in `s///` replacement is interpreted as escape

```sh
$ stryke -e 'my $s = "ab123cd"; $s =~ s/(\d+)/[\1]/; print $s'
ab[<SOH>]cd                 # `\1` → 0x01 control char
$ stryke -e 'my $s = "ab123cd"; $s =~ s/(\d+)/[$1]/; print $s'
ab[123]cd                   # `$1` works
```

Use `$1`/`$2`/… in replacements; the `\N` form is treated as a control
character escape (`\1` → SOH, etc.).

Tests: `backslash_one_in_substitution_inserts_soh_today`,
`dollar_one_in_substitution_inserts_capture`.

Severity: **bug** (compat).


## BUG-077 — Postfix `for` modifier rejected on `my @r = ...` form

```sh
$ stryke -e 'sub f { @_ } my @r = f($_) for (1, 2, 3)'
postfix `for` is not supported on this statement form at -e line 1.
```

Other postfix-`for` forms work (`$x .= "y" for 1..3` is fine). The
`my @r = EXPR for LIST` shape is parser-rejected.

Tests: `postfix_for_on_my_at_assign_is_rejected_today`,
`postfix_for_on_simple_expression_works`.

Severity: **bug**.


## BUG-078 — BEGIN blocks run but their writes to package vars are lost

```sh
$ stryke -e '
our $log = "";
BEGIN { $main::log .= "B:" }
$log .= "M:";
print "[$log]"'
[M:]                        # B: lost
```

When BEGIN's `print`/`die` side effect is observed via stdout/stderr, it
runs as expected. But mutating `our`-declared globals from inside BEGIN
does not persist into the main body. Probably the BEGIN block's
compilation phase resets after main-body parsing assigns the initial
value.

Tests: `begin_block_mutations_to_package_vars_lost_today`,
`begin_runs_before_main_code_in_declaration_order` (the parse-only check).

Severity: **bug**.


## BUG-079 — `sprintf "%n"` is a no-op — **FIXED**

```sh
$ stryke -e 'my $n; sprintf("hello%n", $n); print defined($n) ? "set:$n" : "U"'
U
```

Perl populates the referenced scalar with the count of bytes emitted so
far. Stryke leaves the variable undef. `%n` is a known security hole in
C-style printf and many languages omit it on purpose — pin the omission
so the test catches accidental partial implementations.

Tests: `sprintf_n_does_not_populate_count_today`.

Severity: **bug** / parity (low impact).


## BUG-080 — `sprintf "%p"` and `"%A"` not implemented — **FIXED**

```sh
$ stryke -e 'printf "%p", "hello"'
hello                           # %p ignored
$ stryke -e 'printf "%A", 1.5'
1.5                             # %A ignored
```

Both specifiers fall through to the value's stringification. `%p`
(pointer) is rarely used; `%A` is the uppercase form of `%a` (hex float)
which BUG-057 already covers.

Tests: `sprintf_p_prints_value_as_string_today`,
`sprintf_capital_a_does_not_emit_hex_float_today`.

Severity: **bug** (low impact).


## BUG-081 — `use integer` pragma is not honored

```sh
$ stryke -e 'use integer; print 7 / 3'
2.33333333333333                # CLI: float division
$ stryke ...via lib eval...
Can't locate integer.pm in @INC
```

The CLI silently ignores `use integer`; the library `eval` API tries to
load `integer.pm` from @INC and fails. Either path should switch `/` to
integer truncation when `use integer` is in scope.

Tests: `use_integer_pragma_lib_path_tries_to_load_integer_pm_today`,
`use_integer_pragma_at_least_parses`.

Severity: **bug**.


## BUG-082 — `0o` octal prefix not recognized — **FIXED**

Lexer now recognises the Perl 5.34+ `0o` / `0O` prefix alongside `0x`
(hex), `0b` (binary), and bare-`0` (legacy octal). After the prefix it
reads the same digit pool as bare-`0` octals (decimal digits 0-7 plus
`_` separators), and converts via `i64::from_str_radix(.., 8)`.
Underscore separators (`0o7_7_7`) work, matching Perl.

Tests: `octal_o_prefix_returns_511` (was `_returns_zero_today`),
`classic_zero_prefix_octal_works`,
`octal_literal_pattern_matches_perl`.

Severity: **bug** (parity, FIXED).


## BUG-083 — Regex `/n` flag (no auto-capture) not supported

```sh
$ stryke -e '"abc" =~ /(\w+)/n'                  # CLI
Undefined subroutine &n at -e line 1.
```

Perl 5.22+ added `/n` to disable auto-numbered captures. Stryke parses
the trailing `n` as a bareword sub. CLI raises an undefined-sub error;
the library `eval` API silently returns the string `"n"`.

Tests: `regex_n_flag_silently_returns_n_in_lib_eval_today`.

Severity: **bug**. Workaround: turn captures into `(?:...)` non-capturing
groups manually.


## BUG-084 — Possessive quantifiers (`a++`, `\d++`) act like greedy `+`

```sh
$ stryke -e 'print "aaab" =~ /a++ab/ ? "Y" : "N"'
Y                               # should be N (no backtrack from a++)
$ perl   -e 'print "aaab" =~ /a++ab/ ? "Y" : "N"'
N
```

Stryke's regex engine treats `a++` identically to `a+` — backtracking
proceeds normally. Atomic groups (`(?>a+)`) work correctly (BUG-024
companion); only possessive-quantifier suffixes are missing.

Tests: `possessive_quantifier_does_not_prevent_backtrack_today`,
`greedy_a_plus_with_backtrack_matches`.

Severity: **bug** (regex parity).


## BUG-085 — `printf $fh "fmt", args` writes to STDOUT, ignoring the filehandle

```sh
$ stryke -e '
open my $fh, ">", "/tmp/o" or die;
print  $fh "plain\n";
printf $fh "n=%d\n", 42;
close $fh'
n=42                            # printf went to terminal, not file
$ cat /tmp/o
plain                           # only the print made it to disk
```

`print $fh ...` honors the filehandle correctly; `printf $fh ...` does
not. Workaround: use `print $fh sprintf("fmt", args)` until printf is
fixed.

Tests: `printf_to_filehandle_writes_to_stdout_today`.

Severity: **bug** (surprising; affects CSV/log writers).


## BUG-086 — `use constant { ... }` hashref form rejected; list form collapses

```sh
$ stryke -e 'use constant ARR => (1, 2, 3); my @a = ARR; print "@a"'
3                              # only last comma operand kept
$ perl   -e 'use constant ARR => (1, 2, 3); my @a = ARR; print "@a"'
1 2 3

$ stryke -e 'use constant { ZERO => 0, ONE => 1 }; print ZERO'
use constant: expected list of NAME => VALUE pairs at -e line 1.
```

Single-value `use constant NAME => VALUE` works. The hashref-block form
and the multi-value `(LIST)` form both fail. Workaround: declare each
constant separately, or wrap a list constant in an arrayref:
`use constant DAYS => [qw(mon tue wed)]`.

Tests: `use_constant_simple_scalar`, `use_constant_arithmetic`,
`use_constant_arrayref_holds_list`,
`use_constant_paren_list_collapses_to_last_today`,
`use_constant_hashref_form_is_rejected_today`,
`use_constant_qw_becomes_arrayref_string`.

Severity: **bug** (parity with the canonical Perl idioms).


## BUG-087 — `use warnings` does not emit warnings

```sh
$ stryke -e 'use warnings; my $x; my $y = $x + 1; print $y'
1                              # no warning
$ perl   -e 'use warnings; my $x; my $y = $x + 1; print $y'
Use of uninitialized value $x in addition (+) at -e line 1.
1
```

Stryke parses `use warnings` and `no warnings` without error but no
diagnostic ever fires. CLI flags `-w` and `-W` are also no-ops.

Tests: `use_warnings_silent_on_undef_arithmetic_today`,
`use_warnings_silent_on_string_in_numeric_today`,
`no_warnings_pragma_runs_without_error`,
`lib_eval_runs_undef_arith_without_warnings`.

Severity: **bug**. Many test harnesses rely on `use warnings FATAL =>
'all'` to surface latent bugs.


## BUG-088 — `(&@)` block prototype with trailing args drops the trailing args

```sh
$ stryke -e '
sub myff (&@) { my $cb = shift; print "after-shift count=", scalar @_ }
myff { 1 } 5, 7'
after-shift count=0           # trailing args were not passed
```

Stryke parses `myff { ... } 5, 7` as `myff({...}); 5; 7;` — three
top-level comma operands. Workaround: explicit-paren call form
`myff(sub { ... }, 5, 7)` does pass all args correctly.

Tests: `block_at_prototype_with_trailing_args_evaluates_trailing_as_statements_today`,
`block_prototype_passes_block_as_first_arg`.

Severity: **bug** (common idiom for `apply(\&block, list)` style APIs).


## BUG-093 — `intercept_remove(NAME, KIND)` does not actually remove advice

```sh
$ stryke -e '
fn payload { print "G;" }
before "payload" { print "B;" }
after  "payload" { print "A;" }
payload();
intercept_remove("payload", "before");
payload();              # B; still fires'
B;G;A; B;G;A;
```

`intercept_clear(NAME)` (which removes ALL advice for the named target)
DOES work; only the per-kind variant is broken.

Tests: `intercept_clear_removes_all_advice_for_target`,
`intercept_remove_does_not_remove_advice_today`,
`intercept_remove_unknown_kind_does_not_panic`.

Severity: **bug**.


## BUG-094 — Three-level `eval { die ... } / die $@` chain drops innermost log mutations

```sh
$ stryke -e '
my $log = "";
eval {
  eval {
    eval { die "in\n" };
    $log .= "L1:" . $@;             # this mutation is lost
    die $@;
  };
  $log .= "L2:" . $@;
  die $@;
};
$log .= "L3:" . $@;
print $log'
L2:in
L3:in
                                    # L1: never made it into $log
```

The L1 append happens *between* the innermost `eval` ending and the
re-`die`; somewhere in that window the lexical `$log`'s mutation is
dropped. Two-level chains preserve all writes correctly (the existing
`nested_eval_die_rethrow_preserves_message` test pins that).

Tests: `three_level_die_rethrow_drops_innermost_log_today`,
`nested_eval_die_rethrow_preserves_message` (the 2-level form that
works).

Severity: **bug**.


## BUG-095 — `my ($scalar, @rest) = @_` slurps the FULL @_ into @rest

```sh
$ stryke -e '
sub myff { my ($cb, @rest) = @_; print scalar @rest }
myff(sub { 1 }, 5, 7)'
3                              # @rest captured all 3 — should be 2 (5, 7)
```

`my ($cb, $val) = @_` and `my $cb = shift; my $val = shift` both work
correctly. Only the slurpy-array destructuring form is wrong. Compounds
with BUG-037 (closure coderef + flattened array → scalar count) when
trying to forward args via `$cb->(@rest)`.

Tests: `destructuring_my_scalar_array_returns_full_at_underscore_today`,
`coderef_call_with_named_array_arg_loses_args_today`.

Severity: **bug** (very high impact — breaks every `($head, @tail) = @_`
idiom).


## ~~BUG-089~~ DESIGN-001 — Closures capture outer-scope vars by value, writes are a compile-time error

**Not a bug — intentional language-design choice, strictly enforced.**
Stryke closures snapshot outer-scope `my` variables at capture time
rather than holding a live reference to their storage. This matches
Rust's `move ||` closure semantics, trades shared-mutable state for
race-free dispatch into the parallel runtime (`pmap`, `pfor`,
`cluster`, async/spawn blocks), and removes an entire class of "is
this closure-mutating-outer-var safe across threads?" questions from
the language.

**Strict enforcement** (compile-time): writes to an outer-scope `my`
variable from inside any sub body (`sub { }` / `fn { }` /
`sub foo { }`) are rejected by the compiler with this diagnostic:

```
cannot modify outer-scope `my $count` from inside a closure —
stryke closures capture by value to keep parallel dispatch
race-free. Use `mysync $count` for shared mutable state, or
`--compat` for Perl 5 shared-storage semantics
```

The three opt-out paths:

| Path | Storage | Use case |
|------|---------|----------|
| `mysync $x` | atomic shared cell | counters, accumulators, factory state, observer registries |
| `our $x` / `$main::x` | package global | cross-module shared state (always shared, every mode) |
| `--compat` mode | Perl 5 shared-storage | porting Perl code unchanged |

Reads of outer-scope `my` are fine — you get the snapshot value at
capture time. Mutations through *aggregate references* are fine too
— `my $h = {}; my $f = sub { $h->{k} = 42 }` works because the
ref-identity (the Arc to the underlying hash) is preserved across
capture; only the scalar `$h` itself isn't shared.

`defer { ... }` is exempt — it runs synchronously at scope exit with
intentionally shared state. The check fires only on subs stored as
closure values.

What this means for common patterns:

- Factory with internal state (now requires `mysync`):
  ```
  fn make_counter { mysync $n = 0; sub { ++$n } }
  my $c = make_counter(); $c->(); $c->(); $c->();   # 3
  ```
- For-loop iteration captures each iteration's fresh `my $i` correctly
  (no `mysync` needed — read-only):
  ```
  my @fs; for my $i (1..3) { push @fs, sub { $i } }   # [1, 2, 3]
  ```
- `map { my $captured = $x; sub { $captured } } LIST` — explicit
  per-iteration `my` snapshot, read-only in the closure.

What requires an idiom change vs Perl:

- Outer counter: declare `mysync $n` (or use `--compat`):
  ```
  # Idiomatic stryke (parallel-safe atomic counter)
  mysync $n = 0;
  my $inc = sub { $n++ };

  # Perl-compat (shared storage)
  # stryke --compat -e 'my $n = 0; my $inc = sub { $n++ };'
  ```
- Observer pattern: pass a hash/array ref through the closure (ref
  identity preserved across the snapshot — only scalars are
  copied-by-value).

Tests pinning the documented behaviour:
`closure_captures_outer_var_by_value` (was `_does_not_see_outer_var_mutation_today`),
`closure_modifying_outer_scalar_stays_local` (was `_does_not_propagate_today`),
`closure_does_not_observe_outer_array_push` (was `_today`),
`closure_does_not_observe_outer_hash_extension` (was `_today`),
`fn_factory_returning_sub_captures_factory_param`,
`for_loop_closure_captures_each_iteration_var`,
`factory_with_internal_state_is_a_working_counter`,
`map_inside_closure_captures_unique_per_iteration`.

Status: **DESIGN** (not a bug). Documented behaviour, distinguishes
stryke from Perl 5, motivated by parallel-safety.


## BUG-090 — Slurpy `@rest` / `%rest` in destructure captures the FULL list — **FIXED**

`compile_var_declarations` was emitting `Op::GetArray(tmp)` for every
slurpy position regardless of where in the list it sat. New
`Op::GetArrayFromIndex(name_idx, start)` pushes `tmp[start..]` and the
compiler emits it for the slurpy `@rest` / `%rest` decl, with `start`
set to the decl's index in the destructure pattern. Single-scalar leads
(`my ($a, @rest) = …`) and multi-scalar leads (`my ($a, $b, $c, @rest)
= …`) all read the correct tail now. Hash slurp gets the same treatment
— `my ($a, %h) = (1, k1, v1, k2, v2)` builds `%h` from `tmp[1..]` as
alternating key-value pairs.

Tests: `slurpy_array_destructure_from_literal_list_takes_tail` (was
`_captures_all_today`), `slurpy_array_destructure_from_at_underscore_takes_tail`,
`slurpy_hash_destructure_takes_tail`,
`destructuring_my_scalar_array_takes_at_underscore_tail`,
`coderef_call_with_named_array_arg_passes_through` (the canonical
`my ($cb, @args) = @_; $cb->(@args)` idiom now propagates args).

Severity: **bug** (FIXED — affected every `($head, @tail) = @_`
idiom across the codebase).


## BUG-097 — `print {$fh} ...` braces form does not honor the filehandle

```sh
$ stryke -e '
open my $fh, ">", "/tmp/out" or die;
print {$fh} "data\n";
close $fh;
print "file: ", -s "/tmp/out"'
CODE(__ANON__)file: 0          # the brace expression is evaluated and printed
```

Stryke parses `print {$fh} ...` as `print { ... }` where the braces
introduce a hashref-or-block context, not as the filehandle-disambiguator
form. Workaround: `print $fh "data\n"` (no braces) when `$fh` is a
simple scalar.

Tests: `print_braces_filehandle_form_does_not_write_to_handle_today`,
`print_to_filehandle_writes_to_stdout_today` (BUG-085 cousin).

Severity: **bug**.


## BUG-098 — `eof($fh)` always returns false

```sh
$ stryke -e '
open my $fh, "<", "/tmp/x";
my $line = <$fh>;          # "x\n", file's only line
print eof($fh) ? "Y" : "N"'
N                              # should be Y
```

The `eof` builtin reports false even after all data has been consumed.
Workaround: detect end via undef return from `<$fh>`.

Tests: `eof_always_returns_false_today`,
`readline_on_eof_filehandle_returns_undef`.

Severity: **bug**.


## BUG-091 — `@{$h_ref}{KEYS}` hash-slice through arrayref-deref errors

```sh
$ stryke -e 'my %h = (a=>1, b=>2, c=>3); my $r = \%h;
            my @v = @{$r}{qw(a c)};
            print "@v"'
Can't dereference non-reference as array at -e line 1.
```

The arrow-form workaround is fine: `($r->{a}, $r->{c})` does what one
would want.

Tests: `hash_slice_through_hashref_via_at_brace_deref_fails_today`,
`hash_slice_through_hashref_via_arrow_keys_works`.

Severity: **bug**.


## BUG-092 — Ternary inside `"@{[ ... ]}"` interpolation rejected at parse time

```sh
$ stryke -e 'my $x = 5; my $s = "@{[ $x > 0 ? "pos" : "neg" ]}"; print $s'
Unterminated @{ ... } in double-quoted string at -e line 1.
```

Stryke's interpolation parser bails on the inner `?`/`:` pair. Workaround:
move the ternary out: `my $r = $x > 0 ? "pos" : "neg"; my $s = "...$r..."`.

Tests: `ternary_inside_interpolated_anon_array_is_rejected_today`,
`ternary_outside_interpolation_works`.

Severity: **bug** (parser).


## BUG-099 — `reverse()` with bare empty parens is a parse error

```sh
$ stryke -e 'my @r = reverse(); print scalar @r'
Unexpected token RParen at -e line 1.
```

Calling `reverse` on an empty list should yield the empty list. The
empty-parens form is rejected; passing an empty array variable
(`reverse @empty`) works.

Tests: `reverse_with_bare_empty_parens_is_parse_error_today`,
`reverse_of_empty_array_var_returns_empty`.

Severity: **bug** (small surface).


## BUG-101 — `my ($x) = @arr` returns scalar count, not first element

```sh
$ stryke -e 'my @a = (10, 20, 30); my ($x) = @a; print $x'
3                              # count, not 10
$ perl   -e 'my @a = (10, 20, 30); my ($x) = @a; print $x'
10
$ stryke -e 'sub t { my ($x) = @_; print $x } t("hello", "world")'
2                              # count, not "hello"
```

In Perl, parens around the LHS make the assignment list-context: `my
($x) = LIST` binds `$x` to the first element. Stryke treats it as
scalar-context (same as `my $x = @arr`), giving the count.

The literal-list source DOES work: `my ($x) = ("hello")` binds correctly.
Only `@_` and named-array sources fail. Same family as BUG-090
(slurpy destructure leaks).

Workarounds: `my $x = shift` or `my $x = $_[0]`.

Tests: `single_scalar_destructure_from_array_var_returns_count_today`,
`single_scalar_destructure_from_at_underscore_returns_count_today`,
`single_scalar_destructure_from_literal_list_works`,
`shift_workaround_for_first_element_works`,
`dollar_underscore_zero_workaround_for_first_element_works`.

Severity: **bug** (very high impact). Affects every `my ($self) = @_;`
or `my ($cb) = @_;` extraction pattern in OO + functional code.


## BUG-102 — `refaddr(\&fn)` differs between repeated evaluations

```sh
$ stryke -e 'sub myff { 1 }
            my $r1 = \&myff; my $r2 = \&myff;
            print refaddr($r1) == refaddr($r2) ? "eq" : "ne"'
ne
$ perl -MScalar::Util=refaddr -e '...'
eq
```

Same root issue as BUG-075 (refaddr of `\@a`): each `\&fn` creates a
fresh ref-cell rather than returning the underlying CV's address. Pure
copy via `=` keeps the same refaddr.

Tests: `refaddr_of_repeated_backslash_amp_returns_different_today`.

Severity: **bug** (parity).


## BUG-103 — `prototype($coderef)` empty for anonymous-sub refs

```sh
$ stryke -e 'my $r = sub ($) { 42 }; print prototype($r)'
                                # empty
$ stryke -e 'sub myff ($) { 42 } print prototype(\&myff)'
$
```

Named-sub coderefs report their prototype correctly. Anonymous-sub
coderefs return empty.

Tests: `prototype_of_anonymous_sub_coderef_is_empty_today`,
`prototype_of_named_sub_via_amp_ref_works`.

Severity: **bug** (small surface; a workaround is to assign the anon
sub to a typeglob with a name).


## BUG-104 — `print $x - $y, list` parses `$x` as an indirect filehandle

```sh
$ stryke -e 'my $x = 5; my $y = 3; print $x - $y, "end"'
print on unopened filehandle 5 at -e line 1.
$ stryke -e 'my $x = 5; my $y = 3; print $x + $y, "end"'
8end                              # `+` form works
$ stryke -e 'my $x = 5; my $y = 3; print(($x - $y), "end")'
2end                              # parens work
$ perl   -e 'my $x = 5; my $y = 3; print $x - $y, "end"'
2end                              # Perl handles it
```

The `-` form trips stryke's indirect-filehandle parser because `-`
also means unary minus. The `+` form is unambiguous. Workaround: wrap
the expression in parens, or store the result in a temporary first.

Tests: `print_scalar_minus_scalar_with_trailing_args_parses_as_filehandle_today`,
`print_scalar_plus_scalar_with_trailing_args_works`,
`print_paren_workaround_for_minus_form_works`.

Severity: **bug** (parser ambiguity).


## BUG-105 — `to_json` on a circular reference crashes the process

```sh
$ stryke -e '
my $a = {};
$a->{self} = $a;
my $j = eval { to_json($a) };
print defined($j) ? "ok" : "err: $@"'
thread 'main' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

The `eval { }` cannot catch this — it's a Rust-level stack overflow,
not a Perl-level die. Both direct cycles (`$a->{self} = $a`) and
indirect ones (A points to B, B points to A) trigger the crash. Most
JSON encoders detect cycles and either bail with a Perl-level error or
emit a sentinel.

Tests: `to_json_circular_at_least_parses`,
`to_json_basic_round_trip_works`.

Severity: **bug** (process-level crash; cannot be guarded against from
user code).


## BUG-106 — `to_json($data, $opts_hashref)` serializes both args as an array

```sh
$ stryke -e 'print to_json({a=>1, b=>2}, {pretty => 1})'
[{"a":1,"b":2},{"pretty":1}]
$ perl -MJSON::PP -e 'print JSON::PP->new->pretty->encode({a=>1, b=>2})'
{
   "a" : 1,
   "b" : 2
}
```

Stryke's `to_json` does not recognize a second-argument options hashref
— both args are flattened into a top-level JSON array. Workaround: use
`to_yaml` for human-readable output (which works), or implement
pretty-printing manually.

Tests: `to_json_two_arg_pretty_form_serializes_as_array_today`.

Severity: **bug** (low impact; rarely needed for machine-read JSON).


## BUG-107 — `"$Pkg::Var"` interpolation drops the package prefix — **FIXED**

```sh
$ stryke -e 'package Foo; our $bar = "hello"; package main; print "[$Foo::bar]"'
[hello]
$ perl   -e 'package Foo; our $bar = "hello"; package main; print "[$Foo::bar]"'
[hello]
```

`parse_interpolated_string` in parser.rs now greedy-matches `::` continuations
after the bare ident is read, mirroring the `$#Foo::a` and bare-code paths.
Multi-segment chains (`$A::B::C::x`) are also supported.

A separate lexer issue (the IPv6-zero-compression trap) was misfiring for
3-or-more-segment package paths like `package A::B::C` because the
hex-digit-only ident `B` (1 char, ≤ 4) followed by `::` looked like an
IPv6 address. Fixed by skipping the IPv6 trap when `ident_start` is
preceded by `::`.

Tests: `package_qualified_scalar_interpolates_correctly` (formerly
`..._with_dropped_prefix_today`), `package_qualified_scalar_in_bare_code_works`,
plus `package_decl_parses_three_segments`,
`package_decl_parses_four_segments`,
`package_qualified_scalar_interpolates_with_deeper_namespace`,
`ipv6_literal_fe80_still_lexes_as_address` in
`tests/suite/behavior_pin_2026_05_ap.rs`.

Severity: **parity** (FIXED).


## BUG-108 — `par`/`par_reduce`/`~p>` over a real `@a` array reads scalar count

The chunk-parallel macros work correctly on string sources (chunked per
char) and pass arrayrefs through as a single chunk, but a bare `@a`
source is read in scalar context *before* chunking, so each worker
sees `$_` = the array length and `@_` = `(length,)` instead of the
intended array slice.

```sh
$ s -e 'my @a = (10, 20, 30); my $r = ~> @a par_reduce { sum(@_) }; print "$r\n"'
3
$ s -e 'my @a = (10, 20, 30); my $r = ~p> @a sum; print "$r\n"'
3
$ s -e 'my @a = (10, 20, 30); my @r = ~> @a par { sum(@_) }; print "[@r]\n"'
[]
$ s -e 'my $r = ~p> 1:5 sum; print "$r\n"'
0
$ s -e 'my $r = ~p> [1,2,3] sum; print "$r\n"'
0
```

Expected:

```sh
$ s -e 'my @a = (10, 20, 30); my $r = ~p> @a sum; print "$r\n"'
60
$ s -e 'my $r = ~p> 1:5 sum; print "$r\n"'
15
```

Root cause sits in the chunk-source coercion in
`vm_helper.rs::par_chunk_value` — for `PerlValue::Array` it currently
falls through the scalar-coercion path. Range expressions reach the
coercion as a numeric scalar (`0`); arrayrefs survive but the worker
never deref's them, so `sum` gets a scalar that numifies to 0.

**Workaround:** wrap the array in an explicit deref before threading,
or use `~> @a par { map { ... } @_ }` with the `@_` form (still
broken). The only path that works today is string-input or arrayref
that the body handles with explicit `@$_` deref.

Pinning tests:
`par_reduce_array_source_currently_sees_scalar_count_not_elements`,
`par_reduce_array_source_explicit_reducer_is_also_broken`,
`par_chunk_block_array_source_returns_empty_list`,
`p_arrow_array_source_sees_count_not_elements`,
`p_arrow_range_source_returns_zero`,
`p_arrow_arrayref_source_falls_back_to_single_chunk_with_zero_sum`,
`p_arrow_string_source_chunks_per_char_and_works`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **bug**.


## BUG-109 — `sum(\@a)` and `sum([1,2,3])` return 0 instead of summing

`sum` does not auto-deref a single arrayref argument, so calling it
through any thread-stage that hands it a ref produces zero. The same
likely affects `min`, `max`, `mean`, etc. — anything that takes a
list-of-numbers.

```sh
$ s -e 'p sum([1,2,3])'
0
$ s -e 'my @a = (10,20,30); p sum(\@a)'
0
$ s -e 'my $r = [10,20,30]; p sum(@$r)'   # workaround
60
```

Expected: `sum([1,2,3])` returns 6, `sum(\@a)` returns 60. Either
auto-deref a single arrayref arg or document the limitation as
intentional.

This bug compounds with **BUG-108**: even if chunking handed each
worker an arrayref, `sum` would still report 0.

Pinning tests:
`sum_on_arrayref_returns_zero_not_sum`,
`sum_on_array_ref_via_backslash_returns_zero`,
`sum_on_explicit_deref_works`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **bug**.


## PARITY-040 — Scalar-context `..` flip-flop operator is unimplemented

The classic `print if N..M` line-range flip-flop produces no output
in stryke; Perl emits the lines whose `$.`-counter falls in the
specified range. This breaks the canonical `awk '/start/,/end/'`
translation idiom that motivates flip-flops in the first place.

```sh
$ s -e 'for (1..10) { print "$_," if 3..5 } print "\n"'

$ perl -e 'for (1..10) { print "$_," if 3..5 } print "\n"'
3,4,5,
```

The list-context `..` works correctly (range expansion); only the
scalar-context flip-flop / flip-flap (`...`) variants are missing.
A full fix needs hidden per-occurrence state, the `E0`/`E1` edge-
counter Perl exposes, and the tri-dot non-eager variant.

Pinning test:
`flip_flop_scalar_context_does_not_match_perl_lines`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity**.


## PARITY-041 — Arrayref/hashref in numeric context returns 0, not the heap address

```sh
$ s -e 'my $r = [1,2,3]; print "num=", $r + 0, "\n"'
num=0
$ perl -e 'my $r = [1,2,3]; print "num=", $r + 0, "\n"'
num=4354497000
```

Perl exposes the heap address of a ref when it's used in numeric
context. Scripts that test `if ($ref + 0)` for definedness, or
compare two refs with `==` (numeric ref-equality), break under
stryke. Stringification of refs (`"$ref"`) still produces the
expected `ARRAY(0x...)` / `HASH(0x...)` text.

Pinning test:
`arrayref_in_numeric_context_returns_zero_not_address`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity**.


## PARITY-042 — `chr(N)` for N > 0x10FFFF or N < 0 returns the empty string

```sh
$ s -e 'my $c = chr(0x110000); print length($c), "\n"'
0
$ s -e 'my $c = chr(-1); print length($c), "\n"'
0
$ perl -e 'my $c = chr(0x110000); print length($c), "\n"'   # warns + emits
1
```

Stryke clamps to the valid Unicode range; Perl warns but still emits
a (potentially malformed) character up to chr <= 0x7FFFFFFF. The
stryke behavior is intentionally stricter for UTF-8 hygiene; pinning
both edge cases so a future change is deliberate.

Pinning tests:
`chr_above_max_unicode_returns_empty_string`,
`chr_negative_returns_empty_string`,
`chr_max_valid_unicode_works`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity** (intentional-strictness).


## NOT-A-BUG observations (pinned, but documented as deliberate)

These are known design choices, listed here so a future contributor doesn't
"fix" them:

- **`succ`/`pred` are numeric-only.** `succ("b")` returns `1`, not `"c"`.
  See test `succ_on_string_numifies_to_zero_plus_one`. The Perl-magic form
  is reachable only through `++`, which is governed by PARITY-001 above.

- **Many short names are stryke builtins** (`fact`, `factorial`, `id`,
  `squared`, `cubed`, `f`, etc.). Outside `--compat`, `fn name { ... }`
  for any of these is a parse-time rejection. Tests cover `id` and
  `squared`. Note that `neg` is *not* a builtin — calling `neg(7)` raises
  `Undefined subroutine &neg`, so the unary-minus role still belongs to
  the `-` operator.

- **`p` of an arrayref/hashref prints `ARRAY(0x...)` / `HASH(0x...)`.**
  This matches Perl's `print` semantics for refs. To dump structure, use
  the appropriate dump helper.


## How to add to this file

When you find a new behavior worth tracking:

1. Add a numbered section (continue PARITY-NNN / BUG-NNN / POLISH-NNN).
2. Show the minimal reproducer with `stryke -e '...'` and the observed
   output. If applicable, contrast with `perl -e '...'`.
3. Add a pinning test in `tests/suite/behavior_pin_2026_05.rs` (or a
   dated successor, e.g. `behavior_pin_2026_06.rs` once this file fills).
4. Cite the test name(s) in the BUGS.md entry so they stay linked.

Do not delete entries when a bug is fixed — flip the test from "current
buggy output" to "correct output" and mark the entry **FIXED** with the
commit hash and date.
