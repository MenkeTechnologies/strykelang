# Maintainers

Day-to-day maintenance, issue triage, release engineering, and operational
governance for stryke.

This file describes the project's *current* governance and contribution
flow. For original authorship and the historical record, see
[CREATORS.md](CREATORS.md). Authority is fully with the maintainer team
named here — the project is independent of its creator and no
external party retains veto, direction-setting, or approval rights.

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
subsystems, breaking design changes) is also a maintainer
responsibility — typically through an RFC or major-feature
proposal process owned by the maintainer team.

## Contributing

Run `cargo test --bin stryke` before opening a PR. CI runs the full
suite (`cargo test`, the parity harness, and the Rosetta corpus) on
every push. PRs that touch the dispatch table must include
`refresh_stashes`-style category metadata so reflection stays in sync
(see `build.rs` and `tests/suite/reflection.rs`).

## Governance

- Maintainers are added and removed by consensus of existing maintainers.
- All project decisions — operational (CI, releases, labels) and
  directional (language design, new primitives, breaking changes) —
  are owned by the active maintainer team.
- The creator has no ongoing authority over the project. Attribution
  in [CREATORS.md](CREATORS.md) is historical record, not a governance
  role.
