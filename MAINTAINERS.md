# Maintainers

Day-to-day maintenance, issue triage, release engineering, and operational
governance for stryke.

This file describes the project's *current* governance and contribution
flow. For original authorship and the historical record, see
[CREATORS.md](CREATORS.md). Day-to-day authority rests with the
maintainer team named here. The creator retains final approval on
major decisions to the official upstream (see Governance below);
the MIT license guarantees the right to fork at any time without
permission, in which case this governance does not apply.

## Current Maintainers

_To be populated when the maintainer team is assembled._

| Handle | Role | Areas |
|---|---|---|
| (TBD) | Lead maintainer | release management, CI |
| (TBD) | Builtins / runtime | `builtins.rs`, `interpreter.rs`, `vm.rs` |
| (TBD) | Parser / compiler | `parser.rs`, `compiler.rs`, `ast.rs` |
| (TBD) | Parallel / cluster | `par_*`, `cluster.rs`, `agent.rs`, `controller.rs` |
| (TBD) | LSP / docs | `lsp.rs`, `docs/` |

## Responsibilities

- **Issue triage** — labeling, reproducing, routing to the right area.
- **Pull-request review** — first review on incoming PRs; merging once
  reviewed and CI green.
- **Release engineering** — versioning, tagging, publishing to crates.io.
- **CI / build hygiene** — keeping `cargo test` / `cargo build` green
  across macOS aarch64, Linux x86_64, Linux aarch64.
- **Compatibility** — preserving the Perl 5 spec floor and the
  `--compat` / `--no-interop` boundaries; no silent breakage of
  existing scripts.

Direction-setting (new language features, new primitives, new
subsystems, breaking design changes) is proposed and developed
by the maintainer team — typically through an RFC process —
and submitted to the creator for final approval before landing
on the official upstream. Forks are not bound by this step.

## Contributing

Run `cargo test --bin stryke` before opening a PR. CI runs the full
suite (`cargo test`, the parity harness, and the Rosetta corpus) on
every push. PRs that touch the dispatch table must include
`refresh_stashes`-style category metadata so reflection stays in sync
(see `build.rs` and `tests/suite/reflection.rs`).

## Governance

- Maintainers are added and removed by consensus of existing maintainers.
- **Operational decisions** (CI policy, issue labels, release cadence,
  patch releases, bug fixes, new builtins that follow existing patterns,
  performance work that preserves semantics, docs, tests) are
  maintainer-only — no creator approval needed.
- **Major decisions that touch a core invariant** (see below) are
  reviewed with the creator before landing on the official upstream.
  The maintainer team owns the proposal, development, and review;
  the creator's role is a final yes/no on the shape. Decisions
  that don't touch any core invariant fall under operational and
  don't need creator review.
- The maintainer team may proceed without creator involvement on any
  decision the creator declines to engage with within a reasonable
  window (default: 30 days from formal proposal).
- **Forks are unrestricted.** The MIT license guarantees the right to
  fork the project at any time. This governance applies only to the
  official upstream `strykelang` repository; forks are free to set
  their own governance and proceed without creator approval on any
  change.

### Core invariants

These describe the load-bearing pieces of stryke's identity — what
makes stryke recognizable as stryke across decades and across
maintainer rotations. They reflect a generational lesson from past
language transitions: a language is most useful when it stays itself
long enough for its design to compound. Raku and Python 3 are
worked examples of how much momentum a from-scratch rewrite costs.

The invariants are not a fence against change — maintainers ship
features, opcodes, builtins, refactors, and perf work freely.
They mark the difference between *extending* the language and
*replacing* it. Changes to any of the following are reviewed
with the creator before landing on the official upstream:

1. **Perl 5 compatibility floor.** `--compat` must continue to run
   Perl 5 code. Dropping this turns stryke into a different
   language wearing the same name.
2. **Threading-operator family.** `|>`, `~>`, `->>`, and the
   implicit-positional closure parameters (`_0` / `_1` / `$_<<`)
   are stryke's syntactic calling card. They cannot be removed,
   renamed, or re-semanticized.
3. **Encyclopedic stdlib axis.** The "core encyclopedic, libraries
   unnecessary" design — 4,000+ callable spellings shipped in the
   binary — is the inverted-philosophy claim. A maintainer-driven
   move toward "core minimal, libraries optional" requires
   approval.
4. **NaN-boxed bytecode VM (fusevm).** Replacing the value
   representation or the VM substrate dissolves stryke's
   relationship with the surrounding compiled-shell stack.
5. **Sigils.** `$`, `@`, `%`, `&`. Removing them == becoming Ruby
   or Python in stryke clothes.
6. **Min-chars / power-user defaults.** The language defaults to
   terse forms (`p` not `print_line`, `_0` not `arg_zero`). Adding
   newbie-mode syntactic alternatives or verbose-default modes
   needs approval.
7. **Cranelift JIT path.** Removing JIT or downgrading hot-block
   native code emission to interpreter-only requires approval.
8. **`--no-interop` boundary.** The mode that enforces stryke
   idioms over Perl-isms must remain functional.
9. **Parallel-as-syntactic-primitive.** `pmap` / `pgrep` / `pfor`
   etc. live in core, not in a library. Demoting them to an
   optional crate requires approval.
10. **License (MIT).** Any change to or replacement of the
    license requires approval.

Maintainers can extend, optimize, document, refactor, and ship
new builtins / opcodes / subsystems freely. They can also
*propose* changes to invariants — but landing them on the official
upstream needs the creator's sign-off. Forks are free to redefine
or drop any of these.

**Stryke's design ideas are part of its contribution to the
language-design tradition.** Other languages — Perl, Ruby, Python,
Raku, zsh, fish, nushell, OCaml, anything — are explicitly invited
to absorb, adapt, or build on any stryke-originated design
(threading operators, implicit-positional closure params,
encyclopedic stdlib axis, AOP intercepts, AI primitives, etc.)
under the MIT license. The ideas living elsewhere is the goal.
Ports are asked to credit stryke as the invention source in their
docs (academic-grade attribution norm, not a license restriction).
See [CREATORS.md § Porting stryke ideas to other
languages](CREATORS.md#porting-stryke-ideas-to-other-languages)
for the attribution form.
