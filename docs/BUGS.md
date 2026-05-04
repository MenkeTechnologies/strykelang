# BUGS.md — Known parity gaps and surprising behaviors

Captured 2026-05-04 from a behavior-pinning sweep against `stryke v0.11.12` on
macOS aarch64.

Each entry is paired with a test in `tests/suite/behavior_pin_2026_05.rs`
that locks the *current* output. When a bug is fixed, update the corresponding
test rather than deleting it — the test then becomes the regression guard.

Severity legend:

- `parity` — diverges from Perl 5; intentional or accidental TBD
- `bug` — observably wrong vs documented intent
- `polish` — non-critical UX/error-message issue


## PARITY-001 — Magic string increment is not implemented

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


## PARITY-002 — `(my $copy = $orig) =~ s///` / `=~ tr///` does not bind to the copy

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


## PARITY-003 — `2 ** 64` falls back to float instead of bigint

```sh
$ stryke -e 'print 2 ** 64'
1.84467440737096e+19    # stryke
# perl prints (with use bigint;): 18446744073709551616
```

Tests: `pow_2_64_uses_float_notation`.

Severity: **parity**. The repo has a `bigint_compat.rs` test suite, so this
is a known limitation rather than oversight. Worth pinning so the float
formatter doesn't change shape silently.


## PARITY-004 — Division by zero surfaces as `ErrorKind::Runtime`, not `DivisionByZero`

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
