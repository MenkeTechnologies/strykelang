# BUGS.md — Known parity gaps and surprising behaviors

Captured 2026-05-04 from a behavior-pinning sweep against `stryke v0.11.12` on
macOS aarch64. The sweep produced 17 batches of pin tests
(`tests/suite/behavior_pin_2026_05_a..q.rs`, ~750 cases total) and the entries
below.

Each entry is paired with one or more pin tests that lock the *current*
output. When a bug is fixed, update the corresponding test rather than
deleting it — the test then becomes the regression guard.

Severity legend:

- `parity` — diverges from Perl 5; intentional or accidental TBD
- `bug` — observably wrong vs documented intent
- `polish` — non-critical UX/error-message issue

## High-impact bugs (worth fixing first)

These break common Perl idioms across the codebase:

| ID | Summary |
|----|---------|
| BUG-037 | Closure-captured coderefs called with `@_` flatten to scalar count |
| BUG-089 | Closures capture outer-scope `my` vars by value — outer counter idiom broken |
| BUG-090 | `my ($head, @tail) = LIST` slurps full LIST into `@tail` |
| BUG-095 | `my ($cb, @rest) = @_` slurps full `@_` into `@rest` (same root as BUG-090) |
| BUG-010 | `return (1, 2, 3)` collapses to last comma operand |
| BUG-011 | `my $s = list_sub()` concatenates instead of taking last element |
| BUG-018 | `local $/; <$fh>` does not enable slurp mode |
| BUG-019 | `for (@a) { $_ *= 10 }` does not alias array element for mutation |

These compound: BUG-095 breaks every `($cb, @rest) = @_; $cb->(@rest)`
pattern, BUG-089 breaks every state-tracking closure, and BUG-037 breaks
every coderef-call-with-array-arg. Together they make most functional-
style libraries unusable until fixed.


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


## PARITY-005 — Modulo follows sign-of-dividend, not sign-of-divisor

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


## PARITY-006 — `sprintf "%g"` falls back to `%f` formatting

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


## PARITY-007 — `sprintf "%e"` omits the `+` sign and zero-pad on the exponent

```sh
$ stryke -e 'printf "%e\n", 12345.6789'
1.234568e4
$ perl   -e 'printf "%e\n", 12345.6789'
1.234568e+04
```

Tests: `sprintf_e_format_omits_plus_and_zero_pad_today`.

Severity: **parity**.


## PARITY-008 — `sprintf "%v..."` not implemented

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


## PARITY-009 — `sprintf` positional `%N$s` not implemented

```sh
$ stryke -e 'print sprintf("%2\$s %1\$s", "world", "hello")'
worlds hellos
$ perl   -e 'print sprintf("%2\$s %1\$s", "world", "hello")'
hello world
```

Tests: `sprintf_positional_arg_emits_literal_today`.

Severity: **parity** (i18n-relevant).


## PARITY-010 — `vec($s, $offset, $bits) = N` rejected as complex lvalue

```sh
$ stryke -e 'my $s = ""; vec($s, 0, 8) = 65; print $s'
VM compile error (unsupported): Assign to complex lvalue at -e line 0.
```

Severity: **parity**. `vec` lvalue assignment is the standard way to do
bit-packing in Perl 5.


## PARITY-011 — `CORE::*` namespace not available

```sh
$ stryke -e 'CORE::print "hi"'
Undefined subroutine &CORE::print at -e line 1.
```

Severity: **parity**. Affects code that explicitly disambiguates between
overridden and built-in operations.


## PARITY-012 — `use overload "+" => sub { ... }` rejects anonymous-sub handlers

```sh
$ stryke -e 'package N; use overload "+" => sub { 1 }'
overload handler must be a string literal, number (e.g. fallback => 1),
or \&subname (method in current package) at -e line 1.
```

Workaround: declare a named sub and pass `\&name`. Perl 5 accepts
anonymous subs directly.

Severity: **parity**.


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


## BUG-008 — `%h{KEYS}` kv-slice returns the full hash

```sh
$ stryke -e 'my %h = (a=>1, b=>2, c=>3); my %sub = %h{qw(a c)}; say sort keys %sub'
a b c                       # stryke (wrong)
$ perl   -e 'my %h = (a=>1, b=>2, c=>3); my %sub = %h{qw(a c)}; say sort keys %sub'
a c                         # perl
```

Tests: `kv_slice_returns_full_hash_today`.

Severity: **bug**. Was added to Perl in 5.20; widely used.


## BUG-009 — `exists $h{x}{y}` errors when `$h{x}` is missing

```sh
$ stryke -e 'my %h = (a => {b => 1}); say exists $h{x}{y} ? 1 : 0'
exists argument is not a HASH reference at -e line 1.
$ perl   -e 'my %h = (a => {b => 1}); say exists $h{x}{y} ? 1 : 0'
0
```

The intermediate hash should be auto-vivified to an empty hashref for the
purpose of `exists`, then the inner check returns false. Stryke fails the
deref step instead.

Tests: `exists_on_missing_intermediate_errors_today`,
`exists_on_present_chain_returns_true`.

Severity: **bug**.


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


## PARITY-013 — `length` ignores `use utf8` and always returns byte count

```sh
$ stryke -e 'use utf8; print length("héllo")'
6                       # stryke
$ perl   -e 'use utf8; print length("héllo")'
5
```

Without `use utf8;` both produce 6 (correct byte count). The pragma is the
issue: stryke does not switch `length` to character semantics.

Tests: `length_returns_byte_count_for_unicode_string`,
`length_with_use_utf8_still_counts_bytes_today`.

Severity: **parity**.


## PARITY-014 — `substr($s, $off, $len) = $rep` lvalue not supported

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


## BUG-010 — `return (LIST)` collapses to last comma operand

```sh
$ stryke -e 'sub xs { return (1, 2, 3) } my @a = xs(); print "@a"'
3
$ perl   -e 'sub xs { return (1, 2, 3) } my @a = xs(); print "@a"'
1 2 3
```

Implicit return at the end of a sub body works correctly:
```sh
$ stryke -e 'sub xs { (1, 2, 3) } my @a = xs(); print "@a"'
1 2 3
```

Only `return (...)` with parens around a comma-list is wrong. Returning a
named array (`return @x`) is also fine.

Tests: `explicit_return_paren_list_collapses_to_last_today`,
`implicit_list_return_yields_full_list`,
`return_array_var_passes_through_full_list`.

Severity: **bug**. Affects every multi-value early-return pattern.


## BUG-011 — `my $s = list_returning_sub()` concatenates instead of taking last

```sh
$ stryke -e 'sub xs { (1,2,3) } my $s = xs(); print $s'
123
$ perl   -e 'sub xs { (1,2,3) } my $s = xs(); print $s'
3
```

`scalar xs()` correctly returns `3`, so the keyword path works. The
implicit-scalar-context path (assignment to a scalar lvalue) does not.

Tests: `list_returning_sub_in_scalar_context_concatenates_today`,
`list_in_scalar_context_via_scalar_keyword_takes_last`.

Severity: **bug**. Common Perl pattern (e.g. `my $count = xs();` for the
"return last/array length"-style API).


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


## BUG-017 — `sprintf "%+d"` ignores the `+` flag

```sh
$ stryke -e 'print sprintf("%+5d", 3)'
   3
$ perl   -e 'print sprintf("%+5d", 3)'
   +3
```

Tests: `printf_plus_flag_ignored_today`.

Severity: **bug** (low impact). Affects readable signed output.


## PARITY-015 — `"Inf"` and `"NaN"` strings numify to 0

```sh
$ stryke -e 'print "Inf" + 1, "/", "NaN" + 0'
1/0
$ perl   -e 'print "Inf" + 1, "/", "NaN" + 0'
Inf/NaN
```

Float overflow produces `"inf"` (e.g. `9 ** 9 ** 9`) and `sqrt(-1)` produces
`"nan"`, so the float internals support specials — only the string-to-num
parser doesn't recognise them.

Tests: `numeric_inf_string_does_not_become_infinity_today`,
`numeric_overflow_yields_inf`, `sqrt_negative_yields_nan`.

Severity: **parity**.


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


## BUG-019 — `for (@arr) { $_ ... }` does not alias array elements

```sh
$ stryke -e 'my @a = (1..3); for (@a) { $_ *= 10 } print "@a"'
1 2 3
$ perl   -e 'my @a = (1..3); for (@a) { $_ *= 10 } print "@a"'
10 20 30
```

The named-loop-var form (`for my $x (@a)`) has the same bug. The
explicit-index form (`for my $i (0..$#a) { $a[$i] *= 10 }`) works.

Tests: `for_dollar_underscore_does_not_alias_array_element_today`,
`for_named_loop_var_does_not_alias_array_element_today`,
`for_index_assignment_works`.

Severity: **bug**. Affects every in-place mutation idiom.


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


## BUG-025 — `$SIG{__WARN__}` handler is not invoked

The variable is assignable and reads back as a CODE ref, but `warn` does
not route through it. Captured indirectly: stderr still receives the warn
text and the handler's body never executes.

Tests: `sig_warn_assignment_succeeds` (the assignment does work).

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


## BUG-027 — `$#arr = N` does not change array length

```sh
$ stryke -e 'my @a = (1..5); $#a = 2; print scalar @a, " / @a"'
5 / 1 2 3 4 5
$ perl   -e 'my @a = (1..5); $#a = 2; print scalar @a, " / @a"'
3 / 1 2 3
```

Both truncation (`$#a = $smaller`) and extension (`$#a = $bigger`, fills
with undef) are no-ops.

Tests: `dollar_hash_array_lvalue_does_not_truncate_today`.

Severity: **bug**.


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


## BUG-029 — `$&` does not interpolate inside double-quoted strings

```sh
$ stryke -e '"abXYZcd" =~ /XYZ/; print "[$&]"'
[$&]                    # stryke (literal)
$ perl   -e '"abXYZcd" =~ /XYZ/; print "[$&]"'
[XYZ]
```

`my $m = $&` works correctly; only the interpolation form is broken.

Tests: `match_dollar_amp_captures_whole_match` (the form that works),
`match_dollar_amp_does_not_interpolate_today`.

Severity: **bug** (interpolation parser).


## PARITY-016 — `ref $@ eq "Class"` parses with the wrong precedence

```sh
$ stryke -e 'package E; sub new { bless {}, shift } package main;
             eval { die E->new }; print ref $@ eq "E" ? "Y" : "N"'
N
$ stryke -e 'package E; sub new { bless {}, shift } package main;
             eval { die E->new }; print((ref $@) eq "E" ? "Y" : "N")'
Y
```

Stryke parses `ref $@ eq "E"` as `ref ($@ eq "E")` — the named-unary
operator's argument absorbs the `eq` expression — instead of `(ref $@) eq
"E"` per Perl precedence.

Tests: `ref_dollar_at_eq_string_precedence_today`,
`die_with_blessed_object_preserves_class` (the form that works).

Severity: **parity**. Common idiom for typed-exception dispatch.


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


## BUG-034 — `sprintf "%#x"` / `"%#o"` ignore the `#` flag

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


## BUG-049 — `sprintf` star-width / dynamic-precision (`%*d`, `%.*f`) not implemented

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


## BUG-050 — `$SIG{__DIE__}` handler is not invoked

```sh
$ stryke -e '$SIG{__DIE__} = sub { print "trapped" }; eval { die "x" }; print "after"'
after
$ perl   -e '$SIG{__DIE__} = sub { print "trapped" }; eval { die "x" }; print "after"'
trappedafter
```

`$SIG{__WARN__}` (BUG-025) and `$SIG{__DIE__}` share the same root cause:
hash-store side of `%SIG` works (`ref` returns CODE), but the runtime
hooks that fire signal-pseudo-handlers don't dispatch through `%SIG`.

Tests: `sig_die_handler_not_invoked_today`,
`sig_handler_assignment_returns_code_ref`.

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


## BUG-057 — `sprintf "%a"` (hex-float) not implemented

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


## BUG-079 — `sprintf "%n"` is a no-op

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


## BUG-080 — `sprintf "%p"` and `"%A"` not implemented

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


## BUG-082 — `0o` octal prefix not recognized

```sh
$ stryke -e 'print 0o777'
0
$ stryke -e 'print 0777'
511
$ perl   -e 'print 0o777'
511
```

Perl 5.34+ accepts `0o777` as a synonym for `0777`. Stryke parses `0o`
as the integer zero followed by an unrelated identifier, so `0o777`
evaluates to `0`. Bare leading-zero octals (`0644`, `0755`) work
correctly.

Tests: `octal_o_prefix_returns_zero_today`,
`classic_zero_prefix_octal_works`,
`octal_literal_pattern_matches_perl`.

Severity: **bug** (parity; small surface).


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


## BUG-089 — Closures don't observe outer-scope mutations (capture-by-value)

```sh
$ stryke -e '
my $x = 5;
my $f = sub { $x };
$x = 10;
print $f->()'
5                              # Perl prints 10
$ stryke -e '
my $count = 0;
my $inc = sub { $count++ };
$inc->(); $inc->(); $inc->();
print $count'
0                              # Perl prints 3
```

A closure that references an outer-scope `my` variable receives a
snapshot of that variable's value, not a reference to its storage. The
following Perl idioms break:

- Outer counter: `my $n = 0; my $inc = sub { $n++ }; $inc->(); print $n`
- Cached state: any mutation through the closure stays local to the
  closure's snapshot
- Observer pattern: external updates are invisible to subscribed
  closures

What still works:

- Factory pattern: `sub make_X { my $n; sub { ... } }` — internal `my`
  variables ARE shared between repeat calls of the inner closure, so
  `make_counter()` returns a working counter.
- For-loop iteration: `for my $i (LIST) { push @fs, sub { $i } }` — each
  iteration's `$i` is a fresh `my` and the closure captures it correctly.
- `map { my $captured = $x; sub { $captured } } LIST` — explicit
  per-iteration `my` binding works.

Tests: `closure_does_not_see_outer_var_mutation_today`,
`closure_modifying_outer_scalar_does_not_propagate_today`,
`closure_does_not_observe_outer_array_push_today`,
`closure_does_not_observe_outer_hash_extension_today`,
`fn_factory_returning_sub_captures_factory_param`,
`for_loop_closure_captures_each_iteration_var`,
`factory_with_internal_state_is_a_working_counter`,
`map_inside_closure_captures_unique_per_iteration`.

Severity: **bug** (very high impact). Combined with BUG-095 (slurpy
destructure leak) this breaks most stateful HOF patterns.


## BUG-090 — Slurpy `@rest` / `%rest` in destructure captures the FULL list

```sh
$ stryke -e 'my ($a, $b, @rest) = (1, 2, 3, 4, 5); print scalar @rest'
5                              # @rest captured all 5; should be 3
$ stryke -e 'my ($a, %h) = (1, "k1", "v1", "k2", "v2"); print scalar keys %h'
2                              # keys "1" and "v1" — slurped from offset 0
```

Stryke binds the leading scalars correctly but the slurpy `@`/`%`
captures starting from index 0, not from the position implied by the
preceding scalars. Pure-scalar destructure (`my ($a, $b) = ...`) and
explicit `shift; shift; my @rest = @_` both work.

Tests: `slurpy_array_destructure_from_literal_list_captures_all_today`,
`slurpy_array_destructure_from_at_underscore_captures_all_today`,
`slurpy_hash_destructure_captures_all_today`,
`pure_scalar_destructure_works`,
`shift_then_shift_extracts_correctly`.

Severity: **bug** (very high impact — breaks every `($head, @tail) =
@_` idiom across the codebase).


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
