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
- [\[0x0D\] Standalone Binaries (`pe build`)](#0x0d-standalone-binaries-pe-build)
- [\[0x0E\] Inline Rust FFI (`rust { ... }`)](#0x0e-inline-rust-ffi-rust-----)
- [\[0x0F\] Bytecode Cache (`.pec`)](#0x0f-bytecode-cache-pec)
- [\[0x10\] Distributed `pmap_on` over SSH (`cluster`)](#0x10-distributed-pmap_on-over-ssh-cluster)
- [\[0x11\] Language Server (`--lsp`)](#0x11-language-server---lsp)
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
pe build script.pl -o myapp             # bake into a standalone binary ([0x0D])
pe --lsp                                # language server over stdio ([0x11])
PERLRS_BC_CACHE=1 pe app.pl             # warm starts skip parse + compile ([0x0F])
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
# map / grep / sort / fold / for in parallel (list can be piped in)
my @doubled = @data |> pmap   { $_ * 2     },    progress => 1;
my @evens   = @data |> pgrep  { $_ % 2 == 0 };
my @sorted  = @data |> psort  { $a <=> $b  };
my $sum     = @numbers |> preduce { $a + $b };
pfor @items { process };

# fused map+reduce, chunked map, memoized map, init fold
my $sum2     = @nums |> pmap_reduce  { $_ * 2 } { $a + $b };
my @squared  = @million |> pmap_chunked 1000 { $_ ** 2 };
my @once     = @inputs |> pcache       { expensive };
my $hist     = @words |> preduce_init {}, { my ($acc, $x) = @_; $acc->{$x}++; $acc };

# fan — run a block N times in parallel ($_ = 0..N-1)
fan 8           { work };
fan             { work };                # uses rayon pool size (`pe -j`)
my @r = fan_cap { $_ * $_ };             # capture results in $_ order

# pipelines — sequential or rayon-backed; same chain methods
my @r = (@data |> pipeline)->filter({ $_ > 10 })->map({ $_ * 2 })->take(100)->collect;
### or 
my @r = @data |> pipeline |> filter { $_ > 10 } |> map { $_ * 2 } |> take 100 |> collect;
my @r = @data |> par_pipeline |> filter { $_ > 10 } |> map { $_ * 2 } |> collect;

# multi-stage: batch (each stage drains list before next)
my $n = par_pipeline(
    source  => { readline(STDIN) },
    stages  => [ { parse_json }, { transform } ],
    workers => [4, 2],
    buffer  => 256,
);

# multi-stage: streaming (bounded crossbeam channels, concurrent stages, order NOT preserved)
my @r = ((1..1_000) |> par_pipeline_stream)->filter({ $_ > 500 })->map({ $_ * 2 })->collect();
## or
my @r = (1..1_000) |> par_pipeline_stream |> filter { $_ > 500 } |> map { $_ * 2 } |> collect;

# channels + Go-style select
my ($tx, $rx) = pchannel(128);           # bounded; pchannel() is unbounded
my ($val, $idx) = pselect($rx1, $rx2);
my ($v, $i)     = pselect($rx1, $rx2, timeout => 0.5);   # $i == -1 on timeout

# barrier — N workers rendezvous
my $sync = barrier(3);
fan 3 { $sync->wait; say "all arrived" };

# persistent thread pool (avoids per-task spawn from pmap/pfor)
my $pool = ppool(4);
$pool->submit { heavy_work }  for @tasks;
my @results = $pool->collect();

# parallel file IO
my @logs = "**/*.log" |> glob_par;              # rayon recursive glob
par_lines "./big.log", { say if /ERROR/ };  # mmap + chunked line scan
par_walk  ".", { say if /\.rs$/ };              # parallel directory walk
par_sed qr/\bfoo\b/, "bar", @paths;             # parallel in-place sed (returns # changed)

# native file watcher (notify crate: inotify/kqueue/FSEvents)
watch  "/tmp/x", { say };
pwatch "logs/*", { heavy };

# control thread count
pe -j 8 -e '@data |> pmap { heavy }'

# distributed pmap over an SSH worker pool — see [0x10] for details
my $cluster = cluster(["build1:8", "build2:16"]);
my @r = @huge |> pmap_on $cluster { heavy };
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
(1..100) |> pfor { push @results, $_ * $_ };

mysync %histogram;
(0..999) |> pfor { $histogram{$_ % 10} += 1 };

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
my $data = "https://api.example.com/users/1" |> fetch_json;
say $data->{name};

my @rows = "data.csv" |> csv_read;
my $df   = "data.csv" |> dataframe;
my $db   = "app.db" |> sqlite;
$db->exec("CREATE TABLE t (id INTEGER, name TEXT)");

struct Point { x => Float, y => Float };
my $p = Point->new(x => 1.5, y => 2.0);
typed my $n : Int;
$n = 42;
```

#### Sets

Native sets deduplicate by value (internal canonical keys; insertion order preserved for `->values`). Use the **`set(LIST)`** builtin or **`Set->new(LIST)`**; **`|>`** can supply the list. **`|`** / **`&`** are union / intersection when either side is a set (otherwise bitwise int ops).

```perl
my $s = set(1, 2, 2, 3);                 # 3 members
my $t = (1, 1, 2, 4) |> set;
my $u = $s | $t;                         # union
my $i = $s & $t;                         # intersection
$s->has(2);                              # 1 / 0  (also ->contains / ->member)
$s->size;                                # count (->len / ->count)
my @v = $s->values; # array in insertion order

# mysync: compound |= and &= update shared sets (see [0x04])
```

---

## [0x06] ASYNC / TRACE / TIMER

```perl
# async / spawn / await — lightweight structured concurrency
my $data = async { "https://example.com/" |> fetch };
my $file = spawn { "big.csv" |> \&slurp };
print await($data), await($file);

# trace mysync mutations to stderr (under fan, lines tagged with worker index)
mysync $counter = 0;
trace { fan 10 { $counter++ } };

# timer / bench — wall-clock millis; bench returns "min/mean/p99"
my $ms     = timer { heavy_work };
my $report = bench { heavy_work } 1000;

# eval_timeout — runs block on a worker thread; recv_timeout on main
eval_timeout 5 { slow };

# retry / rate_limit / every (tree interpreter only)
retry { http_call } times => 3, backoff => "exponential";
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
Scalars `$x`, arrays `@a`, hashes `%h`, refs `\$x`/`\@a`/`\%h`/`\&sub`, anon `[...]`/`{...}`, code refs / closures (capture enclosing lexicals), `qr//` regex objects, blessed references, native sets (`set(LIST)` / `Set->new(...)`), `deque()`, `heap()`.

#### Control flow
`if`/`elsif`/`else`/`unless`, `while`/`until`, `do { } while/until`, C-style `for`, `foreach`, `last`/`next`/`redo` with labels, postfix `if`/`unless`/`while`/`until`/`for`, ternary, `try { } catch ($err) { } finally { }`, `given`/`when`/`default`, algebraic `match (EXPR) { PATTERN [if EXPR] => EXPR, ... }` (regex, array, hash, wildcard, literal patterns; bindings scoped per arm), `eval_timeout SECS { ... }`.

#### Operators
Arithmetic, string `.`/`x`, comparison, `eq`/`ne`/`lt`/`gt`/`cmp`, logical `&&`/`||`/`//`/`!`/`and`/`or`/`not`, bitwise (`|`/`&` are set ops on native `Set`), assignment + compound (`+=`, `.=`, `//=`, …), regex `=~`/`!~`, range `..` / `...` (incl. flip-flop with `eof`), arrow `->`, **pipe-forward `|>`** (perlrs extension — threads the LHS as the **first** argument of the RHS call; see [Extensions beyond stock Perl 5](#extensions-beyond-stock-perl-5)).

#### Regex engine
Three-tier compile (Rust `regex` → `fancy-regex` → PCRE2). Perl `$` end anchor (no `/m`) is rewritten to `(?:\n?\z)`. Match `=~`, dynamic `$str =~ $pat`, substitution `s///`, transliteration `tr///`, flags `g`/`i`/`m`/`s`/`x`, captures `$1`…`$n`, named groups → `%+`/`$+{name}`, `\Q...\E`, `quotemeta`, `m//`/`qr//`. Bare `/pat/` in statement/boolean context is `$_ =~ /pat/`.

#### Subroutines
`sub name { }` with optional prototype, anon subs/closures, implicit return of last expression (VM), `@_`/`shift`/`return`, postfix `return ... if COND`, `AUTOLOAD` with `$AUTOLOAD` set to the FQN.

#### Built-ins (selected)

| Category | Functions |
| --- | --- |
| Array | `push`, `pop`, `shift`, `unshift`, `splice`, `reverse`, `sort`, `map`, `grep`, `reduce`, `fold`, `preduce`, `scalar` |
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
- **`chunked` / `windowed` / `fold`** — Use **pipe-forward**: **`LIST |> chunked(N)`**, **`LIST |> windowed(N)`**, **`LIST |> fold { BLOCK }`** (same for **`reduce`**). `List::Util::fold` / **`qw(...) |> List::Util::fold { }`** alias **`List::Util::reduce`**. List context → arrayrefs per chunk/window or the folded value; scalar context → chunk/window count where applicable.

  ```perl
  my @pairs = (1, 2, 3, 4) |> chunked(2);           # ([1,2], [3,4])
  my @slide = (1, 2, 3) |> windowed(2);             # ([1,2], [2,3])
  my @pipe  = (10, 20, 30) |> chunked(2);           # ([10,20], [30])
  my $sum   = (1, 2, 3, 4) |> fold { $a + $b };     # same as reduce
  my $cat   = qw(a b c) |> List::Util::fold { $a . $b };
  ```
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
- **Distributed compute** ([\[0x10\]](#0x10-distributed-pmap_on-over-ssh-cluster)): `cluster([...])` builds an SSH worker pool; `pmap_on $cluster { } @list` and `pflat_map_on $cluster { } @list` fan a map across persistent remote workers with fault tolerance and per-job retries.
- **Standalone binaries** ([\[0x0D\]](#0x0d-standalone-binaries-pe-build)): `pe build SCRIPT -o OUT` bakes a script into a self-contained executable.
- **Inline Rust FFI** ([\[0x0E\]](#0x0e-inline-rust-ffi-rust-----)): `rust { pub extern "C" fn ... }` blocks compile to a cdylib on first run, dlopen + register as Perl-callable subs.
- **Bytecode cache** ([\[0x0F\]](#0x0f-bytecode-cache-pec)): `PERLRS_BC_CACHE=1` skips parse + compile on warm starts via on-disk `.pec` bundles.
- **Language server** ([\[0x11\]](#0x11-language-server---lsp)): `pe --lsp` runs an LSP server over stdio with diagnostics, hover, completion.
- `mysync` shared state ([\[0x04\]](#0x04-shared-state-mysync)).
- `frozen my`, `typed my`, `struct`, algebraic `match`, `try/catch/finally`, `eval_timeout`, `retry`, `rate_limit`, `every`, `gen { ... yield }`.
- **Pipe-forward `|>`** — parse-time desugaring (zero runtime cost); threads the LHS as the **first** argument of the RHS call, left-associative:

  ```perl
  # chain HTTP fetch → JSON decode → jq filter
  my @titles = $url |> fetch_json |> json_decode |> json_jq '.articles[].title';

  # list pipelines
  my $csv = (1..5) |> map { $_ * $_ } |> join(",");    # "1,4,9,16,25"
  my @big = @rows  |> grep { $_->{n} > 10 } |> sort;

  # unary builtins — `x |> length`, `x |> uc`, etc.
  "hello" |> length |> say;                            # 5
  # builtins parsed as `name(ARG)` only (no bare `name` RHS): use `x |> \&slurp` or `slurp("x")`

  # bareword on RHS becomes a unary call: `x |> f` → `f(x)`
  # call on RHS prepends: `x |> f(a, b)` → `f(x, a, b)`
  # map/grep/sort/join/reduce/fold — LHS fills the list slot
  # chunked/windowed — `LIST |> chunked(N)` prepends the list before the size
  # scalar on RHS: `x |> $cr` → `$cr->(x)`
  ```

  Precedence: `|>` binds **looser** than `||` but **tighter** than `?:` / `and`/`or`/`not` — the slot sits between `parse_ternary` and `parse_or_word` in the parser stack. So `$x + 1 |> f` parses as `f($x + 1)`, and `0 || 1 |> yes` parses as `yes(0 || 1)`. The RHS must be a call, builtin, method invocation, bareword, or coderef expression; bare binary expressions / literals on the right are a parse error (`42 |> 1 + 2` is rejected).

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

```sh
# sets: dedupe + union / intersection (`scalar` gives member count, like `scalar @array`)
pe -e 'my $a = set(1,2,2,3); my $b = set(2,3,4); say scalar($a | $b), " ", scalar($a & $b)'
```

---

## [0x0B] BENCHMARKS

`bash bench/run_bench.sh` — perlrs vs perl 5.42.2 on Apple M5 18-core. Mean of 10 hyperfine runs with 3 warmups; **includes process startup** (not steady-state).

```
 perlrs benchmark harness (honest mode)
 ---------------------------------------
  perl5:   perl 5, version 42, subversion 2 (v5.42.2) built for darwin-thread-multi-2level
  perlrs:  This is perlrs v0.1.35 — A highly parallel Perl 5 interpreter (Rust)
  cores:   18
  warmup:  3 runs
  measure: hyperfine (min 10 runs)

  bench          perl5 ms   perlrs ms    noJit ms  perturb ms  rs/perl5  jit/noJit
  ---------      --------   ---------    --------   ---------  --------  ---------
  startup             2.7         3.7         4.0         3.5     1.37x     1.08x
  fib               192.0         8.3         8.5         8.3     0.04x     1.02x
  loop               92.7         3.7         3.8         3.8     0.04x     1.03x
  string             10.7         4.3         4.4         4.4     0.40x     1.02x
  hash               32.0         7.4         7.7         7.6     0.23x     1.04x
  array              26.1        10.3        10.5        10.5     0.39x     1.02x
  regex              91.8        13.3        13.0        13.1     0.14x     0.98x
  map_grep           51.9        15.3        15.3        15.8     0.29x     1.00x

  pmap vs map (perlrs only, 50k items with per-item work)
  bench            map ms     pmap ms     speedup
  ---------      --------    --------    --------
  pmap              272.7       684.8       0.40x

```

**perlrs beats perl5 on 7 of 8 benches** — `fib` and `loop` ~26x, `string` 2.6x, `array` 2.4x, `map_grep` 3.5x. Losses: `hash` 1.46x (Perl 5 hash access is heavily tuned), `regex` 1.08x (effectively a tie), `startup` 1.36x (~900 µs Rust binary load).

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

## [0x0D] STANDALONE BINARIES (`pe build`)

Compile any Perl script to a single self-contained native executable. The output is a copy of the `pe` binary with the script source embedded as a zstd-compressed trailer. `scp` it to any compatible machine and run it — **no `perl`, no `perlrs`, no `@INC`, no CPAN**.

```sh
pe build app.pl                         # → ./app
pe build app.pl -o /usr/local/bin/app   # explicit output path
./app --any --script --args             # all argv reach the embedded script's @ARGV
```

**What's in the box:**

- Parse / compile errors are surfaced **at build time**, not when users run the binary.
- The embedded script is detected at startup by a 32-byte trailer sniff (~50 µs), then decompressed and executed by the embedded VM. A script with no trailer runs normally as `pe`.
- Builds are idempotent: `pe build app.pl -o app` followed by `pe --exe app build other.pl -o other` strips the previous trailer first, so binaries never stack.
- Unix: the output is marked `+x` automatically. macOS: unsigned — `codesign` before distribution if your environment requires it.
- Current AOT runtime sets `@INC = (".")`; modules outside the embedded script have to be inlined. (`require` of a local `.pm` next to the running binary still works.)

**Under the hood** ([`src/aot.rs`](src/aot.rs)): trailer layout is `[zstd payload][u64 compressed_len][u64 uncompressed_len][u32 version][u32 reserved][8B magic b"PERLRSBN"]`. ELF / Mach-O loaders ignore bytes past the mapped segments so the embedded payload is invisible to the OS loader. The `b"PERLRSBN"` magic plus version byte lets a future pre-compiled-bytecode payload ship alongside v1 without breaking already-shipped binaries.

```sh
# 13 MB binary, no external runtime required:
$ pe build hello.pl -o hello
pe build: wrote hello
$ file hello
hello: Mach-O 64-bit executable arm64
$ ./hello alice
hi alice
```

---

## [0x0E] INLINE RUST FFI (`rust { ... }`)

Drop a block of Rust directly into a Perl script. On first run, perlrs compiles it to a cdylib (cached at `~/.cache/perlrs/ffi/<hash>.{dylib,so}`), `dlopen`s it, and registers every exported function as a regular Perl-callable sub.

```perl
rust {
    pub extern "C" fn add(a: i64, b: i64) -> i64 { a + b }
    pub extern "C" fn mul3(x: f64, y: f64, z: f64) -> f64 { x * y * z }
    pub extern "C" fn fib(n: i64) -> i64 {
        let (mut a, mut b) = (0i64, 1i64);
        for _ in 0..n { let t = a + b; a = b; b = t; }
        a
    }
}

say add(21, 21);         # 42
say mul3(1.5, 2.0, 3.0); # 9
say fib(50);             # 12586269025
```

**v1 signature table** (parser rejects anything outside this — users write private Rust helpers freely, only exported fns matching the table become Perl-callable):

| rust signature                               | perl call         |
|----------------------------------------------|-------------------|
| `fn() -> i64` / `fn(i64, ...) -> i64` (1–4 args) | integer → integer  |
| `fn() -> f64` / `fn(f64, ...) -> f64` (1–3 args) | float → float      |
| `fn(*const c_char) -> i64`                   | string → integer   |
| `fn(*const c_char) -> *const c_char`         | string → string    |

**Requirements**: `rustc` must be on `PATH`. First-run compile costs ~1 second; subsequent runs hit the cache and pay only `dlopen` (~10 ms). `#[no_mangle]` is auto-inserted by the wrapper — you don't need to write it. The body is `#![crate_type = "cdylib"]` with `use std::os::raw::c_char; use std::ffi::{CStr, CString};` already in scope.

**How it works** ([`src/rust_sugar.rs`](src/rust_sugar.rs), [`src/rust_ffi.rs`](src/rust_ffi.rs)): the source-level pre-pass desugars every top-level `rust { ... }` into a `BEGIN { __perlrs_rust_compile("<base64 body>", $line); }` call. The `__perlrs_rust_compile` builtin hashes the body, compiles via `rustc --edition=2021 -O` if the cache is cold, `libc::dlopen`s the result, `dlsym`s each detected signature, and stores the raw symbol + arity/type tag in a process-global registry. Calls from Perl flow through a fallback arm in [`crate::builtins::try_builtin`] that dispatches on the signature tag via direct function-pointer transmute — no libffi dep, no per-call alloc, no marshalling overhead beyond the `PerlValue::to_int` / `to_number` / `to_string` calls you'd do for any builtin.

**Combine with AOT for zero-friction deployment:** `pe build script.pl -o prog` bakes the Perl source — which includes the `rust { ... }` block — into a standalone binary. The FFI compile still happens on first run of `./prog`, but the user only needs `rustc` once, then the `~/.cache/perlrs/ffi/` entry is permanent.

**Limitations (v1):**

- Unix only (macOS + Linux). Windows support is a dlopen-equivalent swap away but isn't wired.
- Signatures beyond the table above are silently ignored (the function still exists in the cdylib, just not Perl-callable).
- Body must be self-contained Rust with `std` only — no `Cargo.toml` / external crate deps. If you need `regex` or similar, vendor the minimal code into the block.
- The cdylib runs with the calling process's privileges. Trust model is equivalent to `do FILE`.

---

## [0x0F] BYTECODE CACHE (`.pec`)

`PERLRS_BC_CACHE=1` enables the on-disk bytecode cache. The first run of a script parses + compiles + persists a `.pec` bundle to `~/.cache/perlrs/bc/<sha256>.pec`. Every subsequent run skips **both parse and compile** and feeds the cached chunk straight into the VM.

```sh
PERLRS_BC_CACHE=1 pe my_app.pl              # cold: parse + compile + save
PERLRS_BC_CACHE=1 pe my_app.pl              # warm: load + dispatch
```

**Measured impact** (Apple M5, 13 MB release `pe`, hyperfine `--warmup 5 -N`, mean ± σ):

| script              | cold (no cache) | warm (.pec)    | speedup       | `.pec` size |
|---------------------|-----------------|----------------|---------------|-------------|
| 6 002 lines, 3000 subs | **67.9 ms ± 5.1** | **19.9 ms ± 1.0** | **3.41×**     | 47 KB       |
| 1 002 lines, 500 subs  | 6.8 ms ± 0.5    | 6.5 ms ± 0.5    | 1.06× wall, **1.32× user CPU** | 5 KB |
| 3 lines (toy)       | 3.5 ms ± 0.3    | 4.8 ms ± 0.4    | cache loses    | 1.9 KB      |

The toy-script result is the honest one to call out: for tiny scripts the cache deserialize cost outweighs the parse cost it replaces. The cache wins decisively on anything substantial — startup time becomes O(deserialize) instead of O(parse + compile).

**Tuning knobs:**

- `PERLRS_BC_CACHE=1` — opt-in. (V1 is opt-in to avoid surprising users with stray cache files; flip to opt-out once we have a `pe cache prune` subcommand and confidence in invalidation.)
- `PERLRS_BC_DIR=/path/to/dir` — override the cache location. Useful for test isolation and CI.

**Format** ([`src/pec.rs`](src/pec.rs)): `[4B magic b"PEC2"][zstd-compressed bincode of PecBundle]`. The `PecBundle` carries `format_version`, `pointer_width` (so a cache built on a 64-bit host is rejected on 32-bit), `strict_vars` (a mismatch is treated as a clean miss → re-compile), `source_fingerprint`, the parsed `Program`, and the compiled `Chunk`. Format version 2 introduced zstd compression — files dropped ~10× in size and warm-load latency dropped with them.

**Cache key** ([`pec::source_fingerprint`](src/pec.rs)): SHA-256 of `(crate version, source filename, full source including -M prelude)`. Editing the script, upgrading perlrs, or changing the `-M` flags all force a recompile. The crate version is mixed in so a `cargo install perlrs` upgrade silently invalidates everyone's cache rather than risking a stale-bytecode mismatch.

**Pairs with [`pe build`](#0x0d-standalone-binaries-pe-build):** AOT binaries pick up the cache for free. The first run of a shipped binary parses and compiles the embedded source; every subsequent run on the same machine reuses the cached chunk. The cache key includes the script name baked into the trailer, so two binaries with different embedded scripts never collide.

**Limitations (v1):**

- **Bypassed for `-e` / `-E` one-liners.** Measured: warm `.pec` is ~2-3× *slower* than cold for tiny scripts because the deserialize cost (~1-2 ms for fs read + zstd decode + bincode) dominates the parse+compile work it replaces (~500 µs). Each unique `-e` invocation would also pollute the cache directory with no GC. The break-even is around 1000 lines, so file-based scripts only.
- Bypassed for `-n` / `-p` / `--lint` / `--check` / `--ast` / `--fmt` / `--profile` modes (those paths run a different driver loop).
- No automatic eviction yet — old `.pec` files for edited scripts accumulate. `rm ~/.cache/perlrs/bc/*.pec` is a fine workaround until `pe cache prune` lands.
- Cache hit path cannot fall back to the tree walker mid-run — but this is unreachable in practice because `compile_program` only emits ops the VM implements before persisting.

---

## [0x10] DISTRIBUTED `pmap_on` OVER SSH (`cluster`)

Distribute a `pmap`-style fan-out across many machines via SSH. The dispatcher spawns one persistent `pe --remote-worker` process per slot, performs a HELLO + SESSION_INIT handshake **once** per slot, then streams JOB frames over the same stdin/stdout. Pairs perfectly with `pe build`: ship one binary to N hosts, fan the workload across them.

```perl
# Build the worker pool. Each spec maps to one or more `ssh HOST PE --remote-worker` lanes.
my $cluster = cluster([
    "build1:8",                          # 8 slots on build1, default `pe` from PATH
    "alice@build2:16",                   # 16 slots, ssh as alice
    "build3:4:/usr/local/bin/pe",        # 4 slots, custom remote pe path
    { host => "data1", slots => 12, pe => "/opt/pe" },  # hashref form
    { timeout => 30, retries => 2, connect_timeout => 5 },  # trailing tunables
]);

my @hashes = @big_files |> pmap_on $cluster { sha256(slurp_raw($_)) };

# pflat_map_on for one-to-many mapping
my @lines = @log_paths |> pflat_map_on $cluster { split /\n/, slurp($_) };
```

#### Cluster syntax

Each list element to `cluster([...])` is one of:

| Form | Meaning |
|------|---------|
| `"host"` | One slot on `host`, remote `pe` from `$PATH` |
| `"host:N"` | `N` slots on `host` |
| `"host:N:/path/to/pe"` | `N` slots, custom remote `pe` binary |
| `"user@host:N"` | `ssh` user override (kept verbatim, passed through to ssh) |
| `{ host => "...", slots => N, pe => "..." }` | Hashref form with explicit fields |
| trailing `{ timeout => SECS, retries => N, connect_timeout => SECS }` | Cluster-wide tunables (must be the last argument; consumed only when **all** keys are tunable names) |

**Tunables** (defaults shown):

| Key | Default | Meaning |
|-----|---------|---------|
| `timeout` (alias `job_timeout`) | `60` | Per-job wall-clock budget in seconds. Slots that exceed this are killed and the job is re-enqueued. |
| `retries` | `2` | Retries per job on top of the initial attempt. `retries=2` → up to 3 total tries. |
| `connect_timeout` | `10` | `ssh -o ConnectTimeout=N` for the initial handshake. |

#### Architecture

```
main thread                       ┌── slot 0 (ssh build1) ────┐
┌──────────────────┐              │  worker thread + ssh proc  │
│ enqueue all jobs ├──► work_tx ─►│  HELLO + SESSION_INIT once │
│ collect results  │              │  loop: take JOB from queue │
└──────────────────┘              │        send + read         │
        ▲                         │        push to results     │
        │                         └────────────────────────────┘
        │                         ┌── slot 1 (ssh build1) ────┐
        │                         │  worker thread + ssh proc  │
        │                         └────────────────────────────┘
        │                         ┌── slot 2 (ssh build2) ────┐
        │                         │  ...                       │
        │                         └────────────────────────────┘
        │                                    │
        └────────── result_rx ───────────────┘
```

Each slot runs in its own thread and pulls JOB messages from a shared crossbeam channel. Work-stealing emerges naturally — fast slots drain the queue faster, slow slots take fewer jobs. **No round-robin assignment**, which was the basic v1 implementation's biggest performance bug (fast hosts sat idle while slow hosts queued). The Interpreter on each remote worker is **reused across jobs** so package state, sub registrations, and module loads survive between items.

#### Wire protocol (v2)

Every message is `[u64 LE length][u8 kind][bincode payload]`. The single-byte `kind` discriminator lets future revisions extend the protocol without breaking older workers — an unknown kind is a hard error so version skew is loud. See [`src/remote_wire.rs`](src/remote_wire.rs).

```text
dispatcher                    worker
    │                            │
    │── HELLO ─────────────────►│   (proto version, build id)
    │◄───────────── HELLO_ACK ──│   (worker pe version, hostname)
    │── SESSION_INIT ──────────►│   (subs prelude, block source, captured lexicals)
    │◄────────── SESSION_ACK ───│   (or ERROR)
    │── JOB(seq=0) ────────────►│   (item)
    │◄────────── JOB_RESP(0) ───│
    │── JOB(seq=1) ────────────►│
    │◄────────── JOB_RESP(1) ───│
    │           ...             │
    │── SHUTDOWN ──────────────►│
    │                            └─ exit 0
```

The basic v1 protocol shipped the entire subs prelude on **every** job and spawned a fresh ssh process **per item**. For a 10k-item map across 8 hosts that's 10 000 ssh handshakes (~50–200 ms each) + 10 000 copies of the subs prelude over the wire — minutes of overhead before any work runs. The v2 persistent session amortizes the handshake across the whole map and ships the prelude once.

#### Fault tolerance

When a slot's read or write fails (ssh died, network blip, remote crash, per-job timeout), the worker thread re-enqueues the in-flight job to the shared queue with `attempts++` and exits. Other living slots pick the job up. A job is permanently failed when its attempt count reaches `cluster.max_attempts`. The whole map fails only when **every** slot is dead or every queued job has exhausted its retry budget.

#### `pe --remote-worker`

The worker subprocess. Reads a HELLO frame from stdin, parses subs prelude + block source from SESSION_INIT exactly once, then handles JOB frames in a loop until SHUTDOWN or stdin EOF. Started by the dispatcher via `ssh HOST PE_PATH --remote-worker`. Also reachable directly for local testing:

```sh
echo "..." | pe --remote-worker      # reads framed wire protocol from stdin
pe --remote-worker-v1                # legacy one-shot session for compat tests
```

#### Limitations (v1)

- **Unix only** — hardcoded `ssh`, hardcoded POSIX dlopen path. Windows would need a similar shim.
- **JSON-marshalled values** — `serde_json` round-trip loses bigints, blessed refs, and other heap-only `PerlValue` payloads. The supported types are: undef, bool, i64, f64, string, array, hash. Anything outside that returns an error from `pmap_on`.
- **`mysync` / atomic capture is rejected** — shared state across remote workers can't honour the cross-process mutex semantics in v1. Use the result list and aggregate locally.
- **No streaming results** — the dispatcher buffers the full result vector before returning. For huge fan-outs this is the next thing to fix (likely via `pchannel` integration).
- **No SSH connection pool across calls** — each `pmap_on` invocation builds fresh sessions. Subsequent `pmap_on` calls in the same script reconnect from scratch.

---

## [0x11] LANGUAGE SERVER (`--lsp`)

`pe --lsp` runs an LSP server over stdio. Hooks into the existing parser, lexer, and symbol table — no separate analyzer to maintain. Surfaces:

- **Diagnostics** on save (parse + compile errors with line / column / message)
- **Hover docs** for builtins (`pmap`, `cluster`, `fetch_json`, `dataframe`, …) — including the parallel and cluster primitives from sections [\[0x03\]](#0x03-parallel-primitives) and [\[0x10\]](#0x10-distributed-pmap_on-over-ssh-cluster)
- **Symbol lookup** for subs and packages within the open file
- **Completion** for built-in function names and the keywords listed in [\[0x08\]](#0x08-supported-perl-features)

Wire it into VS Code, JetBrains, or any LSP-aware editor by pointing the client at `pe --lsp` as the language-server command. There is no `Cargo.toml`-style separate `perlrs-lsp` binary in v1 — the same `pe` you run scripts with also acts as its own language server when invoked with `--lsp`.

```jsonc
// .vscode/settings.json
{
  "perlrs.serverPath": "/usr/local/bin/pe",
  "perlrs.serverArgs": ["--lsp"]
}
```

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
