# ZSH Codebase Audit

An engineering audit of the zsh C source code at `~/forkedRepos/zsh/Src/`.

## Scale

- **147,233 lines** of C across Src/, Modules/, Builtins/, Zle/
- **2,578 functions**
- **Zero unit tests.** Not one. Anywhere.

## Function Size

- **15 functions over 500 lines** — these aren't functions, they're entire programs
- **87 functions over 200 lines**
- **226 functions over 100 lines** — 9% of all functions
- Worst offender: `execcmd()` in exec.c — **1,502 lines**, a single function that handles all command execution with 18 gotos

Top 25 longest functions:

| Lines | File | Function |
|-------|------|----------|
| 1,502 | exec.c | execcmd |
| 1,096 | Zle/complist.c | domenuselect |
| 1,000 | builtin.c | bin_print |
| 960 | Zle/compctl.c | makecomplistflags |
| 886 | pattern.c | patmatch |
| 798 | glob.c | zglob |
| 747 | Zle/zle_refresh.c | zrefresh |
| 718 | builtin.c | bin_read |
| 656 | Zle/zle_hist.c | doisearch |
| 616 | params.c | strgetfn |
| 615 | builtin.c | bin_fc |
| 610 | prompt.c | putpromptchar |
| 584 | Zle/compmatch.c | matchonce |
| 526 | Zle/compctl.c | get_compctl |
| 505 | builtin.c | bin_typeset |
| 490 | pattern.c | patcomppiece |
| 471 | Zle/computil.c | parse_cadef |
| 444 | builtin.c | bin_functions |
| 434 | subst.c | paramsubst |
| 426 | Zle/compresult.c | calclist |
| 413 | utils.c | getkeystring |
| 399 | Zle/computil.c | ca_parse_line |
| 399 | glob.c | insert |
| 395 | Modules/zutil.c | bin_zparseopts |
| 390 | Zle/compcore.c | callcompfunc |

## Control Flow

- **186 gotos** across the codebase
- **31 switch statements over 100 lines**
- **55 explicit fall-throughs** in switch cases
- **12 levels of nesting** at the deepest point (compresult.c:5074)

Goto hotspots:

| File | Gotos |
|------|-------|
| lex.c | 22 |
| subst.c | 20 |
| exec.c | 18 |
| jobs.c | 12 |
| utils.c | 9 |

## Global Mutable State

- **1,940 static variables** — nearly 2,000 pieces of hidden global state
- **524 queue_signals/unqueue_signals calls** — hand-rolled mutex discipline that every caller must get right manually, or the shell corrupts itself

Worst offenders:

| File | Static Variables |
|------|-----------------|
| params.c | 92 |
| parse.c | 64 |
| exec.c | 63 |
| module.c | 62 |
| utils.c | 56 |
| glob.c | 46 |
| hist.c | 42 |

## Memory Management

- Custom heap allocator in mem.c (1,882 lines) reimplements what malloc already does
- **1,465 alloc calls vs 957 frees** — 508 unmatched allocations by grep count
- Relies on custom heap to "just free everything later" — memory grows unbounded until a heap pop

Per-file imbalance (allocs with no matching free):

| File | Allocs | Frees | Unmatched |
|------|--------|-------|-----------|
| computil.c | 131 | 54 | **77** |
| init.c | 74 | 17 | **57** |
| utils.c | 76 | 31 | **45** |
| parameter.c | 51 | 11 | **40** |
| builtin.c | 79 | 39 | **40** |
| compcore.c | 110 | 71 | **39** |
| exec.c | 55 | 21 | **34** |
| subst.c | 30 | 2 | **28** |
| string.c | 13 | 0 | **13** |

`string.c` allocates 13 times and never frees anything.

## Type Safety

- **1,032 C casts** — `(char *)`, `(void *)`, `(int)` everywhere
- **208 single-character variable declarations** — `int c;`, `char *s;`, `int d;`

## Code Quality

- **1,150 #ifdef/#ifndef blocks** — preprocessor spaghetti for 90s portability hacks still in the code
- **385 TODO/FIXME/HACK/XXX/BUG markers** — acknowledged problems nobody fixed
- **240 DPUTS calls** — printf debugging as the primary debugging strategy
- `sprintf(tmpbuf, "foo %s", cc->str); /* KLUDGE! */` — their word, not mine
- `SUNKEYBOARDHACK` — a shell option literally named "hack" that ships as a first-class feature

## Testing

- Zero unit tests on 147,233 lines of C
- Integration tests require shared mutable state across test blocks
- No way to run a single test in isolation
- No way to parallelize tests
- Test harness (`ztst.zsh`) is itself 632 lines of zsh script with its own bugs
- The test harness tests the shell by running inside the shell — circular dependency

## Why This Matters

This is the default shell on every Mac sold since Catalina (2019). Every `brew install`, every developer's `.zshrc`, every CI pipeline on macOS runs through a 1,502-line function with 18 gotos, backed by a custom heap allocator with no unit tests, maintained by a handful of volunteers who never refactored it in 30 years.

Apple chose zsh as the default because the license changed from GPL to MIT. Not because of code quality. Not because of testing. Not because of architecture. Because of a license.
