# Stryke JetBrains Plugin

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![IDE](https://img.shields.io/badge/IDE-2024.2%2B-orange.svg)](https://plugins.jetbrains.com/)
[![JDK](https://img.shields.io/badge/JDK-17-blue.svg)](https://adoptium.net/)
[![Plugin SDK](https://img.shields.io/badge/IntelliJ%20Platform%20Gradle-2.16-purple.svg)](https://plugins.jetbrains.com/docs/intellij/tools-intellij-platform-gradle-plugin.html)

### `[FULL IDE FRONT-END FOR THE FASTEST DYNAMIC LANGUAGE IN THE WORLD]`

> *"There is more than one way to do it — now with breakpoints."*

## `[BUILT FOR STRYKE]`

A JetBrains-platform plugin that drives the LSP and DAP servers compiled into the `stryke` binary. Hand-rolled lexer with **48 color slots**, semantic-token overlay from the LSP, **~25,000** hover-card-backed identifiers spanning every builtin / keyword / operator / Perl5-compat / extension / parallel primitive / sketch / phase block, full breakpoint-debugger over DAP with recursive variable expansion + per-frame Evaluate dialog, a **9-tab** reflection tool window fed by `%stryke::*` reflection hashes, **60+ snippet templates** keyed by completion prefix, Extract Variable / Constant / Function refactors plus Shift-F6 cross-file rename, run configs that auto-create from any `.stk` file. Talks to the in-tree `strykelang/lsp_extras.rs` + `strykelang/dap.rs` over JSON-RPC; no upstream `lsp-server` / `dap-types` crates anywhere in the build.

### [`strykelang`](https://github.com/MenkeTechnologies/strykelang) · [`Reference`](https://menketechnologies.github.io/strykelang/reference.html) · [`zshrs`](https://github.com/MenkeTechnologies/zshrs) · [`fusevm`](https://github.com/MenkeTechnologies/fusevm)

---

## Table of Contents

- [\[0x00\] Overview](#0x00-overview)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Editor](#0x02-editor)
- [\[0x03\] LSP](#0x03-lsp)
- [\[0x04\] Code Actions](#0x04-code-actions)
- [\[0x05\] Reflection Tool Window](#0x05-reflection-tool-window)
- [\[0x06\] Run / Debug](#0x06-run--debug)
- [\[0x07\] DAP Protocol](#0x07-dap-protocol)
- [\[0x08\] Refactor / Rename](#0x08-refactor--rename)
- [\[0x09\] Configuration](#0x09-configuration)
- [\[0x0A\] Logs](#0x0a-logs)
- [\[0x0B\] Building](#0x0b-building)
- [\[0x0C\] Plugin Architecture](#0x0c-plugin-architecture)
- [\[0x0D\] Version Compatibility](#0x0d-version-compatibility)
- [\[0x0E\] Limitations](#0x0e-limitations)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] OVERVIEW

stryke ships an **LSP server** and **DAP debug adapter** built into the `stryke` (alias `st`) binary (`stryke --lsp`, `stryke --dap HOST:PORT`). This plugin is the JetBrains-side driver:

- Spawns the LSP / DAP servers on demand, frames JSON-RPC over stdio / TCP, and renders responses through the IDE's native UI affordances (gutter breakpoints, intentions popup, refactor menu, code-folding handles, semantic-tokens layer, reflection tool window).
- Adds **zero new language code paths**. Everything the user sees in the editor comes from one of three sources: the hand-rolled `StrykeLexer.kt` (instant first-paint highlighting), the `textDocument/semanticTokens` overlay (LSP-driven full classification), or the `%stryke::*` reflection hashes serialized by `st -e 'p tj({%stryke::*})'` for the tool window.
- No upstream `lsp-server` / `lsp-types` / `dap-types` / `lsp4ij` dependencies anywhere on the Rust side. JetBrains' own `LspServerSupportProvider` is the only LSP4J consumer; everything else is hand-framed JSON-RPC on top of `serde_json`. Same on the DAP side.

Compiled `editors/intellij/build/distributions/stryke-intellij-<v>.zip` is self-contained: only Kotlin stdlib + IntelliJ Platform classes at runtime.

---

## [0x01] INSTALL

```sh
# Install from disk: Settings → Plugins → ⚙ → Install Plugin from Disk…
# Then pick:
editors/intellij/build/distributions/stryke-intellij-<version>.zip
```

After install: restart the IDE → open any `.stk` file → the LSP starts automatically → the debugger activates the first time you click Debug.

The `stryke` (or `st`) binary must be on `$PATH`, or configured under *Settings → Tools → Stryke → Stryke executable*. The plugin tries `st` first, then `stryke`.

---

## [0x02] EDITOR

| Surface | Behavior |
|---------|----------|
| File association | `.stk` (configurable; see [§0x09](#0x09-configuration) to add `pl` / `pm` / etc.) |
| Lexer | Hand-rolled in `StrykeLexer.kt` — instant first-paint highlighting before the LSP semantic-tokens response lands |
| Color slots | **48** stable `STRYKE_*` `TextAttributesKey`s under *Settings → Editor → Color Scheme → Stryke* |
| Brace matching | `{` / `}`, `(` / `)`, `[` / `]` via `StrykeBraceMatcher.kt` |
| Comments | Cmd/Ctrl-`/` for `#` line comments via `StrykeCommenter.kt`. Stryke's only multi-line comment form is POD `=pod ... =cut` which requires column-0 markers; IntelliJ's block-comment action would land them mid-expression, so the platform's block-comment binding routes to multi-line `#` instead. |
| Quote handler | `"`, `'`, `` ` ``, regex `/…/` and `m{…}` / `qr//` auto-pair |
| External Libraries | Every `~/.stryke/store/<pkg>@<ver>/lib/` directory surfaces under *Project view → External Libraries* (`stryke: <pkg>@<ver>` per entry) via `StrykeAdditionalLibraryRootsProvider`. Files are indexed by the IDE — Cmd-B / Find Usages / Recent Files all see them. Honors `$STRYKE_HOME`. Auto-refreshed via VFS watch on the store root. |

### Lexer coverage

| Token category | Examples |
|----------------|----------|
| Comments | `#` line, `##` doc, POD `=pod` … `=cut` |
| Strings | `"…"`, `'…'`, `qw(…)`, `qq(…)`, `qr/…/`, heredocs (`<<EOT`, `<<~EOT`, `<<'EOT'`) |
| Numbers | `42`, `3.14`, `0xFF`, `0b1010`, `0o755`, `1_000_000`, `1e10` |
| Regex | `/…/flags`, `m{…}flags`, `s/…/…/flags`, `tr/…/…/flags` |
| Declarations | `my` / `our` / `state` / `use` / `package` / `frozen` |
| Function/type | `fn` / `sub` / `class` / `struct` / `trait` / `enum` |
| Control flow | `if` / `unless` / `elsif` / `else` / `while` / `until` / `for` / `foreach` / `do` / `match` / `try` / `given` / `when` |
| Phase blocks | `BEGIN` / `END` / `INIT` / `CHECK` / `UNITCHECK` |
| Word operators | `and` / `or` / `not` / `xor` / `eq` / `ne` / `lt` / `gt` / `cmp` / `x` |
| Booleans | `true` / `false` / `undef` |
| Builtins | full canonical set from `%stryke::builtins` |
| Parallel | `pmap` / `pgrep` / `pfor` / `preduce` / `pforks` / … |
| Sigil vars | `$x` scalar, `@x` array, `%x` hash, `$!` / `$@` special, `$_` / `@_` topic, `$_0` / `$_1` block params |
| Package paths | `Foo::Bar`, `Foo::Bar::baz()` |
| Operators | `=>`, `->`, `~>`, `|>`, `=~`, `!~`, `..`, `...`, `<=>` |
| Punctuation | parens / braces / brackets / commas / semicolons split into independent color categories |

---

## [0x03] LSP

The LSP server is in-process inside the `stryke` binary — `stryke --lsp` (or `st --lsp`) spawns it over stdio. Plugin side starts it via `StrykeLspServerSupportProvider.kt`; descriptor in `StrykeLspServerDescriptor.kt`.

### Capabilities

| Capability | Trigger / scope |
|------------|-----------------|
| `completion` | trigger chars `$` `@` `%` `:` `_` plus all letters; `resolveProvider`; **60+ snippet templates** keyed by prefix |
| `hover` | full markdown cards from `lsp_docs_domains.rs`; category-stub fallback for any `%stryke::builtins` entry without a hand card |
| `definition` / `declaration` / `references` / `documentHighlight` | cross-file via the server's `SymbolTable`. **Package-name jump-to-decl** chases `use Foo::Bar` / `Foo::Bar::func()` references through `static_analysis::resolve_require_path_from_file` (arm 1: project `lib/` → arm 2: `stryke.lock` pin → arm 3: store-scan for highest installed version, release > pre-release per the runtime's `VersionRank`) so Cmd-B on a package whose decl lives in `~/.stryke/store/<pkg>@<ver>/lib/<Pkg>.stk` lands on the `package Foo::Bar` line. Note: the cross-file chase does not yet honor the use-site `use Foo VERSION` pin — it follows the lockfile / highest-installed regardless. The pin is honored by the runtime resolver and surfaces in version-slot completion (next row); goto-def-respects-pin is a planned LSP improvement. |
| `completion` (version slot) | `use GUI <TAB>` / `use Foo 1<TAB>` lists every installed version directly from `~/.stryke/store/<canonical>@<ver>/`. Honors the namespace bridge (`use GUI` ↔ `stryke-gui@*`) and prefix-filters on what's typed. Any offered version is guaranteed to satisfy the resolver at runtime. |
| `documentSymbol` | every `sub` / `fn` / `class` / `struct` / `enum` / `trait` / `package` decl, plus top-level `my` / `our` |
| `foldingRange` | every `{ … }` block, POD `=pod` … `=cut`, 3+ consecutive `#` comment runs |
| `rename` (with `prepareRename`) | scalars / arrays / hashes / subs / functions / types / packages; cross-file for package symbols (see [§0x08](#0x08-refactor--rename)) |
| `semanticTokens/full` | token classes mirroring the lexer; LSP overlay refines what the hand lexer approximates |
| `signatureHelp` | parameter hints with active-arg tracking |
| `codeAction` | line-local quickfixes (*Wrap line in `p`*, *Comment / Uncomment line*) plus Extract refactorings — see [§0x04](#0x04-code-actions) |
| `formatting` | Cmd/Ctrl-Opt-`L` pipes the document through stryke's built-in formatter (`fmt::format_program`): 4-space indent, normalized operator spacing, single-line rewrites for short blocks |
| `publishDiagnostics` | parse + compile errors with line/col |

### Completion snippets (60+)

Tab walks the `${1:...}` placeholders to the final `${0}` cursor:

| Family | Prefixes |
|--------|----------|
| Control flow | `if` / `ifelse` / `ifelsif` / `elsif` / `else` / `while` / `until` / `for` / `forrange` / `foreach` / `do` / `match` / `try` / `given` |
| Declarations | `my` / `fn` / `sub` / `class` / `struct` / `enum` / `trait` |
| Parallel primitives | `pmap` / `pmaps` / `pgrep` / `pfor` / `preduce` / `pforks` / `pwhile` |
| Phase blocks | `BEGIN` / `END` / `INIT` / `CHECK` |
| Module setup | `use` / `strict` / `shebang` / `main` |
| Scaffolds | `web` / `svg` / `system` / `git` / `test` |

### Transport

- **Stdio**, Content-Length-framed JSON-RPC. Hand-rolled framer on top of `serde_json` — no `lsp-server` / `lsp-types` crates.
- Optional `STRYKE_LSP_LOG=<path>` env var dumps every request/response for debugging.
- Server log lives at `~/.stryke/stryke.log` (see [§0x0A](#0x0a-logs)).

---

## [0x04] CODE ACTIONS

| Action | Selection shape | Edit shape |
|--------|-----------------|------------|
| **Extract to variable** (Cmd-Opt-V) | single-line, full-line OR sub-expression | inserts `my $name = <rhs>` above, replaces selection with `$name` |
| **Extract to constant** (Cmd-Opt-C) | single-line, full-line OR sub-expression | inserts `my frozen $NAME = <rhs>` above (uppercase placeholder), replaces selection with `$NAME` |
| **Extract to function** (Cmd-Opt-M) | multi-line | wraps the selection in `fn extracted_fn { … }` above the block, replaces the original range with a call. v1 does no free-variable analysis. |
| **Wrap line in `p`** | caret on line | wraps the current statement in `p(…)` for quick print debugging |
| **Comment / Uncomment line** | caret on line | toggles a `#` prefix on the current line |

All three Extract actions use `kind: "refactor.extract"` (parent kind), so they match `refactor.extract.method` / `refactor.extract.variable` / `refactor.extract.constant` queries via prefix.

Surfaced under **Alt-Enter** (intentions popup). The IntelliJ Refactor menu (Ctrl-T) is reserved for native PSI-based refactorings; LSP-driven extract / inline / rewrite kinds show up in the intentions popup, not under that menu.

---

## [0x05] REFLECTION TOOL WINDOW

*View → Tool Windows → Stryke* (right edge). Fed live from `stryke -e 'p tj({%stryke::*})'` on first open (≈25k entries):

| Tab | Source hash | Notes |
|-----|-------------|-------|
| **All `%all`** | merged union | one tree spanning every category |
| **Builtins `%b`** | `%stryke::builtins` | every builtin grouped by category (math, IO, string, regex, …) |
| **Keywords `%k`** | `%stryke::keywords` | reserved words + control-flow keywords |
| **Operators `%o`** | `%stryke::operators` | arithmetic / logical / regex-bind / arrow / range / list |
| **Special vars `%v`** | `%stryke::special_vars` | `$_`, `$@`, `$!`, `@ARGV`, `%ENV`, `@INC`, `$0`, `$$`, `$0..$9` regex captures |
| **Perl5 `%pc`** | `%stryke::perl_compats` | every name stryke supports for Perl5 compatibility |
| **Extensions `%e`** | `%stryke::extensions` | stryke-only additions (parallel primitives, sketches, async / trace / timer / aop, rust FFI) |
| **Aliases `%a`** | `%stryke::aliases` | alias → canonical name |
| **Descriptions `%d`** | `%stryke::descriptions` | name → one-line summary |

Each tab is a tree grouped by category, with a per-tab search field filtering across name + category.

| Interaction | Effect |
|-------------|--------|
| **Left-click on leaf** | Anchored docs popup. Renders `stryke docs NAME` with ANSI colors decoded via IntelliJ's `AnsiEscapeDecoder` + `ConsoleView` — same body as a terminal `stryke docs <name>` lookup. |
| **Right-click on leaf** | Context menu: *Show Docs* + *Copy Name* |
| **Toolbar → Refresh** | Re-runs `stryke -e 'p tj({%stryke::*})'` and reloads every tab |
| **Toolbar → Settings** | Opens *Settings → Tools → Stryke* |

---

## [0x06] RUN / DEBUG

### Run

| Surface | Behavior |
|---------|----------|
| **Run config** (`StrykeRunConfigurationType`) | toggles for `--no-interop` / `--disasm` / `--profile` / `--flame` / `-d` / `-D`; working directory + script args + interpreter args |
| **Context menu** | *Run with stryke* on any `.stk` file in the editor or project view; auto-creates a config |
| **Producer** | `StrykeRunConfigurationProducer` materializes a run config from the active file |
| **Output** | Standard `ConsoleView` — `p` / `print` / `printf` / `say` stream in real time |
| **File → New → Stryke File** | Standard New-File dialog; pick *Script* (shebanged, `fn main` stub), *Library / module*, or *Empty*. Same entry surfaces in the Project-view right-click *New* submenu. |

### Debug

DAP-backed, over a loopback TCP socket. Plugin spawns `st --dap 127.0.0.1:<port>`; stryke connects back.

| Feature | Notes |
|---------|-------|
| Line breakpoints | Gutter toggle / enable / disable; persistent across sessions |
| Function breakpoints | Run → View Breakpoints → + |
| Continue / Step Over / Step Into / Step Out / Pause / Run to Cursor | Standard XDebugger actions |
| Frames | `file:line` per frame, click to navigate source |
| Variables panel | Three-tier sort — user `my` vars on top, magic block params (`$_`, `$_0`, `$_1`, …, `$a`, `$b`) middle, stryke built-ins (`$stryke::VERSION`, `%ENV`, `%term`, `@INC`, …) bottom. `__synthetic__` compiler internals hidden. |
| Recursive expansion | `[N] (key => val, …)` summary with disclosure triangles, drill in to `key = value` rows; depth-capped at 12, count-capped at 2000 |
| Rich drill-down | `StructInstance` / `ClassInstance` (one row per field, `+`/`#`/`-` visibility markers, `__class` + `__isa` metadata); `EnumInstance` (variant + carried data); `Set` (`Set(N) {a, b, c}`); every sketch type (`TDigestSketch` → count/min/max/mean/sum/p50-p99/compression; `BloomFilter` → inserted/bit_count/k/fpr; `HllSketch` / `CmsSketch` / `TopKSketch`) |
| Evaluate dialog | Pure expressions (`55 + 3`, `sqrt(2)`, `len(@INC)`) plus expressions using the current frame's scalars (`$a * $b`) via prelude injection into a `st -e` subprocess |
| Console | Program `p` / `print` / `printf` / `say` output in real time (autoflush + flush-on-pause) |
| Two debuggers | `st -d file.stk` (TTY REPL, `perl -d` style) and `st --dap HOST:PORT` (consumed by this plugin) — share one `Debugger` state machine |

---

## [0x07] DAP PROTOCOL

Plugin side (`com.menketechnologies.stryke.dap`):

1. `StrykeDebugRunner.doExecute` opens a `ServerSocket(0)` on `127.0.0.1`, captures the port.
2. Spawns `st --dap 127.0.0.1:<port>` via `KillableColoredProcessHandler` — `OSProcessHandler` keeps the process's stdio for Console output, exclusively.
3. Waits up to 10 s for stryke to connect back, then runs DAP over that socket.
4. Creates an `XDebugSession` via `XDebuggerManager.startSession` and returns the descriptor via `getMockRunContentDescriptorIfInitialized` reflection — bypasses the platform's split-debugger `Logger.error("[Split debugger] …")` toast that the deprecated `runContentDescriptor` getter fires.
5. `StrykeDebugProcess.createConsole` builds a `ConsoleView` and `attachToProcess(processHandler)` so program stdout streams in real time.
6. `StrykeDapClient` reads Content-Length-framed JSON-RPC from the socket — **byte-based, not char-based** — so multi-byte UTF-8 in variable reprs doesn't desync framing.
7. On `stopped` event, `onStopped` synchronously fetches `stackTrace` + `scopes` + `variables`, builds `StrykeStackFrame` objects with pre-populated children, calls `session.positionReached`. No async expansion on the UI thread — IntelliJ 2026.1's split-debugger drops those.
8. `StrykeEvaluator` sends `evaluate` requests for the Evaluate dialog; stryke does scalar-prelude injection so `$a * $b` returns the right value from the paused frame.

Stryke side (`strykelang/dap.rs` + `strykelang/debugger.rs`):

- `Debugger` state machine (breakpoints, step modes, call depth) shared between TTY and DAP front-ends. Step-over depends on `enter_sub` / `leave_sub` being called at every VM call dispatch site (`vm.rs:2192..` and `vm_helper.rs:19216..`) so `call_depth` matches the program's logical call stack — without these hooks step-over drops into UDFs instead of skipping them.
- Same-line guard tracks both `last_stop_line` and `last_stop_depth` (`debugger.rs:38..`). Without the depth half, step-in fires on the same source line as the call site (first opcode of the call setup has the same line as `my $r = foo()`), requiring two clicks to actually enter `foo`.
- `set_topic` for implicit `for (@arr) { … }` loops so `$_` / `$_0` / `_` / `_0` all alias.
- Snapshot capture (`capture_locals_with_map`) walks the scope, builds per-variable refs for hashes / arrays / structs / classes / enums / sets / sketches, recursively expanding their children into a `var_ref_map` (depth 12, count 2000) so the DAP `variables` request resolves any ref to its rows.
- stdout/stderr autoflush + flush on every pause so output lands in the Console before the suspend UI takes over.

---

## [0x08] REFACTOR / RENAME

**Shift-F6** on any of these identifiers renames it across the workspace via `textDocument/rename`:

- Scalar / array / hash variables (`$x`, `@xs`, `%h`) — the sigil is included in the identifier extraction, so `$pass` and the `pass` builtin no longer collide.
- `my` / `state` / `our` declarations (and the `frozen my` constant form).
- Subroutine / function declarations (`fn`, `sub`).
- Struct / class / enum / trait names and their constructor / method call sites (`Point->new`, `Color::Red`).
- Package names declared via `package Foo::Bar;`.

Cross-file rename fires when the symbol is package-scoped (sub, type, `our`, package). The server scans every other open document, finds exact-name matches in its `SymbolTable`, and falls back to a textual qualified-name scan for files that reference the symbol without re-declaring it. Locally-scoped `my` / `state` decls and sub parameters are file-scoped and never cross files.

Hovering on the `format` key in `$opts{format}` or the `exec` selector in `$db->exec` does NOT show the `format` / `exec` builtin card — those identifiers are hash keys / method selectors, not builtin references.

Implementation: plugin handler in `StrykeRenameHandler.kt`; server-side rename in `strykelang/lsp_extras.rs::rename`.

---

## [0x09] CONFIGURATION

*Settings → Tools → Stryke*:

| Section     | Setting                                | Default                          | Notes |
|-------------|----------------------------------------|----------------------------------|-------|
| Interpreter | Stryke executable                      | first `st` then `stryke` on `$PATH` | absolute path or blank |
| LSP         | Enable LSP                             | on                               | master toggle |
| LSP         | Extra LSP args                         | empty                            | passed after `--lsp` |
| LSP         | LSP environment                        | empty                            | `KEY=VAL` pairs (e.g. `STRYKE_LOG_LEVEL=debug`) |
| LSP         | Auto-restart LSP on settings change    | on                               | restart picks up new env |
| LSP         | Show builtin hovers                    | on                               | server-provided cards |
| LSP         | Log LSP traffic to file                | off                              | sets `STRYKE_LSP_LOG=<path>` |
| Editor      | Disable lexer highlighting             | off                              | rely only on LSP semantic tokens |
| Editor      | File extensions                        | `stk`                            | comma-separated; add `pl` / `pm` to enable for Perl files |
| Run configs | Default `--no-interop`                 | off                              | strict stryke parser by default |

Color scheme entries: *Settings → Editor → Color Scheme → Stryke* (**48 sub-categories** grouped under Comments / Strings / Numbers / Regex / Keywords / Names / Variables / Operators / Punctuation / Errors).

---

## [0x0A] LOGS

Two append-only logs, both under `~/.stryke/` (or `$STRYKE_HOME/` when that env var is set):

| File | Source | Contents |
|------|--------|----------|
| `~/.stryke/stryke-plugin.log` | Kotlin (plugin) | LSP command line built, DAP `send` / receive (seq + command + bytes), rename / semantic-token routing, breakpoint handler steps |
| `~/.stryke/stryke.log` | Rust (`stryke --lsp` / `--dap`) | Levelled events (`TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`) from both daemons: startup, initialize, every request method (TRACE), didOpen / Change / Close + diagnostics (DEBUG), rename / hover outcomes, DAP launch / breakpoints / step / pause / disconnect, milestone events (`stopped` / `terminated` / `exited`) |

Tail with `tail -f ~/.stryke/stryke.log ~/.stryke/stryke-plugin.log`.

### Server log level (Rust side)

`$STRYKE_LOG_LEVEL` accepts `trace`, `debug`, `info` (default), `warn`, `error` (case-insensitive). Lower levels include everything above.

```sh
export STRYKE_LOG_LEVEL=debug   # verbose for daily use
export STRYKE_LOG_LEVEL=trace   # firehose, every request method logged
```

### Rotation

`~/.stryke/stryke.log` rotates automatically when it crosses the threshold:

| Variable | Default | Meaning |
|----------|---------|---------|
| `STRYKE_LOG_MAX_BYTES` | `5242880` (5 MiB) | Per-file size cap. `0` disables rotation. |
| `STRYKE_LOG_MAX_FILES` | `5` | Number of `.1` … `.N` archives to keep alongside the active file. |

When the active log hits the size cap, `stryke.log.N-1` shifts to `stryke.log.N` (eldest is overwritten), then the active file moves to `stryke.log.1`. Default bounds the log dir at roughly `(N+1) × MAX_BYTES` = ~30 MiB.

### Redirection / sandboxing

`$STRYKE_LOG_FILE=/abs/path/to.log` overrides the resolved path entirely — used by tests and sandbox runs. Per-IDE chatter still goes to `idea.log` via `Logger.getInstance(...)`; the files above are for plugin-and-server events specifically.

---

## [0x0B] BUILDING

```sh
cd editors/intellij
./gradlew buildPlugin             # → build/distributions/stryke-intellij-<v>.zip
./gradlew runIde                  # launches a sandbox IDE with the plugin installed
./gradlew verifyPlugin            # plugin verifier against recommended IDE matrix
./gradlew test                    # runs StrykeLexerTest + StrykeColorSettingsPageTest
```

First build downloads the IntelliJ Platform SDK (~1 GB), takes a few minutes, and is cached under `editors/intellij/.intellijPlatform/` (which is gitignored).

---

## [0x0C] PLUGIN ARCHITECTURE

```
editors/intellij/
├── build.gradle.kts                   # IntelliJ Platform Gradle Plugin 2.16
├── gradle.properties                  # platform version, plugin version, JVM
├── settings.gradle.kts
└── src/main/
    ├── kotlin/com/menketechnologies/stryke/
    │   ├── StrykeLanguage.kt          # Language singleton
    │   ├── StrykeFileType.kt          # .stk → Stryke
    │   ├── StrykeIcons.kt             # icon loader
    │   ├── StrykeColors.kt            # 48 STRYKE_* TextAttributesKey constants
    │   ├── StrykeTokenTypes.kt        # token type enum
    │   ├── StrykeLexer.kt             # hand-rolled lexer
    │   ├── StrykeSyntaxHighlighter.kt # token → color mapping
    │   ├── StrykeColorSettingsPage.kt # IDE color-scheme entries
    │   ├── StrykeBraceMatcher.kt      # {} () []
    │   ├── StrykeCommenter.kt         # `#` line comments
    │   ├── StrykeQuoteHandler.kt      # " ' ` / m{} qr// auto-pair
    │   ├── StrykeSettings.kt          # persistent settings
    │   ├── StrykeSettingsConfigurable.kt
    │   ├── StrykeDebugLog.kt          # plugin-side log writer
    │   ├── lsp/
    │   │   ├── StrykeLspServerSupportProvider.kt
    │   │   └── StrykeLspServerDescriptor.kt
    │   ├── refactor/
    │   │   ├── StrykeRefactoringSupportProvider.kt   # Extract Method/Var/Const routing
    │   │   └── StrykeRenameHandler.kt
    │   ├── navigate/
    │   │   └── StrykeGotoDeclarationHandler.kt       # Cmd-click + Cmd-B
    │   ├── lib/
    │   │   └── StrykeAdditionalLibraryRootsProvider.kt  # SyntheticLibrary per ~/.stryke/store/<pkg>@<ver>/lib/
    │   ├── run/
    │   │   ├── StrykeRunConfigurationType.kt
    │   │   ├── StrykeRunConfigurationOptions.kt
    │   │   ├── StrykeRunConfiguration.kt
    │   │   ├── StrykeRunConfigurationEditor.kt
    │   │   ├── StrykeRunConfigurationProducer.kt
    │   │   ├── StrykeProgramRunner.kt    # Run executor
    │   │   └── StrykeDebugRunner.kt      # Debug executor (DAP)
    │   ├── dap/
    │   │   ├── StrykeDapClient.kt        # byte-based DAP protocol client
    │   │   ├── StrykeDebugProcess.kt     # XDebugProcess
    │   │   ├── StrykeDebuggerEditorsProvider.kt
    │   │   ├── StrykeBreakpointType.kt   # xdebugger.breakpointType
    │   │   ├── StrykeBreakpointHandler.kt
    │   │   ├── StrykeStackFrame.kt
    │   │   ├── StrykeSuspendContext.kt
    │   │   ├── StrykeValue.kt            # XValue with recursive children
    │   │   └── StrykeEvaluator.kt        # Evaluate dialog backend
    │   ├── toolwindow/
    │   │   └── StrykeReflectionToolWindow.kt
    │   └── actions/
    │       └── RunStrykeFileAction.kt
    └── resources/
        ├── META-INF/plugin.xml
        └── icons/stryke.svg
```

The Rust side lives in:

| Module | Purpose |
|--------|---------|
| `strykelang/lsp_extras.rs` | LSP server (`stryke --lsp`) — hover, completion, codeAction, rename, semanticTokens, foldingRange, signatureHelp, diagnostics, formatting |
| `strykelang/dap.rs` + `strykelang/debugger.rs` | DAP server (`stryke --dap HOST:PORT`) — breakpoints, stepping, scopes, variables (recursive), evaluate |
| `strykelang/lsp_docs_domains.rs` | Hover-card bodies grouped by domain (math, IO, string, regex, parallel, sketches, …) |
| `strykelang/fmt.rs` | Formatter (`stryke --fmt`) — same engine `textDocument/formatting` invokes |
| `strykelang/reflection.rs` | Builds the `%stryke::*` hashes that the reflection tool window serializes via `st -e 'p tj({%stryke::*})'` |

---

## [0x0D] VERSION COMPATIBILITY

Plugin version tracks the strykelang Cargo version. `gradle.properties` controls the supported IDE range via `pluginSinceBuild` / `pluginUntilBuild`. Currently targets `2024.2.4` SDK against builds `242..261.*` — every paid JetBrains IDE on **2024.2 +** loads it (RustRover, IDEA Ultimate, GoLand, PyCharm Pro, WebStorm, RubyMine, PhpStorm, CLion, Rider, DataGrip, Aqua). Community editions don't have the LSP API, so the plugin won't load there.

---

## [0x0E] LIMITATIONS

- **No PSI tree or structural navigation** — relies entirely on the LSP for symbol navigation. Disabling the LSP under Settings disables them all.
- **Debugger v1**: no conditional or hit-count breakpoints, no exception breakpoints, no watch expressions, no Set Value, single-thread only. Step-into across `use`d modules works only if the called sub has line mapping in the same compilation unit.
- **Evaluator** injects only scalars from the current frame. Expressions referencing user-defined `@arr` / `%hash` see them as empty in the subprocess. Builtins / globals (`@INC`, `%ENV`, etc.) work.
- **Lexer** is a regex-class scanner; complex constructs (heredocs, `qw()`, nested string interpolation) are not fully tokenized — they fall back to "string". Server-side semantic tokens fill in where the lexer is approximate.
- **`[Split debugger]` toast on Debug start** — the IDE's deprecated `XDebugSession.runContentDescriptor` accessor fires `Logger.error` even when bypassed via reflection if any third-party code touches it during session bring-up. JetBrains' own debug runners suffer the same noise in 2024.3+. Cosmetic only; the debugger works.
- **Reflection hashes (`%stryke::*`)** populate lazily on first user access in DAP mode (eager `ensure_reflection_hashes` triggers a VM stack overflow that's still TBD). The standalone Stryke tool window fetches them via `st -e` and is unaffected.

---

## [0xFF] LICENSE

MIT, same as strykelang.
