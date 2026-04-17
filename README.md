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
- [\[0x12\] Language Reflection](#0x12-language-reflection)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] OVERVIEW

`perlrs` parses and executes Perl 5 scripts with rayon-powered work-stealing primitives across every CPU core. Highlights:

- **New Parallel Subroutines and |> Pipeline Syntactic Sugar**
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
pe --flame script.pr                    # colored flamegraph bars in terminal
pe --flame script.pr > flame.svg        # interactive SVG flamegraph when piped
pe --explain E0001                      # expanded hint for an error code
pe docs                                  # interactive reference book (vim-style: j/k/]/[/t/q)
pe docs pmap                             # jump straight to a topic
pe docs --toc                            # table of contents
pe docs --search parallel                # search all pages
pe serve 8080                           # static file server for $PWD
pe serve 8080 app.pr                    # HTTP server with handler script
pe serve 3000 -e '"hello " . $req->{path}'  # one-liner HTTP server
pe build script.pl -o myapp             # bake into a standalone binary ([0x0D])
pe --lsp                                # language server over stdio ([0x11])
PERLRS_BC_CACHE=1 pe app.pl             # warm starts skip parse + compile ([0x0F])
```

#### Semicolons

A newline ends a statement, so you do not need a trailing `;` on each line. Use semicolons only when you put more than one statement on the same physical line.

```perl
my $answer = 40 + 2
p $answer                       # 42 — one statement per line, no `;` required

my $x = 1; my $y = 2; p $x + $y # 3 — same line needs `;` between statements
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
# Three surface forms work for pmap/pgrep/pfor/pcache/pflat_map:
#   pmap { $_ * 2 } @list              # block form  ($_ = element)
#   pmap $_ * 2, @list                 # expression form
#   pmap double, @list                 # bare-fn form (sub double { $_0 * 2 })
my @doubled = @data |> pmap $_ * 2 , progress => 1;
my @evens   = @data |> pgrep $_ % 2 == 0;
my @sorted  = @data |> psort { $a <=> $b };
my $sum     = @numbers |> preduce { $a + $b };
pfor process, @items;
my @hashes  = pmap sha256, @blobs, progress => 1;     # bare-fn

# fused map+reduce, chunked map, memoized map, init fold
my $sum2     = @nums |> pmap_reduce { $_ * 2 } { $a + $b };
my @squared  = @million |> pmap_chunked 1000 { $_ ** 2 };
my @once     = @inputs |> pcache expensive;
my $hist     = @words |> preduce_init {}, { my ($acc, $x) = @_; $acc->{$x}++; $acc };

# fan — run a block or fn N times in parallel ($_/$_0 = index 0..N-1)
fan 8, work;                       # bare-fn form: fan N, FUNC
fan work, progress => 1;           # uses rayon pool size (`pe -j`)
fan 8 { work($_) };                # block form
fan { work($_) };                  # block form, pool-sized
my @r = fan_cap 8, compute;        # capture results in index order
my @r = fan_cap 8 { $_ * $_ };     # block form, capture

# pipelines — sequential or rayon-backed; same chain methods
my @r = (@data |> pipeline)->filter({ $_ > 10 })->map({ $_ * 2 })->take(100)->collect;
### or 
my @r = @data |> pipeline |> filter $_ > 10 |> map $_ * 2 |> take 100 |> collect;
my @r = @data |> par_pipeline |> filter  $_ > 10 |> map $_ * 2 |> collect;

# multi-stage: batch (each stage drains list before next)
my $n = par_pipeline(
    source  => { readline(STDIN) },
    stages  => [ parse_json, transform ],
    workers => [4, 2],
    buffer  => 256,
);

# multi-stage: streaming (bounded crossbeam channels, concurrent stages, order NOT preserved)
my @r = ((1..1_000) |> par_pipeline_stream)->filter({ $_ > 500 })->map({ $_ * 2 })->collect();
## or
my @r = (1..1_000) |> par_pipeline_stream |> filter $_ > 500 |> map $_ * 2 |> collect;

# channels + Go-style select
my ($tx, $rx) = pchannel(128);           # bounded; pchannel() is unbounded
my ($val, $idx) = pselect($rx1, $rx2);
my ($v, $i)     = pselect($rx1, $rx2, timeout => 0.5);   # $i == -1 on timeout

# barrier — N workers rendezvous
my $sync = barrier(3);
# p is alias to say
fan 3 { $sync->wait; p "all arrived" };

# persistent thread pool (avoids per-task spawn from pmap/pfor)
my $pool = ppool(4);
$pool->submit({ heavy_work($_) }) for @tasks;
my @results = $pool->collect();

# parallel file IO
my @logs = "**/*.log" |> glob_par;              # rayon recursive glob
par_lines "./big.log", { p if /ERROR/ };  # mmap + chunked line scan
par_walk  ".", { p if /\.rs$/ };              # parallel directory walk
par_sed qr/\bfoo\b/, "bar", @paths;             # parallel in-place sed (returns # changed)
my @rs = par_find_files "src", "*.rs";           # parallel recursive file search by glob
my $n  = par_line_count @rs;                     # parallel line count across files

# native file watcher (notify crate: inotify/kqueue/FSEvents)
watch  "/tmp/x", p;
pwatch "logs/*", heavy;

# control thread count
pe -j 8 -e '@data |> pmap heavy'

# distributed pmap over an SSH worker pool — see [0x10] for details
my $cluster = cluster(["build1:8", "build2:16"]);
my @r = @huge |> pmap_on $cluster heavy;
```

**Parallel capture safety** — workers set `Scope::parallel_guard` after restoring captured lexicals. Assignments to captured non-`mysync` aggregates are rejected at runtime; `mysync`, package-qualified names, and topics (`$_`/`$a`/`$b`) are allowed. `pmap`/`pgrep` treat block failures as `undef`/false; use `pfor` when failures must abort.

**Outer topic `$_<`** — inside nested blocks (`fan`, `fan_cap`, `map`, `grep`, `>{}`), `$_` is rebound per iteration. Use `$_<` to access the **previous** topic, `$_<<` for two levels up, up to `$_<<<<` (4 levels). This is a perlrs extension — stock Perl 5 has no equivalent.

```perl
t 10 >{fan `say "outer topic is $_< and inner topic is $_"`}

$_ = 100;
my @r = fan_cap 3 { $_< };               # each worker sees outer topic → (100, 100, 100)

$_ = 100;
my @r = fan_cap 2 {
    my $outer = $_<;                      # 100
    my $cr = sub { $outer + $_< };        # $_< inside sub = caller's $_
    $cr->($_);                            # fan sets $_ = 0, 1
};                                        # @r = (100, 101)

$_ = 50; t 10 >{ $_ + $_< };             # 60 — thread sub stage accesses outer topic

$_ = "outer";
fan_cap 1 { $_ = "inner"; "$_< $_" };    # "outer inner" — interpolation works
```

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
mysync $pq = heap { $a <=> $b };
```

For `mysync` scalars holding a `Set`, `|`/`&` are union/intersection. Without `mysync`, each thread gets an independent copy.

---

## [0x05] NATIVE DATA SCRIPTING

| Area | Builtins |
| --- | --- |
| **HTTP** ([`ureq`](https://crates.io/crates/ureq)) | `fetch`, `fetch_json`, `fetch_async`, `await fetch_async_json`, `par_fetch`, `serve` |
| **JSON** ([`serde_json`](https://crates.io/crates/serde_json)) | `json_encode`, `json_decode` |
| **CSV** ([`csv`](https://crates.io/crates/csv)) | `csv_read` (AoH), `csv_write`, `par_csv_read` |
| **DataFrame** | `dataframe(path)` → columnar; `->filter`, `->group_by`, `->sum`, `->nrow`, `->ncol` |
| **SQLite** ([`rusqlite`](https://crates.io/crates/rusqlite), bundled) | `sqlite(path)` → `->exec`, `->query`, `->last_insert_rowid` |
| **TOML / YAML** | `toml_decode`, `yaml_decode` |
| **Crypto** | `sha1`, `sha224`, `sha256`, `sha384`, `sha512`, `md5`, `hmac`, `hmac_sha256`, `crc32`, `uuid`, `base64_encode/decode`, `hex_encode/decode` |
| **Compression** ([`flate2`](https://crates.io/crates/flate2), [`zstd`](https://crates.io/crates/zstd)) | `gzip`, `gunzip`, `zstd`, `zstd_decode` |
| **Time** ([`chrono`](https://crates.io/crates/chrono), [`chrono-tz`](https://crates.io/crates/chrono-tz)) | `datetime_utc`, `datetime_from_epoch`, `datetime_parse_rfc3339`, `datetime_strftime`, `datetime_now_tz`, `datetime_format_tz`, `datetime_parse_local`, `datetime_add_seconds`, `elapsed` |
| **Structs / Types** | `struct Name { x => Float, y => Int }; Name(x => 1, y => 2)`; `typed my $x : Int\|Str\|Float`; typed sub params `fn ($a: Int, $b: Str) { }` |

```perl
my $data = "https://api.example.com/users/1" |> fetch_json;
p $data->{name};

# Built-in HTTP server — one-liner web API
serve 8080, fn ($req) {
    # $req = { method, path, query, headers, body, peer }
    my $data = +{ path => $req->{path}, method => $req->{method} };
    status => 200, body => json_encode($data)
};
# or with workers: serve 8080, $handler, { workers => 16 };
# JSON content-type auto-detected; undef returns 404

my @rows = "data.csv" |> csv_read;
my $df   = "data.csv" |> dataframe;
my $db   = "app.db" |> sqlite;
$db->exec("CREATE TABLE t (id INTEGER, name TEXT)");

# ─── Structs ────────────────────────────────────────────────────────
# Declaration: typed fields, optional defaults, or bare (Any type)
struct Point { x => Float, y => Float };
struct Point { x => Float = 0.0, y => Float = 0.0 };   # with defaults
struct Pair { key, value };                             # untyped (Any)

# Construction: function-call, positional, or traditional ->new
my $p = Point(x => 1.5, y => 2.0);   # function-call with named args
my $p = Point(1.5, 2.0);             # positional (declaration order)
my $p = Point->new(x => 1.5, y => 2.0); # traditional OO style
my $p = Point();                     # uses defaults if defined

# Field access: getter (0 args) or setter (1 arg)
p $p->x;       # 1.5 — getter
$p->x(3.0);    # setter
p $p->x;       # 3.0

# User-defined methods
struct Circle {
    radius => Float,
    fn area { 3.14159 * $self->radius ** 2 }
    fn scale($factor: Float) {
        Circle(radius => $self->radius * $factor)
    }
}
my $c = Circle(radius => 5);
p $c->area;                          # 78.53975
p $c->scale(2);                      # Circle(radius => 10)

# Built-in methods
my $q = $p->with(y => 5);            # functional update — new instance
my $h = $p->to_hash;                 # { x => 3.0, y => 5 }
my @f = $p->fields;                  # (x, y)
my $c = $p->clone;                   # deep copy

# Smart stringify — print shows struct name and fields
p $p;                                # Point(x => 3, y => 2)

# Structural equality — compares all fields
my $a = Point(1, 2);
my $b = Point(1, 2);
p $a == $b;                          # 1 (equal)
# ────────────────────────────────────────────────────────────────────

typed my $n : Int = 42;

# Typed sub parameters — runtime type checking on call
my $add = fn ($a: Int, $b: Int) { $a + $b };
p $add->(3, 4);                              # 7
# $add->("x", 1);                            # ERROR: sub parameter $a: expected Int

sub greet ($name: Str) { "Hello, $name!" }
p greet("world");                            # Hello, world!

# stringify/str — convert any value to a parseable perlrs literal
my $data = {a => [1, 2], b => "hello"};
my $s = str $data;                           # +{a => [1, 2], b => "hello"}
my $copy = eval $s;                          # round-trip via eval
p $copy->{a}[0];                             # 1

# stringify works with functions (first-class serialization)
my $f = fn ($x: Int) { $x * 2 };
p str $f;                                    # sub ($x: Int) { $x * 2; }
my $f2 = eval str $f;                        # round-trip: deserialize back to callable
p $f2->(21);                                 # 42

# streaming range — bidirectional lazy iterator
range(1, 5) |> e p;                          # 1 2 3 4 5
range(5, 1) |> e p;                          # 5 4 3 2 1
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
my $ms     = timer heavy_work;
my $report = bench heavy_work 1000;

# eval_timeout — runs block on a worker thread; recv_timeout on main
eval_timeout 5 slow;

# retry / rate_limit / every (tree interpreter only)
retry http_call times => 3, backoff => exponential;
rate_limit(10, "1s") hit_api;
every "500ms" tick;

# generators — lazy `yield` values
my $g = gen { yield $_ for 1..5 };
my $next = $g->next;                    # [value, more]
```

---

## [0x07] CLI FLAGS

All stock `perl` flags are supported: `-0`, `-a`, `-c`, `-C`, `-d`, `-D`, `-e`, `-E`, `-f`, `-F`, `-g`, `-h`, `-i`, `-I`, `-l`, `-m`, `-M`, `-n`, `-p`, `-s`, `-S`, `-t`, `-T`, `-u`, `-U`, `-v`, `-V`, `-w`, `-W`, `-x`, `-X`. Perl-style single-dash (`-version`, `-help`) and GNU-style double-dash (`--version`, `--help`) long forms work. Bundled switches are expanded: `-Mstrict` → `-M strict`, `-I/tmp` → `-I /tmp`, `-V:version` → `-V version`, `-lane` → `-l -a -n -e`.

perlrs-specific long flags:

| Flag | Description |
| --- | --- |
| `--lint` / `--check` | Parse + compile bytecode without running |
| `--disasm` / `--disassemble` | Print bytecode disassembly to stderr before VM execution |
| `--ast` | Dump parsed AST as JSON and exit |
| `--fmt` | Pretty-print parsed Perl to stdout and exit |
| `--profile` | Wall-clock profile: per-line + per-sub timings on stderr |
| `--flame` | Flamegraph: colored terminal bars when interactive, SVG when piped (`pe --flame x.pr > flame.svg`) |
| `--no-jit` | Disable Cranelift JIT (bytecode interpreter only) |
| `--compat` | Perl 5 strict-compatibility mode: disable all perlrs extensions (`\|>`, `struct`, `match`, `pmap`, `#{expr}`, etc.) |
| `--explain CODE` | Print expanded hint for an error code (e.g. `E0001`) |
| `--lsp` | Language server over stdio ([\[0x11\]](#0x11-language-server---lsp)) |
| `-j N` / `--threads N` | Set number of parallel threads (rayon) |
| `--remote-worker` | Persistent cluster worker over stdio ([\[0x10\]](#0x10-distributed-pmap_on-over-ssh-cluster)) |
| `--remote-worker-v1` | Legacy one-shot cluster worker over stdio |
| `build SCRIPT [-o OUT]` | AOT compile script to standalone binary ([\[0x0D\]](#0x0d-standalone-binaries-pe-build)) |
| `doc [TOPIC]` | Interactive reference book with vim-style navigation (`pe doc`, `pe doc pmap`, `pe doc --toc`) |
| `serve PORT [SCRIPT]` | HTTP server: static files (`pe serve 8080`), script (`pe serve 8080 app.pr`), one-liner (`pe serve 3000 -e 'EXPR'`) |

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
Three-tier compile (Rust `regex` → `fancy-regex` → PCRE2). Perl `$` end anchor (no `/m`) is rewritten to `(?:\n?\z)`. Match `=~`, dynamic `$str =~ $pat`, substitution `s///`, transliteration `tr///`, flags `g`/`i`/`m`/`s`/`x`/`e`/`r`, captures `$1`…`$n`, named groups → `%+`/`$+{name}`, `\Q...\E`, `quotemeta`, `m//`/`qr//`. The `/r` flag (non-destructive) returns the modified string instead of the match count — auto-injected when `s///` or `tr///` appear as pipe-forward RHS. Bare `/pat/` in statement/boolean context is `$_ =~ /pat/`.

#### Subroutines
`sub name { }` with optional prototype, anon subs/closures, implicit return of last expression (VM), `@_`/`shift`/`return`, postfix `return ... if COND`, `AUTOLOAD` with `$AUTOLOAD` set to the FQN.

#### Built-ins (selected)

| Category | Functions |
| --- | --- |
| Array | `push`, `pop`, `shift`, `unshift`, `splice`, `reverse`, `rev` (scalar reverse), `sort`, `map`, `grep`, `filter`, `reduce`, `fold`, `fore`, `e`, `preduce`, `scalar`, `partition`, `min_by`, `max_by`, `zip_with`, `interleave`, `frequencies`, `count_by`, `pluck`, `grep_v` |
| Hash | `keys`, `values`, `each`, `delete`, `exists`, `select_keys`, `top` |
| String | `chomp`, `chop`, `length`, `substr`, `index`, `rindex`, `split`, `join`, `sprintf`, `printf`, `uc`/`lc`/`ucfirst`/`lcfirst`, `chr`, `ord`, `hex`, `oct`, `crypt`, `fc`, `pos`, `study`, `quotemeta`, `trim`, `lines`, `words`, `chars`, `snake_case`, `camel_case`, `kebab_case` |
| Binary | `pack`, `unpack` (subset `A a N n V v C Q q Z H x w i I l L s S f d` + `*`), `vec` |
| Numeric | `abs`, `int`, `sqrt`, `squared`/`sq`, `cubed`/`cb`, `expt(B,E)`, `sin`, `cos`, `atan2`, `exp`, `log`, `rand`, `srand`, `avg`, `stddev`, `clamp`, `normalize`, `range(N, M)` (lazy bidirectional) |
| I/O | `print`, `p`, `say`, `printf`, `open` (incl. `open my $fh`, files, `-\|` / `\|-` pipes), `close`, `eof`, `readline`, `read`, `seek`, `tell`, `sysopen`, `sysread`/`syswrite`/`sysseek`, handle methods `->print/->say/->printf/->getline/->close/->eof/->getc/->flush`, `slurp`, `input`, backticks/`qx{}`, `capture` (structured: `->stdout/->stderr/->exit`), `pager`/`pg`/`less` (pipes value into `$PAGER`; TTY-gated), `binmode`, `fileno`, `flock`, `getc`, `select`, `truncate`, `formline`, `read_lines`, `append_file`, `to_file`, `read_json`, `write_json`, `tempfile`, `tempdir` |
| Directory | `opendir`, `readdir`, `closedir`, `rewinddir`, `telldir`, `seekdir`, `files`, `filesf`/`f`, `fr` (recursive files, lazy iterator), `dirs`/`d`, `dr` (recursive dirs, lazy iterator), `sym_links`, `sockets`, `pipes`, `block_devices`, `char_devices` |
| File tests | `-e`, `-f`, `-d`, `-l`, `-r`, `-w`, `-s`, `-z`, `-x`, `-t` (defaults to `$_`) |
| System | `system`, `exec`, `exit`, `chdir`, `mkdir`, `unlink`, `rename`, `chmod`, `chown`, `chroot`, `stat`, `lstat`, `link`, `symlink`, `readlink`, `glob`, `glob_par`, `glob_match`, `which_all`, `par_sed`, `par_find_files`, `par_line_count`, `ppool`, `barrier`, `fork`, `wait`, `waitpid`, `kill`, `alarm`, `sleep`, `times`, `dump`, `reset` |
| Sockets | `socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `shutdown`, `socketpair` |
| Network | `gethostbyname`, `gethostbyaddr`, `getpwnam`, `getpwuid`, `getpwent`/`setpwent`/`endpwent`, `getgrnam`, `getgrgid`, `getgrent`/`setgrent`/`endgrent`, `getprotobyname`, `getprotobynumber`, `getservbyname`, `getservbyport` |
| SysV IPC | `msgctl`, `msgget`, `msgsnd`, `msgrcv`, `semctl`, `semget`, `semop`, `shmctl`, `shmget`, `shmread`, `shmwrite` (stubs — runtime error) |
| Type | `defined`, `undef`, `ref`, `bless`, `tied`, `untie` |
| Serialization | `to_json`, `to_csv`, `to_toml`, `to_yaml`, `to_xml`, `ddump`, `stringify`/`str`, `json_encode`/`json_decode` |
| Control | `die`, `warn`, `eval`, `do`, `require`, `caller`, `wantarray`, `goto LABEL`, `continue { }` on loops, `prototype` |

#### Perl-compat highlights

- **OOP** — `@ISA` (incl. `our @ISA` outside `main`), C3 MRO (live, not cached), `$obj->SUPER::method`. `tie` for scalars/arrays/hashes with `TIESCALAR/TIEARRAY/TIEHASH`, `FETCH`/`STORE`, plus `EXISTS`/`DELETE` on tied hashes. `tied` returns the underlying object; `untie` removes the tie.
- **`use overload`** — `'op' => 'method'` or `\&handler`; binary dispatch with `(invocant, other)`, `nomethod`, unary `neg`/`bool`/`abs`, `""` for stringification, `fallback => 1`.
- **`$?` / `$|`** — packed POSIX status from `system`/backticks/pipe close; autoflush on print/printf.
- **`$.`** — undef until first successful read, then last-read line count.
- **`print`/`say`/`p`/`printf` with no args** — uses `$_` (and `printf`'s format defaults to `$_`).
- **Bareword statement** — `name;` calls a sub with `@_ = ($_)`.
- **Typeglobs** — `*foo = \&bar`, `*lhs = *rhs` copies sub/scalar/array/hash/IO slots; package-qualified `*Pkg::name` supported.
- **`%SIG` (Unix)** — `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` as code refs; handlers run between statements/opcodes via `perl_signal::poll`. `IGNORE` and `DEFAULT` honored.
- **`format` / `write`** — partial: `format NAME = ... .` registers a template; pictures `@<<<<`, `@>>>>`, `@||||`, `@####`, `@****`, literal `@@`. `formline` populates `$^A`. `write` (no args) uses `$~` to stdout. Not yet: `write FILEHANDLE`, `$^`.
- **`@INC` / `%INC` / `require` / `use`** — `@INC` is built from `-I`, `vendor/perl`, system `perl`'s `@INC` (set `PERLRS_NO_PERL_INC` to skip), the script dir, `PERLRS_INC`, then `.`. `List::Util` is implemented natively in Rust (`src/list_util.rs`). `use Module qw(a b);` honors `@EXPORT_OK`/`@EXPORT`. Built-in pragmas (`strict`, `warnings`, `utf8`, `feature`, `open`, `Env`) do not load files.
- **`chunked` / `windowed` / `fold`** — Use **pipe-forward**: **`LIST |> chunked(N)`**, **`LIST |> windowed(N)`**, **`LIST |> fold { BLOCK }`** (same for **`reduce`**). `List::Util::fold` / **`qw(...) |> List::Util::fold { }`** alias **`List::Util::reduce`**. List context → arrayrefs per chunk/window or the folded value; scalar context → chunk/window count where applicable.

  ```perl
  my @pairs = (1, 2, 3, 4) |> chunked(2);           # ([1,2], [3,4])
  my @slide = (1, 2, 3) |> windowed(2);             # ([1,2], [2,3])
  my @pipe  = (10, 20, 30) |> chunked(2);           # ([10,20], [30])
  my $sum   = (1, 2, 3, 4) |> fold { $a + $b };     # same as reduce
  my $cat   = qw(a b c) |> fold { $a . $b };
  ```
- **`use strict`** — refs/subs/vars modes (per-mode `use strict 'refs'` etc.). `strict refs` rejects symbolic derefs at runtime; `strict vars` requires a visible binding.
- **`BEGIN` / `UNITCHECK` / `CHECK` / `INIT` / `END`** — Perl order; `${^GLOBAL_PHASE}` matches Perl in tree-walker and VM.
- **String interpolation** — `$var` `#{23 * 52}`, `$h{k}`, `$a[i]`, `@a`, `@a[slice]` (joined with `$"`), `$#a` in slice indices, `$0`, `$1..$n`. `\x{hex}` and unbraced `\x`.
- **`__FILE__` / `__LINE__`** — compile-time literals.
- Heredocs `<<EOF`, POD skipping, shebang handling, `qw()/q()/qq()` with paired delimiters.
- **Special variables** — large set of `${^NAME}` scalars pre-seeded; see [`SPECIAL_VARIABLES.md`](parity/SPECIAL_VARIABLES.md). Still missing vs Perl 5: `English`, full `$^V` as a version object.

#### Extensions beyond stock Perl 5

- Native CSV (`csv_read`/`csv_write`), columnar `dataframe`, embedded `sqlite`.
- HTTP (`fetch`/`fetch_json`/`fetch_async`/`par_fetch`), JSON (`json_encode`/`json_decode`).
- Crypto, compression, time, TOML, YAML helpers (see [\[0x05\]](#0x05-native-data-scripting)).
- All parallel primitives in [\[0x03\]](#0x03-parallel-primitives) (`pmap`, `fan`, `pipeline`, `par_pipeline_stream`, `pchannel`, `pselect`, `barrier`, `ppool`, `glob_par`, `par_walk`, `par_lines`, `par_sed`, `par_find_files`, `par_line_count`, `pwatch`, `watch`).
- **Distributed compute** ([\[0x10\]](#0x10-distributed-pmap_on-over-ssh-cluster)): `cluster([...])` builds an SSH worker pool; `pmap_on $cluster { } @list` and `pflat_map_on $cluster { } @list` fan a map across persistent remote workers with fault tolerance and per-job retries.
- **Standalone binaries** ([\[0x0D\]](#0x0d-standalone-binaries-pe-build)): `pe build SCRIPT -o OUT` bakes a script into a self-contained executable.
- **Inline Rust FFI** ([\[0x0E\]](#0x0e-inline-rust-ffi-rust-----)): `rust { pub extern "C" fn ... }` blocks compile to a cdylib on first run, dlopen + register as Perl-callable subs.
- **Bytecode cache** ([\[0x0F\]](#0x0f-bytecode-cache-pec)): `PERLRS_BC_CACHE=1` skips parse + compile on warm starts via on-disk `.pec` bundles.
- **Language server** ([\[0x11\]](#0x11-language-server---lsp)): `pe --lsp` runs an LSP server over stdio with diagnostics, hover, completion.
- `mysync` shared state ([\[0x04\]](#0x04-shared-state-mysync)).
- `frozen my` (or `const my` — same thing, more familiar spelling), `typed my`, `struct`, algebraic `match`, `try/catch/finally`, `eval_timeout`, `retry`, `rate_limit`, `every`, `gen { ... yield }`.
- **Outer topic `$_<`** — access the enclosing scope's `$_` from nested blocks; up to 4 levels (`$_<` through `$_<<<<`). See [\[0x03\]](#0x03-parallel-primitives).
- **`fore`** (`e`) — side-effect-only list iterator (like `map` but void, returns item count). Works with `{ BLOCK } LIST`, blockless `e EXPR, LIST`, and pipe-forward `|> e say`. Use for print/log/accumulator loops.
- **Pipe-forward `|>`** — parse-time desugaring (zero runtime cost); threads the LHS as the **first** argument of the RHS call, left-associative. `map`, `grep`/`filter`, `sort`, and `e` accept **blockless expressions** on the RHS of `|>` — no `{ }` required for simple transforms:

  ```perl
  # chain HTTP fetch → JSON decode → jq filter
  my @titles = $url |> fetch_json |> json_decode |> json_jq '.articles[].title';

  # blockless list pipelines — no braces needed for simple expressions
  files |> filter /[a-e]/ |> e -f $_ && system("cat $_")
  "a".."z" |> map uc |> e p;                      # A B C … Z
  "a".."z" |> grep /[aeiou]/ |> e p;              # a e i o u
  "a".."z" |> filter 't' |> e p;                  # t  (literal = equality test)
  1..10 |> filter $_ > 5 |> sort |> e p;      # blocks still work
  1..5 |> map $_ * $_ |> join "," |> p;         # 1,4,9,16,25

  # e — side-effect-only iteration (like map but void, returns count)
  qw(apple banana cherry) |> grep /^a/ |> map uc |> e p;  # APPLE

  # unary builtins — `x |> length`, `x |> uc`, `x |> sqrt`, etc.
  "hello" |> length |> p;                            # 5
  16 |> sqrt |> p;                                   # 4
  "ff" |> hex |> p;                                  # 255

  # bareword on RHS becomes a unary call: `x |> f` → `f(x)`
  # call on RHS prepends: `x |> f(a, b)` → `f(x, a, b)`
  # map/grep/filter/sort/join/reduce/fold/e — LHS fills the list slot
  # chunked/windowed — `LIST |> chunked(N)` prepends the list before the size
  # scalar on RHS: `x |> $cr` → `$cr->(x)`

  # regex ops in pipelines — s///, tr///, and m// work as RHS of |>
  # s/// and tr/// auto-inject /r so the modified string flows through:
  "hello world" |> s/world/perl/  |> p;              # hello perl
  "hello world" |> tr/a-z/A-Z/   |> p;              # HELLO WORLD

  # m//g extracts all matches as an array:
  "foo123bar456" |> /\d+/g |> p;                     # 123 456

  # full pipeline: read input, strip newlines, split, count word frequencies
  # man ls | pe -e 'input |> s@\n@@g |> split |> frequencies |> ddump |> p'

  # extract all emails from text, deduplicate
  # cat log.txt | pe -e 'input |> /[\w.]+@[\w.]+/g |> distinct |> e p'

  # capture groups with /g:
  "a=1 b=2" |> /(\w+)=(\w+)/g |> ddump |> p;
  ```

  **Pipeline builtins** — designed for `|>` chains:

  ```perl
  # ── input / output ─────────────────────────────────────────────────
  input                                # slurp all of stdin as one string
  input($fh)                           # slurp a filehandle
  # cat data.txt | pe -e 'input |> lines |> e p'

  # ── string → list ──────────────────────────────────────────────────
  "hello\nworld" |> lines |> ddump |> p;       # ("hello", "world")
  "foo bar baz"  |> words |> ddump |> p;       # ("foo", "bar", "baz")
  "hello"        |> chars |> ddump |> p;       # ("h","e","l","l","o")
  "  hello  "    |> trim  |> p;                # "hello"

  # ── case conversion ────────────────────────────────────────────────
  "helloWorld"     |> snake_case  |> p;   # hello_world
  "hello_world"    |> camel_case  |> p;   # helloWorld
  "Hello World"    |> kebab_case  |> p;   # hello-world

  # ── aggregation / stats ────────────────────────────────────────────
  1 .. 100 |> avg    |> p;                # 50.5
  1 .. 100 |> stddev |> p;                # 28.86607…
  "hello"  |> chars  |> frequencies |> ddump |> p;
  # { h => 1, e => 1, l => 2, o => 1 }

  # ── frequencies + top ──────────────────────────────────────────────
  "the quick brown fox" |> chars |> frequencies |> top 3 |> ddump |> p;
  # top 3 chars by count

  # ── count_by { BLOCK } LIST ────────────────────────────────────────
  1 .. 20 |> count_by { $_ % 2 == 0 ? "even" : "odd" } |> ddump |> p;
  # { odd => 10, even => 10 }

  # ── numeric transforms ─────────────────────────────────────────────
  1 .. 10  |> clamp 3, 7    |> ddump |> p;   # 3 3 3 4 5 6 7 7 7 7
  1 .. 5   |> normalize     |> ddump |> p;   # 0 0.25 0.5 0.75 1

  # ── inverse grep (regex) ───────────────────────────────────────────
  1 .. 10 |> grep_v "^[35]$" |> ddump |> p;  # removes 3 and 5

  # ── hash manipulation ──────────────────────────────────────────────
  my $h = {a => 1, b => 2, c => 3};
  $h |> select_keys "a", "c" |> ddump |> p;  # { a => 1, c => 3 }

  # ── pluck key from list of hashrefs ────────────────────────────────
  my @people = ({name=>"Alice",age=>30}, {name=>"Bob",age=>25});
  @people |> pluck "name" |> ddump |> p;      # ("Alice", "Bob")

  # ── serialization ──────────────────────────────────────────────────
  my $data = {a => 1, b => [2,3]};
  $data |> to_json |> p;                        # {"a":1,"b":[2,3]}
  @people |> to_csv |> p;                      # CSV with headers
  my $cfg = {title => "My App", package => {name => "myapp", version => "1.0"}};
  $cfg |> to_toml |> p;                         # TOML with [package] table
  $data |> to_yaml |> p;                        # YAML with --- header
  $data |> to_xml  |> p;                        # XML with <root> wrapper

  # ── stringify / str — parseable perlrs literals ──────────────────────
  $data |> str |> p;                            # +{a => 1, b => [2, 3]}
  my $fn = fn { $_ * 2 };
  $fn |> str |> p;                              # sub { $_ * 2; }
  range(1, 3) |> str |> p;                      # (1, 2, 3)
  # round-trip: str -> eval -> callable
  my $f = fn ($x: Int) { $x + 1 };
  my $f2 = $f |> str |> eval;
  $f2->(5) |> p;                                # 6

  # ── partition / min_by / max_by / zip_with ─────────────────────────
  my ($yes, $no) = partition { $_ > 5 } 1..10;
  my $smallest = min_by { length } @words;
  my $largest  = max_by { length } @words;
  my @sums = zip_with { $_0 + $_1 } [1,2,3], [10,20,30];  # 11 22 33

  # ── pretty-print (Data::Dumper style) ──────────────────────────────
  my $nested = {key => [1, {nested => "val"}]};
  $nested |> ddump |> p;

  # ── write to file (returns content for further piping) ─────────────
  my $text = "hello\nworld\n";
  $text |> to_file "/tmp/out.txt";

  # ── file I/O helpers ────────────────────────────────────────────────
  my @lines = read_lines "/tmp/out.txt";        # slurp file → list of lines
  append_file "/tmp/out.txt", "extra\n";         # append to file
  my $tmp = tempfile();                          # create temp file, returns path
  my $dir = tempdir();                           # create temp directory, returns path

  # ── JSON file I/O ──────────────────────────────────────────────────
  write_json "/tmp/data.json", {a => 1, b => 2}; # write hash as JSON file
  my $obj = read_json "/tmp/data.json";           # read JSON file → hashref

  # ── interleave ─────────────────────────────────────────────────────
  my @merged = interleave [1,2,3], [10,20,30];  # (1,10,2,20,3,30)

  # ── glob_match / which_all ──────────────────────────────────────────
  p glob_match "*.txt", "readme.txt";          # 1 (matches)
  my @bins = which_all "perl";                   # all paths for "perl" in $PATH
  ```

  **Blockless `|>` rules for `grep`/`filter`**: string literals test `$_ eq EXPR`, numbers test `$_ == EXPR`, regexes test `$_ =~ EXPR`, anything else (e.g. `defined`) uses standard Perl grep semantics (sets `$_`, evaluates expression).

  Precedence: `|>` binds **looser** than `||` but **tighter** than `?:` / `and`/`or`/`not` — the slot sits between `parse_ternary` and `parse_or_word` in the parser stack. So `$x + 1 |> f` parses as `f($x + 1)`, and `0 || 1 |> yes` parses as `yes(0 || 1)`. The RHS must be a call, builtin, method invocation, bareword, or coderef expression; bare binary expressions / literals on the right are a parse error (`42 |> 1 + 2` is rejected).

- **`thread` macro** — Clojure-inspired threading macro for clean multi-stage pipelines without repeating `|>`. Stages are bare function names, functions with blocks, parenthesized calls `name(args)` where `$_` is the threaded-value placeholder (must appear at least once in args, can sit in any position — first, last, middle, nested), or anonymous blocks (`>{}` / `fn {}` / `sub {}`). Use `|>` after `thread` to continue piping.

  ```perl
  # thread shines with multiple block-taking functions — no |> repetition
  @data = 1..20
  thread @data grep { $_ % 2 == 0 } map { $_ * $_ } sort { $_1 <=> $_0 } |> join "," |> p
  # 400,324,256,196,144,100,64,36,16,4

  # Compare: same pipeline with |> requires more syntax
  @data |> grep { $_ % 2 == 0 } |> map { $_ * $_ } |> sort { $_1 <=> $_0 } |> join "," |> p

  # Long data processing pipeline
  @nums = 1..100
  thread @nums grep { $_ % 3 == 0 } map { $_ * 2 } grep { $_ > 50 } sort { $_1 <=> $_0 } |> head 5 |> join "," |> p
  # 198,192,186,180,174

  # Anonymous blocks for custom transforms
  thread 100 >{ $_ / 2 } >{ $_ + 10 } >{ $_ * 3 } p  # 180

  # Process list of hashes
  @users = ({name=>"alice",age=>30}, {name=>"bob",age=>25}, {name=>"carol",age=>35})
  thread @users sort { $_0->{age} <=> $_1->{age} } map { $_->{name} } |> join "," |> p
  # bob,alice,carol

  # String processing with unary builtins
  thread "  hello world  " trim uc p                 # HELLO WORLD

  # Parenthesized call stages — `$_` is the threaded-value placeholder
  sub add2 { $_0 + $_1 }
  thread 10 add2($_, 5) p                            # add2(10, 5)        => 15
  thread 10 add2(5, $_) p                            # add2(5, 10)        => 15  (any position)
  thread 10 add2($_, 5) add2($_, 100) p              # chains: 15 then 115
  sub add3 { $_0 + $_1 + $_2 }
  thread 10 add3(5, $_, 10) p                        # add3(5, 10, 10)    => 25
  # `$_` works inside nested expressions too:
  sub mul { $_0 * $_1 }
  thread 10 mul($_ + 1, 2) p                         # mul(11, 2)         => 22

  # Reduce with $_0/$_1
  thread (1..10) reduce { $_0 + $_1 } p              # 55

  # Sort and unique
  @data = (3,1,4,1,5,9,2,6,5,3)
  thread @data sort { $_0 <=> $_1 } uniq |> join "," |> p   # 1,2,3,4,5,6,9
  ```

  **When to use `thread` vs `|>`:**
  - **`thread`**: Best for chains of block-taking functions (`map { }`, `grep { }`, `sort { }`, `reduce { }`)
  - **`|>`**: Best for blockless expressions (`map $_ * 2`, `grep $_ > 5`) and unary functions

  ```perl
  # |> with blockless expressions — cleanest for simple transforms
  1..20 |> grep $_ % 2 == 0 |> map $_ * $_ |> grep $_ > 50 |> join "," |> p
  # 64,100,144,196,256,324,400

  # thread with blocks — cleanest when every stage needs a block
  thread @data map { complex($_) } grep { validate($_) } sort { $_0 cmp $_1 } |> p
  ```

  **Stage types:**
  - **Bare function**: `thread "hello" uc trim` — applies unary builtins in sequence
  - **Function with block**: `thread @data map { $_ * 2 } grep { $_ > 5 }` — block-taking functions
  - **Anonymous block**: `thread 5 >{ $_ * 2 }` or `fn { }` or `sub { }` — custom transforms

  **Termination:** `|>` ends the `thread` macro: `thread @l f1 f2 f3 |> f4` parses as `(thread @l f1 f2 f3) |> f4`.

  **Numeric/statistical pipelines:**

  ```perl
  # Sum of squares of even numbers 1-10
  thread (1..10) grep { $_ % 2 == 0 } map { $_ * $_ } sum p       # 220

  # Mean of squares
  thread (1..10) map { $_ * $_ } mean p                           # 38.5

  # Multiples of 7 up to 100, doubled, summed
  thread (1..100) grep { $_ % 7 == 0 } map { $_ * 2 } sum p       # 1470

  # Sum of odd squares, sqrt, truncate
  thread (1..50) grep { $_ % 2 == 1 } map { $_ ** 2 } sum sqrt int p  # 144

  # Factorial via product
  thread (1..10) product p                                        # 3628800

  # Remove duplicates, then sum
  thread (1,1,2,2,3,3,4,5,5) uniq sum p                           # 15

  # Shuffle, dedupe, sum (same result, random order internally)
  thread (1..20) shuffle uniq sum p                               # 210

  # Statistical measures
  thread (1..10) mean p                                           # 5.5
  thread (1..10) median p                                         # 5.5
  thread (1..10) stddev p                                         # 2.87228...
  ```

  **String pipelines:**

  ```perl
  # Full transformation
  thread " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p
  # "d-lrow-olleh"

  # String list operations
  thread ("apple","banana","cherry","date") shuffle reverse minstr p  # apple
  thread ("apple","banana","cherry","date") shuffle reverse maxstr p  # date
  ```

  **Sorting and aggregation:**

  ```perl
  # Sort then get min/max
  thread (5,2,8,1,9,3) sort { $_0 <=> $_1 } min p                 # 1
  thread (5,2,8,1,9,3) sort { $_0 <=> $_1 } max p                 # 9

  # Pairs: extract keys and values
  thread (1,2,3,4,5,6) pairkeys |> join "," |> p                  # 1,3,5
  thread (1,2,3,4,5,6) pairvalues |> join "," |> p                # 2,4,6
  ```

  **Compare with `|>` syntax (same result, more typing):**

  ```perl
  # thread version
  thread (1..10) grep { $_ % 2 == 0 } map { $_ * $_ } sum p

  # |> version
  (1..10) |> grep { $_ % 2 == 0 } |> map { $_ * $_ } |> sum |> p
  ```

  **Language comparison — the same 10-stage pipeline:**

  ```perl
  # perlrs: 1 line, reads left-to-right, no noise
  thread " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p
  ```

  ```perl
  # Perl 5: needs CPAN modules, verbose method chains
  use String::CamelCase qw(camelize decamelize);
  use JSON;
  my $s = " hello world ";
  $s =~ s/^\s+|\s+$//g;                    # trim
  $s = uc($s);
  $s = reverse($s);
  $s = lc($s);
  $s = ucfirst($s);
  $s =~ s/([A-Z])/_\l$1/g; $s =~ s/^_//;   # snake_case (manual)
  $s = camelize($s);                        # camel_case (CPAN)
  $s =~ s/([A-Z])/-\l$1/g; $s =~ s/^-//;   # kebab_case (manual)
  print encode_json($s), "\n";
  ```

  ```javascript
  // JavaScript: no built-in case converters, needs helper functions
  const snakeCase = s => s.replace(/([A-Z])/g, '_$1').toLowerCase().replace(/^_/, '');
  const camelCase = s => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
  const kebabCase = s => s.replace(/([A-Z])/g, '-$1').toLowerCase().replace(/^-/, '');
  const ucfirst = s => s.charAt(0).toUpperCase() + s.slice(1);
  const rev = s => s.split('').reverse().join('');

  let s = " hello world ";
  s = s.trim();
  s = s.toUpperCase();
  s = rev(s);
  s = s.toLowerCase();
  s = ucfirst(s);
  s = snakeCase(s);
  s = camelCase(s);
  s = kebabCase(s);
  console.log(JSON.stringify(s));
  ```

  ```python
  # Python 3: no built-in case converters, needs helper functions
  import json
  import re

  def snake_case(s): return re.sub(r'([A-Z])', r'_\1', s).lower().lstrip('_')
  def camel_case(s): return re.sub(r'_([a-z])', lambda m: m.group(1).upper(), s)
  def kebab_case(s): return re.sub(r'([A-Z])', r'-\1', s).lower().lstrip('-')

  s = " hello world "
  s = s.strip()
  s = s.upper()
  s = s[::-1]
  s = s.lower()
  s = s[0].upper() + s[1:]  # ucfirst
  s = snake_case(s)
  s = camel_case(s)
  s = kebab_case(s)
  print(json.dumps(s))
  ```

  **perlrs: 1 line. Perl 5: 10+ lines + CPAN. JavaScript: 15+ lines. Python: 15+ lines.**

  **Lisp hell** — without `|>`, the same pipeline becomes unreadable:

  ```perl
  # perlrs with |> : reads left-to-right
  " hello world " |> trim |> uc |> rev |> lc |> ucfirst |> rev |> snake_case |> camel_case |> kebab_case |> rev |> uc |> lc |> trim |> to_json |> p
  # "d-lrow-olleh"

  # Without |> : nested calls, reads inside-out (lisp hell)
  p(to_json(trim(lc(uc(rev(kebab_case(camel_case(snake_case(rev(ucfirst(lc(rev(uc(trim(" hello world ")))))))))))))))
  ```

  The pipe-forward operator eliminates the cognitive overhead of matching parentheses and reading inside-out.

- **Short aliases** — 1-3 character aliases for common functions, designed for `thread`/`|>` pipelines:

  ```perl
  # Long form
  thread " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p

  # Short form (same result)
  t " hello world " tm uc rv lc ufc sc cc kc tj p
  ```

  | Alias | Function | Alias | Function | Alias | Function |
  |-------|----------|-------|----------|-------|----------|
  | **Thread/Pipe** | | **String** | | **Case** | |
  | `t` | `thread` | `tm` | `trim` | `sc` | `snake_case` |
  | `p` | `say` | `len` | `length` | `cc` | `camel_case` |
  | `pr` | `print` | `ufc` | `ucfirst` | `kc` | `kebab_case` |
  | | | `lfc` | `lcfirst` | `qm` | `quotemeta` |
  | **List** | | `rv` | `reverse` | | |
  | `gr` | `grep` | `ch` | `chars` | **Serialize** | |
  | `so` | `sort` | `ln` | `lines` | `tj` | `to_json` |
  | `rd` | `reduce` | `wd` | `words` | `ty` | `to_yaml` |
  | `hd` | `head/take` | | | `tt` | `to_toml` |
  | `tl` | `tail` | **Unique/Dedup** | | `tc` | `to_csv` |
  | `drp` | `drop/skip` | `uq` | `uniq` | `tx` | `to_xml` |
  | `fl` | `flatten` | `dup` | `dedup` | `dd` | `ddump` |
  | `cpt` | `compact` | `shuf` | `shuffle` | | |
  | `cat` | `concat` | | | **Deserialize** | |
  | `il` | `interleave` | **Stats** | | `jd` | `json_decode` |
  | `en` | `enumerate` | `sq` | `sqrt` | `yd` | `yaml_decode` |
  | `wi` | `with_index` | `med` | `median` | `td` | `toml_decode` |
  | `chk` | `chunk` | `std` | `stddev` | `xd` | `xml_decode` |
  | `zp` | `zip` | `var` | `variance` | `je` | `json_encode` |
  | `fst` | `first` | `clp` | `clamp` | `ye` | `yaml_encode` |
  | `frq` | `frequencies` | `nrm` | `normalize` | `te` | `toml_encode` |
  | `win` | `windowed` | | | `xe` | `xml_encode` |
  | | | **Crypto** | | | |
  | **File/Path** | | `s1` | `sha1` | **Encoding** | |
  | `sl` | `slurp` | `s256` | `sha256` | `b64e` | `base64_encode` |
  | `wf` | `write_file` | `m5` | `md5` | `b64d` | `base64_decode` |
  | `rl` | `read_lines` | `uid` | `uuid` | `hxe` | `hex_encode` |
  | `rb` | `read_bytes` | | | `hxd` | `hex_decode` |
  | `af` | `append_file` | **HTTP** | | `ue` | `url_encode` |
  | `rj` | `read_json` | `ft` | `fetch` | `ud` | `url_decode` |
  | `wj` | `write_json` | `ftj` | `fetch_json` | `gz` | `gzip` |
  | `bn` | `basename` | `fta` | `fetch_async` | `ugz` | `gunzip` |
  | `dn` | `dirname` | `hr` | `http_request` | `zst` | `zstd` |
  | `rp` | `realpath` | `pft` | `par_fetch` | `uzst` | `zstd_decode` |
  | `wh` | `which` | | | | |
  | `pwd` | `getcwd` | **CSV/Data** | | **DateTime** | |
  | `tf` | `tempfile` | `cr` | `csv_read` | `utc` | `datetime_utc` |
  | `tdr` | `tempdir` | `cw` | `csv_write` | `now` | `datetime_now_tz` |
  | `hn` | `gethostname` | `pcr` | `par_csv_read` | `dte` | `datetime_from_epoch` |
  | `el` | `elapsed` | `df` | `dataframe` | `dtf` | `datetime_strftime` |
  | `def` | `defined` | `sql` | `sqlite` | | |

- **`fn` keyword** — alias for `sub`. Both `fn name { }` and `fn { }` work identically to `sub`.

  ```perl
  fn double($x) { $x * 2 }
  p double(21)                    # 42

  my $f = fn { $_ * 2 }
  p $f->(21)                      # 42
  ```

- **Closure arguments `$_0`, `$_1`, ... `$_N`** — numeric closure arguments inspired by Swift. All arguments passed to any sub (named or anonymous) are available as `$_0` (first), `$_1` (second), `$_2` (third), up to `$_N` for any number of arguments. These work alongside or instead of Perl's `@_`, `$_`, `$a`, `$b`. Both `$_` and `$_0` refer to the first argument, so `$_ * 2` and `$_0 * 2` are equivalent — use whichever reads better in context.

  ```perl
  # $_0 in |> pipes (single-arg: $_0 == $_)
  (1..5) |> map { $_0 * 2 } |> join "," |> p           # 2,4,6,8,10
  (1..10) |> grep { $_0 % 2 == 0 } |> sum |> p         # 30

  # $_0/$_1 in |> pipes (two-arg: $_0/$_1 == $a/$b)
  (5,2,8,1) |> sort { $_0 <=> $_1 } |> join "," |> p   # 1,2,5,8
  (1..5) |> reduce { $_0 + $_1 } |> p                  # 15
  (1..5) |> reduce { $_0 * $_1 } |> p                  # 120 (factorial)
  ("banana","apple","cherry") |> sort { length($_0) <=> length($_1) } |> join "," |> p  # apple,banana,cherry

  # $_0/$_1 in thread macro
  thread (1..5) map { $_0 * 2 } sum p                  # 30
  thread (1..5) reduce { $_0 + $_1 } p                 # 15
  thread (1..5) reduce { $_0 * $_1 } p                 # 120
  thread (5,2,8,1) sort { $_0 <=> $_1 } |> join "," |> p  # 1,2,5,8
  thread (1..10) grep { $_0 % 2 == 0 } map { $_0 * $_0 } sum p  # 220

  # Multi-arg anonymous subs: $_0, $_1, ... $_N
  my $add3 = fn { $_0 + $_1 + $_2 };
  p $add3->(1, 2, 3);                        # 6

  my $mul5 = fn { $_0 * $_1 * $_2 * $_3 * $_4 };
  p $mul5->(1, 2, 3, 4, 5);                  # 120

  my $concat = fn { "$_0-$_1-$_2-$_3" };
  p $concat->("a", "b", "c", "d");           # a-b-c-d

  # Direct access via @_ still works
  my $join_args = fn { join("-", @_) };
  p $join_args->("x", "y", "z");             # x-y-z

  # Using $_0 closures with |> pipes
  my $double = fn { $_0 * 2 };
  my $triple = fn { $_0 * 3 };
  5 |> $double |> $triple |> p               # 30

  # Using $_0/$_1 closures in reduce
  my $add = fn { $_0 + $_1 };
  (1..5) |> reduce { $add->($_0, $_1) } |> p # 15

  # Using $_0/$_1/$_2 closure
  my $mul3 = fn { $_0 * $_1 * $_2 };
  p $mul3->(2, 3, 4);                        # 24

  # Using $_0/$_1 closure as comparator
  my $cmp = fn { $_0 <=> $_1 };
  (5,2,8,1) |> sort { $cmp->($_0, $_1) } |> join "," |> p  # 1,2,5,8

  # User-defined functions in thread (bare stage, no block needed)
  sub double { $_0 * 2 }
  sub triple { $_0 * 3 }
  sub add5   { $_0 + 5 }
  sub square { $_0 ** 2 }
  sub half   { $_0 / 2 }
  thread 2 double triple add5 square half p  # 144.5

  sub inc  { $_0 + 1 }
  sub dec  { $_0 - 1 }
  sub dbl  { $_0 * 2 }
  sub neg  { -$_0 }
  sub abs_ { abs($_0) }
  thread 5 inc dbl dec neg abs_ dbl inc p    # 23

  sub wrap  { "[$_0]" }
  sub upper { uc($_0) }
  sub trim_ { trim($_0) }
  sub rev_  { rev($_0) }
  sub bang  { "$_0!" }
  thread "  hello  " trim_ upper rev_ wrap bang p  # [OLLEH]!

  # User-defined functions inside blocks
  sub is_even { $_0 % 2 == 0 }
  thread (1..10) grep { is_even($_) } sum p  # 30

  thread (1..5) map { square($_) } sum p     # 55

  # Multi-arg user-defined functions
  sub add  { $_0 + $_1 }
  sub mul3 { $_0 * $_1 * $_2 }
  p add(3, 4)                                # 7
  p mul3(2, 3, 4)                            # 24

  # Inline transforms with >{ } (arrow block)
  thread 5 >{ $_ * 2 } >{ $_ + 10 } p        # 20
  thread 100 >{ $_0 / 2 } >{ $_0 + 10 } >{ $_0 * 3 } p  # 180
  ```

`perlrs` is **not** a full `perl` replacement: many real `.pm` files (especially XS modules) will not run. See [`PARITY_ROADMAP.md`](parity/PARITY_ROADMAP.md).

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
pe convert examples/fibonacci.pl > examples/fibonacci.pr
pe examples/fibonacci.pr
```

```sh
# sets: dedupe + union / intersection (`scalar` gives member count, like `scalar @array`)
pe -e 'my $a = set(1,2,2,3); my $b = set(2,3,4); p scalar($a | $b), " ", scalar($a & $b)'
```

---

## [0x0B] BENCHMARKS

`bash bench/run_bench.sh` — perlrs vs perl 5.42.2 on Apple M5 18-core. Mean of 10 hyperfine runs with 3 warmups; **includes process startup** (not steady-state).

```
 perlrs benchmark harness (honest mode)
 ---------------------------------------
  perl5:   perl 5, version 42, subversion 2 (v5.42.2) built for darwin-thread-multi-2level
  perlrs:  This is perlrs v0.1.41 — A highly parallel Perl 5 interpreter (Rust)
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

Pull requests and pushes to `main` run [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (Check, Test, Format, Clippy, Doc, Parity, Release Build).

```sh
cargo test --lib                # parser smoke, lexer/value/error/scope, interpreter, vm, jit
cargo test --test integration   # tests/suite/* (runtime, readline list context, line-mode stdin, …)
cargo bench --bench jit_compare # JIT vs interpreter on the same bytecode
bash bench/run_bench.sh         # full perl5 vs perlrs suite (needs hyperfine)
bash parity/run_parity.sh       # exact stdout/stderr parity vs system perl (20 000+ cases)
```

- `Cargo.lock` is committed (CI uses `--locked`). If your global gitignore strips it, force-add updates: `git add -f Cargo.lock`.
- Disable JIT: `PERLRS_NO_JIT=1` or `pe --no-jit`.
- Parity work is tracked in [`PARITY_ROADMAP.md`](parity/PARITY_ROADMAP.md).

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

p add 21, 21         # 42
p mul3 1.5, 2.0, 3.0 # 9
p fib 50             # 12586269025
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

my @hashes = @big_files |> pmap_on $cluster { slurp_raw |> sha_256) };

# pflat_map_on for one-to-many mapping
my @lines = @log_paths |> pflat_map_on $cluster { split /\n/, slurp };
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

## [0x12] LANGUAGE REFLECTION

perlrs exposes its own parser and dispatcher state as plain Perl hashes, so
you can enumerate, look up, filter, and pipe over everything the interpreter
knows about — no separate API surface to learn, just standard hash ops.

The data is derived at compile time by `build.rs` from the source of truth:
section-commented groups in `is_perl5_core` / `perlrs_extension_name` (for
categories), `try_builtin` arm names (for aliases), and `doc_for_label_text`
in `src/lsp.rs` (for descriptions). No hand-maintained list, no stale counts.

#### Hashes

Eight hashes; every direct lookup (`$h{name}`) is **O(1)**. Forward maps:

| Long name | Short | Key → Value |
| --- | --- | --- |
| `%perlrs::builtins` | `%b` | **primary** callable name → category (`"parallel"`, `"string"`, …). Primaries-only — clean unique-op count. |
| `%perlrs::all` | `%all` | **every spelling** (primary + alias) → category. Aliases inherit their primary's tag. Use this for `scalar keys %all`. |
| `%perlrs::perl_compats` | `%pc` | subset of `%b`: Perl 5 core only, name → category |
| `%perlrs::extensions` | `%e` | subset of `%b`: perlrs-only, name → category |
| `%perlrs::aliases` | `%a` | alias → canonical primary (`$a{tj}` → `"to_json"`) |
| `%perlrs::descriptions` | `%d` | name → one-line LSP summary (**sparse**) |

Inverted indexes for constant-time reverse queries:

| Long name | Short | Key → Value |
| --- | --- | --- |
| `%perlrs::categories` | `%c` | category → arrayref of names (`$c{parallel}` → `[pmap, pgrep, …]`) |
| `%perlrs::primaries` | `%p` | primary → arrayref of its aliases (`$p{to_json}` → `[tj]`) |

#### Examples

```sh
# O(1) direct lookups
pe -e 'p $b{pmap}'              # "parallel"
pe -e 'p $b{to_json}'           # "serialization"
pe -e 'p $pc{map}'              # "array / list"
pe -e 'p $e{pmap}'              # "parallel"
pe -e 'p $a{tj}'                # "to_json"
pe -e 'p $d{pmap}'              # LSP one-liner
pe -e 'p $all{tj}'              # "serialization"  (alias resolved via %all)
pe -e 'p scalar @{$c{parallel}}'  # number of parallel ops
pe -e '$p{to_json} |> e p'        # every alias of to_json

# total callable spellings (primaries + aliases), one direct count
pe -e 'p scalar keys %all'

# see just Perl compats
pe -e 'keys %pc |> sort |> p'

# see just perlrs extensions
pe -e 'keys %e |> sort |> p'

# enumerate a whole category in O(1)
pe -e '$c{parallel} |> e p'
pe -e '$c{"array / list"} |> e p'

# browse any of them interactively via the pager
pe -e 'keys %all |> less'

# frequency table: how many ops per category?
pe -e 'my %f; $f{$b{$_}}++ for keys %b; dd \%f'

# find every documented op mentioning "parallel"
pe -e 'keys %d |> grep { $d{$_} =~ /parallel/i } |> sort |> p'

# catalog the full reflection surface
pe -e 'for my $h (qw(b all pc e a d c p)) {
         printf "%%%-4s %d\n", $h, scalar keys %$h
       }'
```

#### Notes

- Every direct `$h{name}` lookup is O(1). Filter queries (`grep { cond }
  keys %h`) are O(n), but the two inverted indexes (`%c`, `%p`) give you
  O(1) reverse-lookups for the two most common "find names by property"
  queries.
- Hash sigil namespace is separate from scalars and subs, so `%a`/`%b`/`%c`/`%d`/`%e`/`%p`/`%pc`
  don't collide with `$a`/`$b` sort specials or the `e` extension sub.
- Short aliases are value copies of the long `%perlrs::*` names — currently
  read-only in practice, so the copy never diverges.
- `%descriptions` is sparse: `exists $d{$name}` doubles as "is this
  documented in the LSP?". Undocumented ops still appear in `%builtins`
  with a category — they just lack a hover summary.
- A value of `"uncategorized"` in `%builtins` means the name is dispatched
  at runtime but doesn't match any `// ── category ──` section comment in
  `parser.rs` yet — a flag for "add a section header here", not an error.

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
