# stryke Package Registry Architecture

## Overview

Stryke's package manager picks the proven winners from a decade of design experiments and skips the legacy mistakes. Specifically:

- **Cargo's model**: single TOML manifest, single deterministic lockfile, immutable registry, semver-aware resolver, integrated CLI for build/test/doc/publish, workspaces as a first-class concept.
- **uv's execution**: Rust-native, parallel resolver, parallel fetch/extract/verify, lockfile-first, milliseconds not minutes.
- **Nix's reproducibility**: every dep hash-pinned in the lockfile, byte-identical results on every machine, no "works on my box" failures.
- **Bundler's restraint**: lockfile is sacred, regenerate explicitly, no surprise version drift between installs.
- **npm's one good idea**: a `[scripts]` table for project-local task running.

Skipped on purpose: per-project deps tree (`node_modules`/`vendor`/`packages`), install-time code execution (`build.rs`/`postinstall`), hoisting, phantom deps, peer deps, mutable registries (no left-pad re-runs), centralized monocultures (private registries are first-class).

**Killer feature**: `s build --release` AOT-compiles your entire program — your code, every dep, the stdlib — through Cranelift to **native machine code**. Output is a single statically-linked ELF/Mach-O/PE binary in `target/release/`. No interpreter on the target machine. No JIT warmup. No bytecode at runtime. Just raw native code, the same kind of artifact Go and Rust produce. SFTP it to a server and run. Perl-grade ergonomics, Go-grade binaries. See [Build Outputs](#build-outputs-one-native-binary-ship-anywhere).

## Project Root Stays Clean

The existing stryke project layout (see `examples/project/`):

```
myproject/
  stryke.toml               # manifest
  stryke.lock               # exact versions + integrity hashes
  main.stk                  # entry point (run with `s` or `s main.stk`)
  lib/                      # module sources, accessed via `require` or `use`
    scanner.stk
    reporter.stk
    ai/
      classifier.stk
  bin/                      # additional executables (auto-discovered as [bin])
    myapp-helper.stk
  t/                        # tests, run with `s test t/`
    test_scanner.stk
    test_reporter.stk
  benches/                  # benchmarks, run with `s bench`
    bench_scanner.stk
    bench_hot_loop.stk
  examples/                 # example programs, built but not published
    quickstart.stk
  target/                   # build outputs (auto-created, gitignored)
    release/
      myapp                 # ← native machine code, statically linked, scp-ready
    debug/
      myapp                 # ← native machine code with debug symbols
    cache/                  # bytecode cache for `s run` JIT (dev only)
  README.md
```

Only `stryke.toml`, `stryke.lock`, and `main.stk` are required. Everything else is convention-discovered when present. Deps live in `~/.stryke/store/` and resolve through the lock file at load time — no directory full of someone else's code in your project tree.

`benches/` is a first-class concern, not an afterthought. Stryke is among the fastest interpreted languages in existence — beating LuaJIT on loop/array/regex workloads, competitive on others — so bench infrastructure ships with the package manager from day one. `s bench` runs every `benches/bench_*.stk` file, captures timing, and emits a comparable report. Performance regressions are caught at PR time, not in production.

## Global Store

```
~/.stryke/
  store/                    # one extracted copy per name@version
    http@1.0.0/
      lib/
      stryke.toml
    json@2.1.0/
    crypto@0.5.0/
  bin/                      # global CLI tools (s install -g)
    mytool -> ../store/mytool@1.0.0/bin/mytool
  cache/                    # downloaded tarballs awaiting extraction
    http-1.0.0.tar.zst
  git/                      # cloned git deps
    github.com-user-mylib-abc123/
  index/                    # registry index mirror (sparse, like cargo's)
```

Paths are human-readable (`name@version`) rather than nix-style hash paths. Hash-pinning happens in the lockfile, not the directory name — you get nix's reproducibility without nix's opaque paths.

## Manifest: stryke.toml

```toml
[package]
name = "myapp"
version = "0.1.0"
description = "My stryke application"
authors = ["user@example.com"]
license = "MIT"
repository = "https://github.com/user/myapp"
edition = "2026"             # language edition pin

[deps]
http = "1.0"                 # semver range
json = "2.1.0"               # exact
crypto = { version = "0.5", features = ["aes"] }
local-lib = { path = "../my-local-lib" }
git-lib = { git = "https://github.com/user/lib", tag = "v1.0.0" }

[dev-deps]
test-utils = "1.0"

[groups.bench]               # bundler-style groups, beyond dev/prod
criterion = "0.5"

[features]
default = ["json"]
json = ["dep:json"]
yaml = ["dep:yaml"]
full = ["json", "yaml"]

[scripts]                    # npm's one good idea
test = "s test t/"
bench = "s bench benches/"
build = "s build --release"  # → target/release/myapp (fat exe)
lint = "s check lib/"

[bin]
myapp = "main.stk"           # executable entry point at project root

# A package that has lib/ is a library. No [lib] table needed —
# the lib/ directory tree is auto-discovered and published as-is.
# Consumers access modules via `use Foo::Bar` → lib/Foo/Bar.stk in the store.

[workspace]                  # first-class from day 1
members = ["crates/*"]

[workspace.deps]             # shared versions across the workspace
http = "1.0"
```

Features are scoped per-package, not unified workspace-wide. A consumer turning on `feature = "yaml"` does not silently flip it on for every other package in the graph — cargo's biggest footgun, fixed.

## Lock File: stryke.lock

```toml
# Auto-generated. Do not edit.
version = 1
stryke = "0.1.0"
resolved = "2026-04-26T12:00:00Z"

[[package]]
name = "http"
version = "1.0.0"
source = "registry+https://registry.stryke.dev"
integrity = "sha256-abc123..."
features = ["default"]
deps = ["json@2.1.0"]

[[package]]
name = "json"
version = "2.1.0"
source = "registry+https://registry.stryke.dev"
integrity = "sha256-def456..."
deps = []

[[package]]
name = "crypto"
version = "0.5.0"
source = "registry+https://registry.stryke.dev"
integrity = "sha256-789ghi..."
features = ["aes"]
deps = ["json@2.1.0"]
```

Sorted deterministically. Two `s install`s from the same lock file on different machines produce byte-identical store contents.

## Commands

One binary, one mental model. Every project task happens through `s`:

```bash
# Project lifecycle
s init                       # interactive new package in cwd
s new myapp                  # new package in ./myapp
s build                      # build (interpreter cache or AOT)
s run                        # build + run main bin
s run myapp                  # run a specific [bin]
s test                       # run tests
s bench                      # run benches
s doc                        # generate docs
s check                      # type/lint without execution
s fmt                        # format
s clean                      # clear local caches

# Dependencies
s install                    # install per stryke.lock
s add http                   # add dep, update lock
s add http@1.0.0             # exact version
s add http --dev             # dev dep
s add http --group=bench     # arbitrary group
s remove http
s update                     # all deps within semver
s update http                # specific
s tree                       # full transitive graph
s outdated                   # what could be bumped
s audit                      # check vuln DB

# Run scripts from [scripts]
s run test
s run build

# Publishing
s publish                    # push to registry
s yank 1.0.0                 # mark version unusable, never delete
s search http                # query registry
s info http                  # package metadata

# Global tools
s install -g mytool
s uninstall -g mytool
s list -g

# Workspace
s vendor                     # opt-in, materialize deps to ./vendor/
                             # (only for offline distribution; never default)
```

`s vendor` exists for one specific use case — shipping a tarball that builds offline without registry access. It is opt-in, never automatic, and is *not* how normal development works.

## Resolution Algorithm

1. Parse `stryke.toml`, collect direct deps + workspace deps + features in scope.
2. Build dependency graph with version constraints.
3. Resolve using PubGrub (same algorithm uv and modern cargo use): preference for highest compatible, deterministic backtracking, clear conflict reports.
4. Verify each `(name, version)` against the lock file's integrity hash if present.
5. Check store for existing extractions.
6. Fetch missing tarballs to `~/.stryke/cache/` (parallel, rayon).
7. Verify hashes (parallel).
8. Extract to `~/.stryke/store/{name}@{version}/` (parallel).
9. Write `stryke.lock`.

No step in this pipeline executes code from any package. Install is pure data movement plus hash verification. The first time a package's code runs is when *your* code imports it, not when `s install` finishes.

## Module Resolution

Two mechanisms, both supported:

**Path-based `require`** (current convention in `examples/project/`):
```stryke
require "./lib/scanner.stk"
```
Resolved relative to the current file. Unaffected by the package manager — works the same whether the file lives in your project, a workspace member, or an extracted store package.

**Namespaced `use`** (for external deps):
```stryke
use Foo::Bar
```
Resolution order:
1. `lib/Foo/Bar.stk` (project-local).
2. Look up `foo` in `stryke.lock`, load `~/.stryke/store/foo@{version}/lib/Bar.stk`.
3. `@INC` system paths.

Lock file is the index — name → version → store path. No symlink farm, no hoisted phantom deps, no "which copy of `lodash` did I actually get."

## Store Sharing

```
project-a/stryke.lock → json@2.1.0 → ~/.stryke/store/json@2.1.0
project-b/stryke.lock → json@2.1.0 → ~/.stryke/store/json@2.1.0
```

Every `(name, version)` exists exactly once on disk per machine. Two thousand projects depending on `json@2.1.0` consume one copy of `json@2.1.0`.

## Build Outputs: One Native Binary, Ship Anywhere

Stryke's flagship capability. `s build --release` is **ahead-of-time native compilation**, full stop. Your code, every dep, and the stdlib all flow through Cranelift to native machine code (x86-64, aarch64, riscv64, etc.) and out to a single statically-linked executable.

What ends up in the binary:

- Your `main.stk` and `lib/` modules — AOT-compiled to native machine code.
- Every dep from `stryke.lock` — same, statically linked from the store.
- The minimal stryke runtime (GC, panic handler, syscall shims) — linked in like Go's runtime, not interpreted.
- Native assets declared in `[package.assets]` — embedded as `.rodata`.

What is **not** in the binary:

- No interpreter.
- No JIT.
- No bytecode at runtime.
- No `.stk` source files.
- No external `.so`/`.dylib`/`.dll` dependencies (unless you opted into dynamic linking).

The output is a real ELF / Mach-O / PE binary. `file myapp` reports it as a native executable. `objdump -d` shows real machine code. It's indistinguishable from a Go or Rust binary on the wire.

### Default Output Paths

```
target/release/<binary>          # native machine code, optimized, statically linked
target/debug/<binary>            # native machine code, debug symbols, less optimization
target/<triple>/release/<binary> # cross-compiled (e.g. x86_64-linux-gnu)
target/cache/*.stkc              # bytecode cache for `s run` JIT mode (dev only)
```

`target/` is auto-created on first build and auto-added to `.gitignore` by `s init`/`s new`. Cargo convention — your existing `.gitignore` rules already cover it.

### Two Execution Modes, One Toolchain

| Mode | Command | Engine | Use case |
|---|---|---|---|
| **JIT** | `s run main.stk` | Cranelift JIT in the VM | Fast iteration, scripts, REPL |
| **AOT** | `s build --release` | Cranelift AOT → linker | Shipping a binary |

The JIT mode is what stryke runs day-to-day during development — sub-millisecond startup, hot paths reach native speed within microseconds. The AOT mode is what you ship to production. Same compiler backend, different output target.

### The SFTP Workflow

```bash
s build --release                              # → target/release/myapp
scp target/release/myapp prod-host:/usr/local/bin/
ssh prod-host /usr/local/bin/myapp
```

That's it. No `pip install`, no `bundle install`, no `npm ci`, no Docker layer cache, no glibc gymnastics (musl target available), no PATH manipulation, no virtualenv. Typical binary is 5-20MB depending on dep count and embedded assets. Startup is microseconds — no interpreter to load, no JIT to warm, no bytecode to deserialize. Just `execve()` and run.

### Cross-Compilation

```bash
s build --release --target=x86_64-linux-gnu       # macOS dev → Linux prod
s build --release --target=aarch64-apple-darwin
s build --release --target=x86_64-pc-windows-gnu
s build --release --target=aarch64-linux-musl     # static linux, no glibc
s build --release --target=wasm32-wasi            # WASM module
```

Cranelift handles the codegen for every target ISA. Linkers and sysroots managed by `s` itself — no `cross` tool, no Docker hack, no `apt install gcc-aarch64-linux-gnu` ceremony.

### Why This Matters

The interpreted-language deployment story has been broken for twenty years:

- **Python**: ship `.py` + `requirements.txt` + pray the target has the right Python. PyInstaller bundles an interpreter and adds 100MB. Nuitka does real C compilation but is fragile on real codebases.
- **Ruby**: ship `.rb` + Gemfile + pray. ruby-packer exists, sees no adoption.
- **Node.js**: ship JS + `package.json` + materialize 200MB of `node_modules/`. `pkg`/`nexe` bundle V8 — still an interpreter, just self-contained.
- **PHP/Perl**: shell out to the system interpreter, hope for the best.

The compiled languages won the deployment war by definition:

- **Go**: native binary, ~10MB, zero deps, the model the entire ops industry lives on.
- **Rust**: native binary, ~5MB stripped, same story.
- **Zig**: native binary, even smaller, cross-compiles trivially.

Stryke is the first language to land on the **compiled side** while keeping interpreted-language ergonomics — sigils, dynamic dispatch, runtime introspection, REPL, hot reload during dev. You get Perl-grade source code productivity, Go-grade binaries on the way out.

`s build` defaults to release for this reason. The killer use case is "ship the binary," not "iterate on the bytecode" — that's what `s run` is for.

## Speed

Designed to match uv. Every install-path step that can parallelize, does:

- Concurrent registry index fetches (sparse index, like cargo 1.70+).
- Concurrent tarball downloads.
- Concurrent SHA-256 verification (rayon).
- Concurrent extraction.
- Resolver itself is parallel where the constraint graph allows.

No build step at install time. No native compilation. Cold install of a 50-dep project should be sub-second on broadband, warm install (everything in store) should be tens of milliseconds.

## Security

- **Immutable registry.** Once `name@version` is published, the bytes are fixed forever. `s yank` marks a version as do-not-resolve but never removes its content. left-pad scenarios are structurally impossible.
- **No install-time code execution.** No `build.rs`, no `postinstall`, no lifecycle hooks. Installing a package cannot run code. Compromised packages can only attack consumers that actually import and execute them.
- **Hash-pinned everything.** Lock file integrity hashes are checked before extraction. Tampered tarballs fail install loud, not silent.
- **Sigstore-style signing** (future). Publishers sign releases; consumers can require signatures from trusted publishers.
- **`s audit`** checks the dep graph against a vulnerability database (RustSec-style advisory feed).
- **Namespacing**: package names are `org/name` to prevent typosquatting. No flat global namespace.

## Reproducibility

Given a `stryke.lock`, two installs on different machines produce identical store contents — bit-for-bit. Verified by:

- Source URL pinned per package.
- SHA-256 of every tarball pinned.
- Resolver version pinned in the lock file (`stryke = "0.1.0"`).
- Resolution timestamp recorded for audit.

This is nix-grade reproducibility without nix-grade UX cost.

## Offline Mode

```bash
s install --offline          # only use cached packages
```

Works if all deps exist in `~/.stryke/store/` or `~/.stryke/cache/`. Combined with `s vendor`, lets you ship a fully offline-buildable archive.

## Workspaces

First-class from day one, not retrofitted:

```toml
# stryke.toml at workspace root
[workspace]
members = ["crates/*"]

[workspace.deps]             # versions inherited by all members
http = "1.0"
json = "2.1.0"

[workspace.package]          # metadata inherited by all members
license = "MIT"
authors = ["user@example.com"]
edition = "2026"
```

Members reference shared versions with `http.workspace = true`. Single lockfile at workspace root. One `s install` resolves the entire monorepo.

## Path Dependencies

```toml
[deps]
mylib = { path = "../mylib" }
```

Path deps load straight from the filesystem, bypassing the store. Edits reflect immediately on the next `s run`.

## Git Dependencies

```toml
[deps]
mylib = { git = "https://github.com/user/mylib" }
mylib = { git = "https://github.com/user/mylib", branch = "dev" }
mylib = { git = "https://github.com/user/mylib", tag = "v1.0.0" }
mylib = { git = "https://github.com/user/mylib", rev = "abc123" }
```

Cloned to `~/.stryke/git/` cache, resolved to a specific commit hash recorded in the lock file. Git deps are pinned in the lock just as tightly as registry deps.

## Registry Protocol

```
https://registry.stryke.dev/
  /api/v1/index/{name}                    # sparse index, single package
  /api/v1/packages/{name}/{version}       # metadata
  /api/v1/packages/{name}/{version}/dl    # tarball
  /api/v1/packages/{name}/{version}/yank  # yank (auth required)
```

Sparse index from day one (cargo took years to ship this). Mirroring, private registries (`registry = "https://my-co.example/"` per dep), auth tokens — standard.

The registry rule that defines the ecosystem: **published versions are immutable**. Yank, never unpublish.

## Implementation Order

1. ✅ `stryke.toml` parser (deps, scripts, bin, workspace). **SHIPPED**
2. ✅ `~/.stryke/store/` and `~/.stryke/cache/` layout. **SHIPPED**
3. ✅ `s install` for path deps only — proves the resolution loop. **SHIPPED**
4. ✅ `s add` / `s remove`. **SHIPPED**
5. ✅ `stryke.lock` generation with integrity hashes. **SHIPPED**
6. ✅ Module resolution integration (lock-driven, store paths). **SHIPPED**
7. ⏳ PubGrub semver resolver — **deferred until registry deployed**.
8. ⏳ Parallel fetch/verify/extract — **deferred until registry deployed**.
9. ⏳ Git deps — **deferred** (clear unimplemented error today).
10. ⏳ Features — partial: per-package feature flags parse and round-trip; resolver-side activation lands with the registry resolver.
11. ✅ Workspaces with shared deps inheritance. **SHIPPED** — `[workspace]` + `members = ["crates/*"]` glob + `{ workspace = true }` inheritance + single root lockfile.
12. ✅ `s install -g` for CLI tools. **SHIPPED** — `s install -g PATH`, `s uninstall -g NAME`, `s list -g`. Launchers go to `~/.stryke/bin/`.
13. ⏳ Sparse registry protocol + first registry deployment. **CLI stubs shipped** (`s search`, `s publish [--dry-run]`, `s yank`); endpoint deployment is the next chunk.
14. ✅ `s publish` (dry-run), `s yank` (stub), `s audit` (stub feed). **CLI shipped, feed/endpoint deferred.**
15. ⏳ Sigstore signing — **deferred until registry deployed**.

Plus the operational commands the RFC's command list calls out: ✅ `s update`, ✅ `s outdated`, ✅ `s vendor`, ✅ `s clean`, ✅ `s run SCRIPT` (npm-style task runner from `[scripts]`).

## Non-Goals

- npm compatibility.
- Node.js interop.
- Peer dependencies.
- Per-project deps directory (no `node_modules`, `vendor`, or `packages` — store-only).
- Hoisting (irrelevant when there is no per-project tree).
- Install-time code execution (no `build.rs`, no `postinstall`).
- Mutable registry (no unpublish, only yank).
- Workspace-wide feature unification (cargo's footgun).
- Phantom deps. Period.
