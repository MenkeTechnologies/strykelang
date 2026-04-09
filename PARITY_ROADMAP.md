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

- **`__FILE__` / `__LINE__`** — compile-time literals; bytecode uses `Compiler::source_file` (wired from `Interpreter::file` in `try_vm_execute`). Covered by `parity/cases/006_magic_line.pl` and integration tests.
- **`$!` (errno dualvar)** — numeric errno + string message (`PerlValue::errno_dual` / `ErrnoDual`); I/O paths set `errno` / `errno_code`; assignment to `$!` updates both (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)). Parity cases for errno-heavy paths still welcome.
- **`$@` (eval dualvar)** — `eval_error` + `eval_error_code` with the same heap representation as **`$!`**; `eval` / `evalblock` use `set_eval_error` / `clear_eval_error` (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)).
- **`%SIG` (Unix)** — `SIGINT` / `SIGTERM` / `SIGALRM` / `SIGCHLD` invoke `%SIG{…}` code refs **between statements** via [`src/perl_signal.rs`](src/perl_signal.rs). Subprocess / controlled parity cases still welcome.
- **`${^GLOBAL_PHASE}`** — tree-walker [`execute_tree`](src/interpreter.rs) sets **`START`** during **`BEGIN`**, **`RUN`** for the main program (after **`BEGIN`** when any exist), and **`END`** while **`END`** blocks run; each `execute_tree` resets to **`RUN`**. Parity: [`parity/cases/007_global_phase.pl`](parity/cases/007_global_phase.pl). **`CHECK` / `INIT` / `DESTRUCT`** are not modeled yet.
- **Lexer `${^NAME}`** — `${…}` after **`$`** is matched before the single-character special branch so **`${^GLOBAL_PHASE}`** tokenizes as one scalar (not **`$` `{`**).
- **`$@` (eval/die)** — `die` / `warn` append **` at FILE line N.`** (trailing period before newline) when the message does not already end with newline, matching Perl 5’s **`$@`** for `eval { die }`. Parity: [`parity/cases/008_eval_at.pl`](parity/cases/008_eval_at.pl).

## Phase 1 — Documented runtime gaps (specials, I/O, signals)

**Goal:** Close items in [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md) and related README bullets in a **test-driven** way.

**Rough order (dependencies matter):**

1. **`$@`** — extend **parity** for **`eval`** / **`die`** edge cases still diverging from Perl 5 (e.g. string-`eval` errors, `eval` in VM-only paths).
2. **`%SIG`** — extend coverage (more signals, Windows behavior if desired); add parity cases for `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` in a controlled subprocess.
3. **`$.` / per-handle line counters** — align with Perl where feasible; add file-reading cases.
4. **`${^GLOBAL_PHASE}`** — add **`CHECK` / `INIT`** (and **`DESTRUCT`** if modeled) when those constructs exist in the parser; add parity cases.

**Done when:** Each item has parity cases (or explicit `SKIP` in `perl` with a comment in the case file explaining why Perl differs).

---

## Phase 2 — VM / bytecode coverage (tree-only features)

**Goal:** Reduce `execute_tree` fallback for hot paths.

**Targets:** `try`/`catch`/`finally`, `given`/`when`, algebraic `match`, `eval_timeout`, `typed my`, `each`, complex lvalues, and any construct currently marked `Unsupported` in [`src/compiler.rs`](src/compiler.rs) where semantics are stable.

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

**Progress:** `require` / `use` / `%INC` / Exporter-style import are implemented; [`src/perl_inc.rs`](src/perl_inc.rs) can merge system `@INC` from a real `perl`. A curated “top N core `.pm`” harness is still **to do** (run under `cargo test` with explicit env, not hardcoded machine paths).

**Done when:** Chosen modules (list them in this file) load and pass their own tests or a curated subset; **XS** modules either work via a bridge (Phase 6) or fail with a **clear** error.

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
