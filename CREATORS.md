# Creators

stryke was created by **Jacob Menke** ([MenkeTechnologies](https://github.com/MenkeTechnologies)).

Original synthesis (2026–): the language design, the bytecode VM (`fusevm`), the
threading-operator family (`|>` / `~>` / `->>`), implicit-positional closure
parameters (`_0` / `_1` / `$_<<`), the `--no-interop` boundary, the AOP
intercept system, the AI-native primitives, and the Rosetta / Exercism
corpora that double as the regression net — all originated here.

For governance, contribution flow, and current maintainers, see
[MAINTAINERS.md](MAINTAINERS.md).

## Porting stryke ideas to other languages

The synthesis above is offered as **prior art for the language-design
commons**, not as fenced IP. Any language — Perl, Ruby, Python, Raku,
zsh, fish, nushell, OCaml, Scala, a new project, anything — is welcome
to absorb, adapt, or fully port any stryke-originated idea. No
permission needed beyond what the MIT license already grants. Examples
of inventions that other languages should feel free to take:

- The three-axis threading-operator family (`|>` / `~>` / `->>`) with
  bare-fn / arrow-block / placeholder forms in one operator family.
- Implicit-positional closure parameters with depth-encoding
  (`_0` / `_0<` / `_0<<` / `_N<<<<`).
- The `--no-interop` "bot firewall" pattern (parser-enforced idiomatic
  rules that bots can't ignore by reading docs).
- The AOP intercept layer for shells / scripting languages.
- AI primitives as syntactic forms (`ai`, `tool fn`, MCP integration
  at the language level rather than as a library).
- Per-package symbol-table stashes refreshed lazily on read.
- The encyclopedic-stdlib design philosophy itself (inverting "core
  minimal, libraries optional" → "core encyclopedic, libraries
  unnecessary").

What [MAINTAINERS.md](MAINTAINERS.md) governs is the *official
strykelang upstream* — protecting it from identity-dissolving
changes. It does not restrict anyone from copying stryke's ideas
elsewhere.

License: MIT (see [LICENSE](LICENSE)).
