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

#### INTERACTIVE REPL // `pe` WITH NO SCRIPT

When you run the **`pe`** binary **from a terminal** with **no program file**, **no `-e` / `-E`**, and not in **`-n` / `-p`** (or other batch-only modes such as **`-c`**, **`--ast`**, **`--fmt`**, **`--profile`**, **`-u`**), it starts a **readline** session: line editing, history (saved to **`~/.perlrs_history`** when possible), and **Tab** completion for keywords/builtins plus current lexical variables and subroutine names. Type **`exit`** or **`quit`** or send **EOF** (Ctrl-D) to leave. If stdin is **not** a TTY (e.g. a pipe), **`pe`** reads **one line** from stdin like **`perlrs`**. The **`perlrs`** binary keeps the previous behavior for the same flags (read a single line from stdin when no script is given).

#### EXECUTING INLINE CODE // DIRECT INJECTION

```sh
# inject a single line of perl
pe -e 'print "Hello, world!\n"'

# execute a script file
pe script.pl arg1 arg2

# check syntax without executing
pe -c script.pl

# dump abstract syntax tree as JSON (linting, IDE tooling, formatters, static analysis)
pe --ast script.pl
pe --ast -e 'sub foo { 1 }'

# pretty-print parsed Perl to stdout (best-effort; tree-walker-oriented AST; no execution)
pe --fmt script.pl
pe --fmt -e 'my $x = 1; say $x'

# wall-clock profile: per-statement and per-sub timings on stderr (tree-walker only; VM disabled)
pe --profile script.pl
```

#### `__DATA__` // EMBEDDED DATA HANDLE

A line whose trimmed text is exactly `__DATA__` ends the program text. Everything after that line is stored as bytes on the **`DATA`** input handle, so `<DATA>` and `readline` on **`DATA`** read that trailing section (same idea as Perl). Shebang stripping and **`-x`** extraction apply only to the program portion above the marker.

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

# parallel map in batches (one interpreter per chunk — amortizes spawn cost)
my @out = pmap_chunked 1000 { $_ ** 2 } @million_items;

# sequential left fold vs parallel tree fold (use preduce only for associative ops)
my $sum = reduce { $a + $b } @numbers;
my $psum = preduce { $a + $b } @numbers;

# lazy pipeline (ops run on collect(); chain with anonymous subs)
my @result = pipeline(@data)
    ->filter(sub { $_ > 10 })
    ->map(sub { $_ * 2 })
    ->take(100)
    ->collect();

# parallel grep — filter elements in parallel
my @evens = pgrep { $_ % 2 == 0 } @data;

# parallel foreach — execute side effects concurrently
pfor { process($_) } @items;

# fan — run a block N times in parallel (`$_` is 0..N-1)
fan 8 { work($_) }

# typed channels — pass messages between parallel blocks
my ($tx, $rx) = pchannel();
fan 10 { $tx->send($_) };
while (my $msg = $rx->recv()) { print "$msg\n" }

# multiplexed recv (Go-style select via crossbeam `Select`)
my ($tx1, $rx1) = pchannel();
my ($tx2, $rx2) = pchannel();
$tx1->send("first");
my ($val, $idx) = pselect($rx1, $rx2);  # $idx is 0-based (first arg = 0)

# deque — double-ended queue (not in stock Perl)
my $q = deque();
$q->push_back(1); $q->push_front(0);
# pop_front / pop_back / size (or len)

# heap — priority queue with a Perl comparator (`$a` / `$b`, like `sort`)
my $pq = heap(sub { $a <=> $b });
$pq->push(3); my $min = $pq->pop();

# parallel sort — sort using all cores
my @sorted = psort { $a <=> $b } @data;

# chain parallel operations
my @result = pmap { $_ ** 2 } pgrep { $_ > 100 } @data;

# parallel recursive glob (rayon directory walk), then process files in parallel
my @logs = glob_par("**/*.log");
pfor { process($_) } @logs;

# persistent thread pool (reuse worker OS threads; avoids per-task thread spawn from pmap/pfor)
my $pool = ppool(4);
$pool->submit(sub { heavy_work($_) }, $_) for @tasks;   # optional 2nd arg binds $_
my @results = $pool->collect();

# control thread count
pe -j 8 -e 'my @r = pmap { heavy_work($_) } @data'
```

Each parallel block receives its own interpreter context with captured lexical scope // no data races. Use `mysync` to share state.

#### EXECUTION TRACE // `trace`

Wrap parallel (or any) code to print **`mysync` scalar** mutations to **stderr** — useful when debugging races or ordering. Under `fan N { }`, lines are tagged with the worker index (same as `$_`).

```perl
mysync $counter = 0;
trace { fan 10 { $counter++ } };
# stderr: [thread 0] $counter: 0 → 1
# stderr: [thread 3] $counter: 1 → 2
# ... (order varies)
```

Outside `fan`, mutations are labeled `[main]`.

#### TIMER // `timer`

Wall-clock benchmark: **`timer { BLOCK }`** returns elapsed time as a **float** in **milliseconds** (sub-ms resolution).

```perl
my $elapsed = timer { heavy_work() };
print "took ${elapsed}ms\n";
```

#### ASYNC / AWAIT // lightweight I/O parallelism

**`async { BLOCK }`** runs the block on a **worker thread** and returns a task handle immediately. **`await EXPR`** joins: if `EXPR` is that handle, it blocks until the block finishes and returns its value; otherwise `await` passes the value through.

Use this to overlap **`fetch_url`**, **`slurp`**, or other I/O-bound work without blocking the main interpreter until you **`await`**.

```perl
my $data = async { fetch_url("https://example.com/") };
my $file = async { slurp("big.csv") };
print await($data), await($file);
```

Each `async` worker gets a **clone of the interpreter’s subs** and a **captured lexical scope** (including **`mysync`** storage), so closures and shared state behave like other parallel primitives.

#### NATIVE CSV / SQLITE / STRUCTS // data scripting

**HTTP** — [`ureq`](https://crates.io/crates/ureq) blocking GET. **`fetch(url)`** returns the response body as a string. **`fetch_json(url)`** parses JSON with [`serde_json`](https://crates.io/crates/serde_json): JSON objects become **hashrefs**, arrays become **arrays**, `null` → `undef`, numbers → integers or floats. (Lower-level **`fetch_url $url`** is still available as an expression form.)

**CSV** — [`csv`](https://crates.io/crates/csv) backed. `csv_read(path)` returns an array of **hashrefs** (first row is the header). `csv_write(path, row, …)` or `csv_write(path, \@rows)` writes rows (each row is a hash or hashref); header columns are the union of keys in first-seen order.

**SQLite** — embedded database via [`rusqlite`](https://crates.io/crates/rusqlite) with **bundled** libsqlite (no system SQLite required). `sqlite(path)` returns a handle: `->exec(sql, ?…)`, `->query(sql, ?…)` (rows as hashrefs), `->last_insert_rowid`.

**Structs** — `struct Name { field => Type, … }` with `Type` one of `Int`, `Str`, `Float`. Constructor: `Name->new(field => value, …)`. Field read: `$obj->fieldname` (same as a method call). The VM builds native struct instances (not plain blessed hashes) when the struct is declared in the same program.

**Typed `my`** — `typed my $x : Int` (or `Str` / `Float`): assignments are checked at runtime; mismatches are type errors.

```perl
my $data = fetch_json("https://api.example.com/users/1");
say $data->{name};

my $raw = fetch("https://example.com/");

my @rows = csv_read("data.csv");
csv_write("out.csv", { name => "a", id => "1" });

my $db = sqlite("app.db");
$db->exec("CREATE TABLE t (id INTEGER, name TEXT)");
my @r = $db->query("SELECT * FROM t WHERE id > ?", 0);

typed my $n : Int;
$n = 42;

struct Point { x => Float, y => Float };
my $p = Point->new(x => 1.5, y => 2.0);
say $p->x;
```

#### THREAD-SAFE SHARED STATE // `mysync`

`mysync` declares variables backed by `Arc<Mutex>` that are shared across parallel blocks. All reads/writes go through the lock automatically. Compound operations (`++`, `+=`, `.=`, and `|=`, `&=` on scalars holding a native `Set`) are fully atomic — the lock is held for the entire read-modify-write cycle.

For **`mysync` scalars that hold a `Set`** (from `mysync $s = Set->new(...)`), union (`|`) and intersection (`&`) treat the stored value as a set even when the underlying storage is the mutex-wrapped scalar; bitwise `|` / `&` on plain integers is unchanged.

**`deque()` and `heap(...)`**: `mysync $q = deque()` (and `mysync $pq = heap(sub { $a <=> $b })`) stores the value without an extra `Atomic` shell — they already use `Arc<Mutex<…>>`. Use them like any other `mysync` scalar in `fan` / `pmap` / `pfor` so all workers share one queue or heap.

```perl
# shared scalar — atomic increment
mysync $counter = 0;
fan 10000 { $counter++ };
print $counter;  # always exactly 10000

# shared array — thread-safe push/pop/shift
mysync @results;
pfor { push @results, $_ * $_ } (1..100);
print scalar @results;  # always exactly 100

# shared hash — atomic element access
mysync %histogram;
pfor { $histogram{$_ % 10} += 1 } (0..999);
# each bucket is exactly 100

# mix all three
mysync $total = 0;
mysync @items;
mysync %stats;
fan 1000 {
    $total++;
    push @items, $_;
    $stats{$_ % 5} += 1;
};
# $total == 1000, @items == 1000, sum(%stats) == 1000
```

Without `mysync`, each parallel thread gets an independent copy — changes are not visible to other threads or the parent. With `mysync`, all threads share the same underlying storage via `Arc<Mutex>`.

 ┌──────────────────────────────────────────────────────────────┐
 │ ATOMIC OPS: $x++ &nbsp;&nbsp; ++$x &nbsp;&nbsp; $x += N &nbsp;&nbsp; $x .= "s" &nbsp;&nbsp; $s |= $t (Set) │
 │ ATOMIC OPS: $h{k} += N &nbsp;&nbsp; $a[i] += N &nbsp;&nbsp; push @a, $v      │
 │ LOCK SCOPE: held for full read-modify-write // zero races   │
 └──────────────────────────────────────────────────────────────┘

---

## [0x04] CLI FLAGS

![pe -h](img/pe-help.png)

---

## [0x05] SUPPORTED PERL FEATURES

#### DATA TYPES
- Scalars (`$x`), arrays (`@a`), hashes (`%h`)
- References: `\$x`, `\@a`, `\%h`, `\&sub`
- Array refs `[1,2,3]`, hash refs `{a => 1}`
- Code refs / closures `sub { ... }`
- Regex objects `qr/.../`
- Blessed references (basic OOP)
- `typed my $x : Int|Str|Float` (runtime-checked assignments)
- `struct Name { field => Type, … }` with `Name->new(…)` and `$obj->field`
- Native CSV (`csv_read` / `csv_write`) and SQLite (`sqlite` + `->exec` / `->query`)
- `fetch` / `fetch_json` (HTTP GET via `ureq`; JSON → Perl values)

#### CONTROL FLOW
- `if`/`elsif`/`else`, `unless`
- `while`, `until`, `do { } while/until` (block runs before the first condition check)
- `for` (C-style), `foreach`
- `last`, `next`, `redo` with labels
- Postfix: `expr if COND`, `expr unless COND`, `expr while COND`, `expr for @list`
- Ternary `?:`
- **`try { } catch ($err) { }` [`finally { }`]** — statement form only (not an arbitrary expression, so not `my $x = try { … }`); catches `die` and other runtime errors (not `exit`, not `last`/`next`/`return` flow); the error string is bound to the scalar in `catch`. Optional **`finally`** runs after a successful `try` or after `catch` finishes (including if `catch` propagates an error); if `finally` fails, that error is returned (Perl-style).
- **`given (EXPR) { when (COND) { } default { } }`** — topic is **`$_`**; `when` tests in order (regex `=~` for regex literals, string equality for string/number literals, otherwise string comparison to the evaluated condition); first match wins; put **`default` last** (tree-walker only)
- **`eval_timeout SECS { }`** — runs the block on a **worker OS thread**; the main thread waits up to **`SECS`** seconds via `recv_timeout` (no Unix `alarm`); on timeout you get a runtime error (the worker may keep running in the background—avoid relying on cancellation for correctness)

#### OPERATORS
- Arithmetic: `+`, `-`, `*`, `/`, `%`, `**`
- String: `.` (concat), `x` (repeat)
- Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`, `<=>`
- String comparison: `eq`, `ne`, `lt`, `gt`, `le`, `ge`, `cmp`
- Logical: `&&`, `||`, `//`, `!`, `and`, `or`, `not`
- Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>` (for native `Set` values, `|` / `&` are union / intersection instead of integer bitwise ops)
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `.=`, `|=`, `&=`, `//=`, etc.
- Regex: `=~`, `!~`
- Range: `..`
- Arrow dereference: `->`

#### REGEX ENGINE
- Match: `$str =~ /pattern/flags`
- Dynamic pattern (string): `$str =~ $pattern` and `$str !~ $pattern` (bytecode `RegexMatchDyn`; empty flags)
- Substitution: `$str =~ s/pattern/replacement/flags`
- Transliterate: `$str =~ tr/from/to/`
- Flags: `g`, `i`, `m`, `s`, `x`
- Capture variables: `$1`, `$2`, … (all numbered groups, not only 1–9); named groups `(?<name>…)` or `(?P<name>…)` populate **`%+`** and **`$+{name}`** (same rules as the Rust `regex` crate)
- Literal spans: `\Q…\E` (metacharacters escaped); `quotemeta` for dynamic patterns
- Quote-like: `m//`, `qr//`

#### SUBROUTINES
- Named subs with `sub name { ... }`
- Anonymous subs / closures
- Recursive calls
- `@_` argument passing, `shift`, `return`
- `return EXPR if COND` (postfix modifiers on return)
- **`AUTOLOAD`**: missing subs and methods dispatch to `sub AUTOLOAD` with `$AUTOLOAD` set to the fully qualified name (e.g. `main::foo`, `MyClass::bar`)

#### BUILT-IN FUNCTIONS

 ┌──────────────────────────────────────────────────────────────┐
 │ **Array**: push, pop, shift, unshift, splice, reverse,      │
 │ sort, map, grep, reduce, preduce, scalar                    │
 │ **Hash**: keys, values, each, delete, exists                │
 │ **String**: chomp, chop, length, substr, index, rindex,     │
 │ split, join, sprintf, printf, uc, lc, ucfirst, lcfirst,     │
 │ chr, ord, hex, oct, crypt, fc, pos, study, quotemeta          │
 │ **Binary**: pack, unpack (subset A a N n V v C Q q Z H x;   │
 │ `*` repeat; result is byte data)                              │
 │ **Numeric**: abs, int, sqrt, sin, cos, atan2, exp, log,     │
 │ rand, srand                                                 │
 │ **I/O**: print, say, printf, open (files + `-|` / `|-`       │
 │ piped shell), close, eof, readline,                         │
 │ slurp, capture (structured shell: ->stdout/stderr/exit),   │
 │ binmode, fileno, flock, getc, sysread, syswrite, sysseek,  │
 │ select (timeout sleep / handle no-op), truncate             │
 │ **Directory**: opendir, readdir, closedir, rewinddir,        │
 │ telldir, seekdir                                              │
 │ **File tests**: -e, -f, -d, -l, -r, -w, -s, -z, -t (TTY)   │
 │ **System**: system, exec, exit, chdir, mkdir, unlink, rename, │
 │ chmod, chown (Unix), stat, lstat, link, symlink, readlink,   │
 │ glob, glob_par, ppool, barrier,                               │
 │ **Data**: csv_read, csv_write (header row → AoH), sqlite     │
 │ fork, wait, waitpid, kill, alarm, sleep, times (Unix where  │
 │ noted in source)                                            │
 │ **Socket** (std::net): socket, bind, listen, accept,         │
 │ connect, send, recv, shutdown                               │
 │ **Type**: defined, undef, ref, bless                        │
 │ **Set**: `Set->new(…)` — native set; `|` union, `&` intersection │
 │ **Control**: die, warn, eval, do, require, caller,         │
 │ wantarray (void / scalar 0 / list 1; bytecode passes context), │
 │ `goto EXPR` (same-block labels), `continue { }` on loops, │
 │ `prototype` on code refs; sub prototypes parsed on `sub`     │
 └──────────────────────────────────────────────────────────────┘

#### EXTENSIONS BEYOND STOCK PERL 5
- **`csv_read PATH` / `csv_write PATH, \@rows`** — native CSV via the Rust `csv` crate. The first row is column headers; each data row is a hashref (string cells). `csv_write` uses the first row’s key order for columns.
- **`sqlite(PATH)`** — embedded SQLite via `rusqlite` (bundled libsqlite). Handle methods: `->exec(SQL, ?bind…)`, `->query(SQL, ?bind…)` (array of hashrefs), `->last_insert_rowid`.
- **`par_lines PATH, sub { ... }`** — memory-map the file, split into line-aligned byte chunks, process chunks in parallel with rayon; each line sets `$_` for the coderef (CRLF-safe; tree-walker only; use `mysync` for shared counters across workers).
- **`pwatch GLOB, sub { ... }`** — register native file/directory watches with the `notify` crate (inotify/kqueue/FSEvents); block in the event loop and dispatch each glob-matching path to the coderef on a rayon worker with `$_` set to the path (tree-walker only; use `mysync` for shared state).
- **`barrier(N)`** — returns a handle backed by `std::sync::Barrier`; `->wait` for phased parallelism (e.g. with `fan`). Party count is clamped to at least 1 (bytecode + tree-walker).
- **`sort` / `psort` fast path** — `{ $a <=> $b }`, `{ $a cmp $b }`, `{ $b <=> $a }`, `{ $b cmp $a }` compare without invoking the block per pair (VM + tree-walker).
- **`reduce` / `preduce`** — list fold with `$a` (accumulator) and `$b` (next item); `reduce` is strictly left-to-right; `preduce` uses rayon (order not fixed; use only when the operation is associative).
- **`frozen my`** — immutable bindings (reassignment rejected in the bytecode path).
- **`typed my $x : Type`** — optional scalar types (`Int`, `Str`, `Float`) with **runtime** checks on declaration and every assignment; `typed my` runs on the tree-walker (bytecode falls back when the program uses it).
- **`try` / `given` / `eval_timeout`** — implemented in the tree interpreter only; the bytecode compiler returns unsupported for these constructs, so execution falls back to `execute_tree` automatically.

#### OTHER FEATURES
- `Interpreter::execute` returns `Err(ErrorKind::Exit(code))` for `exit` (including code 0); the `perlrs` binary maps that to `process::exit`.
- **`@INC` / `%INC` / `require` / `use`** — The `perlrs` / `pe` driver builds `@INC` by: each **`-I`** directory, then **`<crate>/vendor/perl`** when present (e.g. a stub **`List/Util.pm`** so `require List::Util` updates **`%INC`**), then the same paths **system `perl` reports for `@INC`** (via `perl -e 'print join "\n", @INC'`, so other core/site `.pm` paths match Perl’s search order), then the **script’s directory** (when the program file is not `-e` / `-` / `repl`), then **`PERLRS_INC`**, then **`.`** (duplicates removed). Set **`PERLRS_NO_PERL_INC`** to omit the `perl` query (e.g. no `perl` on `PATH`). **`List::Util`** (all **`EXPORT_OK`** names from Perl 5’s module, including **`reduce`**, **`any`**, **`pairs`**, **`zip`**, …) is implemented **natively in Rust** (`src/list_util.rs`) and registered on interpreter startup; core Perl still loads XS for these, but perlrs does not. Pure perlrs execution is still **not** full Perl 5: many other real `.pm` files (especially XS) will not run even when found. Relative paths (`Foo::Bar` → `Foo/Bar.pm`) are searched in order; successful loads record the relative path in **`%INC`**; repeated `require` is a no-op. **`use Module;`** is processed in **source order** before `BEGIN` blocks (and before the VM main chunk runs). After a successful load, **`use Module qw(a b);`** imports only those subs; bare **`use Module;`** imports every sub registered under that package prefix; **`use Module qw();`** imports nothing. **Qualified calls** like **`Foo::bar()`** are valid syntax. Built-in pragmas (`strict`, `warnings`, `utf8`, …) do not load a file. Version-only `require` (e.g. `require 5.010`) succeeds without loading.
- **Pragmas (porting)** — `use strict` enables **refs / subs / vars**; `use strict 'refs'` or `use strict qw(refs subs)` selects modes; `no strict` clears all, `no strict 'refs'` clears one. **Runtime:** **`strict refs`** rejects symbolic scalar/array/hash derefs (string used as a variable name) with a Perl-like message; **`$$foo`** is parsed as symbolic scalar deref of **`$foo`** (falls back to the tree interpreter when bytecode is compiled). **`strict vars`** requires a visible binding (`my`/`our`/prior assignment in scope) for unqualified scalars, arrays, and hashes (package-qualified names and built-in specials like `$_`, `$1`, … are exempt); **`strict subs`** appends a hint to undefined subroutine errors. **`use warnings` / `no warnings`** toggle the interpreter warnings flag (reserved for future warning surfaces). **`feature_bits`** (crate **`FEAT_*`**) are set by **`use feature`** / **`no feature`**; **`say`** is gated by **`FEAT_SAY`** (on by default like Perl 5.10+; disable with **`no feature 'say'`** or **`no feature;`**). **`use utf8` / `no utf8`** set **`utf8_pragma`**. **`no`** pragmas run in the same **prepare** phase as **`use`**. The interpreter starts with **`@ARGV`**, **`@_`**, **`%ENV`**, and **`@INC`/`%INC`** pre-bound so **`strict vars`** matches typical Perl scripts.
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
 │      └── Parallel:   pmap, pgrep, psort, pfor, fan │
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

## [0x08] BENCHMARKS

 ┌──────────────────────────────────────────────────────────────┐
 │ BENCHMARK SUITE // perlrs vs perl 5.42.2 // Apple M5 18-core │
 └──────────────────────────────────────────────────────────────┘

```
  TEST                    perl5(ms) perlrs(ms)      RATIO
  ──────────────────── ───────── ────────── ─────
  startup                     6.8ms      6.4ms      0.94x  ✓ faster
  fib(25)                    23.6ms     48.4ms      2.05x
  loop 10k                    7.1ms      7.6ms      1.07x  ≈ parity
  string concat 10k           7.0ms     11.2ms      1.60x
  hash 1k                     7.3ms     18.5ms      2.53x
  array sort 10k              7.8ms    236.9ms     30.37x
  regex match 1k              7.2ms     35.4ms      4.92x
  map+grep 10k                7.6ms      7.7ms      1.01x  ≈ parity
```

> Measured on macOS with `perl v5.42.2` vs `perlrs` release build (LTO + O3).
> Times include process startup (~7ms). Run with `bash bench/run_bench.sh`.

#### Analysis

- **startup** and **map+grep** are at parity or faster — the Rust binary cold-starts faster than perl and the bytecode VM dispatches map/grep at native speed
- **loop** is within 7% — the VM's flat dispatch loop with integer fast paths nearly matches perl's decades-old bytecode engine
- **fib** is 2x slower — recursive function calls still pay scope-frame overhead; a register-based VM would close this gap
- **string** is 1.6x — each `$s = $s . "x"` clones the growing string; an in-place append optimization would fix this
- **hash** is 2.5x — hash iteration via `keys %h` + `$h{$k}` involves more indirection than perl's internal HV
- **regex** is 4.9x — the regex itself is cached and fast (Rust `regex` crate with SIMD), but the 1000-iteration for-loop + if-block overhead dominates
- **`s///` and `tr///`** — compile to `RegexSubst` / `RegexTransliterate` bytecode (same `regex` crate work as `m//`); previously the compiler rejected these and forced a full tree-walker run for the whole program
- **array sort** is 30x in the table above — `bench/bench_array.pl` uses `sort { $a <=> $b }`, which now has a **native fast path** (no per-compare `exec_block`); re-run `bench/run_bench.sh` for a fresh ratio. Arbitrary `{ ... }` comparators still use the interpreter each compare

#### Parallel speedup

```
  fan 18 { system("sleep 0.1") }   →  0.12s total  (vs 1.8s sequential)
```

True parallelism across all cores via rayon work-stealing. The `fan`, `pmap`, `pgrep`, `pfor`, and `psort` commands distribute work automatically.

---

## [0x09] DEVELOPMENT & CI

Pull requests and pushes to `main` run the workflow in [`.github/workflows/ci.yml`](.github/workflows/ci.yml). You can also run it manually from the repository **Actions** tab (**workflow dispatch**). On a pull request, the **Checks** tab (or the merge box) shows the aggregate status; open the **CI** workflow run for per-job logs (Check, Test, Format, Clippy, Doc, Release Build).

Library unit tests (parser smoke batches `parse_smoke_*`, **`parser_shape_tests`**, lexer/token/value/error/scope/`ast`, **`interpreter_unit_tests`**, **`crate_api_tests`**, **`run_semantics_tests`** / **`run_semantics_more`** (`run` coverage), **`bytecode::Chunk`** pool/intern/jump patching, **`compiler`** compile-to-op smoke checks, **`vm`** hand-built bytecode execution, `parse` / `try_vm_execute`); excludes `tests/` integration suite):

```sh
cargo test --lib
```

Integration tests live in `tests/integration.rs` and `tests/suite/` (grouped modules such as `runtime_extra` and `runtime_more` for assignment, builtins, aggregates, control flow, and regex/subs):

```sh
cargo test --test integration
```

Extended parse-only smoke coverage is in `src/parse_smoke_extended.rs` and `src/parse_smoke_batch2.rs` (built only with `cfg(test)`).

CI uses `cargo … --locked`; **`Cargo.lock` is committed** so dependency resolution matches CI and release builds. If you use a global gitignore that ignores `Cargo.lock`, force-add updates when dependencies change: `git add -f Cargo.lock`.

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
