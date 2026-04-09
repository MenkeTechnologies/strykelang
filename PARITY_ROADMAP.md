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

## Phase 1 — Documented runtime gaps (specials, I/O, signals)

**Goal:** Close items in [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md) and related README bullets in a **test-driven** way.

**Rough order (dependencies matter):**

1. **`$!` / `$@`** — dualvar semantics (numeric + string) where Perl exposes both; add parity cases for errno paths.
2. **`%SIG`** — deliver OS signals into Perl subs **between ops** (documented behavior in README); add cases for `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` in a controlled subprocess.
3. **`$.` / per-handle line counters** — align with Perl where feasible; add file-reading cases.
4. **`${^GLOBAL_PHASE}`** — real phase transitions vs a static string.

**Done when:** Each item has parity cases (or explicit `SKIP` in `perl` with a comment in the case file explaining why Perl differs).

---

## Phase 2 — VM / bytecode coverage (tree-only features)

**Goal:** Reduce `execute_tree` fallback for hot paths.

**Targets:** `try`/`catch`/`finally`, `given`/`when`, algebraic `match`, `eval_timeout`, `typed my`, and any construct currently marked `Unsupported` in [`src/compiler.rs`](src/compiler.rs) where semantics are stable.

**Done when:** Compiler emits bytecode for a subset; **existing** integration tests still pass; new parity cases cover **before/after** behavior.

---

## Phase 3 — Regular expressions

**Goal:** Either **document** divergence from Perl 5’s engine (regex crate vs Perl) or **narrow** it with a deliberate strategy (subset docs, alternative engine, or explicit “Perl-compatible mode” for patterns).

**Done when:** `parity/cases/` includes regex patterns that matter to real scripts; failures drive a written **compatibility matrix** (not vibes).

---

## Phase 4 — `require` / `use` / pure-Perl core

**Goal:** Run more **pure-Perl** modules from `@INC` without silent wrong behavior.

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
