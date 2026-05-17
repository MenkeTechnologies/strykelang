# Stryke JetBrains Plugin

JetBrains IDE support for [stryke](https://github.com/MenkeTechnologies/strykelang) — the massively
parallel scripting language (Rust bytecode VM + Cranelift JIT + Rayon).

## Features

### Editor
- **`.stk` file association** with a hand-rolled lexer that tokenizes
  comments / doc-comments, strings / heredocs, integers / floats, regex
  literals + flags, declaration keywords (`my` / `our` / `use` / `package`),
  function-declaration keywords (`fn` / `sub` / `class` / `struct` / `trait`
  / `enum`), control flow (`if` / `while` / `for` / `return`), phase blocks
  (`BEGIN` / `END` / `INIT` / `CHECK`), word operators (`and` / `or` / `eq`
  / `cmp` / `x`), booleans / `undef`, builtins (general + parallel like
  `pmap` / `pgrep`), package paths (`Foo::Bar`), sigil variables split by
  type (`$x` scalar, `@x` array, `%x` hash, `$!`/`$@` special, `$_`/`@_`
  topic, `_0`/`_1` block params), arrow / fat-comma / pipe / range / regex-
  bind operators, parens / braces / brackets / commas / semicolons.
- **44 color slots** under *Settings → Editor → Color Scheme → Stryke* —
  every token category is independently themeable with stable
  `STRYKE_*` `TextAttributesKey` names.
- **Comments**: `#` line comments — Ctrl/Cmd-`/`. (Stryke's only
  multi-line comment form is POD `=pod ... =cut`, which requires
  column-0 markers; IntelliJ's block-comment action inserts at the
  selection anchors so the markers would land mid-expression. For
  block-style commenting select multiple lines and use Cmd-`/` —
  IntelliJ prepends `# ` to each line.)

### LSP
- LSP client wired to `st --lsp` over stdio. Server capabilities:
  - `completion` with trigger characters `$` `@` `%` `:` `_` and all
    letters, plus `resolveProvider`. Includes **60+ snippet templates**
    keyed by prefix: control flow (`if`, `ifelse`, `ifelsif`, `elsif`,
    `else`, `while`, `until`, `for`, `forrange`, `foreach`, `do`,
    `match`, `try`, `given`), declarations (`my`, `fn`, `sub`, `class`,
    `struct`, `enum`, `trait`), parallel primitives (`pmap`, `pmaps`,
    `pgrep`, `pfor`, `preduce`, ...), phase blocks (`BEGIN`, `END`),
    module setup (`use`, `strict`, `shebang`, `main`), web / SVG / system
    / git scaffolds, and a full `test` file template. Tab walks the
    `${1:...}` placeholders to the final `${0}` cursor.
  - `hover` (full markdown cards from `lsp_docs_domains.rs`; falls back to
    category-stub for any builtin in `CATEGORY_MAP` that lacks a hand-
    written card)
  - `definition` / `declaration` / `references` / `documentHighlight`
  - `rename` with prepare
  - `documentSymbol`
  - `semanticTokens` (full document)
  - `signatureHelp` (parameter hints with active-arg tracking)
  - `codeAction` — line-local quickfixes (*Wrap line in `p`*,
    *Comment / Uncomment line*) plus refactorings:
    - *Extract to variable* — `my $name = …` inserted above the line,
      selection replaced with `$name` (single-line selection)
    - *Extract to constant* — same but `my frozen $NAME = …` with an
      uppercase placeholder (single-line selection)
    - *Extract to function* — wraps the selection in
      `fn extracted_fn { … }` and replaces the span with a call
      (multi-line selection; v1 does no free-variable analysis)
    - Surfaced under **Alt-Enter** (intentions / context actions). The
      IntelliJ Refactor menu (Ctrl-T) is reserved for native PSI-based
      refactorings; LSP-driven extract / inline / rewrite kinds show up
      in the intentions popup, not under that menu.
  - `foldingRange` — fold every `{ … }` block (`fn`, `class`, `struct`,
    `enum`, `if`, `while`, `for`, hash literals, ...), `=pod` ... `=cut`
    POD blocks, and 3+ consecutive `#`-line comment runs.
    Cmd-Shift-Minus collapses all, Cmd-Shift-Plus expands all.
  - `formatting` — Cmd/Ctrl-Opt-`L` (or `Code → Reformat File…`) pipes
    the document through stryke's built-in formatter (`fmt::format_program`):
    4-space indent, normalized spacing around operators, single-line
    rewrites for short blocks.
  - `publishDiagnostics` (parse + compile errors with line/col)

### Run / Debug
- **Run configurations** for `.stk` files with `--no-interop` / `--disasm`
  / `--profile` / `--flame` / `-d` / `-D` toggles, working directory,
  script args, interpreter args.
- **Context-menu *Run with stryke*** on any `.stk` file in the editor or
  project view; auto-creates a run config.
- **Debugger** (DAP-backed over TCP socket):
  - Line breakpoints from the gutter (toggle, enable/disable)
  - Function breakpoints (Run → View Breakpoints → +)
  - Continue / Step Over / Step Into / Step Out / Pause / Run to Cursor
  - **Frames** with file:line per frame, source navigation
  - **Variables panel** sorted in three tiers — user `my` vars on top,
    magic block params (`$_`, `$_0`, `$_1`, …, `$a`, `$b`) in the middle,
    stryke built-ins (`$stryke::VERSION`, `%ENV`, `%term`, `@INC`, …) at
    the bottom. `__synthetic__` compiler internals are hidden.
  - **Recursive hash / array expansion** — `[N] (key => val, …)` summary
    with disclosure triangles, click to drill in to `key = value` rows,
    works to arbitrary depth (capped at 12 to avoid cycles).
  - **Rich object drill-down** for user-defined types and sketches:
    `StructInstance` / `ClassInstance` (one row per field, visibility
    marker `+` / `#` / `-` for class fields, `__class` and `__isa`
    metadata rows), `EnumInstance` (variant + carried data), `Set`
    (`Set(N) {a, b, c}` with one row per element), and every sketch type
    (`TDigestSketch` exposes count / min / max / mean / sum / p50-p99 /
    compression; `BloomFilter` exposes inserted / bit_count / k / fpr;
    `HllSketch` / `CmsSketch` / `TopKSketch` similarly).
  - **Evaluate** dialog — pure expressions (`55 + 3`, `sqrt(2)`, `len(@INC)`)
    plus expressions using current frame's scalars (`$a * $b`) via prelude
    injection into a `st -e` subprocess
  - **Console** shows the program's `p` / `print` / `printf` output in
    real time (autoflush + flush-on-pause)
  - **Two separate debuggers, sharing the runtime state machine**:
    - `st -d file.stk` — TTY REPL, `perl -d` style, runs in the shell
    - `st --dap [HOST:PORT]` — Debug Adapter Protocol server consumed by
      this plugin

### Reflection tool window
- *View → Tool Windows → Stryke* (right edge).
- **9 tabs** — `%all` plus `%builtins` / `%keywords` / `%operators` /
  `%special_vars` / `%perl_compats` / `%extensions` / `%aliases` /
  `%descriptions` — fed live from `stryke -e 'p tj({%stryke::*})'` on
  first open (≈25k entries).
- Each tab is a tree grouped by category, with a per-tab search field that
  filters across name + category.
- **Left-click on any leaf → docs popup** anchored at the click. Renders
  `stryke docs <name>` with ANSI colors interpreted via IntelliJ's
  `AnsiEscapeDecoder` + `ConsoleView` (so the popup matches what
  `stryke docs <name>` looks like in your terminal).
- **Right-click on any leaf → context menu** with *Show Docs* + *Copy
  Name*.
- Toolbar: *Refresh* (re-runs `stryke -e` and reloads) + *Settings* (jumps
  to Stryke settings).

## Requirements

- A paid JetBrains IDE on **2024.2+** (RustRover, IDEA Ultimate, GoLand,
  PyCharm Pro, WebStorm, RubyMine, PhpStorm, CLion, Rider, DataGrip,
  Aqua). The LSP API isn't in Community editions, so the plugin won't
  load there.
- The `stryke` binary on `$PATH`, or configured under *Settings → Tools →
  Stryke → Stryke executable*. The plugin will look for `st`, then
  `stryke`, on `$PATH`.

## Building

```sh
cd editors/intellij
./gradlew buildPlugin             # produces build/distributions/stryke-intellij-<v>.zip
./gradlew runIde                  # launches a sandbox IDE with the plugin installed
./gradlew verifyPlugin            # plugin verifier against recommended IDE matrix
```

First build downloads the IntelliJ Platform SDK (~1 GB), takes a few
minutes, and is cached under `editors/intellij/.intellijPlatform/` (which
is gitignored).

## Installing

1. *Settings → Plugins → ⚙ → Install Plugin from Disk…*
2. Pick `build/distributions/stryke-intellij-<version>.zip`.
3. Restart the IDE.
4. Open any `.stk` file. The LSP starts automatically; the debugger
   activates when you click Debug.

## Configuration

*Settings → Tools → Stryke*:

| Section | Setting | Default | Notes |
|---------|---------|---------|-------|
| Interpreter | Stryke executable | first `st` then `stryke` on `$PATH` | absolute path or blank |
| LSP | Enable LSP | on | master toggle |
| LSP | Extra LSP args | empty | passed after `--lsp` |
| LSP | LSP environment | empty | `KEY=VAL` pairs (e.g. `RUST_LOG=info`) |
| LSP | Auto-restart LSP on settings change | on | |
| LSP | Show builtin hovers | on | server-provided cards |
| LSP | Log LSP traffic to file | off | + path picker |
| Editor | Disable lexer highlighting | off | rely only on LSP semantic tokens |
| Editor | File extensions | `stk` | comma-separated; add `pl`, `pm` etc. |
| Run configs | Default `--no-interop` | off | strict stryke parser by default |

Color scheme entries: *Settings → Editor → Color Scheme → Stryke* (44
sub-categories grouped under Comments / Strings / Numbers / Regex /
Keywords / Names / Variables / Operators / Punctuation / Errors).

## How the debugger works

Plugin side (`com.menketechnologies.stryke.dap`):
1. `StrykeDebugRunner.doExecute` opens a `ServerSocket(0)` on
   `127.0.0.1`, captures the port.
2. Spawns `st --dap 127.0.0.1:<port>` via `KillableColoredProcessHandler`.
3. Waits up to 10 s for stryke to connect back, then runs DAP over that
   socket — `OSProcessHandler` keeps the process's stdio for Console
   output, exclusively.
4. Creates an `XDebugSession` via `XDebuggerManager.startSession` and
   returns the descriptor via `getMockRunContentDescriptorIfInitialized`
   reflection (avoids the platform's split-debugger
   `Logger.error("[Split debugger] …")` toast that the deprecated
   `runContentDescriptor` getter fires).
5. `StrykeDebugProcess.createConsole` builds a `ConsoleView` and
   `attachToProcess(processHandler)` so program stdout streams in real
   time.
6. `StrykeDapClient` reads Content-Length-framed JSON-RPC from the
   socket — byte-based (not char-based) so multi-byte UTF-8 in variable
   reprs doesn't desync framing.
7. On `stopped` event, `onStopped` synchronously fetches `stackTrace` +
   `scopes` + `variables`, builds `StrykeStackFrame` objects with
   pre-populated children, calls `session.positionReached`. No async
   expansion on the UI thread — IntelliJ 2026.1's split-debugger drops
   those.
8. `StrykeEvaluator` sends `evaluate` requests for the Evaluate dialog;
   stryke does scalar-prelude injection so `$a * $b` returns the right
   value from the paused frame.

Stryke side (`strykelang/dap.rs` + `strykelang/debugger.rs`):
- `Debugger` state machine (breakpoints, step modes, call depth)
  shared between TTY and DAP front-ends. Step-over depends on
  `enter_sub` / `leave_sub` being called at every VM call dispatch site
  (`vm.rs:2192..` and `vm_helper.rs:19216..`) so `call_depth` matches
  the program's logical call stack — without these hooks step-over
  drops into UDFs instead of skipping them.
- Same-line guard tracks both `last_stop_line` and `last_stop_depth`
  (`debugger.rs:38..`). Without the depth half, step-in fires on the
  same source line as the call site (first opcode of the call setup
  has the same line as `my $r = foo()`), requiring the user to click
  step-in twice to actually enter `foo`.
- `set_topic` for implicit `for (@arr) { … }` loops so `$_` / `$_0` /
  `_` / `_0` all alias.
- Snapshot capture (`capture_locals_with_map`) walks the scope, builds
  per-variable refs for hashes / arrays / structs / classes / enums /
  sets / sketches, recursively expanding their children into a
  `var_ref_map` (depth-capped at 12, count-capped at 2000) so the DAP
  `variables` request resolves any ref to its rows.
- stdout/stderr autoflush + flush on every pause so output lands in the
  Console before the suspend UI takes over.

## Plugin architecture

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
    │   ├── StrykeColors.kt            # 44 STRYKE_* TextAttributesKey constants
    │   ├── StrykeTokenTypes.kt        # token type enum
    │   ├── StrykeLexer.kt             # hand-rolled lexer
    │   ├── StrykeSyntaxHighlighter.kt # token → color mapping
    │   ├── StrykeColorSettingsPage.kt # IDE color-scheme entries
    │   ├── StrykeCommenter.kt         # `#` line comment
    │   ├── StrykeSettings.kt          # persistent settings
    │   ├── StrykeSettingsConfigurable.kt
    │   ├── lsp/
    │   │   ├── StrykeLspServerSupportProvider.kt
    │   │   └── StrykeLspServerDescriptor.kt
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

## Version compatibility

Plugin version tracks the strykelang Cargo version. `gradle.properties`
controls the supported IDE range via `pluginSinceBuild` /
`pluginUntilBuild`. Currently targets `2024.2.4` SDK against builds
`242 .. 261.*`.

## Logs

Two append-only logs, both under `~/.stryke/` (or `$STRYKE_HOME/` when
that env var is set):

| File | Source | Contents |
|------|--------|----------|
| `~/.stryke/stryke-plugin.log` | Kotlin (plugin) | LSP command line built, DAP `send` / receive (seq + command + bytes), rename / semantic-token routing, breakpoint handler steps |
| `~/.stryke/stryke.log` | Rust (`stryke --lsp` / `--dap`) | Levelled events (`TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`) from both daemons: startup, initialize, every request method (TRACE), didOpen/Change/Close + diagnostics (DEBUG), rename / hover outcomes, DAP launch / breakpoints / step / pause / disconnect, milestone events (`stopped` / `terminated` / `exited`) |

Tail with `tail -f ~/.stryke/stryke.log ~/.stryke/stryke-plugin.log`.

### Server log level (Rust side)

`$STRYKE_LOG_LEVEL` accepts `trace`, `debug`, `info` (default), `warn`,
`error` (case-insensitive). Lower levels include everything above.

```sh
export STRYKE_LOG_LEVEL=debug   # verbose for daily use
export STRYKE_LOG_LEVEL=trace   # firehose, every request method logged
```

### Rotation

`~/.stryke/stryke.log` rotates automatically when it crosses the
threshold:

| Variable | Default | Meaning |
|----------|---------|---------|
| `STRYKE_LOG_MAX_BYTES` | `5242880` (5 MiB) | Per-file size cap. `0` disables rotation. |
| `STRYKE_LOG_MAX_FILES` | `5` | Number of `.1` … `.N` archives to keep alongside the active file. |

When the active log hits the size cap, `stryke.log.N-1` shifts to
`stryke.log.N` (eldest is overwritten), then the active file moves to
`stryke.log.1`. Default bounds the log dir at roughly `(N+1) ×
MAX_BYTES` = ~30 MiB.

### Redirection / sandboxing

`$STRYKE_LOG_FILE=/abs/path/to.log` overrides the resolved path entirely
— used by tests and sandbox runs. Per-IDE chatter still goes to
`idea.log` via `Logger.getInstance(...)`; these files are for
plugin-and-server events specifically.

## Refactor / Rename

`Shift-F6` on any of these identifiers renames it across the workspace
via the LSP server's `textDocument/rename`:

- Scalar / array / hash variables (`$x`, `@xs`, `%h`) — the sigil is
  included in the identifier extraction, so `$pass` and the `pass`
  builtin no longer collide.
- `my` / `state` / `our` declarations (and the `frozen my` constant form).
- Subroutine / function declarations (`fn`, `sub`).
- Struct / class / enum / trait names and their constructor / method
  call sites (`Point->new`, `Color::Red`).
- Package names declared via `package Foo::Bar;`.

Cross-file rename fires when the symbol is package-scoped (sub, type,
`our`, package). The server scans every other open document, finds
exact-name matches in its `SymbolTable`, and falls back to a textual
qualified-name scan for files that reference the symbol without
re-declaring it. Locally-scoped `my`/`state` decls and sub parameters
are file-scoped and never cross files.

Hovering on the `format` key in `$opts{format}` or the `exec` selector
in `$db->exec` no longer shows the `format` / `exec` builtin card —
those identifiers are hash keys / method selectors, not builtin
references.

## Limitations

- **No PSI tree or structural navigation** — relies entirely on the LSP
  for symbol navigation.
- **Debugger v1**: no conditional or hit-count breakpoints, no exception
  breakpoints, no watch expressions, no Set Value, single-thread only.
  Step-into across `use`d modules works only if the called sub has line
  mapping in the same compilation unit.
- **Evaluator** injects only scalars from the current frame. Expressions
  referencing user-defined `@arr` / `%hash` see them as empty in the
  subprocess. Builtins / globals (`@INC`, `%ENV`, etc.) work.
- **Lexer** is a regex-class scanner; complex constructs (heredocs,
  `qw()`, nested string interpolation) are not fully tokenized — they
  fall back to "string". Server-side semantic tokens fill in where the
  lexer is approximate.
- **`[Split debugger]` toast on Debug start** — the IDE's deprecated
  `XDebugSession.runContentDescriptor` accessor fires `Logger.error`
  even when bypassed via reflection if any third-party code touches it
  during session bring-up. JetBrains' own debug runners suffer the same
  noise in 2024.3+. Cosmetic only; the debugger works.
- **Reflection hashes (`%stryke::*`)** populate lazily on first user
  access in DAP mode (eager `ensure_reflection_hashes` triggers a VM
  stack overflow that's still TBD). The standalone Stryke tool window
  fetches them via `st -e` and is unaffected.

## License

MIT, same as strykelang.
