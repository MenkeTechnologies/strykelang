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

The synthesis above is offered as a contribution to the
language-design tradition. Any language — Perl, Ruby, Python,
Raku, zsh, fish, nushell, OCaml, Scala, a new project, anything —
is invited to absorb, adapt, or build on any stryke-originated idea.
No permission needed beyond what the MIT license already grants.
Examples of inventions other languages can take:

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

The ideas living elsewhere is the goal — that's how a language
contributes to the field. [MAINTAINERS.md](MAINTAINERS.md)
describes how the *official strykelang upstream* keeps itself
recognizable as stryke across maintainer rotations, separate from
how the ideas travel.

License: MIT (see [LICENSE](LICENSE)).
