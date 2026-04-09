```
 ██████╗ ███████╗██████╗ ██╗     ██████╗ ███████╗
 ██╔══██╗██╔════╝██╔══██╗██║     ██╔══██╗██╔════╝
 ██████╔╝█████╗  ██████╔╝██║     ██████╔╝███████╗
 ██╔═══╝ ██╔══╝  ██╔══██╗██║     ██╔══██╗╚════██║
 ██║     ███████╗██║  ██║███████╗██║  ██║███████║
 ╚═╝     ╚══════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝
```

[![CI](https://github.com/MenkeTechnologies/perlrs/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/perlrs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/perlrs.svg)](https://crates.io/crates/perlrs)
[![Downloads](https://img.shields.io/crates/d/perlrs.svg)](https://crates.io/crates/perlrs)
[![Docs.rs](https://docs.rs/perlrs/badge.svg)](https://docs.rs/perlrs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

### `[PARALLEL PERL5 INTERPRETER // RUST-POWERED EXECUTION ENGINE]`

 ┌──────────────────────────────────────────────────────────────┐
 │ STATUS: ONLINE &nbsp;&nbsp; CORES: ALL &nbsp;&nbsp; SIGNAL: ████████░░       │
 └──────────────────────────────────────────────────────────────┘

> *"There is more than one way to do it — in parallel."*

---

## [0x00] OVERVIEW

`perlrs` is a Perl 5 compatible interpreter written in Rust that brings native parallelism to Perl scripting. It parses and executes Perl 5 scripts with rayon-powered work-stealing parallel primitives across all available CPU cores.

 ┌──────────────────────────────────────────────────────────────┐
 │ RAYON THREADS: ALL CORES &nbsp;&nbsp; REGEX: SIMD-ACCELERATED         │
 │ BINARY SIZE: 2MB STRIPPED &nbsp;&nbsp; BUILD: LTO + O3               │
 └──────────────────────────────────────────────────────────────┘

---

## [0x01] SYSTEM REQUIREMENTS

- Rust toolchain // `rustc` + `cargo`

## [0x02] INSTALLATION

#### DOWNLOADING PAYLOAD FROM CRATES.IO

```sh
cargo install perlrs
```

#### COMPILING FROM SOURCE

```sh
git clone https://github.com/MenkeTechnologies/perlrs
cd perlrs
cargo build --release
```

[perlrs on Crates.io](https://crates.io/crates/perlrs)

#### ZSH COMPLETION // TAB-COMPLETE ALL THE THINGS

```sh
# copy to a directory in your fpath
cp completions/_perlrs /usr/local/share/zsh/site-functions/_perlrs
cp completions/_pe /usr/local/share/zsh/site-functions/_pe

# or add the completions directory to fpath in your .zshrc
fpath=(/path/to/perlrs/completions $fpath)

# then reload completions
autoload -Uz compinit && compinit
```

After reloading your shell, `pe <TAB>` will complete all flags, options, and script files.

---

## [0x03] USAGE

#### EXECUTING INLINE CODE // DIRECT INJECTION

```sh
# inject a single line of perl
pe -e 'print "Hello, world!\n"'

# execute a script file
pe script.pl arg1 arg2

# check syntax without executing
pe -c script.pl
```

#### PROCESSING DATA STREAMS // STDIN OPERATIONS

```sh
# line-by-line processing
echo "data" | pe -ne 'print uc $_'

# auto-print mode (like sed)
cat file.txt | pe -pe 's/foo/bar/g'

# slurp entire input at once
cat file.txt | pe -gne 'print length($_), "\n"'

# auto-split fields
echo "a:b:c" | pe -a -F: -ne 'print $F[1], "\n"'
```

#### PARALLEL EXECUTION // MULTI-CORE OPERATIONS

```perl
# parallel map — transform elements across all cores
my @doubled = pmap { $_ * 2 } @data;

# parallel grep — filter elements in parallel
my @evens = pgrep { $_ % 2 == 0 } @data;

# parallel foreach — execute side effects concurrently
pfor { process($_) } @items;

# parallel sort — sort using all cores
my @sorted = psort { $a <=> $b } @data;

# chain parallel operations
my @result = pmap { $_ ** 2 } pgrep { $_ > 100 } @data;

# control thread count
pe -j 8 -e 'my @r = pmap { heavy_work($_) } @data'
```

Each parallel block receives its own interpreter context with captured lexical scope // no data races.

---

## [0x04] CLI FLAGS

All standard Perl 5 CLI flags are supported:

```
  ── DATA I/O ──────────────────────────────────────────
  -0[octal]          // Specify record separator (\0 if no arg; -0777 for slurp)
  -a                 // Autosplit mode with -n or -p (splits $_ into @F)
  -C[number/list]    // Enable listed Unicode features
  -c                 // Check syntax only (runs BEGIN and CHECK blocks)
  -d[t][:MOD]        // Run program under debugger or module Devel::MOD
  -D[number/letters] // Set debugging flags
  -e CODE            // One line of program (several -e's allowed)
  -E CODE            // Like -e, but enables all optional features
  -f                 // Don't do $sitelib/sitecustomize.pl at startup
  -F/pattern/        // split() pattern for -a switch
  -g                 // Read all input in one go (slurp), alias for -0777
  -i[extension]      // Edit <> files in place (backup if extension supplied)
  -I DIRECTORY       // Specify @INC directory (several allowed)
  -l[octnum]         // Enable line ending processing
  -M MODULE          // Execute "use module..." before program
  -m MODULE          // Execute "use module ()" before program (no import)
  -n                 // Assume "while (<>) { ... }" loop around program
  -p                 // Like -n but print line also, like sed
  -s                 // Enable switch parsing for programfile args
  -S                 // Look for programfile using PATH
  -t                 // Enable tainting warnings
  -T                 // Enable tainting checks
  -u                 // Dump core after parsing program
  -U                 // Allow unsafe operations
  -v                 // Print version, patchlevel and license
  -V[:configvar]     // Print configuration summary
  -w                 // Enable many useful warnings
  -W                 // Enable all warnings
  -x[directory]      // Ignore text before #!perl line
  -X                 // Disable all warnings

  ── PARALLEL EXTENSIONS ───────────────────────────────
  -j N               // Set number of parallel threads (rayon)
```

---

## [0x05] SUPPORTED PERL FEATURES

#### DATA TYPES
- Scalars (`$x`), arrays (`@a`), hashes (`%h`)
- References: `\$x`, `\@a`, `\%h`, `\&sub`
- Array refs `[1,2,3]`, hash refs `{a => 1}`
- Code refs / closures `sub { ... }`
- Regex objects `qr/.../`
- Blessed references (basic OOP)

#### CONTROL FLOW
- `if`/`elsif`/`else`, `unless`
- `while`, `until`, `do...while`
- `for` (C-style), `foreach`
- `last`, `next`, `redo` with labels
- Postfix: `expr if COND`, `expr unless COND`, `expr while COND`, `expr for @list`
- Ternary `?:`

#### OPERATORS
- Arithmetic: `+`, `-`, `*`, `/`, `%`, `**`
- String: `.` (concat), `x` (repeat)
- Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`, `<=>`
- String comparison: `eq`, `ne`, `lt`, `gt`, `le`, `ge`, `cmp`
- Logical: `&&`, `||`, `//`, `!`, `and`, `or`, `not`
- Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>`
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `.=`, `//=`, etc.
- Regex: `=~`, `!~`
- Range: `..`
- Arrow dereference: `->`

#### REGEX ENGINE
- Match: `$str =~ /pattern/flags`
- Substitution: `$str =~ s/pattern/replacement/flags`
- Transliterate: `$str =~ tr/from/to/`
- Flags: `g`, `i`, `m`, `s`, `x`
- Capture variables: `$1`, `$2`, etc.
- Quote-like: `m//`, `qr//`

#### SUBROUTINES
- Named subs with `sub name { ... }`
- Anonymous subs / closures
- Recursive calls
- `@_` argument passing, `shift`, `return`
- `return EXPR if COND` (postfix modifiers on return)

#### BUILT-IN FUNCTIONS

 ┌──────────────────────────────────────────────────────────────┐
 │ **Array**: push, pop, shift, unshift, splice, reverse,      │
 │ sort, map, grep, scalar                                     │
 │ **Hash**: keys, values, each, delete, exists                │
 │ **String**: chomp, chop, length, substr, index, rindex,     │
 │ split, join, sprintf, printf, uc, lc, ucfirst, lcfirst,     │
 │ chr, ord, hex, oct                                          │
 │ **Numeric**: abs, int, sqrt                                 │
 │ **I/O**: print, say, printf, open, close, eof, readline     │
 │ **File tests**: -e, -f, -d, -l, -r, -w, -s, -z             │
 │ **System**: system, exec, exit, chdir, mkdir, unlink         │
 │ **Type**: defined, undef, ref, bless                        │
 │ **Control**: die, warn, eval, do, require, caller           │
 └──────────────────────────────────────────────────────────────┘

#### OTHER FEATURES
- `use strict`, `use warnings` (recognized)
- `package` declarations
- `BEGIN`/`END` blocks
- String interpolation with `$var`, `$hash{key}`, `$array[idx]`
- Heredocs (`<<EOF`)
- `qw()`, `q()`, `qq()`
- POD documentation skipping
- Shebang line handling

---

## [0x06] ARCHITECTURE

```
 ┌─────────────────────────────────────────────────────┐
 │  Source Code                                        │
 │      │                                              │
 │      ▼                                              │
 │  Lexer (src/lexer.rs)                               │
 │      │ Tokens                                       │
 │      ▼                                              │
 │  Parser (src/parser.rs)                             │
 │      │ AST                                          │
 │      ▼                                              │
 │  Interpreter (src/interpreter.rs)                   │
 │      ├── Sequential: map, grep, sort, foreach       │
 │      └── Parallel:   pmap, pgrep, psort, pfor       │
 │              │                                      │
 │              ▼                                      │
 │          RAYON WORK-STEALING SCHEDULER              │
 │          ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓               │
 │          CORE 0 │ CORE 1 │ ... │ CORE N             │
 └─────────────────────────────────────────────────────┘
```

- **Lexer** // Context-sensitive tokenizer handling Perl's ambiguous syntax (regex vs division, hash vs modulo, heredocs, interpolated strings)
- **Parser** // Recursive descent with Pratt precedence climbing for expressions
- **Interpreter** // Tree-walking execution with proper lexical scoping, `Arc<RwLock>` for thread-safe reference types
- **Parallelism** // Each parallel block gets an isolated interpreter with captured scope; rayon handles work-stealing scheduling

---

## [0x07] EXAMPLES

```sh
pe examples/fibonacci.pl
pe examples/text_processing.pl
pe examples/parallel_demo.pl
```

---

## [0x08] DEVELOPMENT & CI

Pull requests and pushes to `main` run the workflow in [`.github/workflows/ci.yml`](.github/workflows/ci.yml). You can also run it manually from the repository **Actions** tab (**workflow dispatch**). On a pull request, the **Checks** tab (or the merge box) shows the aggregate status; open the **CI** workflow run for per-job logs (Check, Test, Format, Clippy, Doc, Release Build).

---

## [0xFF] LICENSE

 ┌──────────────────────────────────────────────────────┐
 │ MIT LICENSE // UNAUTHORIZED REPRODUCTION WILL BE MET │
 │ WITH FULL ICE                                        │
 └──────────────────────────────────────────────────────┘

---

```
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
░░ >>> PARSE. EXECUTE. PARALLELIZE. OWN YOUR CORES. <<< ░░
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
```

##### created by [MenkeTechnologies](https://github.com/MenkeTechnologies)
