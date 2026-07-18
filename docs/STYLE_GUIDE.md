# stryke style guide

Authoritative idioms for stryke source. The principle behind every rule: **min chars win**. stryke is the power-user language; every design call defaults to the terser form, no newbie concessions, no optional verbosity.

Every example pair is **DO / DON'T** with one-line rationale. The DON'T side is valid stryke (so old Perl ports keep parsing) — but new code that uses it is wrong on style, and `--no-interop` rejects most of it at parse time as a forcing function.

---

## 0. The ten hard rules

Every other idiom is downstream of these. If a piece of stryke code violates any of them, it is wrong style, full stop.

1. **`fn` not `sub`.** `sub` is Perl 5 baggage and is rejected by `--no-interop`.
2. **`p` not `print`, never `say`.** `p` is the stryke verb, has the record separator built in, costs one character. `say` is rejected by `--no-interop`.
3. **Use `|>` / `~>` over nested calls.** `f(g(h(x)))` reads inside-out; `x |> h |> g |> f` reads left-to-right and shows up at the call site without diving the cursor through three closing parens. **Always pipeline.**
4. **No semicolons at end of line.** stryke statement-terminates on newline. Trailing `;` is noise that visually weights every line. The only place `;` belongs is **between statements on the same line** (rare, e.g. C-style `for` headers): `for ($i = 0; $i < 10; $i++)`.
5. **Implicit closure parameters always.** Never write `sub { my $x = shift; … }` or `fn { my ($a, $b) = @_; … }`. Use `_` for slot 0 and `_N` for slot N. Named params are only correct when the closure is a real top-level `fn` whose arity > 2 *and* the names carry meaning.
6. **Expression-bodied `fn` for one-liners.** `fn double = _ * 2` over `fn double($a) { $a * 10 }` over `fn double { _ * 2 }`. No braces, no named param, implicit topic. The block form is for multi-statement bodies only.
7. **Implicit return — never `return` on the tail expression.** The last expression in any `fn` body, arrow-block, or expression-position block IS the return value. `return` is **only** correct as an early-return guard (`return undef if $err`). Writing `return $x` at the bottom of a body is noise.
8. **No parens on fn calls unless precedence demands them.** stryke calls take args list-operator-style. `p "hello"`, `len @a`, `uc rev "abc"` — paren-less. Add parens **only** when the call is followed by an operator that would re-bind (`len(@a) > 1`), when nesting needs disambiguation, or when an empty arg list is the intent (`time()`).
9. **No trailing `1` (or any sentinel truthy value) at the end of a library / module file.** That's Perl 5's `package returns true` ritual — `require` there demanded the last evaluated expression be truthy, so every `.pm` ended in `1;`. stryke `use`/`import` resolution does not consult the file's tail expression, so the `1` is pure noise inherited from CPAN muscle memory. Library files end at the last `fn` (or `class` / `const`) definition. `--no-interop` rejects a bare-integer tail expression in module position.
10. **`val` for immutable, `var` for mutable — never bare `my` in new code.** stryke flips Perl 5's mutable-by-default `my` into Scala/Kotlin-style immutable-by-default: `val` marks the binding *frozen* at compile time (reassignment, push/pop, `$x[i] =`, `$h{k} =`, `delete $h{k}` all error at compile time, not run time); `var` is the mutable counterpart. Both desugar to `my` in the AST — same scope, same bytecode, same slot allocation — so the cost is zero. Default to `val`; reach for `var` only when you actually mutate. `val` also accepts a type annotation (`val $port : Int = 8080`). `for val $x (LIST)` and `for var $x (LIST)` both work — use `for val` unless the loop body reassigns the iterator (almost never). Bare `my` stays valid for CPAN ports (it's what Perl 5 spells), but new stryke code wrong-on-style; `--no-interop` rejects it.

```stryke
# RIGHT — fn, p, pipeline, no semicolons, implicit params, expression-body, implicit return
fn shout = uc(_) . "!"
@lines |> maps { trim(_) |> shout } |> greps { len(_) > 0 } |> ep

# WRONG — every rule violated
sub shout { my $s = shift; return uc($s) . "!"; }
for my $line (@lines) {
    my $trimmed = trim($line);
    my $upper   = shout($trimmed);
    if (length($upper) > 0) {
        print "$upper\n";
    }
}
```

The wrong version is 7× the characters and reads bottom-up because of the `for` ↔ `print` distance. Walk the violations: `sub` (rule 1), `print` (rule 2), nested `length(...)` no pipeline (rule 3), trailing `;` everywhere (rule 4), explicit `my $s = shift` not implicit `_` (rule 5), block-bodied `fn` for a one-liner (rule 6), explicit `return` on tail (rule 7). All seven, in one wrong shout-and-loop.

---

## 0a. Paren-less calls — list-operator style

stryke is a list-operator language: function args bind right of the function name without parens. `p "hello"`, `len @a`, `uc rev "abc"` are all preferred. Parens are noise unless they're earning their keep.

| DO | DON'T | Why |
|---|---|---|
| `p "hello"` | `p("hello")` | Single arg; parens add nothing. |
| `p $x, $y, $z` | `p($x, $y, $z)` | Multi-arg works the same paren-less. |
| `len @a` | `len(@a)` | Builtin call; LHS-binds. |
| `uc rev "abc"` | `uc(rev("abc"))` | Right-to-left chain reads naturally; no escape parens. |
| `len(@a) > 1` | `len @a > 1` | **Parens NEEDED** — without them, `>` rebinds and you get `len(@a > 1)` which is `len(bool)` = 0. |
| `len(keys %h)` | `len keys %h` | Same — keys's list result needs to be sealed before `len` can act on it explicitly. |
| `time()` | `time` | When you genuinely want zero args, `()` makes the call-vs-bareword intent obvious. |

Three legitimate reasons to add parens:
1. **Precedence** — when a binary operator follows the call: `len(@a) > 1`, `uc($s) . "!"`.
2. **Disambiguation** — when two adjacent unparenthesized list-ops would slurp each other's args ambiguously: `func1(arg1, arg2), func2(arg3)`.
3. **Zero-arg explicit** — `time()` over `time` when readers might mistake the bareword for a value lookup.

In a pipeline, parens are almost never needed because `|>` / `~>` already supply the staging — write `$x |> uc |> rev` not `rev(uc($x))` and not `($x) |> uc() |> rev()`.

---

## 1. Output

| DO | DON'T | Why |
|---|---|---|
| `p $x` | `print $x`, `print "$x\n"`, `say $x` | `p` is the canonical print-with-newline. One char vs five-to-seven. **`say` is rejected by `--no-interop`.** |
| `warn "$msg"` | `print STDERR "$msg\n"` | `warn` is the stryke-and-Perl verb for stderr. Auto-appends `\n` if the message doesn't end with one. Catchable via `$SIG{__WARN__}`. |
| `@xs \|> e p` | `for (@xs) { p $_ }` | Pipe-forward + `e` (each) collapses the loop to four chars after the pipe. |
| `@xs \|> ep` | `@xs \|> e p` | `ep` = `e { p }` shorthand. **Goes to stdout, NOT stderr** — name is "each-print", not "err-print". |
| `ddump $struct` | `use Data::Dumper; print Dumper($struct)` | Built-in pretty-printer; one bareword. |

---

## 1a. String interpolation — `#{ … }` / `${ … }` over `.` concat

stryke supports four interpolation forms inside double-quoted strings. **Always interpolate; never build strings with `.` concatenation when interpolation reaches.**

| Form | Example | Use when |
|---|---|---|
| `$name` | `"hello, $name!"` | Bare scalar; the cheapest form. |
| `${name}` | `"file_${name}.txt"` | Bare scalar **with disambiguation** — when adjacent chars would extend the var name (`${name}_v2`, not `$name_v2` which looks for `$name_v2`). |
| `#{ EXPR }` | `"x squared is #{$x * $x}"`, `"sum: #{sum @a}"`, `"page #{1 + $i}"` | Ruby-style interpolation of an **arbitrary expression** — math, function calls, method calls, anything. The only form that's not just-a-variable. |
| `@arr` | `"items: @arr"` | Array interpolation (joined by `$"`, default `" "`). |

| DO | DON'T | Why |
|---|---|---|
| `"hello, $name!"` | `"hello, " . $name . "!"` | Three concatenations vs zero; reads as one literal. |
| `"x squared is #{$x * $x}"` | `"x squared is " . ($x * $x)` | `#{}` lets the expression sit inline; no parens-and-dots dance. |
| `"sum: #{sum @a}"` | `"sum: " . sum(@a)` | Function calls work in `#{}`. |
| `"file_${name}.txt"` | `"file_" . $name . ".txt"` | `${}` disambiguates when the next char would extend the var name. |
| `"items: @arr"` | `"items: " . join(" ", @arr)` | Array interpolation auto-joins with `$"` (default space). |

`.` concat is **only** correct when both sides are non-string-literals (`$a . $b` joining two variables) or when computing a string lazily across statements. Inside a single string-literal context, always interpolate.

```stryke
# RIGHT — interpolated
my $name = "world"
p "hello, $name!"
p "sum is #{sum @nums}"
p "page ${i}_of_${total}"

# WRONG — concatenated
p "hello, " . $name . "!"
p "sum is " . sum(@nums)
p "page " . $i . "_of_" . $total
```

---

## 2. Topic variables

The stryke topic system has four equivalent spellings for slot 0 — pick the shortest that reads in context.

| DO | DON'T | Why |
|---|---|---|
| `_` (bare) | `$_` | Sigil-prefixed `$_` is Perl-ism only needed inside string interpolation; bare `_` is the stryke calling card. |
| `_1`, `_2` | `$a`, `$b` (outside sort/reduce blocks) | `$a`/`$b` are sort/reduce-only magic; `_N` is universal positional. |
| `_<` (outer) | `my $cap = _; …` | `_<...<` chains depth — one `<` per closure level outward. |
| `_[3]` | `substr(_, 3, 1)` | `_[N]` is char-of-topic; alias for `_!N!`. |

**Note:** `$_[N]` (with `$` and `[]`) is the legacy Perl `@_` first-positional-arg, kept for compat. Don't use in new code; use `_N` for positional or named params for clarity.

---

## 3. Function definition

### Expression-bodied form (preferred)

For one-liner functions — anything that fits on one line — use the **expression-bodied** form: `fn name = <expr>`. No braces, no named params, implicit topic.

| DO | DON'T | Why |
|---|---|---|
| `fn double = _ * 2` | `fn double { _ * 2 }` | No braces; expression body. Same length saved per definition. |
| `fn double = _ * 2` | `fn double($a) { $a * 10 }` | Implicit `_` over named `$a`; brace-less form. |
| `fn add = _0 + _1` | `fn add($a, $b) { $a + $b }` | `_N` over named for arity ≤ 3 unless names carry meaning. |
| `fn shout = uc(_) . "!"` | `fn shout { uc(shift) . "!" }` | `_` is the topic; `shift` is Perl-ism. |
| `fn area = _0 * _1 \|> int` | `fn area($w, $h) { return int($w * $h) }` | Pipeline + implicit + no return keyword (last expr is the value). |

The block-bodied form (`fn name { … }`) is correct when:
- The body is multi-statement and won't fit on one line
- Names genuinely document the role (e.g. `fn parse_url($scheme, $host, $port, $path)`)
- You need explicit `my` declarations or control flow inside the function

For one-liners, **always prefer `fn name = expr`**.

### Implicit return — never write `return` unless it's an early-return

The last expression in a `fn` body is the return value. **Never write `return` for the tail expression** — it's noise, and it visually hides the actual value the function produces.

| DO | DON'T | Why |
|---|---|---|
| `fn area = _0 * _1` | `fn area($w, $h) { return $w * $h }` | Tail position auto-returns. |
| `fn classify { … last expr … }` | `fn classify { … return last_expr }` | Same; block bodies too. |
| `fn find_first { for (@_) { return $_ if /good/ } undef }` | (no DON'T) | **Early-return is the only legitimate use of `return`.** Use it freely for guard clauses and early exits — never for the tail. |

```stryke
# RIGHT — implicit return, expression-bodied where possible
fn double = _ * 2
fn shout  = uc(_) . "!"
fn area   = _0 * _1

fn classify($n) {
    return "zero"     if $n == 0       # early return: legit
    return "negative" if $n < 0        # early return: legit
    $n < 10 ? "small" : "big"          # tail expression: NO return keyword
}

# WRONG — explicit return on tail
fn double($x) { return $x * 2 }
fn classify($n) {
    if ($n == 0) { return "zero" }
    if ($n < 0)  { return "negative" }
    return $n < 10 ? "small" : "big"
}
```

The same rule applies inside arrow-blocks (`>{ … }`), `if`/`else` arms, `match` arms, and any other expression-position block.

### Other rules

| DO | DON'T | Why |
|---|---|---|
| `fn double = _ * 2` | `sub double { $_[0] * 2 }` | `fn` opens the topic-var protocol; **`sub` is rejected by `--no-interop`**. Always `fn`. |
| `fn Rosetta::Foo::helper = …` | `fn helper = …` (when `helper` is a builtin) | Never shadow a builtin; namespace under `Module::Sub::name`. |

Builtin-shadowing is a hard rule — `fn close`, `fn log`, `fn cos`, `fn open` etc. all will abort when overriding the runtime function. Use a namespace (`MyApp::close`) or pick a different name.

---

## 4. Ranges

| DO | DON'T | Why |
|---|---|---|
| `1:10` | `1..10` | `:` is the stryke polymorphic range; works for ints, chars, dates, IPs. |
| `'a':'z'` | `'a'..'z'` | Same operator, type-inferred from literals. |
| `1:100:5` | — | Optional step is the third colon. |
| `'I'~'V'` | `'I':'V'` | Roman ranges use `~`, the full-extension-range separator (see below). |

**Roman ranges use `~`, not `:`.** Roman digits (`I V X L C D M`) collide with
Perl character ranges, so under `:` / `..` / `...` they stay char ranges —
`'I':'V'` is `I, J, K, …, V` and `'C':'M'` is `C, D, …, M` (11 letters). The `~`
"full-extension-range" separator disambiguates: `'I'~'V'` is `I, II, III, IV, V`
and `'C'~'M'` is the 901-element roman span `C … M`. Stepping works too:
`'I'~'X':2` → `I, III, V, VII, IX`. `~` is the separator to reach for whenever a
range type would otherwise collide with Perl's literal (char / numeric) ranges;
non-colliding types (dates, IPs, hex) infer under either separator.

**Eager `:` vs lazy `range()` — both valid, different semantics.** The colon
range is **eager**: `1:N` materializes the whole list at once. The `range()`
builtin is the **lazy** counterpart: `range(start, end[, step])` yields values
on demand, so `range(1, 1e9) |> take 3` is instant and constant-memory where
`(1:1e9) |> take 3` builds a billion-element list first. They are two range
forms with different evaluation, not a right/wrong pair — reach for `range()`
when you want laziness (very large or streamed spans, early `take`, pipeline
chains) and `:` for ordinary eager spans.

---

## 5. Length / count — `len` always

`len` is the **one** stryke verb for length/count. It works on strings, arrays, hashes, sets, iterators, and anything else with a measurable size. Never reach for `length`, `scalar`, or `0 +` numeric coercion — they're all Perl-isms with separate type-specific quirks that `len` collapses into one bareword.

| DO | DON'T | Why |
|---|---|---|
| `len($s)` | `length($s)` | `length` is Perl 5; `len` is three chars and unifies with array/hash forms. |
| `len(@arr)` | `scalar(@arr)` | `scalar` is a context-coercion verb, not a length verb — wrong tool. |
| `len(keys %h)` | `scalar(keys %h)`, `0 + keys %h` | Same call shape, no implicit-numeric tricks. |
| `len(%h)` | `scalar(keys %h)` | Hash → key count directly. |
| `len($iter)` | manually drain the iterator and count | Lazy iterator's known-size case is one call. |
| `unpack_first(...)` | `scalar(unpack(...))` | Use the explicit non-list builtin instead of forcing scalar context for first-element. |

Aliases: `cnt`, `count` — same builtin. Use `len` by default; `cnt`/`count` are accepted in code where "count" reads more naturally (`pgrep { … } |> cnt`).

**Both `length` and `scalar` are rejected by `--no-interop`.** There's a stryke replacement for every Perl 5 use of either:

| Perl 5 | stryke |
|---|---|
| `length $s` | `len($s)` |
| `scalar @arr` | `len(@arr)` |
| `scalar keys %h` | `len(keys %h)` or `len(%h)` |
| `scalar(unpack ...)` | `unpack_first(...)` |
| `scalar(@arr) > 0` | `len(@arr) > 0`, or just `@arr` in boolean context |
| `0 + @arr` | `len(@arr)` |

---

## 6. Reverse / rev

| DO | DON'T | Why |
|---|---|---|
| `rev @xs` | `reverse @xs` | Three chars vs seven. |
| `rev "abc"` | `reverse "abc"` | Same; works on scalars too. |

---

## 6a. String / array slicing — `[N:M]` not `substr`

stryke uses bracket-indexing with the `:` range operator for substring extraction. **Never call `substr` on a stryke value.** `[]` slicing is one syntactic form for both strings and arrays.

| DO | DON'T | Why |
|---|---|---|
| `$s[1]` | `substr($s, 1, 1)` | Single-char index. |
| `$s[1:3]` | `substr($s, 1, 3)` | Slice from index 1 through index 3 **inclusive**. |
| `$s[-1]` | `substr($s, -1, 1)` | Last char. Negative indexing built in. |
| `$s[-3:-1]` | `substr($s, -3, 3)` | Last three chars. |
| `@arr[2:5]` | `@arr[2..5]` | Array slicing uses the same `:` range op as everywhere else. |

**Important semantic difference vs Perl `substr`:** stryke's `[N:M]` is **inclusive on both ends**. `$s[1:3]` returns 3 chars (indices 1, 2, 3). Perl's `substr($s, 1, 3)` returns 3 chars starting at index 1 (length-based). Same result for that example, but the mental model is different and they diverge for any range that crosses an end:

```stryke
my $s = "hello"
p $s[1:3]              # "ell"  — chars 1, 2, 3 inclusive
p substr($s, 1, 3)     # "ell"  — start=1, len=3 (Perl-style; works in compat mode)
p $s[1:99]             # "ello" — clamps to end gracefully; Perl would warn
```

**Open-ended slices** are supported,  so `$s[2:]` reads as "from index 2 to end" and `$s[0:N]` reads as "from start to N":

```stryke
my $s = "hello"
p $s[2:]              # "llo"  — index 2 through last
p $s[0:2]               # "hel"  — start through index 2
p $s[-3:]             # "llo"  — last 3 chars
```

For arrays, the open-ended bracket forms also work directly:

```stryke
my @a = (10, 20, 30, 40, 50)
p @a[2:]                # 30, 40, 50  — from index 2 to end
p @a[:2]                # 10, 20, 30  — start through index 2
```

---

## 6b. No nested calls — pipeline everything

Inside-out reading is the single biggest readability tax in mainstream languages. stryke's `|>` and `~>` collapse the indirection: every transformation reads top-to-bottom, left-to-right, in the order it executes.

| DO | DON'T | Why |
|---|---|---|
| `$x \|> trim \|> uc \|> first_word` | `first_word(uc(trim($x)))` | Pipeline reads in execution order. |
| `@xs \|> sort \|> rev \|> head(3)` | `head(rev(sort(@xs)), 3)` | The `head` arg-position requires recoding when `head` becomes `nth`. Pipeline is rewrite-stable. |
| `$json \|> from_json \|> ->{ users } \|> maps { _->{name} }` | `[map { $_->{name} } @{(from_json($json))->{users}}]` | Six chars per stage. Each stage standalone debuggable. |
| `$h ~> $key ~> $default` (Racket-style fallback) | `defined $h->{$key} ? $h->{$key} : $default` | `~>` short-circuits on undef. |

**Rule:** if you wrote two open parens in a row (`f(g(`), refactor to a pipeline. The only exception is constructor-style call sites where the arg is a literal struct (`Point(1, 2)`).

The threading-operator family is stryke's universal-access protocol — every value, every callable, and every reflection-metadata access flows through `~>` `~p>` `~p>` `~s>` `~d>` without adapter code.

---

## 7. Pipeline / threading operators

stryke ships distinct threading operators. Each has its own syntactic shape and execution semantics — they are not interchangeable.

### 7.1 `|>` — pipe-forward (one stage at a time)

Single-stage pipe. LHS becomes the **first** argument of the RHS callable. Stages are separated by `|>`. Cheap, zero-runtime-cost desugaring.

```stryke
"hello" |> uc |> rev |> p              # OLLEH
1:10 |> grep _ > 5 |> map _ * 2 |> e p
$url |> fetch_json |> json_jq '.name' |> p
"hello world" |> s/world/perl/ |> p     # hello perl
```

Use `|>` when each stage is a single function call and you want the most explicit, line-by-line readable form. It's the day-to-day default.

### 7.2 `~>` — thread-first macro (Racket / Clojure `->`)

Multi-stage thread. Source goes immediately after `~>`; stages are **space-separated** (not arrow-separated). The threaded value is injected as the **first** argument of each stage. `|>` terminates the macro and drops back to regular pipe-forward.

```stryke
~> "  hello  " tm uc rv |> p           # "OLLEH" (trim → uc → reverse)
~> @data grep { _ > 5 } map { _ * 2 } sort { _0 <=> _1 } |> p
~> 10 div(2) p                         # bare-fn stage: div(10, 2)
~> 10 div(_, 2) p                      # explicit `_` placeholder: div(10, 2)
```

Stages can be: bare function name, `name(args)` with `_` placeholder, `>{}` anonymous block, or `map { … }` / `grep { … }` / `sort { … }`. Spellings: `~>`, `thread`, `t`.

Use `~>` when you have ≥3 stages and want to drop one operator-character per stage; the macro form pays for itself at three calls.

### 7.3 `->>` / `~>>` — thread-last (Clojure `->>`)

Same syntax as `~>`, but the threaded value goes in as the **last** argument — the right shape for list-consuming functions like `map`/`grep`/`reduce` whose Perl-tradition arity is `(block, list)`.

```stryke
fn div = _0 / _1
->> 10 div(2) p                        # div(2, 10) = 0.2  (10 → last arg)
->> 10 div(_, 2) p                     # explicit `_` overrides: div(10, 2) = 5

~>> (1,2,3,4,5) take(3) p              # take(3, list) = (1,2,3)
```

Use thread-last when the callee's natural last arg is your data — saves wrapping `_` placeholders.

### 7.4 `~s>` — per-item streaming pipeline

Each stage runs as an **independent worker thread** connected to the next via a bounded channel. Items flow through concurrently; intermediate arrays are never materialized.

```stryke
~s> [1, 2, 3, 4, 5] map { _ * 10 } map { _ + 1 }
~s> @lines map { json_decode } grep { _->{ok} } map { _->{val} }
~s> gen { yield_lines "huge.log" } map { parse } grep { /ERROR/ }
```

Use `~s>` when stages have **uneven latencies** (I/O + CPU mix) — fast stages can pull ahead while slow ones catch up. Output order is non-deterministic. `~s>>` is the thread-last variant. The macro's value is the count of items the last stage emitted.

### 7.5 `~p>` — chunk-parallel pipeline

Splits the input into chunks, runs the **whole pipeline on each chunk** in parallel via rayon, auto-merges the per-chunk results. Equivalent to `par_reduce { stage1 |> stage2 |> ... } SRC`.

```stryke
~p> c("**.rs") letters freq                 # histogram across all .rs files
~p> @big map { _ * 2 } sum                  # parallel map+sum, auto-summed across chunks
~p> $huge letters freq ||> values |> sum    # ||> drops back to sequential for the merged whole
~p> $huge letters freq |then| values |> sum # english-spelled equivalent of ||>
```

Auto-merger picks strategy from the first chunk's type: hash-of-numbers merges key-wise with `+` (canonical histogram), numeric scalars sum, arrays concat, strings concat. Use `||>` or `|then|` to switch to sequential `~>` mode for stages that need the merged whole. `~p>>` is the thread-last variant.

Use `~p>` when each chunk is independent CPU work and you want all-cores utilization without writing `pmap`/`par_reduce` by hand.

### 7.6 Choosing between them

| Scenario | Pick |
|---|---|
| 1–2 stages, mostly readable as-is | `\|>` |
| 3+ stages, no I/O, one-character-per-stage cost matters | `~>` |
| Last-arg-is-the-data style (Perl-tradition `map BLOCK LIST`) | `->>` / `~>>` |
| I/O-heavy stages with uneven latency, want pipeline parallelism | `~s>` |
| CPU-heavy stages on a big collection, want chunk parallelism | `~p>` |

### 7.7 Stage notation forms

Inside any `~>` / `~>>` / `~s>` / `~p>` stage, four notations are accepted:

```stryke
~> $x  uc                  # bare fn: x → uc(x)
~> $x  div(_, 2)           # placeholder: x → div(x, 2)
~> $x  >{ _ * 2 + 1 }      # arrow-block: x → (x*2+1)
~> @xs map { _ * 10 } sum  # block-arg form: map { … } @xs → sum(...)
```

Mix freely. The placeholder `_` is always available and overrides the implicit position when present.

---

## 8. Iteration shorthand

| DO | DON'T | Why |
|---|---|---|
| `@xs \|> e p` | `for my $x (@xs) { p $x }` | `e` is each-call; `e p` = call `p` on each element. |
| `@xs \|> ep` | `@xs \|> e p` (when chained from a pipeline) | `ep` is shorthand for `e { p }` — each-then-print. **Equivalent to `e p`, both go to stdout.** |
| `pmaps { … } @xs` | `[pmap { … } @xs]` then iterate | `pmaps` is the streaming parallel iter — lazy, no intermediate vec. |
| `pgreps { … } @xs` | `[pgrep { … } @xs]` | Streaming parallel filter; same reason. |
| `flat_maps`, `pflat_maps` | `flat_map { … } \|> e` | Stream variant; no list materialization. |

Streaming variants (`pmaps`, `pgreps`, `pflat_maps`, `maps`, `greps`, `flat_maps`) are the default when chaining; the array-returning forms (`pmap`, `pgrep`, `flat_map`) are only correct when you genuinely need a vec.

---

## 9. File I/O

| DO | DON'T | Why |
|---|---|---|
| `c"path"` | `slurp("path")`, `do { open my $fh, "<", "path"; local $/; <$fh> }` | `c"…"` is slurp-as-quote-prefix. Two extra chars over the path itself. |
| `spurt $path, $data` | `open my $fh, ">", $path; print $fh $data; close $fh` | One builtin call. |
| `lines("path")` | `open … while <$fh>` | Lazy line iterator. |
| `glob("*.stk")` | `<*.stk>` | `<>` is reserved for stdin; use `glob` explicitly. |

---

## 10. Parallelism

| DO | DON'T | Why |
|---|---|---|
| `pmaps { f(_) } @xs` | spawn threads manually | `pmaps` rides rayon work-stealing; correct + faster. |
| `varsync $count = 0` | shared mutex by hand | `varsync` and `oursync` are the stryke-shared-state primitive; lockless reads, lock-on-write. |
| `pchannel` | hand-rolled mpsc | Built-in unbounded channel with select. |
| `barrier($n)` | hand-rolled CountdownLatch | One call. |

When calling parallel operations from `pmaps` workers, prefer per-thread idioms (`set_no_interop_mode_tls`) over global atomics — sibling workers don't race.

---

## 11. Sort / reduce blocks

`$a` and `$b` are **only valid inside** sort/reduce comparator blocks (the Perl-ism). Don't reference them anywhere else, and don't shadow them with `my $a` / `my $b` in regular code.

| DO | DON'T | Why |
|---|---|---|
| `sort { _0 cmp _1 } @xs` | `sort { $a cmp $b } @xs` | `_0`/`_1` is the universal positional protocol; `$a`/`$b` is comparator-block-only magic. |
| `reduce { _0 + _1 } @xs` | `reduce { $a + $b } @xs` | Same. |

stryke accepts the `$a`/`$b` form for Perl 5 compat but `--no-interop` rejects it.

---

## 12. Implicit closure params

stryke's calling card. Never write named params for one-liner closures. Three syntactic homes, each with its own bracket form — pick by where the closure lives.

### Block form `{ … }` — for `map` / `grep` / `sort` / `reduce` and `|>` pipelines

The plain block is what list builtins accept. Use it when the closure is the block-arg of a builtin or any stage of a pipe-forward `|>` chain.

| DO | DON'T | Why |
|---|---|---|
| `map { _ * 2 } @xs` | `map { $_ * 2 } @xs` | `_` is the bare topic; `$_` is the Perl-ism (rejected by `--no-interop`). |
| `grep { _ > 5 } @xs` | `grep { $_ > 5 } @xs` | Same. |
| `sort { _0 <=> _1 } @xs` | `sort { $a <=> $b } @xs` | Universal positional, not sort-block magic. |
| `@xs \|> maps { _ * 2 }` | `@xs \|> maps >{ _ * 2 }` | **`\|>` uses plain `{ }`, not `>{ }`.** `>{ }` is reserved for `~>` thread macros. |

### Arrow-block `>{ … }` — **only inside `~>` / `~>>` / `~s>` / `~p>` thread macros**

`>{ }` is a syntactic primitive *of* the thread-macro family — it marks an anonymous-fn stage in `~>` chains. Outside threading it's a parse error ("Not a code reference").

| DO | DON'T | Why |
|---|---|---|
| `~> @xs >{ _ * 2 } sum` | `~> @xs { _ * 2 } sum` | `~>` stages need either `name`, `name(args)`, `>{...}`, or `name { … }`. A bare `{}` doesn't parse as a stage. |
| `~> @xs map { _ * 2 } sum` | always-`>{}` style | `map { … }` is the block-arg stage form, equally valid in `~>`. Pick `>{}` for standalone closures, `name { … }` when calling a builtin. |
| `val $f = fn { _ * 2 }` | `my $f = >{ _ * 2 }` | **Arrow-block is not a value.** Bare `>{ }` outside threading errors with "Not a code reference". |

### `fn { … }` / `fn name = …` — anonymous and named function values

For storing a closure in a variable or returning one, use `fn`. Anonymous `fn { … }` accepts the topic protocol exactly like the block forms above.

| DO | DON'T | Why |
|---|---|---|
| `val $f = fn { _ * 2 }` | `my $f = sub { $_[0] * 2 }` | `fn` opens the topic-var protocol; `sub` doesn't. |
| `val $f = fn { _ * 2 }` | `my $f = >{ _ * 2 }` | `>{ }` is threading-only — error outside `~>`. |
| `fn double = _ * 2` (top-level) | `fn double { return _ * 2 }` | Expression-bodied named fn (rule 6) + implicit return (rule 7). |

### Outer chain `_<` — one `<` per closure level

`_<` reaches the topic of the *enclosing* closure; chain `<`s for deeper. Most useful in nested `~>` where each stage opens a fresh topic frame.

```stryke
# Multiply each element by its row index across nested rows
~> @rows enumerate map { ~> _0 map { _ * _<< } }   # _<< = outer-outer topic

# Capture-style: explicit `my` works but is the verbose form
my $outer = …
@xs |> maps { _ + $outer }    # explicit lexical capture
@xs |> maps { _ + _< }        # `_<` reads cleaner once internalized
```

### Quick reference — which bracket form?

| Context | Form |
|---|---|
| `map`/`grep`/`sort`/`reduce` block-arg | `{ … }` |
| Stage in `\|>` pipeline (after a builtin that takes a block) | `{ … }` |
| Stage in `~>` / `~>>` / `~s>` / `~p>` thread macro | `{ … }` (block-arg) **or** `>{ … }` (arrow-block) |
| Stored in a variable (anon fn) | `fn { … }` |
| Top-level named function | `fn name = …` (one-liner) or `fn name { … }` (multi-stmt) |

---

## 12b. No semicolons at end of line

stryke statement-terminates on newline. Trailing `;` is noise that adds a character per line and visually weights every statement equally with no payoff.

| DO | DON'T | Why |
|---|---|---|
| `val $x = 1` | `my $x = 1;` | Newline already terminates; `val` over bare `my` (rule 10). |
| `p $x` | `p $x;` | Same. |
| `for val $i (1:10) { p $i }` | `for my $i (1..10) { p $i; }` | Both sides; range too. |

**The only legitimate `;`** is **separating statements on the same line** — almost always inside a C-style `for` header:

```stryke
for (my $i = 0; $i < 10; $i++) { … }
```

Or, very rarely, a one-liner with two statements that genuinely belong glued:

```stryke
$count++; last if $count > 100      # acceptable
```

Even that is usually wrong — split to two lines unless brevity is the actual goal. **`use strict` / `--no-interop` will not flag trailing `;`** (legal Perl 5), but new stryke code that ships with them is wrong on style and will fail review.

---

## 13. Multi-line expressions

stryke terminates statements at newline (Perl/shell-like), not at `;`. Multi-line operators silently produce wrong values without explicit continuation.

| DO | DON'T | Why |
|---|---|---|
| `$x = a()<br>&nbsp;&nbsp;\|> b()<br>&nbsp;&nbsp;\|> c()` | use `\`<br>at the end of each line | Pipelines auto-continue across newlines when starting with `\|>` or `~>`. |
| `$x = $a + \\<br>&nbsp;&nbsp;$b * \\<br>&nbsp;&nbsp;$c` | `$x = $a + $b * $c` (one line if it fits) | Use `\` for binary-op-mid-expression continuation. |

If a pipeline starts a fresh line with `|>` or `~>`, the parser auto-extends the previous statement. Otherwise newlines terminate.

---

## 14. Hash declarations

For hashes with **more than 3 keys**, one `key => value` pair per line. Makes block editing (visual select, delete, reorder) cheap.

```stryke
my %config = (
    host    => "localhost",
    port    => 8080,
    timeout => 30,
    retries => 3,
)
```

Short hashes (≤3 keys) can stay on one line:

```stryke
my %sm = (a => 1, b => 2)
```

---

## 15. No defensive code

| DO | DON'T | Why |
|---|---|---|
| `val $sum = sum(@xs)` | `my $sum = @xs ? sum(@xs) : 0` | Trust the builtin's empty-list contract. |
| `die "bad input" if $n < 0` | check + return undef + caller checks for undef | `die` is cheap; failure paths are explicit. |
| direct call | guard with `if defined $obj && ref $obj && $obj->can('method')` | Trust internal code. Validate at boundaries (user input, network), nowhere else. |

Don't add `my $foo` declarations inside loops just to be defensive — declare once at function top, assign inside the loop. (zsh-rule, but applies to stryke too: re-declaring `my` in a loop is wasteful and may surface as warnings under future strictness.)

---

## 16. Test runner

stryke's `test()` builtin and `s t` CLI default to the **worker pool** — pre-forked persistent stryke processes that fork-on-receive per test. Don't roll your own.

| DO | DON'T | Why |
|---|---|---|
| `s t t/` | `for f in t/*.stk; do stryke $f; done` | Pool reuses warm stryke; ~5–7× faster than the shell loop. |
| `test("t/")` from a stryke script | `system "stryke test t/"` | Builtin avoids the outer fork. |
| `test_no_interop("t/")` | `system "stryke --no-interop test t/"` | Per-thread TLS no-interop, race-free. |
| `s t -j 1 t/foo.stk` to debug | guess-and-check at full parallelism | Serial mode pinpoints which file hangs. |

See [Test Runner — Worker Pool Architecture](../README.md#0x0c-test-test-runner--worker-pool-architecture) for the topology and wire protocol.

---

## 17. Strict / no-interop modes

stryke does **not** enforce `use strict;` by default — that's a Perl-ism. Topic vars (`_N`, `$_N`) and special vars (`@ARGV`, `%ENV`, …) are **always** exempt regardless.

- **`use strict;`** — opt-in Perl-style undefined-var enforcement. Use only in modules you're porting from CPAN.
- **`--no-interop`** — bot firewall. Rejects every entry in this guide's DON'T column at parse time. Use in CI for new stryke code.
- **`--compat`** — locks stryke to stock Perl 5 syntax/builtins. Use for parity testing only.

---

## 18. Naming

| Convention | Where | Example |
|---|---|---|
| `snake_case` | function names, variables | `fn parse_config`, `$line_count` |
| `Module::Sub::name` | namespaced functions | `fn Rosetta::Strings::reverse_words` |
| `SCREAMING_SNAKE` | true constants | `const MAX_RETRIES = 3` |
| `CamelCase` | class / struct / enum names | `class HttpClient`, `enum Color` |
| short single-letter | only as block topic / loop index | `for $i (1:10)`, `{ _ }` |

**Never** name a function after a builtin. Always namespace if there's any chance of clash. Run `s 'p $stryke::all{your_name} // "free"'` to check.

---

## 19. When to break the rules

Three legitimate reasons:

1. **Porting CPAN code where line-by-line parity matters** — keep the original spelling, add `# perl-port` comment, don't mix idioms.
2. **Code that already exists** — don't refactor neighboring style mid-PR. Match the file's existing convention; queue the migration as a separate change.
3. **Performance critical paths verified by benchmark** — if `_<<` resolution shows up hot in a profile, switching to explicit lexical capture is fine. Rarely needed; profile first.

Anywhere else: stryke idioms win. The DON'T column is for compat, not for new code.
