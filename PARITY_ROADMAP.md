# Perl 5 parity roadmap (perlrs)

This is an **ordered engineering program**, not a promise of bit-identical `perl` for every program. Each phase has a **definition of done** so work can land in sequence without scope creep.

---

## Phase 0 — Measure (baseline)

**Goal:** Stop guessing; every later change is justified by a failing parity case or a new `parity/cases/*.pl` file.

**Done when:**

- `parity/run_parity.sh` runs `perl` and `pe` on the same scripts and compares **combined stdout+stderr** (exact bytes, `LC_ALL=C`).
- `parity/cases/` holds at least a small **seed** set that passes today; new cases are added **whenever** a bug is fixed or a Perl edge case is claimed.

**Run:** `bash parity/run_parity.sh` (from repo root; builds `target/release/pe` if missing).

---

## Completed slices (reference)

- **Typeglob assignment** — `*foo = \&bar` and `*foo = *bar` copy subroutine entries (`Interpreter::subs`) plus scalar/array/hash slots and `glob_handle_alias` (`Interpreter::copy_typeglob_slots`). Tree interpreter only until bytecode supports assigning to `ExprKind::Typeglob`. Lexer: `y` after `::` is not forced into `tr`/`y` when followed by `;`, `=`, etc., so names like `Foo::y` tokenize as identifiers.
- **`__FILE__` / `__LINE__`** — compile-time literals; bytecode uses `Compiler::source_file` (wired from `Interpreter::file` in `try_vm_execute`). Covered by `parity/cases/006_magic_line.pl` and integration tests.
- **`$!` (errno dualvar)** — numeric errno + string message (`PerlValue::errno_dual` / `ErrnoDual`); I/O paths set `errno` / `errno_code`; assignment to `$!` updates both (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)). Parity cases for errno-heavy paths still welcome.
- **`$@` (eval dualvar)** — `eval_error` + `eval_error_code` with the same heap representation as **`$!`**; `eval` / `evalblock` use `set_eval_error` / `clear_eval_error` (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)).
- **`%SIG` (Unix)** — `SIGINT` / `SIGTERM` / `SIGALRM` / `SIGCHLD` invoke `%SIG{…}` code refs **between statements** via [`src/perl_signal.rs`](src/perl_signal.rs). Subprocess / controlled parity cases still welcome.
- **`${^GLOBAL_PHASE}`** — tree-walker [`execute_tree`](src/interpreter.rs) and bytecode [`compile_program`](src/compiler.rs) both drive **`Interpreter::global_phase`** to match Perl 5 (including **`START`** during **`UNITCHECK`** blocks; VM emits **`Op::SetGlobalPhase`**). Parity: [`parity/cases/007_global_phase.pl`](parity/cases/007_global_phase.pl), [`parity/cases/009_global_phase_all.pl`](parity/cases/009_global_phase_all.pl). **`DESTRUCT`** is not modeled.
- **Lexer `${^NAME}`** — `${…}` after **`$`** is matched before the single-character special branch so **`${^GLOBAL_PHASE}`** tokenizes as one scalar (not **`$` `{`**).
- **String `\x{hex}`** — double-quoted / `qq` escapes: braced Unicode scalars and unbraced one- or two-digit hex (Perl-like). Parity: [`parity/cases/010_string_hex_escape.pl`](parity/cases/010_string_hex_escape.pl).
- **`$@` (eval/die)** — `die` / `warn` append **` at FILE line N.`** (trailing period before newline) when the message does not already end with newline, matching Perl 5’s **`$@`** for `eval { die }`. Parity: [`parity/cases/008_eval_at.pl`](parity/cases/008_eval_at.pl).

## Phase 1 — Documented runtime gaps (specials, I/O, signals)

**Goal:** Close items in [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md) and related README bullets in a **test-driven** way.

**Rough order (dependencies matter):**

1. **`$@`** — extend **parity** for **`eval`** / **`die`** edge cases still diverging from Perl 5 (e.g. string-`eval` errors, `eval` in VM-only paths).
2. **`%SIG`** — extend coverage (more signals, Windows behavior if desired); add parity cases for `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` in a controlled subprocess.
3. **`$.` / per-handle line counters** — align with Perl where feasible; add file-reading cases.
4. **`${^GLOBAL_PHASE}`** — extend parity cases for **`UNITCHECK` / `CHECK` / `INIT`** if needed; **`DESTRUCT`** when modeled.

**Done when:** Each item has parity cases (or explicit `SKIP` in `perl` with a comment in the case file explaining why Perl differs).

---

## Phase 2 — VM / bytecode coverage (tree-only features)

**Goal:** Reduce `execute_tree` fallback for hot paths.

**Targets:** `typed my`, `each`, complex lvalues, and any construct currently marked `Unsupported` in [`src/compiler.rs`](src/compiler.rs) where semantics are stable. **Done (VM + tree):** `try`/`catch`/`finally`; `given`/`when`/`default` via [`Op::Given`](src/bytecode.rs) (body still interpreted); [`Op::EvalTimeout`](src/bytecode.rs); algebraic `match` via [`Op::AlgebraicMatch`](src/bytecode.rs).

**Progress (non-exhaustive):** `do { } while (COND)` is parsed as [`StmtKind::DoWhile`](src/ast.rs) and compiled to the bytecode VM; `splice` / `unshift` on plain `@array` compile to `CallBuiltin` with real mutating implementations (see [`Interpreter::splice_builtin_execute`](src/interpreter.rs)).

**Done when:** Compiler emits bytecode for a subset; **existing** integration tests still pass; new parity cases cover **before/after** behavior.

---

## Phase 3 — Regular expressions

**Goal:** Either **document** divergence from Perl 5’s engine or **narrow** it with a deliberate strategy.

**Progress:** After expanding Perl `\Q…\E` / flags, compilation tries [`regex`](https://docs.rs/regex) first, then [`fancy-regex`](https://docs.rs/fancy-regex) on failure (e.g. backreferences), then **PCRE2** ([`pcre2`](https://docs.rs/pcre2)) when both Rust engines reject the pattern. See [`src/perl_regex.rs`](src/perl_regex.rs). This is still **not** full PCRE/Perl parity; it removes a large class of “invalid regex” hard failures.

**Done when:** `parity/cases/` includes regex patterns that matter to real scripts; failures drive a written **compatibility matrix** (not vibes).

---

## Phase 4 — `require` / `use` / pure-Perl core

**Goal:** Run more **pure-Perl** modules from `@INC` without silent wrong behavior.

**Progress:** `require` / `use` / `%INC` / Exporter-style import are implemented; [`src/perl_inc.rs`](src/perl_inc.rs) can merge system `@INC` from a real `perl`. The **Top-N CPAN smoke** harness lives under [`parity/cpan_topn/`](parity/cpan_topn/README.md): list in [`parity/cpan_topn/MODULES.txt`](parity/cpan_topn/MODULES.txt), install via `install_deps.sh`, run via `run_cpan_topn.sh` under **`pe`** with **`pe -I …/local/lib/perl5`**. `use VERSION` (e.g. `use 5.008;`) is accepted as a no-op so common CPAN headers parse. **`use constant NAME => EXPR`** is handled natively in [`Interpreter::apply_use_constant`](src/interpreter.rs) (installs a nullary sub per pair; does **not** load core `constant.pm`, which uses syntax we do not parse yet). Unquoted expression terms are [`ExprKind::Bareword`](src/ast.rs) (resolved to a nullary sub if one exists, else stringified like a Perl bareword); quoted `'…'` / `"…"` literals stay [`ExprKind::String`](src/ast.rs). Fat-arrow hash keys (`foo => …`) still parse as `String`. **`join`’s** list operands evaluate with list `wantarray` so `join(",", List::Util::uniq(...))` passes list context into `uniq`. Lexer accepts **`$::{$key}`** (main stash / `%::`) for core `Carp.pm` / introspection. Parser accepts **`*$_{…}`** / **`*$scalar`** (typeglob from a sigil expression) as [`TypeglobExpr`](src/ast.rs). Anonymous subs accept an optional prototype before the block: **`sub () { … }`** (shared `parse_sub_prototype_opt` with named subs) so assignments like **`*UTF8_REGEXP_PROBLEM = sub () { 1 }`** in **`Carp.pm`** parse. For **JSON::PP**, put **`vendor/perl` before system paths in `-I`** so the minimal [`vendor/perl/Carp.pm`](vendor/perl/Carp.pm) shadows core Carp; full **`require JSON::PP`** still needs **`eval qq/…/`** string-eval + **`qq` interpolation** to match Perl-generated subs (tracked as the next gap).
**Recent parser/runtime for `require` chains:** chained hash subscripts on deref results (`$x->{a}{b}` / `$h{k}{k2}`); `my`/`our` declaration with postfix `if`/`unless`; `sub name : attr { }` attribute skipping; statement labels as `LABEL:` at statement start only (not lexer `ALLCAPS:` inside `?:`); `our $x ||= 0` / `//=`; `\&{ EXPR }` (dynamic subroutine coderef) and `*{ EXPR }` (dynamic typeglob slot) for **Exporter.pm**-style code; `%$href` (hash deref of scalar ref); symbolic `@{ EXPR }`; **`local $h{k} = …`** (hash slot localization, e.g. **`local $SIG{__WARN__}`** in system **Exporter.pm**) — tree interpreter; bytecode falls back.

**Done when:** `parity/cpan_topn` smoke is green in CI; **XS** modules either work via a bridge (Phase 6) or fail with a **clear** error.

---

## Phase 5 — `format` / `write` reports

**Goal:** Implement enough of Perl’s report formatting to run scripts that use `format`/`write`.

**Done when:** Parser + runtime + parity cases for picture lines and `write` (see [`src/format.rs`](src/format.rs)).

---

## Phase 6 — XS / embedding (only if required)

**Goal:** Run extensions that are not expressible in pure Perl.

**Options:** FFI to `libperl`, subprocess delegation to `perl`, or explicit “no XS” policy for this interpreter.

**Done when:** Policy is chosen and documented; one reference XS module is either supported or explicitly out of scope.

---

## Maintenance rule

- Any PR that claims “Perl compatibility” for a feature must add or extend **`parity/cases/`** (or `cargo test` coverage that compares to `perl` where applicable).
- Update this file when a **phase completes** or **scope changes**.
