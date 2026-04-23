# ZSH Codebase Audit

An engineering audit of the zsh C source code. Read the code yourself: it's all there.

## Why Port ZSH to Rust?

Because the C code is indefensible. Not "legacy code that was good for its era" — indefensible by the standards of any era. The Linux kernel was written in the same timeframe with orders of magnitude better code organization, review process, and testing. BSD utilities from the same period have cleaner function decomposition. There is no excuse for what's in this codebase.

147,233 lines of C. Zero unit tests. A custom heap allocator. 186 gotos. 1,940 global mutable statics. A 1,502-line function that handles all command execution. This is the default shell on every Mac in the world.

Rust eliminates entire categories of these bugs by existing. Ownership replaces the hand-rolled heap. The type system replaces 1,032 C casts. The borrow checker replaces 524 manual signal-queue mutex calls. `cargo test` replaces nothing — because there was nothing to replace.

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

### The Heap Trick

macOS `leaks` tool reports 0 leaks on `zsh -f -c` commands. Sounds clean, right? It's not. The custom heap allocator in mem.c (1,882 lines) doesn't free individual allocations — it just blows away the entire heap when the process exits. The OS cleans up after them. It's not "no leaks," it's "we never bothered to free anything."

### The Numbers

- Custom heap allocator reimplements what malloc already does
- **1,465 alloc calls vs 957 frees** — 508 unmatched allocations
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

`string.c` allocates 13 times and **never frees anything**.

### Memory Leak Points

**174 alloc-then-early-return leak points** — places where memory is allocated, then an error path returns without freeing it:

| File | Leak Points |
|------|-------------|
| subst.c | 15 |
| builtin.c | 15 |
| computil.c | 14 |
| utils.c | 12 |
| compctl.c | 10 |
| exec.c | 8 |
| init.c | 8 |
| glob.c | 8 |
| zutil.c | 7 |
| module.c | 7 |
| curses.c | 7 |
| params.c | 6 |

### Heap Alloc Without Cleanup

14 files call `zhalloc`/`hcalloc` (heap allocate) but never call `popheap` (heap free) — they allocate and walk away, relying on someone else to clean up:

| File | Heap Allocs | popheap Calls |
|------|-------------|---------------|
| parameter.c | 39 | 0 |
| compctl.c | 30 | 0 |
| subst.c | 24 | 0 |
| glob.c | 18 | 0 |
| parse.c | 14 | 0 |
| computil.c | 12 | 0 |
| module.c | 9 | 0 |

### String Duplication Leaks

`ztrdup` copies a string. `zsfree` frees it. These files copy strings and never free them:

| File | ztrdup | zsfree | Unfreed |
|------|--------|--------|---------|
| computil.c | 77 | 24 | **53** |
| init.c | 58 | 16 | **42** |
| builtin.c | 42 | 31 | **11** |
| exec.c | 17 | 9 | **8** |
| pcre.c | 8 | 1 | **7** |
| zutil.c | 10 | 3 | **7** |
| stat.c | 6 | 0 | **6** |
| regex.c | 6 | 0 | **6** |
| pattern.c | 6 | 0 | **6** |

In an interactive session running for hours, every tab completion, every glob expansion, every parameter substitution that hits one of these 174 leak points adds unreclaimable memory. There are no tests for this because there are no tests for anything.

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

- **Zero unit tests** on 147,233 lines of C
- Integration tests require shared mutable state across test blocks
- No way to run a single test in isolation
- No way to parallelize tests
- Test harness (`ztst.zsh`) is 632 lines of zsh script that tests the shell by running inside the shell — circular dependency
- Tests depend on ordering: test 47 silently requires state from test 12

## Why This Matters

This is the default shell on every Mac sold since Catalina (2019). Every `brew install`, every developer's `.zshrc`, every CI pipeline on macOS runs through a 1,502-line function with 18 gotos, backed by a custom heap allocator with no unit tests, maintained by a handful of volunteers who never refactored it in 30 years.

Apple chose zsh as the default because the license changed from GPL to MIT. Not because of code quality. Not because of testing. Not because of architecture. Because of a license.

## Completion System (compsys): Library Code in Shell Scripts

The zsh completion system runs core library code as **interpreted shell script**. Not compiled. Not bytecoded. Not cached. Interpreted line by line through the same evaluator that runs through the 1,502-line `execcmd()` with 18 gotos.

### The Numbers

- **986 completion functions** totaling **105,050 lines of shell script**
- **5,397 lines** in the core "standard library" alone (Base/)
- `_git` completion: **9,026 lines** of shell script — bigger than most entire programs

### What Happens When You Press Tab on `git`

11,656 lines of interpreted shell script execute:

| Function | Lines | What it does |
|----------|-------|-------------|
| `_main_complete` | 418 | Entry point dispatch |
| `_complete` | 144 | Completion strategy |
| `_normal` | 40 | Normal completion |
| `_dispatch` | 91 | Function lookup |
| `_git` | 9,026 | Git-specific completions |
| `_arguments` | 589 | Argument parser |
| `_describe` | 140 | Description formatter |
| `_path_files` | 895 | Filesystem walker |
| `_files` | 153 | File completion |
| `_values` | 160 | Value completion |
| **TOTAL** | **11,656** | **Interpreted shell script per Tab press** |

For comparison, the entire Lua interpreter is ~30,000 lines of C. A single `git <TAB>` interprets one-third of a Lua interpreter worth of code — in shell script.

### The Startup Tax

`compinit` runs on **every shell startup**:

1. Iterates over every directory in `$fpath` (43 dirs in a typical setup)
2. Globs every file starting with `_` — **986 files**
3. Opens each file, reads the first line, parses `#compdef` or `#autoload` headers
4. Registers each completion via `compdef` or `autoload`

Cost: **0.49 seconds** even with the `-C` "fast" cached path. Without the cache, it opens and reads all 986 files from disk.

The `-C` flag caches the result in `.zcompdump`, but still validates the cache by comparing file counts — which means it stats every directory in `$fpath` on every startup anyway.

### Why Shell Script?

The entire completion system is shell functions specifically so users can override any piece by putting a file earlier in `$fpath`. That's the design rationale: monkey-patching over performance. The cost is that every Tab press runs at shell-script speed instead of native speed.

`_arguments` is a parser. `_path_files` is a filesystem walker. `_describe` is a formatter. These are operations you write in C or Rust — tight loops, string manipulation, data structure lookups. They wrote them in shell script and run them through an interpreter on every Tab press.

### The zshrs Alternative

zshrs uses SQLite-backed completion indexing. One database lookup instead of 11,656 lines of interpreted shell script. Completions are indexed once at install time, not scanned from disk on every shell startup.

### The Biggest Completion Functions

| Lines | File |
|-------|------|
| 9,026 | `_git` |
| 3,162 | `_perforce` |
| 2,292 | `_gcc` |
| 1,948 | `_tmux` |
| 1,449 | `_zfs` |
| 1,148 | `_postgresql` |
| 964 | `_cvs` |
| 945 | `_mount` |
| 895 | `_path_files` |
| 850 | `_composer` |
| 818 | `_ssh` |
| 809 | `_perf` |
| 801 | `_selinux` |
| 796 | `_apt` |

Every one of these is **interpreted shell script** that runs on every Tab press for that command. Not compiled. Not optimized. Interpreted.

## Autoload: Disk I/O Blocking the User on the Hot Path

When you define an autoloaded function in zsh, this is what you get:

```
zpwrAgIntoFzf () {
    # undefined
    builtin autoload -Xz
}
```

That's not a function. It's a stub. The real function body doesn't exist in memory. When you type `zpwrAgIntoFzf` and press Enter, here's what happens — **blocking your input**:

1. Shell sees the stub, triggers autoload
2. **Scans every directory in `$fpath`** — 43 directories in a typical setup
3. Stats each directory
4. Looks for a file named `zpwrAgIntoFzf` in each one
5. If `.zwc` (wordcode) files exist, reads those binary blobs too
6. Reads the matching file from disk
7. Parses it as shell script
8. Replaces the stub with the real function body
9. Finally executes it

**All of this happens synchronously, blocking the user, on every first invocation of every autoloaded function.**

With 986 completion functions autoloaded via `compinit`, plus user functions, plus framework functions (oh-my-zsh, prezto, zinit all use autoload heavily), a typical shell session has hundreds of these stubs waiting to trigger disk I/O the moment you call them.

### .zwc Files: Fake Compilation

`.zwc` files are zsh's "compiled" format — binary blobs scattered across every fpath directory. They're not real compilation:

- They skip the lex/parse step — that's it
- The shell still **interprets every line** at shell-script speed
- No optimization, no bytecode, no JIT
- Undocumented binary format with no versioning
- Littered across the filesystem with no cleanup mechanism

### The Call Stack

```
User presses Enter
  → shell sees autoload stub
    → scan 43 fpath directories (stat syscalls)
      → find file on disk (open, read syscalls)
        → check for .zwc (more open, read syscalls)
          → parse shell script (lex.c with 22 gotos)
            → replace stub with function body
              → finally execute the function
```

All blocking. All synchronous. All on the hot path between the user pressing Enter and seeing output.

### The zshrs Alternative

zshrs indexes functions at install time in SQLite. Function lookup is one indexed database query — no fpath scanning, no disk I/O on the hot path, no `.zwc` litter.

## Conclusion

Read the code. That's all you need to know about why this port exists.
