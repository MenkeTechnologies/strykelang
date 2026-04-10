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

> *"There is more than one way to do it — in parallel."*

A Perl 5 compatible interpreter in Rust with native parallel primitives, NaN-boxed values, three-tier regex, bytecode VM + Cranelift JIT, and rayon work-stealing across all cores.

---

## Table of Contents

- [\[0x00\] Overview](#0x00-overview)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Usage](#0x02-usage)
- [\[0x03\] Parallel Primitives](#0x03-parallel-primitives)
- [\[0x04\] Shared State (`mysync`)](#0x04-shared-state-mysync)
- [\[0x05\] Native Data Scripting](#0x05-native-data-scripting)
- [\[0x06\] Async / Trace / Timer](#0x06-async--trace--timer)
- [\[0x07\] CLI Flags](#0x07-cli-flags)
- [\[0x08\] Supported Perl Features](#0x08-supported-perl-features)
- [\[0x09\] Architecture](#0x09-architecture)
- [\[0x0A\] Examples](#0x0a-examples)
- [\[0x0B\] Benchmarks](#0x0b-benchmarks)
- [\[0x0C\] Development & CI](#0x0c-development--ci)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] OVERVIEW

`perlrs` parses and executes Perl 5 scripts with rayon-powered work-stealing primitives across every CPU core. Highlights:

- **Runtime values** — `PerlValue` is a NaN-boxed `u64`: immediates (`undef`, `i32`, raw `f64` bits) and tagged `Arc<HeapObject>` pointers for big ints, strings, arrays, hashes, refs, regexes, atomics, channels.
- **Three-tier regex** — Rust [`regex`](https://docs.rs/regex) → [`fancy-regex`](https://docs.rs/fancy-regex) (backrefs) → [`pcre2`](https://docs.rs/pcre2) (PCRE-only verbs).
- **Bytecode VM + JIT** — match-dispatch interpreter with Cranelift block + linear-sub JIT (`src/vm.rs`, `src/jit.rs`).
- **Rayon parallelism** — every parallel builtin uses work-stealing across all cores.
- **Binary size** ~10 MB stripped (LTO + O3).

---

## [0x01] INSTALL

```sh
cargo install perlrs
# or from source
git clone https://github.com/MenkeTechnologies/perlrs && cd perlrs && cargo build --release
```

#### Zsh tab completion

```sh
cp completions/_perlrs /usr/local/share/zsh/site-functions/_perlrs
# or: fpath=(/path/to/perlrs/completions $fpath) in .zshrc
autoload -Uz compinit && compinit
```

`pe <TAB>` then completes flags, options, and script files.

---

## [0x02] USAGE

```sh
pe -e 'print "Hello, world!\n"'         # inline code
pe script.pl arg1 arg2                  # script + args
pe -lane 'print $F[0]'                  # bundled short switches
pe -c script.pl                         # syntax check
pe --lint script.pl                     # parse + compile (no run)
pe --disasm script.pl                   # bytecode listing on stderr
pe --ast script.pl                      # AST as JSON
pe --fmt script.pl                      # pretty-print parsed source
pe --profile script.pl                  # folded stacks + per-line/per-sub ns
pe --explain E0001                      # expanded hint for an error code
```

#### Interactive REPL

Run `pe` in a terminal with no script and no `-e`/`-n`/`-p`/etc. to enter a readline session: line editing, history (`~/.perlrs_history`), tab completion for keywords, lexicals in scope, sub names, methods after `->` on blessed objects, and file paths. `exit`/`quit`/Ctrl-D leaves. Non-TTY stdin is read as a complete program.

#### `__DATA__`

A line whose trimmed text is exactly `__DATA__` ends the program; the trailing bytes are exposed via the `DATA` filehandle.

#### Stdin / `-n` / `-p` / `-i`

```sh
echo data | pe -ne 'print uc $_'        # line loop
cat f.txt | pe -pe 's/foo/bar/g'        # auto-print like sed
pe -i -pe 's/foo/bar/g' file1 file2     # in-place edit (parallel across files)
pe -i.bak -pe 's/x/y/g' *.txt           # with backup suffix
echo a:b:c | pe -aF: -ne 'print $F[1]'  # auto-split
```

`-l` chomps each record and sets `$\`. `eof` with no args is true on the last line of stdin or each `@ARGV` file (Perl-compat).

**Text decoding** — script reads, `require`, `do`, `slurp`, `<>`, backticks, `par_lines`, etc. all use UTF-8 when valid, else Latin-1 octets per line/chunk (matches stock `perl` tolerance). `use open ':encoding(UTF-8)'` switches `<>` to UTF-8 with `U+FFFD` replacement.

---

## [0x03] PARALLEL PRIMITIVES

Each parallel block runs in its own interpreter context with **captured lexical scope** — no data races. Use `mysync` for shared counters. Optional `progress => 1` enables an animated stderr bar (TTY) or per-item log lines (non-TTY).

```perl
# map / grep / sort / fold / for in parallel
my @doubled = pmap   { $_ * 2     } @data,    progress => 1;
my @evens   = pgrep  { $_ % 2 == 0 } @data;
my @sorted  = psort  { $a <=> $b  } @data;
my $sum     = preduce{ $a + $b    } @numbers;
pfor                 { process }    @items;

# fused map+reduce, chunked map, memoized map, init fold
my $sum2     = pmap_reduce  { $_ * 2 } { $a + $b } @nums;
my @squared  = pmap_chunked 1000 { $_ ** 2 } @million;
my @once     = pcache       { expensive } @inputs;
my $hist     = preduce_init {}, { my ($acc, $x) = @_; $acc->{$x}++; $acc } @words;

# fan — run a block N times in parallel ($_ = 0..N-1)
fan 8           { work };
fan             { work };                # uses rayon pool size (`pe -j`)
my @r = fan_cap { $_ * $_ };             # capture results in $_ order

# pipelines — sequential or rayon-backed; same chain methods
my @r = pipeline(@data)->filter({ $_ > 10 })->map({ $_ * 2 })->take(100)->collect();
my @r = par_pipeline(@data)->filter({ $_ > 10 })->map({ $_ * 2 })->collect();

# multi-stage: batch (each stage drains list before next)
my $n = par_pipeline(
    source  => { readline(STDIN) },
    stages  => [ { parse_json }, { transform } ],
    workers => [4, 2],
    buffer  => 256,
);

# multi-stage: streaming (bounded crossbeam channels, concurrent stages, order NOT preserved)
my @r = par_pipeline_stream((1..1_000))->filter({ $_ > 500 })->map({ $_ * 2 })->collect();

# channels + Go-style select
my ($tx, $rx) = pchannel(128);           # bounded; pchannel() is unbounded
my ($val, $idx) = pselect($rx1, $rx2);
my ($v, $i)     = pselect($rx1, $rx2, timeout => 0.5);   # $i == -1 on timeout

# barrier — N workers rendezvous
my $sync = barrier(3);
fan 3 { $sync->wait; say "all arrived" };

# persistent thread pool (avoids per-task spawn from pmap/pfor)
my $pool = ppool(4);
$pool->submit({ heavy_work }) for @tasks;
my @results = $pool->collect();

# parallel file IO
my @logs = glob_par("**/*.log");                # rayon recursive glob
par_lines "./big.log", sub { say if /ERROR/ };  # mmap + chunked line scan
par_walk  ".", { say if /\.rs$/ };              # parallel directory walk
par_sed qr/\bfoo\b/, "bar", @paths;             # parallel in-place sed (returns # changed)

# native file watcher (notify crate: inotify/kqueue/FSEvents)
watch  "/tmp/x", { say };
pwatch "logs/*", sub { ... };

# control thread count
pe -j 8 -e 'pmap { heavy } @data'
```

**Parallel capture safety** — workers set `Scope::parallel_guard` after restoring captured lexicals. Assignments to captured non-`mysync` aggregates are rejected at runtime; `mysync`, package-qualified names, and topics (`$_`/`$a`/`$b`) are allowed. `pmap`/`pgrep` treat block failures as `undef`/false; use `pfor` when failures must abort.

---

## [0x04] SHARED STATE (`mysync`)

`mysync` declares variables backed by `Arc<Mutex>` shared across parallel blocks. Compound ops (`++`, `+=`, `.=`, `|=`, `&=`) hold the lock for the full read-modify-write cycle — fully atomic.

```perl
mysync $counter = 0;
fan 10000 { $counter++ };               # always exactly 10000
print $counter;

mysync @results;
pfor { push @results, $_ * $_ } (1..100);

mysync %histogram;
pfor { $histogram{$_ % 10} += 1 } (0..999);

# deque() and heap(...) already use Arc<Mutex<...>> internally
mysync $q  = deque();
mysync $pq = heap({ $a <=> $b });
```

For `mysync` scalars holding a `Set`, `|`/`&` are union/intersection. Without `mysync`, each thread gets an independent copy.

---

## [0x05] NATIVE DATA SCRIPTING

| Area | Builtins |
| --- | --- |
| **HTTP** ([`ureq`](https://crates.io/crates/ureq)) | `fetch`, `fetch_json`, `fetch_async`, `await fetch_async_json`, `par_fetch` |
| **JSON** ([`serde_json`](https://crates.io/crates/serde_json)) | `json_encode`, `json_decode` |
| **CSV** ([`csv`](https://crates.io/crates/csv)) | `csv_read` (AoH), `csv_write`, `par_csv_read` |
| **DataFrame** | `dataframe(path)` → columnar; `->filter`, `->group_by`, `->sum`, `->nrow`, `->ncol` |
| **SQLite** ([`rusqlite`](https://crates.io/crates/rusqlite), bundled) | `sqlite(path)` → `->exec`, `->query`, `->last_insert_rowid` |
| **TOML / YAML** | `toml_decode`, `yaml_decode` |
| **Crypto** | `sha256`, `hmac`, `hmac_sha256`, `uuid`, `base64_encode/decode`, `hex_encode/decode` |
| **Compression** ([`flate2`](https://crates.io/crates/flate2), [`zstd`](https://crates.io/crates/zstd)) | `gzip`, `gunzip`, `zstd`, `zstd_decode` |
| **Time** ([`chrono`](https://crates.io/crates/chrono), [`chrono-tz`](https://crates.io/crates/chrono-tz)) | `datetime_utc`, `datetime_from_epoch`, `datetime_parse_rfc3339`, `datetime_strftime`, `datetime_now_tz`, `datetime_format_tz`, `datetime_parse_local`, `datetime_add_seconds` |
| **Structs / Types** | `struct Name { x => Float, y => Int }; Name->new(...)`; `typed my $x : Int\|Str\|Float` |

```perl
my $data = fetch_json("https://api.example.com/users/1");
say $data->{name};

my @rows = csv_read("data.csv");
my $df   = dataframe("data.csv");
my $db   = sqlite("app.db");
$db->exec("CREATE TABLE t (id INTEGER, name TEXT)");

struct Point { x => Float, y => Float };
my $p = Point->new(x => 1.5, y => 2.0);
typed my $n : Int;
$n = 42;
```

---

## [0x06] ASYNC / TRACE / TIMER

```perl
# async / spawn / await — lightweight structured concurrency
my $data = async { fetch_url("https://example.com/") };
my $file = spawn { slurp("big.csv") };
print await($data), await($file);

# trace mysync mutations to stderr (under fan, lines tagged with worker index)
mysync $counter = 0;
trace { fan 10 { $counter++ } };

# timer / bench — wall-clock millis; bench returns "min/mean/p99"
my $ms     = timer { heavy_work() };
my $report = bench { heavy_work() } 1000;

# eval_timeout — runs block on a worker thread; recv_timeout on main
eval_timeout 5 { slow };

# retry / rate_limit / every (tree interpreter only)
retry { http_call() } times => 3, backoff => "exponential";
rate_limit(10, "1s") { hit_api() };
every("500ms") { tick() };

# generators — lazy `yield` values
my $g = gen { yield $_ for 1..5 };
my $next = $g->next;                    # [value, more]
```

---

## [0x07] CLI FLAGS

![pe -h](img/pe-help.png)

---

## [0x08] SUPPORTED PERL FEATURES

#### Data
Scalars `$x`, arrays `@a`, hashes `%h`, refs `\$x`/`\@a`/`\%h`/`\&sub`, anon `[...]`/`{...}`, code refs / closures (capture enclosing lexicals), `qr//` regex objects, blessed references, native `Set->new(...)`, `deque()`, `heap()`.

#### Control flow
`if`/`elsif`/`else`/`unless`, `while`/`until`, `do { } while/until`, C-style `for`, `foreach`, `last`/`next`/`redo` with labels, postfix `if`/`unless`/`while`/`until`/`for`, ternary, `try { } catch ($err) { } finally { }`, `given`/`when`/`default`, algebraic `match (EXPR) { PATTERN [if EXPR] => EXPR, ... }` (regex, array, hash, wildcard, literal patterns; bindings scoped per arm), `eval_timeout SECS { ... }`.

#### Operators
Arithmetic, string `.`/`x`, comparison, `eq`/`ne`/`lt`/`gt`/`cmp`, logical `&&`/`||`/`//`/`!`/`and`/`or`/`not`, bitwise (`|`/`&` are set ops on native `Set`), assignment + compound (`+=`, `.=`, `//=`, …), regex `=~`/`!~`, range `..` / `...` (incl. flip-flop with `eof`), arrow `->`.

#### Regex engine
Three-tier compile (Rust `regex` → `fancy-regex` → PCRE2). Perl `$` end anchor (no `/m`) is rewritten to `(?:\n?\z)`. Match `=~`, dynamic `$str =~ $pat`, substitution `s///`, transliteration `tr///`, flags `g`/`i`/`m`/`s`/`x`, captures `$1`…`$n`, named groups → `%+`/`$+{name}`, `\Q...\E`, `quotemeta`, `m//`/`qr//`. Bare `/pat/` in statement/boolean context is `$_ =~ /pat/`.

#### Subroutines
`sub name { }` with optional prototype, anon subs/closures, implicit return of last expression (VM), `@_`/`shift`/`return`, postfix `return ... if COND`, `AUTOLOAD` with `$AUTOLOAD` set to the FQN.

#### Built-ins (selected)

| Category | Functions |
| --- | --- |
| Array | `push`, `pop`, `shift`, `unshift`, `splice`, `reverse`, `sort`, `map`, `grep`, `reduce`, `preduce`, `scalar` |
| Hash | `keys`, `values`, `each`, `delete`, `exists` |
| String | `chomp`, `chop`, `length`, `substr`, `index`, `rindex`, `split`, `join`, `sprintf`, `printf`, `uc`/`lc`/`ucfirst`/`lcfirst`, `chr`, `ord`, `hex`, `oct`, `crypt`, `fc`, `pos`, `study`, `quotemeta` |
| Binary | `pack`, `unpack` (subset `A a N n V v C Q q Z H x` + `*`) |
| Numeric | `abs`, `int`, `sqrt`, `sin`, `cos`, `atan2`, `exp`, `log`, `rand`, `srand` |
| I/O | `print`, `say`, `printf`, `open` (incl. `open my $fh`, files, `-\|` / `\|-` pipes), `close`, `eof`, `readline`, handle methods `->print/->say/->printf/->getline/->close/->eof/->getc/->flush`, `slurp`, backticks/`qx{}`, `capture` (structured: `->stdout/->stderr/->exit`), `binmode`, `fileno`, `flock`, `getc`, `sysread/syswrite/sysseek`, `select`, `truncate` |
| Directory | `opendir`, `readdir`, `closedir`, `rewinddir`, `telldir`, `seekdir` |
| File tests | `-e`, `-f`, `-d`, `-l`, `-r`, `-w`, `-s`, `-z`, `-x`, `-t` (defaults to `$_`) |
| System | `system`, `exec`, `exit`, `chdir`, `mkdir`, `unlink`, `rename`, `chmod`, `chown`, `stat`, `lstat`, `link`, `symlink`, `readlink`, `glob`, `glob_par`, `par_sed`, `ppool`, `barrier`, `fork`, `wait`, `waitpid`, `kill`, `alarm`, `sleep`, `times` |
| Sockets | `socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `shutdown` |
| Type | `defined`, `undef`, `ref`, `bless` |
| Control | `die`, `warn`, `eval`, `do`, `require`, `caller`, `wantarray`, `goto LABEL`, `continue { }` on loops, `prototype` |

#### Perl-compat highlights

- **OOP** — `@ISA` (incl. `our @ISA` outside `main`), C3 MRO (live, not cached), `$obj->SUPER::method`. `tie` for scalars/arrays/hashes with `TIESCALAR/TIEARRAY/TIEHASH`, `FETCH`/`STORE`, plus `EXISTS`/`DELETE` on tied hashes.
- **`use overload`** — `'op' => 'method'` or `\&handler`; binary dispatch with `(invocant, other)`, `nomethod`, unary `neg`/`bool`/`abs`, `""` for stringification, `fallback => 1`.
- **`$?` / `$|`** — packed POSIX status from `system`/backticks/pipe close; autoflush on print/printf.
- **`$.`** — undef until first successful read, then last-read line count.
- **`print`/`say`/`printf` with no args** — uses `$_` (and `printf`'s format defaults to `$_`).
- **Bareword statement** — `name;` calls a sub with `@_ = ($_)`.
- **Typeglobs** — `*foo = \&bar`, `*lhs = *rhs` copies sub/scalar/array/hash/IO slots; package-qualified `*Pkg::name` supported.
- **`%SIG` (Unix)** — `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` as code refs; handlers run between statements/opcodes via `perl_signal::poll`. `IGNORE` and `DEFAULT` honored.
- **`format` / `write`** — partial: `format NAME = ... .` registers a template; pictures `@<<<<`, `@>>>>`, `@||||`, `@####`, `@****`, literal `@@`. `write` (no args) uses `$~` to stdout. Not yet: `write FILEHANDLE`, `$^`, `formline`.
- **`@INC` / `%INC` / `require` / `use`** — `@INC` is built from `-I`, `vendor/perl`, system `perl`'s `@INC` (set `PERLRS_NO_PERL_INC` to skip), the script dir, `PERLRS_INC`, then `.`. `List::Util` is implemented natively in Rust (`src/list_util.rs`). `use Module qw(a b);` honors `@EXPORT_OK`/`@EXPORT`. Built-in pragmas (`strict`, `warnings`, `utf8`, `feature`, `open`, `Env`) do not load files.
- **`use strict`** — refs/subs/vars modes (per-mode `use strict 'refs'` etc.). `strict refs` rejects symbolic derefs at runtime; `strict vars` requires a visible binding.
- **`BEGIN` / `UNITCHECK` / `CHECK` / `INIT` / `END`** — Perl order; `${^GLOBAL_PHASE}` matches Perl in tree-walker and VM.
- **String interpolation** — `$var`, `$h{k}`, `$a[i]`, `@a`, `@a[slice]` (joined with `$"`), `$#a` in slice indices, `$0`, `$1..$n`. `\x{hex}` and unbraced `\x`.
- **`__FILE__` / `__LINE__`** — compile-time literals.
- Heredocs `<<EOF`, POD skipping, shebang handling, `qw()/q()/qq()` with paired delimiters.
- **Special variables** — large set of `${^NAME}` scalars pre-seeded; see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md). Still missing vs Perl 5: `English`, full `$^V` as a version object.

#### Extensions beyond stock Perl 5

- Native CSV (`csv_read`/`csv_write`), columnar `dataframe`, embedded `sqlite`.
- HTTP (`fetch`/`fetch_json`/`fetch_async`/`par_fetch`), JSON (`json_encode`/`json_decode`).
- Crypto, compression, time, TOML, YAML helpers (see [\[0x05\]](#0x05-native-data-scripting)).
- All parallel primitives in [\[0x03\]](#0x03-parallel-primitives) (`pmap`, `fan`, `pipeline`, `par_pipeline_stream`, `pchannel`, `pselect`, `barrier`, `ppool`, `glob_par`, `par_walk`, `par_lines`, `par_sed`, `pwatch`, `watch`).
- `mysync` shared state ([\[0x04\]](#0x04-shared-state-mysync)).
- `frozen my`, `typed my`, `struct`, algebraic `match`, `try/catch/finally`, `eval_timeout`, `retry`, `rate_limit`, `every`, `gen { ... yield }`.

`perlrs` is **not** a full `perl` replacement: many real `.pm` files (especially XS modules) will not run. See [`PARITY_ROADMAP.md`](PARITY_ROADMAP.md).

---

## [0x09] ARCHITECTURE

```
 ┌─────────────────────────────────────────────────────┐
 │  Source ──▶ Lexer ──▶ Parser ──▶ AST                │
 │                                    │                │
 │                                    ▼                │
 │                            Compiler (compiler.rs)   │
 │                                    │                │
 │                                    ▼                │
 │                            Bytecode (bytecode.rs)   │
 │                                    │                │
 │            ┌───────────────────────┼───────────┐    │
 │            ▼                       ▼           ▼    │
 │   Tree-walker fallback     VM (vm.rs)   Cranelift   │
 │   (interpreter.rs)            │            JIT       │
 │                               ▼                      │
 │                  Rayon work-stealing scheduler       │
 │                  CORE 0 │ CORE 1 │ ... │ CORE N      │
 └─────────────────────────────────────────────────────┘
```

- **Lexer** ([`src/lexer.rs`](src/lexer.rs)) — context-sensitive tokenizer for Perl's ambiguous syntax (regex vs division, hash vs modulo, heredocs, interpolation).
- **Parser** ([`src/parser.rs`](src/parser.rs)) — recursive descent + Pratt precedence climbing.
- **Compiler / VM** ([`src/compiler.rs`](src/compiler.rs), [`src/vm.rs`](src/vm.rs)) — match-dispatch interpreter; `try_vm_execute` runs bytecode first then falls back to tree-walker on `CompileError::Unsupported` or unsupported ops. Compiled subs use slot ops for frame-local `my` scalars (O(1)). Lowering covers `BEGIN`/`UNITCHECK`/`CHECK`/`INIT`/`END` with `Op::SetGlobalPhase`, `mysync`, `tie`, scalar compound assigns via `Scope::atomic_mutate`, regex values, named-sub coderefs, folds, `pcache`, `pselect`, `par_lines`, `par_walk`, `par_sed`, `pwatch`, `each`, four-arg `substr`, dynamic `keys`/`values`/`delete`/`exists`, etc.
- **JIT** ([`src/jit.rs`](src/jit.rs)) — Cranelift two-tier JIT (linear-sub + block) with cached `OwnedTargetIsa`, tiered after `PERLRS_JIT_SUB_INVOKES` (default 50) interpreter invocations. Block JIT validates a CFG, joins typed `i64`/`f64` slots at merges, and compiles straight-line numeric hot loops. Disable with `--no-jit` / `PERLRS_NO_JIT=1`.
- **Feature work policy** — prefer **new VM opcodes** in [`bytecode.rs`](src/bytecode.rs), lowering in [`compiler.rs`](src/compiler.rs), implementation in [`vm.rs`](src/vm.rs). Do **not** add new `ExprKind`/`StmtKind` variants for new behavior.
- **Tree-walker** ([`src/interpreter.rs`](src/interpreter.rs)) — fallback execution with `Arc<RwLock>` for thread-safe ref types; `fib_like_tail.rs` specializes simple integer-base-case recursive `f(n-1)+f(n-2)` patterns to avoid nested scope frames.
- **Parallelism** — each parallel block spawns an isolated interpreter with captured scope; rayon does work-stealing.

---

## [0x0A] EXAMPLES

```sh
pe examples/fibonacci.pl
pe examples/text_processing.pl
pe examples/parallel_demo.pl
```

---

## [0x0B] BENCHMARKS

`bash bench/run_bench.sh` — perlrs vs perl 5.42.2 on Apple M5 18-core. Mean of 10 hyperfine runs with 3 warmups; **includes process startup** (not steady-state).

```
 ---------------------------------------
  perl5:   perl 5, version 42, subversion 2 (v5.42.2) built for darwin-thread-multi-2level
  perlrs:  This is perlrs v0.1.29 — A highly parallel Perl 5 interpreter (Rust)
  cores:   18
  warmup:  3 runs
  measure: hyperfine (min 10 runs)

  bench          perl5 ms   perlrs ms    noJit ms  perturb ms  rs/perl5  jit/noJit
  ---------      --------   ---------    --------   ---------  --------  ---------
  startup             2.6         3.5         3.6         3.7     1.35x     1.03x
  fib               196.4         7.7         7.8         7.8     0.04x     1.01x
  loop               95.6         4.0         4.3         3.9     0.04x     1.07x
  string             11.6         4.8         4.9         4.6     0.41x     1.02x
  hash               35.0         7.9         8.4         7.5     0.23x     1.06x
  array              27.2        10.7        12.5        11.9     0.39x     1.17x
  regex              98.2        14.0        14.2        14.4     0.14x     1.01x
  map_grep           57.1        17.1        16.6        17.7     0.30x     0.97x

  pmap vs map (perlrs only, 50k items with per-item work)
  bench            map ms     pmap ms     speedup
  ---------      --------    --------    --------
  pmap              272.7       684.8       0.40x

```

**perlrs beats perl5 on 5 of 8 benches** — `fib` and `loop` ~26x, `string` 2.6x, `array` 2.4x, `map_grep` 3.5x. Losses: `hash` 1.46x (Perl 5 hash access is heavily tuned), `regex` 1.08x (effectively a tie), `startup` 1.36x (~900 µs Rust binary load).

**JIT impact is essentially zero on this suite** (`jit/noJit` within ±6%). The wins over Perl 5 come from the **bytecode interpreter**, not the JIT — the current Cranelift block JIT only covers a narrow band of frame-slot numeric hot loops.

The `noJit` column is `perlrs --no-jit`. The `perturb` column re-runs each bench through a renamed but functionally-equivalent file so any future shape-matcher that short-circuits the canonical bench files would show a divergence.

#### Parallel speedup

```
  map  (50k items, per-item work):  236.1 ms
  pmap (50k items, 18 cores):       465.1 ms   →  0.51x
```

`pmap` is **slower** on this workload — 50k items × per-item cost is too small to amortize worker spin-up. Parallel wins require heavier per-item work (~100 ms+) or much larger N.

---

## [0x0C] DEVELOPMENT & CI

Pull requests and pushes to `main` run [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (Check, Test, Format, Clippy, Doc, Release Build).

```sh
cargo test --lib                # parser smoke, lexer/value/error/scope, interpreter, vm, jit
cargo test --test integration   # tests/suite/* (runtime, readline list context, line-mode stdin, …)
cargo bench --bench jit_compare # JIT vs interpreter on the same bytecode
bash bench/run_bench.sh         # full perl5 vs perlrs suite (needs hyperfine)
bash parity/run_parity.sh       # exact stdout/stderr parity vs system perl
```

- `Cargo.lock` is committed (CI uses `--locked`). If your global gitignore strips it, force-add updates: `git add -f Cargo.lock`.
- Disable JIT: `PERLRS_NO_JIT=1` or `pe --no-jit`.
- Parity work is tracked in [`PARITY_ROADMAP.md`](PARITY_ROADMAP.md).

---

## [0xFF] LICENSE

MIT — see [`LICENSE`](LICENSE).

---

```
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
░░ >>> PARSE. EXECUTE. PARALLELIZE. OWN YOUR CORES. <<< ░░
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
```

##### created by [MenkeTechnologies](https://github.com/MenkeTechnologies)
