# Stryke JetBrains Plugin

JetBrains IDE support for [stryke](https://github.com/MenkeTechnologies/strykelang) — the massively
parallel scripting language (Rust bytecode VM + Cranelift JIT + Rayon).

## Features

- **`.stk` file association** with a hand-rolled lexer that highlights keywords, builtins,
  sigil variables (`$`, `@`, `%`), pipes (`|>`, `~>`), regex literals, strings, comments, numbers.
- **LSP client** wired to `st --lsp`: completion, hover (with the full `lsp_docs_domains.rs`
  cards), goto-definition, find-references, rename, document symbols, diagnostics, document
  highlights.
- **Run configurations** with optional `--no-interop` (strict stryke parser) toggle, plus
  context-menu *Run with stryke* on any `.stk` file.
- **Color settings page** under *Settings → Editor → Color Scheme → Stryke*.
- **`#`-line comment** support (Ctrl/Cmd-`/`).

## Requirements

- A paid JetBrains IDE on **2024.2+** (RustRover, IDEA Ultimate, GoLand, PyCharm Pro,
  WebStorm, RubyMine, PhpStorm, CLion, Rider, DataGrip, Aqua).
- The LSP API is **not available in Community editions**, so the plugin will not load there.
- `st` (the stryke binary) must be on `$PATH`, or configured under
  *Settings → Tools → Stryke → Stryke executable*.

## Building

```sh
cd editors/intellij
./gradlew buildPlugin             # produces build/distributions/stryke-intellij-<v>.zip
./gradlew runIde                  # launches a sandbox IDE with the plugin installed
./gradlew verifyPlugin            # plugin verifier against recommended IDE matrix
```

The first build downloads the IntelliJ Platform SDK (~1 GB), takes a few minutes, and is cached.

## Installing

1. *Settings → Plugins → ⚙ → Install Plugin from Disk…*
2. Pick `build/distributions/stryke-intellij-<version>.zip`.
3. Restart the IDE.
4. Open any `.stk` file. The LSP starts automatically.

## Configuration

| Setting | Default | Notes |
|---------|---------|-------|
| `Stryke executable` | first `st` (then `stryke`) on `$PATH` | Settings → Tools → Stryke |
| LSP transport | stdio | spawned with `st --lsp` |
| Run config `--no-interop` | off | enables strict stryke parser (no Perl 5 fallbacks) |

## Plugin architecture

```
editors/intellij/
├── build.gradle.kts                   # IntelliJ Platform Gradle Plugin 2.1.0
├── gradle.properties                  # platform version, plugin version, JVM
├── settings.gradle.kts
└── src/main/
    ├── kotlin/com/menketechnologies/stryke/
    │   ├── StrykeLanguage.kt          # Language singleton
    │   ├── StrykeFileType.kt          # .stk → Stryke
    │   ├── StrykeIcons.kt             # icon loader
    │   ├── StrykeTokenTypes.kt        # token type enum
    │   ├── StrykeLexer.kt             # hand-rolled lexer
    │   ├── StrykeSyntaxHighlighter.kt # token → color mapping
    │   ├── StrykeColorSettingsPage.kt # IDE color-scheme entries
    │   ├── StrykeCommenter.kt         # # line comment
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
    │   │   └── StrykeRunConfigurationProducer.kt
    │   └── actions/
    │       └── RunStrykeFileAction.kt
    └── resources/
        ├── META-INF/plugin.xml
        └── icons/stryke.svg
```

## Version compatibility

Plugin version tracks the strykelang Cargo version. The `pluginSinceBuild` /
`pluginUntilBuild` range in `gradle.properties` constrains the supported IDE builds.

## Limitations (v1)

- No PSI tree or structural navigation — relies entirely on the LSP for symbol navigation.
- No debugger integration.
- Lexer is a regex-class scanner; complex constructs (heredocs, `qw()`, nested string
  interpolation) are not fully tokenized — they fall back to "string".
- Sketch inspector / reflection panels are not yet implemented (future work).

## License

MIT, same as strykelang.
