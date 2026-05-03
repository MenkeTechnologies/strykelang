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
- **Major decisions** (new language semantics, breaking compatibility
  changes, removal of existing features, top-level architectural
  pivots, license changes, governance changes to this document)
  require final approval from the creator before landing on the
  official upstream. The maintainer team owns the proposal,
  development, and review; the creator's role is a yes/no on the
  final shape.
- The maintainer team may proceed without creator involvement on any
  decision the creator declines to engage with within a reasonable
  window (default: 30 days from formal proposal).
- **Forks are unrestricted.** The MIT license guarantees the right to
  fork the project at any time. This governance applies only to the
  official upstream `strykelang` repository; forks are free to set
  their own governance and proceed without creator approval on any
  change.
