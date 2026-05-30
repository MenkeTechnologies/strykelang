```
 ███████╗████████╗██████╗ ██╗   ██╗██╗  ██╗███████╗
 ██╔════╝╚══██╔══╝██╔══██╗╚██╗ ██╔╝██║ ██╔╝██╔════╝
 ███████╗   ██║   ██████╔╝ ╚████╔╝ █████╔╝ █████╗
 ╚════██║   ██║   ██╔══██╗  ╚██╔╝  ██╔═██╗ ██╔══╝
 ███████║   ██║   ██║  ██║   ██║   ██║  ██╗███████╗
 ╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝
```

[![CI](https://github.com/MenkeTechnologies/strykelang/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/strykelang/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/strykelang.svg)](https://crates.io/crates/strykelang)
[![Downloads](https://img.shields.io/crates/d/strykelang.svg)](https://crates.io/crates/strykelang)
[![Docs.rs](https://docs.rs/strykelang/badge.svg)](https://docs.rs/strykelang)
 [![Docs](https://img.shields.io/badge/docs-online-blue.svg)](https://menketechnologies.github.io/strykelang/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

### `[THE FASTEST DYNAMIC LANGUAGE IN THE WORLD FOR PARALLEL OPERATIONS]`

> *"There is more than one way to do it — in parallel."*
>
> *"100% TDP — beware."*
>
> *"The hottest language ever created. Literally."*

## `[PATENT PENDING]`

The 2nd fastest dynamic language runtime ever benchmarked for singlethreaded — behind only Mike Pall's LuaJIT, and beating it on 3 of 8 benchmarks. The fastest on all mulithreaded benchmarks.  A Perl 5 compatible interpreter in Rust with native parallel primitives, NaN-boxed values, three-tier regex, bytecode VM + Cranelift JIT, streaming iterators, and rayon work-stealing across all cores. Faster than perl5, Python, Ruby, Julia, and Raku on every benchmark.

### [`Read the Docs`](https://menketechnologies.github.io/strykelang/) &middot; [`Full Reference`](https://menketechnologies.github.io/strykelang/reference.html)

---

## Table of Contents

- [\[0x00\] Overview](#0x00-overview)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Usage](#0x02-usage)
- [\[0x03\] Parallel Primitives](#0x03-parallel-primitives)
- [\[0x04\] Shared State (`mysync`)](#0x04-shared-state-mysync)
- [\[0x05\] Native Data Scripting](#0x05-native-data-scripting)
- [\[0x06\] Async / Trace / Timer](#0x06-async--trace--timer)
- [\[0x06b\] AOP — Before / After / Around Advice](#0x06b-aop--before--after--around-advice)
- [\[0x07\] CLI Flags](#0x07-cli-flags)
- [\[0x08\] Supported Perl Features](#0x08-supported-perl-features)
- [\[0x08a\] `--no-interop` Mode](#0x08a---no-interop-mode)
- [\[0x08b\] String Coordinates — Bytes vs Codepoints](#0x08b-string-coordinates--bytes-vs-codepoints)
- [\[0x09\] Architecture](#0x09-architecture)
- [\[0x0A\] Examples](#0x0a-examples)
- [\[0x0B\] Benchmarks](#0x0b-benchmarks)
- [\[0x0C\] Development & CI](#0x0c-development--ci)
- [\[0x0C-test\] Test Runner — Worker Pool Architecture](#0x0c-test-test-runner--worker-pool-architecture)
- [\[0x0D\] Standalone Binaries (`stryke build`)](#0x0d-standalone-binaries-stryke-build)
- [\[0x0E\] Inline Rust FFI (`rust { ... }`)](#0x0e-inline-rust-ffi-rust---)
- [\[0x0F\] Bytecode Cache (rkyv)](#0x0f-bytecode-cache-rkyv)
- [\[0x10\] Distributed `pmap_on` / `~d>` over SSH (`cluster`)](#0x10-distributed-pmap_on--d-over-ssh-cluster)
- [\[0x10a\] Infrastructure Load Testing](#0x10a-infrastructure-load-testing)
- [\[0x10b\] Agent/Controller Architecture](#0x10b-agentcontroller-architecture)
- [\[0x10c\] Scriptable Distributed Compute — `congregation` / `pray` / `annex`](#0x10c-scriptable-distributed-compute--congregation--pray--annex)
- [\[0x11\] Language Server (`stryke lsp`)](#0x11-language-server-stryke-lsp)
- [\[0x12\] Language Reflection](#0x12-language-reflection)
- [\[0x14\] Package Manager](#0x14-package-manager)
- [\[0x15\] Web Framework (`s_web`)](#0x15-web-framework-s_web)
- [\[0x16\] AI Primitives](#0x16-ai-primitives)
- [\[0x17\] Expect / Interactive Automation](#0x17-expect--interactive-automation)
- [\[0x18\] Documentation](#0x18-documentation)
- [\[0xFF\] License](#0xff-license)

### What's new — NAT traversal + value lineage + multi-target teleport

Stack added across `v1` → `v1.5` (under [\[0x10b\] Agent/Controller](#0x10b-agentcontroller-architecture)):

| Layer | Builtins | What it does |
|---|---|---|
| Network probes | [`kick`](#builtins-kick--udp_send--tcp-knock--udp-multi-shot) / [`udp_send`](#builtins-kick--udp_send--tcp-knock--udp-multi-shot) | TCP liveness probe + fire-and-forget UDP send (Wake-on-LAN, NAT keepalive) |
| P2P primitives | [`udp_open` / `udp_send_to` / `udp_recv` / `udp_recv_from` / `udp_close` / `stun` / `stun_classify` / `punch`](#builtins-udp_open--stun--punch--udp_send_to--udp_recv--udp_close--p2p-over-the-open-internet) | Persistent UDP socket pool, RFC 8489 STUN client (IPv4 + IPv6), UDP hole-punching, symmetric-NAT detection |
| TURN fallback | [`turn_allocate` / `turn_permission` / `turn_send` / `turn_recv` / `turn_refresh`](#builtins-turn_allocate--turn_permission--turn_send--turn_recv--turn_refresh--turn-relay-fallback-rfc-8656) | RFC 8656 TURN client (HMAC-SHA1 auth) for when hole-punching fails (~20-30% of cases: symmetric NATs, UDP-blocking firewalls) |
| Orchestration | [`ice::connect`](#ice-lite-orchestrating-direct--punch--relay-in-stryke-source) (stryke source) | Three-rung ladder: direct → punch → relay, first-success wins |
| Value lineage | [`mark` / `provenance` / `unmark`](#builtins-mark--provenance--unmark--value-lineage-as-a-first-class-feature) | Arc-keyed per-value lineage tracking with weak-ref GC, zero overhead when unused |
| Multi-target IPC | [`teleport` / `arrive`](#builtins-teleport--arrive--multi-target-shm-ipc) | POSIX-SHM broadcast value to N stryke processes + per-receiver UDS notify. One segment, N readers — beats N×bincode-over-pipe for big payloads |

Demos: [`p2p_chat.stk`](examples/p2p_chat.stk) / [`p2p_chat_v2.stk`](examples/p2p_chat_v2.stk) / [`turn_relay_chat.stk`](examples/turn_relay_chat.stk) / [`ice_orchestrator.stk`](examples/ice_orchestrator.stk) / [`turn_health_check.stk`](examples/turn_health_check.stk) / [`provenance_basics.stk`](examples/provenance_basics.stk) / [`provenance_audit_log.stk`](examples/provenance_audit_log.stk) / [`provenance_chain_walkthrough.stk`](examples/provenance_chain_walkthrough.stk) / [`teleport_broadcast.stk`](examples/teleport_broadcast.stk)

---

## [0x00] OVERVIEW

`stryke` parses and executes Perl 5 scripts with rayon-powered work-stealing primitives across every CPU core. Highlights:

- **Server farms first** — the first language designed for distributed infrastructure load testing
- **Bare metal heat** — `heat(60)` pins ALL cores to 100% TDP for 60 seconds
- **Agent/Controller architecture** — `stryke controller` + `stryke agent` for fleet-wide stress testing
- **AI is a primitive, not a library** — `ai "summarize this", $doc` with auto-attached `tool fn`s, MCP client+server, agent loop, RAG memory, vector search ([§ 0x16](#0x16-ai-primitives))
- **Web framework `s_web`** — Rails-shaped scaffold + ERB engine + SQLite ORM + admin panel + auth + PWA + Dockerfile, `s_web new app --app everything --theme cyberpunk --auth --admin --migrate` ([§ 0x15](#0x15-web-framework-s_web))
- **PTY-driven interactive automation** — `pty_spawn`/`pty_expect`/`pty_send`/`pty_interact`, the modern Tcl/Expect successor with cluster fanout ([§ 0x17](#0x17-expect--interactive-automation))
- **rkyv KV store** — *world-first*. First-class CRUD store with rkyv as the on-disk codec. `kv_open`/`kv_put`/`kv_get`/`kv_del`/`kv_exists`/`kv_keys`/`kv_scan`/`kv_len`/`kv_commit`/`kv_batch`/`kv_close`/`kv_stats` ship as native builtins. Reads are `mmap + validate + cast` (zero-copy, no parse, no allocate per row) — the same primitive `script_cache.rs` already uses for bytecode. SQLite-shaped API ergonomics, beats SQLite on reads for any store that fits comfortably in RAM. Atomic rewrite on commit (tmp + rename), versioned format header (`STKV` magic + `format_version`), all-or-nothing `kv_batch`. Phase 2 ships `stryke kvd` server + remote `kv_connect` client over the same archive bytes.
- **Sketch algebra** — *world-first*. Probabilistic data structures are first-class operands for `+ | & ^ -`: `$bloom_a + $bloom_b` (Bloom union), `$hll_a + $hll_b` (HyperLogLog union), `$cms_a + $cms_b` (Count-Min counter sum), `$topk_a + $topk_b` (SpaceSaving merge), `$td_a + $td_b` (t-digest centroid merge), `$rb_a | $rb_b` / `& ^ -` (Roaring set algebra). Operators are functional — operands are never mutated. No other language treats these as syntactic primitives; everywhere else they are library function calls.
- **All zsh glob qualifiers in a scripting language** — world-first. Every qualifier from zsh's `zshexpn(1)` works wherever stryke takes a glob (`glob`, `glob_par`, `slurp`/`c`/`cat`, `swallow`/`swa`, `ingest`/`ing`, `pwatch`, `<...>`, `par_find_files`): file-type, permission, ownership, size/links/time numerics, sort + descending sort, `[N,M]` selection, `(N)` null-glob, `(D)` dotfiles, `(F)` non-empty dir, `(f<bits>)` mode match, `(d<N>)` device, `(e'CMD')` eval, `(P…)`/`(Q…)` join words, `^` negate, `-` follow-symlinks toggle, `,` OR, `:` colon modifiers. Backed by the zshrs glob engine — single source of truth, zero stryke-side reimplementation.
- **Package manager** — Cargo-shaped `stryke.toml` + `stryke.lock`, `s add`/`s install`/`s tree` resolver, hash-pinned reproducible builds ([§ 0x14](#0x14-package-manager))
- **New Parallel Subroutines and |> Pipeline Syntactic Sugar**
- **Runtime values** — `StrykeValue` is a NaN-boxed `u64`: immediates (`undef`, `i32`, raw `f64` bits) and tagged `Arc<HeapObject>` pointers for big ints, strings, arrays, hashes, refs, regexes, atomics, channels.
- **Three-tier regex** — Rust [`regex`](https://docs.rs/regex) → [`fancy-regex`](https://docs.rs/fancy-regex) (backrefs) → [`pcre2`](https://docs.rs/pcre2) (PCRE-only verbs).
- **Bytecode VM + JIT** — match-dispatch interpreter with Cranelift block + linear-sub JIT (`strykelang/vm.rs`, `strykelang/jit.rs`).
- **Rayon parallelism** — every parallel builtin uses work-stealing across all cores.
- **10,431 standard library primaries** in `%b` (11,159 keys in `%all` including aliases and keywords) — largest bareword library of any language; clears Wolfram v14.3's high-band estimate (~7,300) by ~3,131
- **40 MB single static binary** — `~/.cargo/bin/s` ships every builtin in one file, ~3.6 KB amortized per builtin, ~200&times; denser than Wolfram Engine per builtin/byte, sub-10 ms cold start

---

## [0x01] INSTALL

```sh
# Via Homebrew tap (auto-bumped by each release; formula is `stryke`)
brew tap MenkeTechnologies/menketech
brew install stryke

# Or via crates.io
cargo install strykelang

# Or from source
git clone https://github.com/MenkeTechnologies/strykelang && cd strykelang && cargo build --release
```

#### Zsh tab completion

```sh
cp completions/_stryke /usr/local/share/zsh/site-functions/_stryke
# or: fpath=(/path/to/stryke/completions $fpath) in .zshrc
autoload -Uz compinit && compinit
```

`stryke <TAB>` then completes flags, options, and script files.

---

## [0x01b] CONCISENESS — STRYKE VS THE WORLD

stryke is the **most concise yet readable ASCII-only general-purpose scripting language** — shorter than Perl, Ruby, Python, and AWK for real-world tasks.

### vs mainstream languages

| Task | stryke | chars | perl | chars | ruby | chars | python | chars |
|------|--------|-------|------|-------|------|-------|--------|-------|
| hello world | `p"hello"` | **8** | `print"hello"` | 12 | `puts"hello"` | 10 | `print("hello")` | 14 |
| sum 1-100 | `p sum 1:100` | **11** | `use List::Util'sum';say sum 1..100` | 38 | `p (1..100).sum` | 15 | `print(sum(range(1,101)))` | 24 |
| double+filter+sum | `~>1:10map{_*2}fi{_>5}sum p` | **28** | `say for grep{$_>5}map{$_*2}1..10` | 36 | `p (1..10).map{...}.select{...}.sum` | 42 | `print(sum(x for x in[...]))` | 56 |
| max of list | `p max 3,1,4,1,5` | **15** | `use List::Util'max';say max(...)` | 38 | `p [3,1,4,1,5].max` | 17 | `print(max([3,1,4,1,5]))` | 23 |
| reverse string | `p rev"hello"` | **12** | `say reverse"hello"` | 18 | `puts"hello".reverse` | 18 | `print("hello"[::-1])` | 20 |
| count array | `p cnt 1:10` | **10** | `say scalar 1..10` | 17 | `p (1..10).count` | 16 | `print(len(range(1,11)))` | 23 |
| join with comma | `p join",",1:5` | **14** | `say join",",1..5` | 17 | `puts (1..5).to_a.join(",")` | 24 | `print(",".join(map(...)))` | 36 |
| first element | `p first 1:10` | **13** | `say((1..10)[0])` | 16 | `p (1..10).first` | 16 | `print(list(range(...))[0])` | 27 |
| any even | `p any{even}1:5` | **14** | `use List::Util'any';say any{$_%2==0}1..5` | 42 | `p (1..5).any?{|x|x%2==0}` | 25 | `print(any(x%2==0 for x in range(1,6)))` | 38 |
| unique values | `p uniq 1,2,2,3` | **15** | `use List::Util'uniq';say uniq(...)` | 38 | `p [1,2,2,3].uniq` | 17 | `print(list(set([...])))` | 27 |

**stryke wins every task** against Perl, Ruby, and Python.

### vs K (array language)

K is more terse for pure array math: `+/1+!100` (8 chars) vs stryke `p sum 1:100` (11 chars). But K is a financial DSL, not a general-purpose language — it lacks:

| Feature | stryke | K |
|---------|--------|---|
| HTTP client | `fetch"url"` | ❌ |
| JSON parsing | `json_decode $s` | needs lib |
| Regex | `$s=~/\d+/` | limited |
| SHA256/crypto | `sha256"data"` | ❌ |
| Parallel map | `pmap{$_*2}@a` | ❌ |
| Compression | `gzip $data` | ❌ |
| Base64 | `b64e"hi"` | ❌ |
| UUID | `uuid` | ❌ |
| SQLite | `db_query $db,$sql` | ❌ |
| TOML/YAML | `toml_decode $s` | ❌ |

K is a calculator. stryke is a programming language.

### vs golf languages

GolfScript, Pyth, 05AB1E, Jelly — these are shorter but are write-only puzzles designed for competitions, not real software. stryke remains readable and maintainable.

---

## [0x01c] WHY STRYKE — ONE-LINER COMPARISON

`stryke` is a **one-liner-first** language. No `-e` flag needed, everything built in, shortest syntax wins.

### Character count — real tasks

| Task | `stryke` | `perl` | `ruby` | `python` | `awk` / other |
|---|---|---|---|---|---|
| Print hello world | `s 'p "hello world"'` **19c** | `perl -e 'print "hello world\n"'` 32c | `ruby -e 'puts "hello world"'` 29c | `python3 -c 'print("hello world")'` 34c | `echo \| awk '{print "hello world"}'` 36c |
| Sum 1..100 | `s 'p sum 1..100'` **16c** | `perl -MList::Util=sum -e 'print sum 1..100'` 45c | `ruby -e 'puts (1..100).sum'` 28c | `python3 -c 'print(sum(range(1,101)))'` 38c | — |
| Word frequencies | `s -an 'freq(@F) \|> dd'` **22c** | `perl -ane '$h{$_}++ for @F}{print "$_ $h{$_}\n" for keys %h'` 61c | — | — | `awk '{for(i=1;i<=NF;i++) a[$i]++} END{...}'` 65c+ |
| SHA256 of file | `s 'p s256"f"'` **13c** | `perl -MDigest::SHA=sha256_hex -e '...'` 70c+ | — | `python3 -c 'import hashlib;...'` 80c+ | `shasum -a 256 f` 15c |
| Fetch JSON API | `s 'fetch_json(URL) \|> dd'` **25c** | needs `LWP` + `JSON` modules | needs `net/http` + `json` | needs `urllib` + `json` | `curl -s URL \| jq .` ~40c |
| CSV → JSON | `s 'csv_read("f") \|> tj \|> p'` **28c** | needs `Text::CSV` + `JSON` | needs `csv` + `json` | needs `csv` + `json` imports | — |
| Parallel map | `s '1:1e6 \|> pmap { $_ * 2 }'` **29c** | not built in | not built in | not built in | `xargs -P8` 50c+ |
| Streaming parallel | `s 'range(0,1e9) \|> pmaps { $_ * 2 } \|> take 10'` **42c** | not built in | not built in | not built in | not built in |
| Sparkline | `s '(3,7,1,9) \|> spark \|> p'` **27c** | not built in | not built in | not built in | not built in |
| In-place sed (parallel) | `s -i -pe 's/foo/bar/g' *.txt` **28c** | `perl -i -pe 's/foo/bar/g' *.txt` 33c (sequential) | `ruby -i -pe '$_.gsub!(...)'` 35c+ | — | `sed -i '' 's/foo/bar/g' *.txt` 31c (sequential) |

### Feature matrix

| Feature | stryke | perl5 | ruby | python | awk | jq | nushell |
|---|---|---|---|---|---|---|---|
| No `-e` flag needed | **yes** | no | no | no (`-c`) | — | — | — |
| No semicolons | **yes** | no | yes | yes | yes | yes | yes |
| Built-in HTTP | **yes** | no | no | no | no | no | yes |
| Built-in JSON | **yes** | no | no | yes | no | **yes** | yes |
| Built-in CSV | **yes** | no | no | yes | no | `@csv` | yes |
| Built-in SQLite | **yes** | no | no | yes | no | no | yes |
| Parallel map/grep | **yes** | no | no | no | no | no | `par-each` |
| Pipe-forward `\|>` | **yes** | no | no | no | no | `\|` | `\|` |
| Thread macro `~>` | **yes** | no | no | no | no | no | no |
| In-place edit `-i` | **parallel** | sequential | sequential | no | no | no | no |
| Zsh glob qualifiers `(/)`/`(.)`/`(L+N)`/`(om[1])` | **yes** | no | no | no | no | no | no |
| Regex engine | **3-tier** | PCRE | Onigmo | `re` | ERE | PCRE | — |
| Data viz (spark/bars/flame) | **yes** | no | no | no | no | no | no |
| Clipboard (clip/paste) | **yes** | no | no | no | no | no | `clip` |
| `$NR`/`$NF` AWK compat | **yes** | `-MEnglish` | no | no | native | no | no |
| Typed structs/enums/classes | **yes** | no | native | native | no | no | native |
| JIT compiler | **Cranelift** | no | YJIT | no | no | no | no |
| Single binary | **33MB** | system pkg | system pkg | system pkg | system pkg | 3MB | 50MB+ |

---

## [0x02] USAGE

```sh
stryke 'p "Hello, world!"'                 # inline code — no -e needed
stryke 'p 1 + 2'                           # just quote and go
stryke script.stk arg1 arg2                  # script + args
stryke -lane 'p $F[0]'                     # bundled short switches
stryke -c script.stk                          # syntax check
stryke --lint script.stk                     # parse + compile (no run)
stryke --disasm script.stk                   # bytecode listing on stderr
stryke --ast script.stk                      # AST as JSON
stryke --fmt script.stk                      # pretty-print parsed source
stryke --profile script.stk                  # folded stacks + per-line/per-sub ns
stryke --flame script.stk                   # colored flamegraph bars in terminal
stryke --flame script.stk > flame.svg       # interactive SVG flamegraph when piped
stryke --explain E0001                      # expanded hint for an error code
stryke docs                                  # interactive reference book (vim-style: j/k/]/[/t/q)
stryke docs pmap                             # jump straight to a topic
stryke docs --toc                            # table of contents
stryke docs --search parallel                # search all pages
stryke 'doctor'                               # runtime health check — version / flags / paths / toolchain
stryke 'health'                               # alias for `doctor`
stryke 'lsp_words |> ep'                      # dump every name LSP tab-complete knows about
stryke 'p now'                                # current Unix-epoch seconds (alias of `time`)
stryke serve                                # static file server for $PWD on port 8000
stryke serve 8080 app.stk                   # HTTP server with handler script
stryke serve 3000 -e '"hello " . $req->{path}'  # one-liner HTTP server
stryke build script.stk -o myapp             # bake into a standalone binary ([0x0D])
stryke fmt -i .                              # format all .stk files recursively in place
stryke fmt lib/utils.stk                     # print formatted source to stdout
stryke minify app.stk                        # one-liner output (strip comments / POD, collapse newlines → `;`)
stryke minify -i lib/*.stk                   # minify in place — output still parses to the same AST
stryke check *.stk                           # parse + compile without executing (CI/editor)
stryke disasm script.stk                     # disassemble bytecode (learning/debugging)
stryke profile script.stk                    # run with profiling, structured output
stryke profile --flame script.stk -o out.svg # flamegraph to file
stryke bench                                 # run all benchmarks in bench/ or benches/
stryke init myapp                            # scaffold a new project (stryke.toml, lib/, t/, benches/)
stryke new myapp                             # alias for `init` that creates ./myapp/
stryke install                               # populate stryke.lock from stryke.toml (path deps; registry deps soon)
stryke add mylib --path=../mylib             # add a local path dep (registry deps land in RFC phase 7-8)
stryke remove mylib                          # drop a dep, regenerate stryke.lock
stryke tree                                  # print resolved dep graph from stryke.lock
stryke info mylib                            # show lockfile entry + store path for a dep
stryke repl                                  # start interactive REPL explicitly
stryke repl --load lib.stk                   # pre-load a library, then enter REPL
stryke lsp                                   # language server over stdio ([0x11])
stryke completions zsh                       # emit zsh completions to stdout
stryke ast script.stk                        # dump AST as JSON
stryke gen-docs                              # walk `.`, write Markdown docs for every .stk/.pl/.pm to docs/
stryke gen-docs lib --out site/api           # walk `lib/`, write to `site/api/` (mirrors source layout + index.md)
stryke prun *.stk                            # run multiple files in parallel
stryke -j 4 *.stk                             # run multiple files in parallel (4 threads)
stryke convert app.pl                        # convert Perl to stryke syntax with |> pipes
stryke deconvert app.stk                     # convert stryke back to Perl syntax
stryke app.stk                                # warm starts skip parse + compile via ~/.stryke/scripts.rkyv ([0x0F])
```

> **`-e` is optional.** If the first argument isn't a file on disk and looks like code, `stryke` runs it directly. `stryke 'p 42'` and `stryke -e 'p 42'` are equivalent. Use `-e` when combining with `-n`/`-p`/`-l`/`-a` (e.g. `stryke -lane 'p $F[0]'`).

#### Semicolons

A newline ends a statement, so you do not need a trailing `;` on each line. Use semicolons only when you put more than one statement on the same physical line.

```perl
my $answer = 40 + 2
p $answer                       # 42 — one statement per line, no `;` required

my $x = 1; my $y = 2; p $x + $y # 3 — same line needs `;` between statements
```

#### Interactive REPL

Run `stryke` with no arguments to enter a readline session: line editing, history (`~/.stryke/history`), tab completion for keywords, lexicals in scope, sub names, methods after `->` on blessed objects, and file paths. `exit`/`quit`/Ctrl-D leaves. Non-TTY stdin is read as a complete program.

**Edit mode (emacs / vi):** the REPL defaults to emacs keybindings (Ctrl-A, Ctrl-E, etc.). To switch to vi modal editing (insert / normal modes, `Esc` to leave insert, `h`/`j`/`k`/`l` to navigate, etc.) drop this into `~/.stryke/config.toml`:

```toml
[repl]
mode = "vi"   # or "emacs" (default)
```

`STRYKE_REPL_MODE=vi stryke` overrides the config for a single session — useful for sanity-checking either mode without editing the file.

#### `__DATA__`

A line whose trimmed text is exactly `__DATA__` ends the program; the trailing bytes are exposed via the `DATA` filehandle.

#### Stdin / `-n` / `-p` / `-i`

```sh
echo data | stryke -ne 'print uc $_'        # line loop
cat f.txt | stryke -pe 's/foo/bar/g'        # auto-print like sed
stryke -i -pe 's/foo/bar/g' file1 file2     # in-place edit (parallel across files)
stryke -i.bak -pe 's/x/y/g' *.txt           # with backup suffix
echo a:b:c | stryke -aF: -ne 'print $F[1]'  # auto-split
```

`-l` chomps each record and sets `$\`. `eof` with no args is true on the last line of stdin or each `@ARGV` file (Perl-compat).

**Text decoding** — script reads, `require`, `do`, `<>`, backticks, `par_lines`, etc. all use UTF-8 when valid, else Latin-1 octets per line/chunk (matches stock `perl` tolerance). `use open ':encoding(UTF-8)'` switches `<>` to UTF-8 with `U+FFFD` replacement. **`slurp` returns raw bytes** (Perl-default byte-string semantics — `length` is byte count, `spew` round-trips exact bytes); text ops (`eq`, regex, `substr`) still work because byte values stringify via the same UTF-8-or-Latin-1 decoder on demand.

---

## [0x03] PARALLEL PRIMITIVES

Each parallel block runs in its own interpreter context with **captured lexical scope** — no data races. Use `mysync` for shared counters. Optional `progress => 1` enables an animated stderr bar (TTY) or per-item log lines (non-TTY).

```perl
# map / grep / sort / fold / for in parallel (list can be piped in)
# Three surface forms work for pmap/pgrep/pfor/pcache/pflat_map:
#   pmap { $_ * 2 } @list              # block form  ($_ = element)
#   pmap $_ * 2, @list                 # expression form
#   pmap double, @list                 # bare-fn form (sub double { $_0 * 2 })
my @doubled = @data |> pmap $_ * 2 , progress => 1
my @evens   = @data |> pgrep $_ % 2 == 0
my @sorted  = @data |> psort { $a <=> $b }
my $sum     = @numbers |> preduce { $a + $b }
pfor process, @items
my @hashes  = pmap sha256, @blobs, progress => 1  # bare-fn

# streaming parallel — lazy iterators, bounded memory, output as it completes
range(0, 1e9) |> pmaps { expensive($_) } |> take 10 |> ep  # stops after 10 results
range(0, 1e6) |> pgreps { is_prime($_) } |> ep              # parallel filter, streaming
range(0, 1e6) |> pflat_maps { [$_, $_ * 10] } |> ep         # parallel flat-map, streaming

# fused map+reduce, chunked map, memoized map, init fold
my $sum2     = @nums |> pmap_reduce { $_ * 2 } { $a + $b }
my @squared  = @million |> pmap_chunked 1000 { $_ ** 2 }
my @once     = @inputs |> pcache expensive
my $hist     = @words |> preduce_init {}, { my ($acc, $x) = @_; $acc->{$x}++; $acc }

# fan — run a block or fn N times in parallel ($_/$_0 = index 0..N-1)
fan 8, work  # bare-fn form: fan N, FUNC
fan work, progress => 1  # uses rayon pool size (`stryke -j`)
fan 8 { work($_) }  # block form
fan { work($_) }  # block form, pool-sized
my @r = fan_cap 8, compute  # capture results in index order
my @r = fan_cap 8 { $_ * $_ }  # block form, capture

# pipelines — sequential or rayon-backed; same chain methods
my @r = (@data |> pipeline)->filter({ $_ > 10 })->map({ $_ * 2 })->take(100)->collect
### or 
my @r = @data |> pipeline |> filter $_ > 10 |> map $_ * 2 |> take 100 |> collect
my @r = @data |> par_pipeline |> filter  $_ > 10 |> map $_ * 2 |> collect

# multi-stage: batch (each stage drains list before next)
my $n = par_pipeline(
    source  => { readline(STDIN) },
    stages  => [ parse_json, transform ],
    workers => [4, 2],
    buffer  => 256,
)

# multi-stage: streaming (bounded crossbeam channels, concurrent stages, order NOT preserved)
my @r = ((1..1_000) |> par_pipeline_stream)->filter({ $_ > 500 })->map({ $_ * 2 })->collect()
## or
my @r = (1..1_000) |> par_pipeline_stream |> filter $_ > 500 |> map $_ * 2 |> collect

# channels + Go-style select
my ($tx, $rx) = pchannel(128)  # bounded; pchannel() is unbounded
my ($val, $idx) = pselect($rx1, $rx2)
my ($v, $i)     = pselect($rx1, $rx2, timeout => 0.5)  # $i == -1 on timeout

# barrier — N workers rendezvous
my $sync = barrier(3)
fan 3 { $sync->wait; p "all arrived" }

# persistent thread pool (avoids per-task spawn from pmap/pfor)
my $pool = ppool(4)
$pool->submit({ heavy_work($_) }) for @tasks
my @results = $pool->collect()

# parallel file IO
my @logs = "**/*.log" |> glob_par  # rayon recursive glob
par_lines "./big.log", { p if /ERROR/ }  # mmap + chunked line scan
par_walk  ".", { p if /\.rs$/ }  # parallel directory walk
par_sed qr/\bfoo\b/, "bar", @paths  # parallel in-place sed (returns # changed)
my @rs = par_find_files "src", "*.rs"  # parallel recursive file search by glob
my $n  = par_line_count @rs  # parallel line count across files

# native file watcher (notify crate: inotify/kqueue/FSEvents)
watch  "/tmp/x", p
pwatch "logs/*", heavy

# control thread count
stryke -j 8 -e '@data |> pmap heavy'

# distributed pmap over an SSH worker pool — see [0x10] for details
my $cluster = cluster(["build1:8", "build2:16"])
my @r = @huge |> pmap_on $cluster heavy
```

**Parallel capture safety** — workers set `Scope::parallel_guard` after restoring captured lexicals. Assignments to captured non-`mysync` aggregates are rejected at runtime; `mysync`, package-qualified names, and topics (`$_`/`$a`/`$b`) are allowed. `pmap`/`pgrep` treat block failures as `undef`/false; use `pfor` when failures must abort.

**Nested implicit-param matrix `_N<<<<<`** — *world-first*. Every **block-form** closure iter (`grep { ... }`, `map { ... }`, `sort { ... }`, `~> @arr map { ... }`, `fi { ... }`, etc.) shifts an outer-topic chain across all positional slots, up to 5 frames back. Read the previous topic with `_<`, two back with `_<<`, up to five back with `_<<<<<`. Same for every positional slot: `_1<<`, `_2<<<<<`, etc. No other language has this — Clojure `%`, Scala `_`, Ruby `_1`, Swift `$0`, Raku `$^a` all stop at the current scope.

**`{}` is the shift trigger.** EXPR-form HOFs (`grep EXPR, LIST`, `map EXPR, LIST`, `reject EXPR, LIST`, `grepv EXPR, LIST`, `|> grep EXPR`, etc. — anything with no braces) **do not shift the chain**. The expression is evaluated in the surrounding lexical scope with `$_`/`$_0` rebound per iter; everything else (including all positional aliases `$_1`, `$_2`, …) stays put. This makes higher-order combinator patterns work without chain-ascent boilerplate:

```perl
# Strain (Exercism): keep elements matching the predicate. The fn's args
# are `$_` (arrayref) and `$_1` (predicate coderef). EXPR-form `grep _1, …`
# evaluates `_1` per iter — and since there's no `{}` block, slot 1 stays
# bound to the caller's $_1 (the coderef), which the runtime then dispatches.
fn Exercism::Strain::keep    = [grep  _1, @$_]
fn Exercism::Strain::discard = [grepv _1, @$_]   # grepv ≡ reject (inverse)
```

The rule reads the same in both directions: **`{}` → shift, no `{}` → no shift**. Block boundaries are scope boundaries; expression positions are not.

**Indexed-ascent shortcut `_<N`** — past depth 2, counting chevrons gets error-prone. The lexer accepts `_<N` (where N is a positive integer) as syntactic sugar for `_<<<...<` (N chevrons). So `_<3` ≡ `_<<<` (more readable past depth 2), `_<5` ≡ `_<<<<<`, etc. Mixed forms work too: `$_2<3` reaches positional 2 from 3 frames up. Disambiguator: `_<3>` and `_<3:5>` remain string-slice syntax; `_<3` (without trailing `>` or `:`) is indexed-ascent.

The matrix:

```
slot 0 — bare `_` aliases `_0`, FOUR equivalent spellings per level:
  current  _    ≡ $_    ≡ _0    ≡ $_0
  1 up     _<   ≡ $_<   ≡ _0<   ≡ $_0<       (also: _<1, $_<1)
  2 up     _<<  ≡ $_<<  ≡ _0<<  ≡ $_0<<      (also: _<2, $_<2)
  3 up     _<<< ≡ $_<<< ≡ _0<<< ≡ $_0<<<     (also: _<3, $_<3)
  4 up     _<<<<  ≡ $_<<<<  ≡ _0<<<<  ≡ $_0<<<<   (also: _<4, $_<4)
  5 up     _<<<<< ≡ $_<<<<< ≡ _0<<<<< ≡ $_0<<<<<  (also: _<5, $_<5)

slot N ≥ 1 — two spellings per level (plus indexed form):
  current  _N   ≡ $_N
  1 up     _N<  ≡ $_N<        (also: _N<1, $_N<1)
  ...
  5 up     _N<<<<< ≡ $_N<<<<<  (also: _N<5, $_N<5)
```

The `<` glyph is iconic: "back/before/earlier" is universal in math and ASCII (`<-`, `<<`, version comparison). `_` is "the topic" (Perl `$_`, Ruby `_1`, Scala `_`). Composition tells you the meaning at sight.

```perl
# Rolling difference — no temp var, no naming.
~> @prices map { _ - _< }
# Python: [prices[i]-prices[i-1] for i in range(1,len(prices))] — 41 chars
# Ruby:   prices.each_cons(2).map { |a,b| b-a }              — 38 chars
# stryke: ~> @prices map { _ - _< }                          — 24 chars

# 3-arg sub, reach back 4 closures from inside nested maps:
fn deep($_0, $_1, $_2) {
    ~> 1:1 map { ~> 1:1 map { ~> 1:1 map { ~> 1:1 map {
        # _N<<<< reads the Nth positional of `deep`
        _0<<<< . "," . _1<<<< . "," . _2<<<<      # "alpha,beta,gamma"
    } } } }
}
deep("alpha", "beta", "gamma")

# Cartesian-style sum across two arrays, golf form:
~> @outer pmap { ~> @inner pmap { _< + _ } } sum
# (`_<` rolls through previous topics across iter boundaries — same primitive
# powers running totals, moving averages, deltas)

# fan / fan_cap also rebind topic per worker:
$_ = 100
my @r = fan_cap 3 { $_< }                      # (100, 100, 100)
fan_cap 1 { $_ = "inner"; "$_< $_" }           # "outer inner"
$_ = 50; ~> 10 >{ $_ + $_< }                   # 60
```

Implementation: `strykelang/scope.rs::set_closure_args` shifts every active slot's chain on each frame entry; `strykelang/lexer.rs` lexes `_<+`, `_N<+`, and the indexed-ascent forms `_<N`/`_M<N` (bare and `$`-prefixed) as single tokens. Depth cap is hardcoded at 5 levels (`debug_assert!(level <= 5)` in `scope.rs::topic_slot_key`); past depth 5 the chain falls off and reads return undef. Bumping the cap is a one-line change.

### Mutation semantics — topic variants align with `|param|` block params

A user writing `$_ = ...` or `$_< = ...` inside a block mutates **only the current frame**. Topic variants follow the exact same rule as `|$x|` block params and inner `my $x`: writes do not leak outward. The chain shift on the next frame entry remains purely a function of the *outer* topic value, never the inner mutation.

| form | mutation propagates to outer scope? | mechanism |
|---|---|---|
| `\|$x\|` block param | NO — frame-local | param binding lives in callee frame |
| `my $x` inside a block | NO — frame-local | new lexical binding in current frame |
| `my $x` outer + inner closure writes `$x` | rejected at compile time | DESIGN-001 (closures capture by value) |
| `mysync $x` outer + inner closure writes `$x` | YES — explicit `Arc<Mutex>` opt-in | shared cell, atomic compound ops |
| `our $x` | YES — package-global by design | symbol table, not lexical |
| `$_`, `$_<`, `$_<<`, `$_<<<`, `$_<<<<`, `$_<<<<<` | NO — frame-local | `Frame::set_scalar_raw` bypasses CaptureCell write-through |
| `$_0`, `$_1`, … `$_N` and `$_N<+` chain forms | NO — frame-local | same path as topic-chain writes |

Implementation: `strykelang/scope.rs::Scope::set_scalar` recognizes topic-variant names via `is_topic_variant_name` (regex `^_[0-9]*<*$`) and routes the write through `Frame::set_scalar_raw`, which bypasses the CaptureCell write-through that named outer-scope `my` variables use. Result: `$_<` always reads the lexical outer-scope topic of the current closure, never an in-flight mutation from a sibling iteration.

---

## [0x04] SHARED STATE (`mysync`)

`mysync` declares variables backed by `Arc<Mutex>` shared across parallel blocks. Compound ops (`++`, `+=`, `.=`, `|=`, `&=`) hold the lock for the full read-modify-write cycle — fully atomic.

```perl
mysync $counter = 0
fan 10000 { $counter++ }  # always exactly 10000
print $counter

mysync @results
(1..100) |> pfor { push @results, $_ * $_ }

mysync %histogram
(0..999) |> pfor { $histogram{$_ % 10} += 1 }

# deque() and heap(...) already use Arc<Mutex<...>> internally
mysync $q  = deque()
mysync $pq = heap { $a <=> $b }
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
| **Steganography** ([`image`](https://crates.io/crates/image), `sha2`, `crc32fast`) — world-first polymorphic stego builtin | `hide(CARRIER, SECRET [, KEY])` / `reveal(STEGO [, KEY])` / `hide_capacity(CARRIER)` — auto-dispatches PNG LSB (R/G/B, alpha skipped) for `\x89PNG` bytes vs zero-width-char text stego (U+200B/U+200C between visible chars) for everything else. 4-byte length + 4-byte CRC32 envelope catches tampering. Optional KEY enables SHA-256(key‖counter)-derived XOR mask on the secret. |
| **Compression** ([`flate2`](https://crates.io/crates/flate2), [`zstd`](https://crates.io/crates/zstd)) | `gzip`, `gunzip`, `zstd`, `zstd_decode` |
| **Time** ([`chrono`](https://crates.io/crates/chrono), [`chrono-tz`](https://crates.io/crates/chrono-tz)) | `datetime_utc`, `datetime_from_epoch`, `datetime_parse_rfc3339`, `datetime_strftime`, `datetime_now_tz`, `datetime_format_tz`, `datetime_parse_local`, `datetime_add_seconds`, `elapsed` |
| **Structs / Enums / Classes / Types** | `struct Point { x => Float }`, `enum Color { Red, Green }` (exhaustive `match`), `class Dog extends Animal { breed: Str; fn bark { } }`, `abstract class`/`final class`, `trait Printable { fn to_str }` (enforced, default method inheritance), `pub`/`priv`/`prot` visibility, `static count: Int`, `BUILD`/`DESTROY`, `final fn`, `methods()`/`superclass()`/`does()`, `static::method()`, `typed my $x : Int` |
| **Cyberpunk Terminal Art** | `cyber_city` (neon cityscape), `cyber_grid` (synthwave perspective grid), `cyber_rain`/`matrix_rain` (digital rain), `cyber_glitch`/`glitch_text` (text corruption), `cyber_banner`/`neon_banner` (block-letter banners), `cyber_circuit` (circuit board), `cyber_skull`, `cyber_eye` — all output ANSI-colored Unicode art |

```perl
my $data = "https://api.example.com/users/1" |> fetch_json
p $data->{name}

# Built-in HTTP server — one-liner web API
serve 8080, fn ($req) {
    # $req = { method, path, query, headers, body, peer }
    my $data = +{ path => $req->{path}, method => $req->{method} }
    status => 200, body => json_encode($data)
}
# or with workers: serve 8080, $handler, { workers => 16 }
# JSON content-type auto-detected; undef returns 404

my @rows = "data.csv" |> csv_read
my $df   = "data.csv" |> dataframe
my $db   = "app.db" |> sqlite
$db->exec("CREATE TABLE t (id INTEGER, name TEXT)")

# ─── Structs ────────────────────────────────────────────────────────
# Declaration: typed fields, optional defaults, or bare (Any type)
struct Point { x => Float, y => Float }
struct Point { x => Float = 0.0, y => Float = 0.0 }  # with defaults
struct Pair { key, value }  # untyped (Any)

# Construction: function-call, positional, or traditional ->new
my $p = Point(x => 1.5, y => 2.0)  # function-call with named args
my $p = Point(1.5, 2.0)  # positional (declaration order)
my $p = Point->new(x => 1.5, y => 2.0)  # traditional OO style
my $p = Point()  # uses defaults if defined

# Field access: getter (0 args) or setter (1 arg)
p $p->x  # 1.5 — getter
$p->x(3.0)  # setter
p $p->x  # 3.0

# User-defined methods
struct Circle {
    radius => Float,
    fn area { 3.14159 * $self->radius ** 2 }
    fn scale($factor: Float) {
        Circle(radius => $self->radius * $factor)
    }
}
my $c = Circle(radius => 5)
p $c->area  # 78.53975
p $c->scale(2)  # Circle(radius => 10)

# Built-in methods
my $q = $p->with(y => 5)  # functional update — new instance
my $h = $p->to_hash  # { x => 3.0, y => 5 }
my @f = $p->fields  # (x, y)
my $c = $p->clone  # deep copy

# Smart stringify — print shows struct name and fields
p $p  # Point(x => 3, y => 2)

# Structural equality — compares all fields
my $a = Point(1, 2)
my $b = Point(1, 2)
p $a == $b  # 1 (equal)
# ────────────────────────────────────────────────────────────────────

# ─── Enums (algebraic data types) ───────────────────────────────────
# Declaration: variants with optional typed data
enum Color { Red, Green, Blue }  # unit variants (no data)
enum Maybe { None, Some => Any }  # Some carries any value
enum Result { Ok => Int, Err => Str }  # typed data per variant

# Construction: Enum::Variant() syntax
my $c = Color::Red()  # unit variant
my $m = Maybe::Some(42)  # variant with data
my $r = Result::Err("not found")  # typed variant

# Smart stringify — shows enum name, variant, and data
p $c  # Color::Red
p $m  # Maybe::Some(42)
p $r  # Result::Err(not found)

# Type checking on variants with data
# Result::Ok("bad")  # ERROR: expected Int
# Maybe::Some()  # ERROR: requires data
# Color::Red(42)  # ERROR: does not take data

# Exhaustive enum matching — all variants must be covered or use `_` catch-all
my $light = Light::On()
my $s = match ($light) {
    Light::On()  => "on",
    Light::Off() => "off",
}
# Missing a variant without `_` → error:
# match ($c) { Color::Red() => "r" }  # ERROR: missing variant(s) Green, Blue
# ────────────────────────────────────────────────────────────────────

# ─── Cyberpunk Terminal Art ────────────────────────────────────────
p cyber_banner("STRYKE")          # large neon block-letter banner
p cyber_city()                    # procedural neon cityscape (80x24)
p cyber_city(120, 40, 99)         # custom width, height, seed
p cyber_grid(80, 20)              # synthwave perspective grid
p cyber_rain(80, 24)              # matrix-style digital rain
p cyber_glitch("BREACH", 7)       # glitch-distort text (intensity 1-10)
p cyber_circuit(60, 20)           # circuit board with traces and nodes
p cyber_skull()                   # neon skull (or "large" for big version)
p cyber_eye("large")              # all-seeing eye motif
# All output ANSI-colored Unicode — pipe to `less -R` or print directly.
# ────────────────────────────────────────────────────────────────────

# ─── Classes (full OOP) ────────────────────────────────────────────
# Declaration: class Name extends Parent impl Trait { fields; methods }
class Animal {
    name: Str
    age: Int = 0
    fn speak { p "Animal: " . $self->name }
}

# Inheritance with extends
class Dog extends Animal {
    breed: Str = "Mixed"
    fn bark { p "Woof! I am " . $self->name }
    fn speak { p $self->name . " barks!" }  # override
}

# Construction: named or positional
my $dog = Dog(name => "Rex", age => 5, breed => "Lab")
my $dog = Dog("Rex", 5, "Lab")  # positional

# Field access: getter (0 args) or setter (1 arg)
p $dog->name        # Rex
$dog->age(6)        # setter
p $dog->age         # 6

# Instance methods
$dog->bark()        # Woof! I am Rex
$dog->speak()       # Rex barks!

# Static methods: fn Self.name
class Math {
    fn Self.add($a, $b) { $a + $b }
    fn Self.pi { 3.14159 }
}
p Math::add(3, 4)   # 7
p Math::pi()        # 3.14159

# Traits (interfaces)
trait Printable { fn to_str }
class Item impl Printable {
    name: Str
    fn to_str { $self->name }
}

# Multiple inheritance
class C extends A, B { }

# isa checks inheritance chain
p $dog->isa("Dog")     # 1
p $dog->isa("Animal")  # 1
p $dog->isa("Cat")     # "" (false)

# Built-in methods (same as struct)
my @f = $dog->fields()       # (name, age, breed)
my $h = $dog->to_hash()      # { name => "Rex", ... }
my $d2 = $dog->with(age => 1) # functional update
my $d3 = $dog->clone()       # deep copy

# Smart stringify
p $dog  # Dog(name => Rex, age => 5, breed => Lab)

# Visibility (pub/priv/prot)
class Secret {
    pub visible: Int = 1
    priv hidden: Int = 42
    prot internal: Str = "base"         # subclass-only access
    pub fn get_hidden { $self->hidden } # internal access ok
}
class Child extends Secret {
    fn get_internal { $self->internal }  # prot: ok from subclass
}

# Abstract classes — cannot be instantiated; abstract methods enforced
abstract class Shape {
    name: Str
    fn area            # abstract method (no body) — subclasses must implement
    fn kind { "shape" } # concrete method — inherited by subclasses
}
class Circle extends Shape {
    radius: Float
    fn area { 3.14159 * $self->radius * $self->radius }
}
# Shape() → error!  Circle(name => "c", radius => 5) → ok
# class BadShape extends Shape { }  # → error: must implement abstract method `area`

# Static fields (class variables) — shared across all instances
class Counter {
    static count: Int = 0
    name: Str
    fn BUILD { Counter::count(Counter::count() + 1) }
}
my $a = Counter(name => "a")
my $b = Counter(name => "b")
p Counter::count()  # 2

# BUILD constructor hook — runs after field init, parent BUILD first
class Logger {
    log: Str = ""
    fn BUILD { $self->log("initialized") }
}

# DESTROY destructor — explicit via $obj->destroy(), child first
class Resource {
    fn DESTROY { p "cleanup" }
}
my $r = Resource()
$r->destroy()  # prints "cleanup"

# Trait enforcement — required methods checked at class definition
trait Drawable { fn draw }
# class Oops impl Drawable { }  # → error: missing required method `draw`
class Box impl Drawable {
    fn draw { "drawn" }    # satisfies trait contract
}
p Box()->does("Drawable")  # 1

# Trait default methods — inherited by implementing classes, overridable
trait Greetable {
    fn greeting { "Hello" }  # default method (has body)
    fn name                  # required method (no body)
}
class Person impl Greetable {
    n: Str
    fn name { $self->n }
    # greeting inherited from trait — Person()->greeting() returns "Hello"
}
class FormalPerson impl Greetable {
    n: Str
    fn name { $self->n }
    fn greeting { "Good day" }  # override the default
}

# Final classes — cannot be extended
final class Singleton { value: Int = 1 }
# class Bad extends Singleton { }  # → error

# Final methods — cannot be overridden
class Secure {
    final fn id { 42 }
    fn label { "secure" }  # can be overridden
}

# Reflection: methods(), superclass()
my @m = $dog->methods()     # ("speak", "bark", ...)
my @p = $dog->superclass()  # ("Animal")

# Late static binding: static::method() resolves to runtime class
class Base {
    fn class_name { static::identify() }
    fn identify { "Base" }
}
class Child extends Base {
    fn identify { "Child" }
}
Child()->class_name()  # "Child" (not "Base")

# Operator overloading for native classes
class Vec2 {
    x: Int; y: Int
    fn op_add($other) {
        Vec2(x => $self->x + $other->x, y => $self->y + $other->y)
    }
    fn op_eq($other) { $self->x == $other->x && $self->y == $other->y }
    fn stringify { "(" . $self->x . "," . $self->y . ")" }
}
my $v = Vec2(x => 1, y => 2) + Vec2(x => 3, y => 4)
p $v  # (4,6)
# Supported: op_add op_sub op_mul op_div op_mod op_pow op_concat
#            op_eq op_ne op_lt op_gt op_le op_ge op_spaceship op_cmp
#            op_neg op_bool op_abs op_numify stringify
# ────────────────────────────────────────────────────────────────────

typed my $n : Int = 42

# Typed fn parameters — runtime type checking on call
my $add = fn ($a: Int, $b: Int) { $a + $b }
p $add->(3, 4)  # 7
# $add->("x", 1)  # ERROR: sub parameter $a: expected Int

fn greet ($name: Str) { "Hello, $name!" }
p greet("world")  # Hello, world!

# stringify/str — convert any value to a parseable stryke literal
my $data = {a => [1, 2], b => "hello"}
my $s = str $data  # +{a => [1, 2], b => "hello"}
my $copy = eval $s  # round-trip via eval
p $copy->{a}[0]  # 1

# stringify works with functions (first-class serialization)
my $f = fn ($x: Int) { $x * 2 }
p str $f  # fn ($x: Int) { $x * 2; }
my $f2 = eval str $f  # round-trip: deserialize back to callable
p $f2->(21)  # 42

# streaming range — bidirectional lazy iterator
range(1, 5) |> e p                          # 1 2 3 4 5
range(5, 1) |> e p                          # 5 4 3 2 1
```

#### Sets

Native sets deduplicate by value (internal canonical keys; insertion order preserved for `->values`). Use the **`set(LIST)`** builtin or **`Set->new(LIST)`**; **`|>`** can supply the list. **`|`** / **`&`** are union / intersection when either side is a set (otherwise bitwise int ops).

```perl
my $s = set(1, 2, 2, 3)  # 3 members
my $t = (1, 1, 2, 4) |> set
my $u = $s | $t  # union
my $i = $s & $t  # intersection
$s->has(2)  # 1 / 0  (also ->contains / ->member)
$s->size  # count (->len / ->count)
my @v = $s->values  # array in insertion order

# mysync: compound |= and &= update shared sets (see [0x04])
```

---

## [0x06] ASYNC / TRACE / TIMER

```perl
# async / spawn / await — lightweight structured concurrency
my $data = async { "https://example.com/" |> fetch }
my $file = spawn { "big.csv" |> \&slurp }
print await($data), await($file)

# trace mysync mutations to stderr (under fan, lines tagged with worker index)
mysync $counter = 0
trace { fan 10 { $counter++ } }

# timer / bench — wall-clock millis; bench returns "min/mean/p99"
my $ms     = timer heavy_work
my $report = bench heavy_work 1000

# eval_timeout — runs block on a worker thread; recv_timeout on main
eval_timeout 5 slow

# retry / rate_limit / every (tree interpreter only)
retry http_call times => 3, backoff => exponential
rate_limit(10, "1s") hit_api
every "500ms" tick

# generators — lazy `yield` values
my $g = gen { yield $_ for 1..5 }
my $next = $g->next  # [value, more]
```

---

## [0x06b] AOP — BEFORE / AFTER / AROUND ADVICE

Aspect-oriented advice on user subs. Glob pointcuts, three advice kinds, `proceed()` for around. Same surface as zshrs's `intercept` builtin (`zshrs/src/exec.rs`), adapted to a real language: keyword statements instead of a CLI builtin.

```perl
# Before — runs before the matched sub. Sees $INTERCEPT_NAME, @INTERCEPT_ARGS.
before "fetch" { warn "calling fetch with @INTERCEPT_ARGS" }

# After — runs after. Sees $INTERCEPT_RESULT, $INTERCEPT_MS, $INTERCEPT_US.
after "fetch" { warn "fetch returned $INTERCEPT_RESULT in ${INTERCEPT_MS}ms" }

# Around — wraps. Must call proceed() to invoke the original.
around "expensive" {
    my $cached = cache_get($INTERCEPT_ARGS[0]);
    return $cached if defined $cached;
    my $r = proceed();
    cache_put($INTERCEPT_ARGS[0], $r);
    $r
}

# Glob patterns: *, ?
before "log_*"  { ... }     # any sub starting with log_
before "*"       { ... }     # every sub call

# Management
my @list = intercept_list();   # [[id, kind, pattern], ...]
intercept_remove($id);         # by id
intercept_clear();             # drop all
```

Semantics:
- Multiple `before` / `after` advices on the same name all fire (registration order).
- The first matching `around` wraps; later `around` matches on the same name are skipped (mirrors zshrs `run_intercepts`).
- `around` is AspectJ-style: the block's evaluated value is the call's return. `proceed()` runs the original and returns its value; the block can transform (`proceed() + 100`), forward (`proceed()`), or replace (omit the call and return a value directly).
- Recursion guard: calling the advised sub from inside its own advice runs the original directly without re-firing advice (no infinite loop).
- Coverage: user-defined subs only. Builtins (`print`, `pmap`, etc.) are not interceptable in v1.
- Pattern is a string literal (`"foo"`, `"log_*"`); the leading keyword only commits to advice parsing when followed by a string literal, so `before(...)` as a normal call still works.
- Advice bodies are lowered to bytecode at compile time and dispatched through the VM (`run_block_region`) — the same path used by `map { }` / `grep { }` blocks. This keeps compile-time name resolution (`our`-qualified scalars, lexical slots) consistent inside advice and outside it. The tree-walker is banned from the advice path; see `tests/tree_walker_absent_aop.rs` for the source-level invariant.
- Body lowering requires the final statement to be an expression (the same constraint as `map { }` block lowering). Bodies that end in a literal `for`/`while`/`if` block, or contain a literal `return`, are rejected at advice-firing time with a runtime error — rewrite the body so it ends in an expression and avoids early-`return`.

Builtins from inside advice bodies:
- `proceed()` — only legal inside `around`; runs the original sub with the saved args, returns its value.
- `intercept_list()` — returns `[[id, kind, pattern], ...]` for all registered advices.
- `intercept_remove($id)` — removes one by id; returns the count removed (0 or 1).
- `intercept_clear()` — drops all; returns count cleared.

---

## [0x07] CLI FLAGS

All stock `perl` flags are supported: `-0`, `-a`, `-c`, `-C`, `-d`, `-D`, `-e`, `-E`, `-f`, `-F`, `-g`, `-h`, `-i`, `-I`, `-l`, `-m`, `-M`, `-n`, `-p`, `-s`, `-S`, `-t`, `-T`, `-u`, `-U`, `-v`, `-V`, `-w`, `-W`, `-x`, `-X`. Perl-style single-dash (`-version`, `-help`) and GNU-style double-dash (`--version`, `--help`) long forms work. Bundled switches are expanded: `-Mstrict` → `-M strict`, `-I/tmp` → `-I /tmp`, `-V:version` → `-V version`, `-lane` → `-l -a -n -e`.

stryke-specific long flags:

| Flag | Description |
| --- | --- |
| `--lint` / `--check` | Parse + compile bytecode without running |
| `--disasm` / `--disassemble` | Print bytecode disassembly to stderr before VM execution |
| `--ast` | Dump parsed AST as JSON and exit |
| `--fmt` | Pretty-print parsed Perl to stdout and exit |
| `--profile` | Wall-clock profile: per-line + per-sub timings on stderr |
| `--flame` | Flamegraph: colored terminal bars when interactive, SVG when piped (`stryke --flame x.stk > flame.svg`) |
| `--record` | Record one row per stryke run (wall-clock, exit code, argv) to `~/.stryke/perf.sqlite`. Inherits to child processes via `STRYKE_RECORD=1` env, so `s --record t TESTS...` records one row per test file. Query via the `perfview` builtin. |
| `--no-jit` | Disable Cranelift JIT (bytecode interpreter only) |
| `--compat` | Perl 5 strict-compatibility mode: disable all stryke extensions (`\|>`, `struct`, `enum`, `match`, `pmap`, `#{expr}`, etc.) |
| `--no-interop` | Reject Perl-isms (`sub`, `say`, `reverse`, `scalar`, `$a`/`$b` outside sort blocks); force idiomatic stryke (`fn`, `p`, `rev`, `len`, `$_0`/`$_1`). See [\[0x08a\]](#0x08a---no-interop-mode) |
| `--explain CODE` | Print expanded hint for an error code (e.g. `E0001`) |
| `--lsp` | Language server over stdio ([\[0x11\]](#0x11-language-server-stryke-lsp)) |
| `--dap [HOST:PORT]` | Debug Adapter Protocol server. With no arg → stdio; with `HOST:PORT` → connects to a TCP socket the spawner is listening on. Used by the JetBrains plugin under [`editors/intellij/`](editors/intellij/) |
| `-d` | TTY debugger (`perl -d` style REPL on stdin/stderr). Use with `--`: `stryke -d -- script.stk` |
| `-j N` / `--threads N` | Set number of parallel threads (rayon) |
| `--remote-worker` | Persistent cluster worker over stdio ([\[0x10\]](#0x10-distributed-pmap_on--d-over-ssh-cluster)) |
| `--remote-worker-v1` | Legacy one-shot cluster worker over stdio |
| `build SCRIPT [-o OUT]` | AOT compile script to standalone binary ([\[0x0D\]](#0x0d-standalone-binaries-stryke-build)) |
| `doc [TOPIC]` | Interactive reference book with vim-style navigation (`stryke doc`, `stryke doc pmap`, `stryke doc --toc`) |
| `serve [PORT] [SCRIPT]` | HTTP server (default port 8000): static files (`stryke serve`), script (`stryke serve 8080 app.stk`), one-liner (`stryke serve 3000 -e 'EXPR'`) |
| `fmt [-i] FILE...` | Format source files in place or to stdout (`stryke fmt -i .` formats all recursively) |
| `minify [-i] FILE...` | Strip comments / POD / blank lines and collapse statements onto a single line with `;` separators. Output still parses to the same AST as the input. |
| `check FILE...` | Parse + compile without executing; report errors with `file:line:col` (CI/editor integration) |
| `disasm FILE` | Disassemble bytecode to stderr (learning the VM, debugging perf) |
| `profile [--flame] [--json] FILE` | Run with profiling; `--flame` generates SVG, `-o FILE` writes to file |
| `bench [FILE\|DIR]` | Discover and run benchmarks from `bench/` or `benches/` (`bench_*.stk`, `b_*.stk`) |
| `init [NAME]` | Scaffold a new project: `main.stk`, `lib/`, `bench/`, `t/`, `.gitignore` |
| `repl [--load FILE]` | Start interactive REPL explicitly, with optional pre-loaded file |
| `lsp` | Start Language Server Protocol over stdio (equivalent to `--lsp`) |
| `completions [SHELL]` | Emit shell completions to stdout (`stryke completions zsh > _stryke`) |
| `ast FILE` | Dump parsed AST as JSON to stdout |
| `prun FILE...` | Run multiple script files in parallel using all cores |
| `convert [-i] FILE...` | Convert Perl source to stryke syntax with `\|>` pipes |
| `deconvert [-i] FILE...` | Convert stryke `.stk` files back to standard Perl syntax |

![stryke -h](img/stryke-help.png)

### `getopts` builtin — Getopt::Long-style argv parsing

For parsing your *own* script's argv (`@ARGV`), stryke ships a `getopts` builtin shaped after Perl's `Getopt::Long`. It returns a hash of canonical-name → value, and removes consumed options from `@ARGV` so the leftover positional arguments stay behind.

When called with just SPECS, `getopts` operates on `@ARGV` directly — no `\@ARGV` boilerplate needed:

```perl
my %opts = %{ getopts([
    "verbose|v",         # bool flag (present = 1)
    "file|f=s",          # required string
    "count|n=i",         # required int
    "rate=f",            # required float
    "out:s",             # optional string ("" if absent)
    "tag|t=s@",          # repeatable → arrayref
    "define|D=s%",       # --define key=val → hashref
    "debug+",            # incremental: -ddd → 3
    "color!",            # negatable: --no-color → 0
]) };

# Hash form lets each spec carry a default:
my %opts = %{ getopts({
    "verbose|v" => 0,
    "count|n=i" => 10,
    "tag|t=s@"  => [],
}) };

# Explicit-array-ref form (parse a list other than @ARGV):
my @args = ("--verbose", "file.txt");
my %opts = %{ getopts(\@args, [ "verbose|v" ]) };
```

The first argument is interpreted as follows: an array ref of spec strings or a hash ref of specs → operates on `@ARGV`; an array ref *followed by* a SPECS argument → explicit-argv form. The two-arg `(SPECS, META)` form is recognised when the second argument is a hash ref containing only `prog`/`desc`/`epilog` keys (see auto-help below).

Spec language (subset of Perl's `Getopt::Long`):

| Spec        | Meaning                                                      |
| ---         | ---                                                          |
| `name`      | Bool flag, no argument                                       |
| `n\|name`   | Same option, multiple names; first is canonical              |
| `name=s`    | Required string arg                                          |
| `name=i`    | Required int arg                                             |
| `name=f`    | Required float arg                                           |
| `name:s`    | Optional string (defaults to `""` if flag is given without)  |
| `name:i`    | Optional int (defaults to `0`)                               |
| `name:f`    | Optional float (defaults to `0.0`)                           |
| `name=s@`   | Repeatable → arrayref (`=i@` / `=f@` typed)                  |
| `name=s%`   | `--name key=val` → hashref (`=i%` / `=f%` typed)             |
| `name!`     | Negatable bool; `--no-name` sets `0`                         |
| `name+`     | Counter: each occurrence increments by 1                     |

Parsing rules:

- `--name`, `--name=value`, and `--name value` all accepted for long options.
- `-n`, `-n value`, and `-nvalue` all accepted for short options.
- Bundling: `-vDR` is parsed as `-v -D -R`. If a char in the bundle takes an argument, the rest of the token becomes its value (`-vfx.txt` → `-v -f x.txt`). Bundling and `=` are mutually exclusive — `-X=value` is treated as a single short option name with an inline value.
- `--` terminates option parsing; everything after is positional.
- Numeric tokens (`-5`, `-3.14`) are treated as positionals.
- The first non-option positional stops parsing (no intermixed mode in v1).
- Unknown options or type mismatches raise a runtime error.

Defaults when an option is absent: bool / negatable bool / counter → `0`; `=s@` → empty arrayref; `=s%` → empty hashref; required scalars (`=s`/`=i`/`=f`) → not present in the returned hash unless given a default via the hash form.

#### Per-option metadata + auto-`--help`

The hash form also accepts a hashref *value* carrying per-option metadata (`help`, `default`, `required`, `metavar`). When any spec has a `help` string and the user hasn't claimed their own `--help`/`-h`, `getopts` intercepts `--help`/`-h` in the input, prints a formatted usage block to stdout, and `exit(0)`s.

```perl
my %opts = %{ getopts({
    "verbose|v" => { help => "enable verbose output" },
    "file|f=s"  => { help => "output path", default => "out.txt" },
    "count|n=i" => { help => "iterations", required => 1 },
    "tag|t=s@"  => { help => "tag (repeatable)" },
    "color!"    => { help => "colored output" },
}, {
    prog   => "myscript",        # banner program name (default: argv[0] basename)
    desc   => "do a thing",      # banner description line
    epilog => "see docs",        # trailing line after the option list
}) };
```

Metadata keys (D1 form):

| Key        | Meaning                                                          |
| ---        | ---                                                              |
| `help`     | Help-text shown next to the option in `--help` output            |
| `default`  | Default value if the option is not given                         |
| `required` | Error at end of parse if the option (or its default) is missing  |
| `metavar`  | Override the placeholder shown next to the option in help output |

Scalar and hashref values can be mixed in the same spec hash — a bare scalar value is still treated as the `default`:

```perl
my %opts = %{ getopts({
    "verbose|v" => 0,                                # default only (legacy form)
    "file|f=s"  => { help => "output path" },        # D1 metadata
}) };
```

`--help` output for the first example above:

```
Usage: myscript [OPTIONS]

do a thing

Options:
  -v, --verbose        enable verbose output
  -f, --file VALUE     output path (default: out.txt)
  -n, --count N        iterations (required)
  -t, --tag VALUE      tag (repeatable)
  --color, --no-color  colored output
  -h, --help           show this help and exit

see docs
```

---

## [0x08] SUPPORTED PERL FEATURES

#### Data
Scalars `$x`, arrays `@a`, hashes `%h`, refs `\$x`/`\@a`/`\%h`/`\&sub`, anon `[...]`/`{...}`, code refs / closures (capture enclosing lexicals), `qr//` regex objects, blessed references, native sets (`set(LIST)` / `Set->new(...)`), `deque()`, `heap()`.

#### Control flow
`if`/`elsif`/`else`/`unless`, `while`/`until`, `do { } while/until`, C-style `for`, `foreach`, `last`/`next`/`redo` with labels, postfix `if`/`unless`/`while`/`until`/`for`, ternary, `try { } catch ($err) { } finally { }`, `given`/`when`/`default`, algebraic `match (EXPR) { PATTERN [if EXPR] => EXPR, ... }` (regex, array, hash, wildcard, literal patterns; bindings scoped per arm; exhaustive enum variant checking), `eval_timeout SECS { ... }`.

#### Operators
Arithmetic, string `.`/`x`, comparison (including **Raku-style chained comparisons** like `1 < $x < 10`), `eq`/`ne`/`lt`/`gt`/`cmp`, logical `&&`/`||`/`//`/`!`/`and`/`or`/`not`, bitwise (`|`/`&` are set ops on native `Set`), assignment + compound (`+=`, `.=`, `//=`, …), regex `=~`/`!~`, range `..` / `...` (incl. flip-flop with `eof`), arrow `->`, **pipe-forward `|>`** (stryke extension — threads the LHS as the **first** argument of the RHS call; see [Extensions beyond stock Perl 5](#extensions-beyond-stock-perl-5)).

#### Regex engine
Three-tier compile (Rust `regex` → `fancy-regex` → PCRE2). Perl `$` end anchor (no `/m`) is rewritten to `(?:\n?\z)`. Match `=~`, dynamic `$str =~ $pat`, substitution `s///`, transliteration `tr///`, flags `g`/`i`/`m`/`s`/`x`/`e`/`r`, captures `$1`…`$n`, named groups → `%+`/`$+{name}`, `\Q...\E`, `quotemeta`, `m//`/`qr//`. The `/r` flag (non-destructive) returns the modified string instead of the match count — auto-injected when `s///` or `tr///` appear as pipe-forward RHS. Bare `/pat/` in statement/boolean context is `$_ =~ /pat/`.

#### Subroutines
`fn name { }` with optional prototype, **typed parameters** (`fn add($a: Int, $b: Int)`), **default parameter values** (`fn greet($name = "world")`), anon subs/closures, implicit return of last expression (VM), `@_`/`shift`/`return`, postfix `return ... if COND`, `AUTOLOAD` with `$AUTOLOAD` set to the FQN.

#### Built-ins (selected)

| Category | Functions |
| --- | --- |
| Array | `push`, `pop`, `shift`, `unshift`, `splice`, `splice_last` (last removed — `--no-interop` replacement for `scalar splice`), `rev` (string / list reverse), `sort`, `map`, `grep`, `filter`, `reduce`, `fold`, `fore`, `e`, `preduce`, `len`/`cnt`/`count` (element count — replaces `scalar @a`), `partition`, `min_by`, `max_by`, `zip_with`, `interleave`, `frequencies`, `tally`, `count_by`, `pluck`, `grep_v`, `head`, `tail`, `first` |
| Hash | `keys`, `values`, `each`, `delete`, `exists`, `select_keys`, `top`, `deep_clone`/`dclone`, `deep_merge`/`dmerge`, `deep_equal`/`deq` |
| Functional | `compose`/`comp`, `partial`, `curry`, `memoize`/`memo`, `once`, `constantly`, `complement`, `juxt`, `fnil` |
| String | `chomp`, `chop`, `length`, `substr`, `index`, `rindex`, `split`, `join`, `sprintf`, `printf`, `uc`/`lc`/`ucfirst`/`lcfirst`, `chr`, `ord`, `hex`, `oct`, `crypt`, `fc`, `pos`, `study`, `quotemeta`, `trim`, `lines`, `words`, `chars`, `digits`, `numbers`, `graphemes`, `columns`, `sentences`, `paragraphs`, `sections`, `snake_case`, `camel_case`, `kebab_case` |
| Binary | `pack`, `unpack` (subset `A a N n V v C Q q Z H x w i I l L s S f d` + `*`), `unpack_first` / `unpack1` / `up1` (first decoded element — `--no-interop` replacement for `scalar unpack`), `vec` |
| Numeric | `abs`, `int`, `sqrt`, `squared`/`sq`, `cubed`/`cb`, `expt(B,E)`, `sin`, `cos`, `atan2`, `exp`, `log`, `rand`, `srand`, `avg`, `stddev`, `clamp`, `normalize`, `range(N, M)` (lazy bidirectional) |
| I/O | `print`, `p`, `printf`, `open` (incl. `open my $fh`, files, `-\|` / `\|-` pipes), `close`, `eof`, `readline`, `read`, `seek`, `tell`, `sysopen`, `sysread`/`syswrite`/`sysseek`, handle methods `->print/->p/->printf/->getline/->close/->eof/->getc/->flush`, `slurp`, `swallow`/`swa` (glob → hash `{abspath => bytes}`), `ingest`/`ing` (streaming `[path, bytes]` iterator), `burp` (inverse of swallow — hash → files, mkdir -p), `input`, backticks/`qx{}`, `capture` (structured: `->stdout/->stderr/->exit`), `pager`/`pg`/`less` (pipes value into `$PAGER`; TTY-gated), `binmode`, `fileno`, `flock`, `getc`, `select`, `truncate`, `formline`, `read_lines`, `append_file`, `to_file`, `read_json`, `write_json`, `tempfile`, `tempdir`, `xopen`/`xo` (system open — `open` on macOS, `xdg-open` on Linux), `clip`/`clipboard`/`pbcopy` (copy to clipboard), `paste`/`pbpaste` (read clipboard) |
| Directory | `opendir`, `readdir`, `closedir`, `rewinddir`, `telldir`, `seekdir`, `files`, `filesf`/`f`, `fr` (recursive files, lazy iterator), `dirs`/`d`, `dr` (recursive dirs, lazy iterator), `sym_links`, `sockets`, `pipes`, `block_devices`, `char_devices` |
| File tests | `-e`, `-f`, `-d`, `-l`, `-r`, `-w`, `-s`, `-z`, `-x`, `-t` (defaults to `$_`) |
| System | `system`, `exec`, `exit`, `chdir`, `mkdir`, `unlink`, `rename`, `chmod`, `chown`, `chroot`, `stat`, `lstat`, `link`, `symlink`, `readlink`, `glob`, `glob_par`, `glob_match`, `which_all`, `par_sed`, `par_find_files`, `par_line_count`, `ppool`, `barrier`, `fork`, `wait`, `waitpid`, `kill`, `alarm`, `sleep`, `times`, `dump`, `reset` |
| System Stats | `mem_total`, `mem_free`, `mem_used`, `swap_total`, `swap_free`, `swap_used`, `disk_total`, `disk_free`, `disk_avail`, `disk_used`, `load_avg`, `sys_uptime`, `page_size`, `os_version`, `os_family`, `endianness`, `pointer_width`, `proc_mem`/`rss` |
| Sockets | `socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `shutdown`, `socketpair` |
| Network | `gethostbyname`, `gethostbyaddr`, `getpwnam`, `getpwuid`, `getpwent`/`setpwent`/`endpwent`, `getgrnam`, `getgrgid`, `getgrent`/`setgrent`/`endgrent`, `getprotobyname`, `getprotobynumber`, `getservbyname`, `getservbyport` |
| SysV IPC | `msgctl`, `msgget`, `msgsnd`, `msgrcv`, `semctl`, `semget`, `semop`, `shmctl`, `shmget`, `shmread`, `shmwrite` (stubs — runtime error) |
| Type | `defined`, `undef`, `ref`, `bless`, `tied`, `untie`, `type_of`, `byte_size` |
| Serialization | `to_json`, `to_csv`, `to_toml`, `to_yaml`, `to_xml`, `to_html`, `to_markdown`, `to_table`/`tbl`, `ddump`, `stringify`/`str`, `json_encode`/`json_decode` |
| Visualization | `sparkline`/`spark`, `bar_chart`/`bars`, `flame`/`flamechart`, `histo`, `gauge`, `spinner`, `spinner_start`/`spinner_stop` |
| Control | `die`, `warn`, `eval`, `do`, `require`, `caller`, `wantarray`, `goto LABEL`, `continue { }` on loops, `prototype` |
| Number Theory | `prime_factors`, `divisors`, `num_divisors`, `sum_divisors`, `is_perfect`, `is_abundant`, `is_deficient`, `collatz_length`, `collatz_sequence`, `lucas`, `tribonacci`, `nth_prime`, `primes_up_to`/`sieve`, `next_prime`, `prev_prime`, `triangular_number`, `pentagonal_number`, `is_pentagonal`, `perfect_numbers`, `twin_primes`, `goldbach`, `prime_pi`, `totient_sum`, `subfactorial`, `bell_number`, `partition_number`, `multinomial`, `is_smith`, `aliquot_sum`, `abundant_numbers`, `deficient_numbers` |
| Statistics | `skewness`, `kurtosis`, `linear_regression`, `moving_average`, `exponential_moving_average`, `coeff_of_variation`, `standard_error`, `normalize_array`, `cross_entropy`, `euclidean_distance`, `minkowski_distance`, `mean_absolute_error`, `mean_squared_error`, `median_absolute_deviation`, `winsorize`, `weighted_mean` |
| Geometry | `area_circle`, `area_triangle`, `area_rectangle`, `area_trapezoid`, `area_ellipse`, `circumference`, `perimeter_rectangle`, `perimeter_triangle`, `polygon_area`, `sphere_volume`, `sphere_surface`, `cylinder_volume`, `cone_volume`, `heron_area`, `point_distance`, `midpoint`, `slope`, `triangle_hypotenuse`, `degrees_to_compass` |
| Financial | `npv`, `depreciation_linear`, `depreciation_double`, `cagr`, `roi`, `break_even`, `markup`, `margin`, `discount`, `tax`, `tip` |
| Encoding | `morse_encode`/`morse`, `morse_decode`, `nato_phonetic`, `int_to_roman`, `roman_to_int`, `binary_to_gray`, `gray_to_binary`, `pig_latin`, `atbash`, `braille_encode`, `phonetic_digit`, `to_emoji_num` |
| Color | `hsl_to_rgb`, `rgb_to_hsl`, `hsv_to_rgb`, `rgb_to_hsv`, `color_blend`, `color_lighten`, `color_darken`, `color_complement`, `color_invert`, `color_grayscale`, `random_color`, `ansi_256`, `ansi_truecolor`, `color_distance` |
| Constants | `pi`, `tau`, `phi`, `epsilon`, `speed_of_light`, `gravitational_constant`, `planck_constant`, `avogadro_number`, `boltzmann_constant`, `elementary_charge`, `electron_mass`, `proton_mass`, `i64_max`, `i64_min`, `f64_max`, `f64_min` |
| Matrix | `matrix_transpose`, `matrix_inverse`, `matrix_hadamard`, `matrix_power`, `matrix_flatten`, `matrix_from_rows`, `matrix_map`, `matrix_sum`, `matrix_max`, `matrix_min` |
| DSP / Signal | `convolution`, `autocorrelation`, `fft_magnitude`, `zero_crossings`, `peak_detect` |
| Algorithms | `next_permutation`, `is_balanced_parens`, `eval_rpn`, `merge_sorted`, `binary_insert`, `reservoir_sample`, `run_length_encode_str`, `run_length_decode_str`, `range_expand`, `range_compress`, `group_consecutive_by`, `histogram`, `bucket`, `clamp_array`, `normalize_range` |
| Validation | `luhn_check`, `is_valid_hex_color`, `is_valid_cidr`, `is_valid_mime`, `is_valid_cron`, `is_valid_latitude`, `is_valid_longitude` |
| Text | `ngrams`, `bigrams`, `trigrams`, `char_frequencies`, `is_anagram`, `is_pangram`, `mask_string`, `chunk_string`, `camel_to_snake`, `snake_to_camel`, `collapse_whitespace`, `remove_vowels`, `remove_consonants`, `strip_html`, `metaphone`, `double_metaphone`, `initials`, `acronym`, `superscript`, `subscript`, `leetspeak`, `zalgo`, `sort_words`, `unique_words`, `word_frequencies`, `string_distance`, `string_multiply` |
| Misc | `fizzbuzz`, `roman_numeral_list`, `look_and_say`, `gray_code_sequence`, `sierpinski`, `mandelbrot_char`, `game_of_life_step`, `tower_of_hanoi`, `pascals_triangle`, `truth_table`, `base_convert`, `roman_add`, `haversine`, `bearing`, `bmi`, `bac_estimate` |

#### Perl-compat highlights

- **OOP** — `@ISA` (incl. `our @ISA` outside `main`), C3 MRO (live, not cached), `$obj->SUPER::method`. `tie` for scalars/arrays/hashes with `TIESCALAR/TIEARRAY/TIEHASH`, `FETCH`/`STORE`, plus `EXISTS`/`DELETE` on tied hashes. `tied` returns the underlying object; `untie` removes the tie.
- **`use overload`** — `'op' => 'method'` or `\&handler`; binary dispatch with `(invocant, other)`, `nomethod`, unary `neg`/`bool`/`abs`, `""` for stringification, `fallback => 1`.
- **`$?` / `$|`** — packed POSIX status from `system`/backticks/pipe close; autoflush on print/printf.
- **`$.`** — undef until first successful read, then last-read line count.
- **`print`/`p`/`printf` with no args** — uses `$_` (and `printf`'s format defaults to `$_`).
- **Bareword statement** — `name;` calls a scwub with `@_ = ($_)`.
- **Typeglobs** — `*foo = \&bar`, `*lhs = *rhs` copies sub/scalar/array/hash/IO slots; package-qualified `*Pkg::name` supported.
- **`%SIG` (Unix)** — `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` as code refs; handlers run between statements/opcodes via `perl_signal::poll`. `IGNORE` and `DEFAULT` honored.
- **`format` / `write`** — partial: `format NAME = ... .` registers a template; pictures `@<<<<`, `@>>>>`, `@||||`, `@####`, `@****`, literal `@@`. `formline` populates `$^A`. `write` (no args) uses `$~` to stdout. Not yet: `write FILEHANDLE`, `$^`.
- **`@INC` / `%INC` / `require` / `use`** — `@INC` is built from `-I`, `vendor/perl`, system `perl`'s `@INC` (set `STRYKE_NO_PERL_INC` to skip), the script dir, `STRYKE_INC`, then `.`. List utilities (`sum`, `min`, `max`, `uniq`, `reduce`, `pairs`, `zip`, `mesh`, …) are stryke-native bare-name builtins implemented in Rust at `strykelang/list_builtins.rs` — no Perl module shim, no module to import. `use Module qw(a b);` honors `@EXPORT_OK`/`@EXPORT` for actual user modules. Built-in pragmas (`strict`, `warnings`, `utf8`, `feature`, `open`, `Env`) do not load files.
- **`chunked` / `windowed` / `fold`** — Use **pipe-forward**: **`LIST |> chunked(N)`**, **`LIST |> windowed(N)`**, **`LIST |> fold { BLOCK }`** (same for **`reduce`**). `fold` is an alias for `reduce`. List context → arrayrefs per chunk/window or the folded value; scalar context → chunk/window count where applicable.

  ```perl
  my @pairs = (1, 2, 3, 4) |> chunked(2)  # ([1,2], [3,4])
  my @slide = (1, 2, 3) |> windowed(2)  # ([1,2], [2,3])
  my @pipe  = (10, 20, 30) |> chunked(2)  # ([10,20], [30])
  my $sum   = (1, 2, 3, 4) |> fold { $a + $b }  # same as reduce
  my $cat   = qw(a b c) |> fold { $a . $b }
  ```
- **`use strict`** — refs/subs/vars modes (per-mode `use strict 'refs'` etc.). `strict refs` rejects symbolic derefs at runtime; `strict vars` requires a visible binding.
- **`BEGIN` / `UNITCHECK` / `CHECK` / `INIT` / `END`** — Perl order; `${^GLOBAL_PHASE}` matches Perl.
- **String interpolation** — `$var` `#{23 * 52}`, `$h{k}`, `$a[i]`, `@a`, `@a[slice]` (joined with `$"`), `$#a` in slice indices, `$0`, `$1..$n`. Escapes: `\n \r \t \a \b \f \e \0`, `\x{hex}`, `\xHH`, `\u{hex}`, `\o{oct}`, `\NNN` (octal), `\cX` (control), `\N{U+hex}`, `\N{UNICODE NAME}`, `\U..\E`, `\L..\E`, `\u`, `\l`, `\Q..\E`.
- **Triple-quoted strings** — `"""..."""` for interpolating multiline strings (same `$var`/`@arr`/`#{expr}`/escape rules as `"..."`); raw newlines preserved verbatim, no indent stripping. `r"""..."""` is the raw form: zero interpolation, zero backslash escapes — every byte copied literally until the closing `"""`. `""` (two quotes) inside the body does NOT close — only `"""` does.
- **`__FILE__` / `__LINE__`** — compile-time literals.
- Heredocs `<<EOF`, POD skipping, shebang handling, `qw()/q()/qq()` with paired delimiters.
- **Special variables** — large set of `${^NAME}` scalars pre-seeded; see [`SPECIAL_VARIABLES.md`](parity/SPECIAL_VARIABLES.md). Still missing vs Perl 5: `English`, full `$^V` as a version object.

#### Extensions beyond stock Perl 5

- Native CSV (`csv_read`/`csv_write`), columnar `dataframe`, embedded `sqlite`.
- HTTP (`fetch`/`fetch_json`/`fetch_async`/`par_fetch`), JSON (`json_encode`/`json_decode`).
- Crypto, compression, time, TOML, YAML helpers (see [\[0x05\]](#0x05-native-data-scripting)).
- All parallel primitives in [\[0x03\]](#0x03-parallel-primitives) (`pmap`, `fan`, `pipeline`, `par_pipeline_stream`, `pchannel`, `pselect`, `barrier`, `ppool`, `glob_par`, `par_walk`, `par_lines`, `par_sed`, `par_find_files`, `par_line_count`, `pwatch`, `watch`).
- **Distributed compute** ([\[0x10\]](#0x10-distributed-pmap_on--d-over-ssh-cluster)): `cluster([...])` builds an SSH worker pool; `pmap_on $cluster { } @list` and `pflat_map_on $cluster { } @list` fan a map across persistent remote workers with fault tolerance and per-job retries.
- **Standalone binaries** ([\[0x0D\]](#0x0d-standalone-binaries-stryke-build)): `stryke build SCRIPT -o OUT` bakes a script into a self-contained executable.
- **Inline Rust FFI** ([\[0x0E\]](#0x0e-inline-rust-ffi-rust---)): `rust { pub extern "C" fn ... }` blocks compile to a cdylib on first run, dlopen + register as Perl-callable subs.
- **Bytecode cache** ([\[0x0F\]](#0x0f-bytecode-cache-rkyv)): single rkyv shard at `~/.stryke/scripts.rkyv` — `mmap` + zero-copy `ArchivedHashMap` lookup skips lex/parse/compile on warm starts. Disable with `STRYKE_CACHE=0`.
- **Language server** ([\[0x11\]](#0x11-language-server-stryke-lsp)): `stryke lsp` runs an LSP server over stdio with diagnostics, hover, completion.
- `mysync` shared state ([\[0x04\]](#0x04-shared-state-mysync)).
- `frozen my` (or `const my` — same thing, more familiar spelling), `typed my`, `struct`, `enum`, `class` (full OOP with `extends`/`impl`), `trait`, algebraic `match`, `try/catch/finally`, `eval_timeout`, `retry`, `rate_limit`, `every`, `gen { ... yield }`.
- **Raku-style chained comparisons** — `1 < $x < 10` desugars to `(1 < $x) && ($x < 10)` at parse time. Works with all comparison operators (`<`, `<=`, `>`, `>=`, `lt`, `le`, `gt`, `ge`) and chains of any length.
- **Default parameter values** — `fn greet($name = "world")`, `fn range(@vals = (1,2,3))`, `fn config(%opts = (debug => 0))`. Defaults evaluated at call time when argument not provided.
- **Functional composition** — `compose`, `partial`, `curry`, `memoize`, `once`, `constantly`, `complement`, `juxt`, `fnil`:

  ```perl
  my $f = compose(fn { $_ + 1 }, fn { $_ * 2 })
  $f(5)  # 11 (double then inc)

  my $add5 = partial(fn { $_[0] + $_[1] }, 5)
  $add5(3)  # 8

  my $cadd = curry(fn { $_[0] + $_[1] }, 2)
  $cadd(1)(2)  # 3

  my $fib = memoize(fn { ... })  # cached by args
  my $init = once(fn { expensive_setup() })  # called at most once
  ```
- **Deep structure utilities** — `deep_clone`/`dclone`, `deep_merge`/`dmerge`, `deep_equal`/`deq`, `tally`:

  ```perl
  my $b = deep_clone($a)  # recursive deep copy
  my $m = deep_merge(\%a, \%b)  # recursive hash merge
  deep_equal([1,2,{x=>3}], [1,2,{x=>3}])  # 1 (structural eq)
  my $t = tally("a","b","a")  # {a => 2, b => 1}
  ```
- **Bare `_` as topic shorthand** — in any expression position, bare `_` is equivalent to `$_`. Inspired by Raku's WhateverCode and Scala's placeholder syntax. Enables ultra-concise blocks: `map{_*2}` instead of `map{$_ * 2}`. The sigil-free form compresses better — no spaces needed around `_` when adjacent to operators.
- **Outer topic `$_<`** — access the enclosing scope's `$_` from nested blocks; up to 5 levels (`$_<` through `$_<<<<<`, or the indexed form `$_<5`). See [\[0x03\]](#0x03-parallel-primitives).
- **`fore`** (`e`) — side-effect-only list iterator (like `map` but void, returns item count). Works with `{ BLOCK } LIST`, blockless `e EXPR, LIST`, and pipe-forward `|> e p`. Use for print/log/accumulator loops.
- **Pipe-forward `|>`** — parse-time desugaring (zero runtime cost); threads the LHS as the **first** argument of the RHS call, left-associative. `map`, `grep`/`filter`, `sort`, and `e` accept **blockless expressions** on the RHS of `|>` — no `{ }` required for simple transforms:

  ```perl
  # chain HTTP fetch → JSON decode → jq filter
  my @titles = $url |> fetch_json |> json_decode |> json_jq '.articles[].title'

  # blockless list pipelines — no braces needed for simple expressions
  files |> filter /[a-e]/ |> e -f $_ && system("cat $_")
  "a".."z" |> map uc |> e p                      # A B C … Z
  "a".."z" |> grep /[aeiou]/ |> e p              # a e i o u
  "a".."z" |> filter 't' |> e p                  # t  (literal = equality test)
  1..10 |> filter $_ > 5 |> sort |> e p      # blocks still work
  1..5 |> map $_ * $_ |> join "," |> p  # 1,4,9,16,25

  # e — side-effect-only iteration (like map but void, returns count)
  qw(apple banana cherry) |> grep /^a/ |> map uc |> e p  # APPLE

  # unary builtins — `x |> length`, `x |> uc`, `x |> sqrt`, etc.
  "hello" |> length |> p  # 5
  16 |> sqrt |> p  # 4
  "ff" |> hex |> p  # 255

  # bareword on RHS becomes a unary call: `x |> f` → `f(x)`
  # call on RHS prepends: `x |> f(a, b)` → `f(x, a, b)`
  # map/grep/filter/sort/join/reduce/fold/e — LHS fills the list slot
  # chunked/windowed — `LIST |> chunked(N)` prepends the list before the size
  # scalar on RHS: `x |> $cr` → `$cr->(x)`

  # regex ops in pipelines — s///, tr///, and m// work as RHS of |>
  # s/// and tr/// auto-inject /r so the modified string flows through:
  "hello world" |> s/world/perl/  |> p  # hello perl
  "hello world" |> tr/a-z/A-Z/   |> p  # HELLO WORLD

  # m//g extracts all matches as an array:
  "foo123bar456" |> /\d+/g |> p  # 123 456

  # full pipeline: read input, strip newlines, split, count word frequencies
  # man ls | stryke 'input |> s@\n@@g |> split |> frequencies |> ddump |> p'

  # extract all emails from text, deduplicate
  # cat log.txt | stryke 'input |> /[\w.]+@[\w.]+/g |> distinct |> e p'

  # capture groups with /g:
  "a=1 b=2" |> /(\w+)=(\w+)/g |> ddump |> p
  ```

  **Pipeline builtins** — designed for `|>` chains:

  ```perl
  # ── input / output ─────────────────────────────────────────────────
  input                                # slurp all of stdin as one string
  input($fh)                           # slurp a filehandle
  # cat data.txt | stryke 'input |> lines |> e p'

  # ── string → list ──────────────────────────────────────────────────
  "hello\nworld" |> lines |> ddump |> p  # ("hello", "world")
  "foo bar baz"  |> words |> ddump |> p  # ("foo", "bar", "baz")
  "hello"        |> chars |> ddump |> p  # ("h","e","l","l","o")
  "  hello  "    |> trim  |> p  # "hello"

  # ── case conversion ────────────────────────────────────────────────
  "helloWorld"     |> snake_case  |> p  # hello_world
  "hello_world"    |> camel_case  |> p  # helloWorld
  "Hello World"    |> kebab_case  |> p  # hello-world

  # ── aggregation / stats ────────────────────────────────────────────
  1 .. 100 |> avg    |> p  # 50.5
  1 .. 100 |> stddev |> p  # 28.86607…
  "hello"  |> chars  |> frequencies |> ddump |> p
  # { h => 1, e => 1, l => 2, o => 1 }

  # ── frequencies + top ──────────────────────────────────────────────
  "the quick brown fox" |> chars |> frequencies |> top 3 |> ddump |> p
  # top 3 chars by count

  # ── count_by { BLOCK } LIST ────────────────────────────────────────
  1 .. 20 |> count_by { $_ % 2 == 0 ? "even" : "odd" } |> ddump |> p
  # { odd => 10, even => 10 }

  # ── numeric transforms ─────────────────────────────────────────────
  1 .. 10  |> clamp 3, 7    |> ddump |> p  # 3 3 3 4 5 6 7 7 7 7
  1 .. 5   |> normalize     |> ddump |> p  # 0 0.25 0.5 0.75 1

  # ── inverse grep (regex) ───────────────────────────────────────────
  1 .. 10 |> grep_v "^[35]$" |> ddump |> p  # removes 3 and 5

  # ── hash manipulation ──────────────────────────────────────────────
  my $h = {a => 1, b => 2, c => 3}
  $h |> select_keys "a", "c" |> ddump |> p  # { a => 1, c => 3 }

  # ── pluck key from list of hashrefs ────────────────────────────────
  my @people = ({name=>"Alice",age=>30}, {name=>"Bob",age=>25})
  @people |> pluck "name" |> ddump |> p  # ("Alice", "Bob")

  # ── serialization ──────────────────────────────────────────────────
  my $data = {a => 1, b => [2,3]}
  $data |> to_json |> p  # {"a":1,"b":[2,3]}
  @people |> to_csv |> p  # CSV with headers
  my $cfg = {title => "My App", package => {name => "myapp", version => "1.0"}}
  $cfg |> to_toml |> p  # TOML with [package] table
  $data |> to_yaml |> p  # YAML with --- header
  $data |> to_xml  |> p  # XML with <root> wrapper
  fr |> map +{name => $_, size => format_bytes(size)} |> th |> to_file("report.html") |> xopen  # cyberpunk HTML table → browser
  fr |> map +{name => $_, size => format_bytes(size)} |> tmd |> to_file("report.md") |> xopen  # GFM Markdown table → viewer
  # same pipelines in ~> syntax:
  ~> fr map +{name => $_, size => format_bytes(size)} th to_file($_, "report.html") xopen
  ~> fr map +{name => $_, size => format_bytes(size)} tmd to_file($_, "report.md") xopen
  fr |> map +{name => $_, size => format_bytes(size)} |> tbl |> p                      # plain-text aligned table
  fr |> map +{name => $_, size => format_bytes(size)} |> tmd |> clip                   # markdown table → clipboard

  # ── data visualization ─────────────────────────────────────────────
  # sparkline — inline Unicode trend line from numbers
  (3,7,1,9,4,2,8,5) |> spark |> p  # ▃▆▁█▄▂▇▅
  map { int(rand(100)) } 1..20 |> spark |> p  # random sparkline

  # bar_chart (bars) — horizontal colored bars from hashref
  qw(a b a c a b) |> freq |> bars |> p  # word frequency bars
  cat("Cargo.toml") |> words |> freq |> bars |> p  # word freq from file
  fr |> map { path_ext($_) } |> freq |> bars |> p  # file extension breakdown

  # histo — vertical histogram, top N by count
  cat("Cargo.toml") |> chars |> freq |> histo |> p  # character distribution
  map { int(rand(10)) } 1..100 |> freq |> histo |> p  # dice roll distribution

  # to_table (tbl) — plain-text column-aligned table with box drawing
  qw(a b a c a b) |> freq |> tbl |> p  # freq as table
  fr |> map +{name => $_, size => format_bytes(size)} |> tbl |> p  # file listing table
  fr |> map +{name => $_, ext => path_ext($_)} |> tbl |> p  # files with extensions

  # flame — terminal flamechart from nested hashrefs
  flame({main => {parse => 30, eval => {compile => 15, run => 45}}, init => 10}) |> p
  cat("Cargo.toml") |> chars |> freq |> flame |> p  # flat flame from char freq

  # gauge — single-value progress bar with color coding
  p gauge(0.73)  # [██████████████████████░░░░░░░░] 73%
  p gauge(45, 100)  # value/max form
  fr |> cnt |> gauge($_, 500) |> p  # file count vs target

  # spinner — animated braille spinner while block runs
  my $r = spinner "loading" { sleep 2; 42 }  # returns block result
  my $data = spinner "fetching" { fetch_json($url) }  # wrap any slow operation
  # spinner_start / spinner_stop — manual control for multi-step work
  my $s = spinner_start("processing")
  do_step1(); do_step2(); do_step3()
  spinner_stop($s)

  # clip — copy pipeline output to clipboard
  fr |> map +{name => $_, size => format_bytes(size)} |> tmd |> clip  # markdown table → clipboard
  cat("Cargo.toml") |> words |> freq |> tbl |> clip  # table → clipboard

  # combine charts: same data, multiple views
  my %f = %{cat("Cargo.toml") |> words |> freq}
  %f |> bars |> p  # horizontal bars
  %f |> histo |> p  # vertical histogram
  %f |> tbl |> p  # aligned table
  %f |> flame |> p  # flamechart
  values %f |> spark |> p  # inline sparkline

  # ~> syntax equivalents — no |> needed
  ~> qw(a b a c a b) freq bars p
  ~> qw(a b a c a b) freq histo p
  ~> qw(a b a c a b) freq tbl p
  ~> (3,7,1,9,4) spark p

  # ── inline ANSI colors ─────────────────────────────────────────────
  p "#{red}ERROR#{off} #{green_bold}OK#{off}"  # color names as #{} builtins
  p "#{rgb(255,100,0)}ORANGE#{off}"  # true color (24-bit)
  p "#{color256(196)}RED#{off}"  # 256-color palette

  # ── stringify / str — parseable stryke literals ──────────────────────
  $data |> str |> p  # +{a => 1, b => [2, 3]}
  my $fn = fn { $_ * 2 }
  $fn |> str |> p  # fn { $_ * 2; }
  range(1, 3) |> str |> p  # (1, 2, 3)
  # round-trip: str -> eval -> callable
  my $f = fn ($x: Int) { $x + 1 }
  my $f2 = $f |> str |> eval
  $f2->(5) |> p  # 6

  # ── partition / min_by / max_by / zip_with ─────────────────────────
  my ($yes, $no) = partition { $_ > 5 } 1..10
  my $smallest = min_by { length } @words
  my $largest  = max_by { length } @words
  my @sums = zip_with { $_0 + $_1 } [1,2,3], [10,20,30]  # 11 22 33

  # ── pretty-print (indented dump) ───────────────────────────────────
  my $nested = {key => [1, {nested => "val"}]}
  $nested |> ddump |> p

  # ── write to file (returns content for further piping) ─────────────
  my $text = "hello\nworld\n"
  $text |> to_file "/tmp/out.txt"

  # ── file I/O helpers ────────────────────────────────────────────────
  my @lines = read_lines "/tmp/out.txt"  # slurp file → list of lines
  append_file "/tmp/out.txt", "extra\n"  # append to file
  my $tmp = tempfile()  # create temp file, returns path
  my $dir = tempdir()  # create temp directory, returns path

  # ── JSON file I/O ──────────────────────────────────────────────────
  write_json "/tmp/data.json", {a => 1, b => 2}  # write hash as JSON file
  my $obj = read_json "/tmp/data.json"  # read JSON file → hashref

  # ── interleave ─────────────────────────────────────────────────────
  my @merged = interleave [1,2,3], [10,20,30]  # (1,10,2,20,3,30)

  # ── glob_match / which_all ──────────────────────────────────────────
  p glob_match "*.txt", "readme.txt"  # 1 (matches)
  my @bins = which_all "perl"  # all paths for "perl" in $PATH

  # ── zsh glob qualifiers — world's first in a scripting language ────
  # Stryke imports the full zshrs glob engine (zsh-compatible). Every
  # builtin that accepts a glob — `glob`, `glob_par`, `slurp`/`c`/`cat`,
  # `swallow`/`swa`, `ingest`/`ing`, `pwatch`, `par_find_files`,
  # `<*.txt>`, … — applies the qualifiers without a single line of
  # stryke-side parsing. Source of truth is `zsh::glob` from `../zshrs`.
  my @dirs = glob "**(/)"          # directories only, recursive
  my @files = glob "**(.)"         # regular files only, recursive
  my @links = glob "**(@)"         # symlinks only
  my @exec = glob "**(*)"          # executable files
  my @big = glob "**(L+1024)"      # files larger than 1024 bytes
  my @recent = glob "**(om[1])"    # most recently modified, take 1
  my @safe = glob "doesnotexist*(N)"  # NULL_GLOB — empty list, no error

  # `c()` is a slurp; non-regular results are a hard error, by design —
  # asking to read a directory is always a bug:
  c "**(.)"   # OK: concatenated contents of every file recursively
  c "**(/)"   # ERROR: "slurp: not a regular file: ./sub"

  # `swallow` is the per-file hash sibling of `slurp` — returns
  # `{ canonical_abspath => raw_bytes }`. Always bytes (works for binary
  # files); keys are absolute paths with symlinks flattened via
  # `fs::canonicalize`. Same hard-fail rule on non-regular matches.
  my %src = swallow "src/**/*.rs"   # every Rust source file, raw bytes
  my %imgs = swa "assets/**/*.{png,jpg}"
  my %safe = swallow "missing*(N)"  # (N) null-glob → empty hash

  # `ingest` is the streaming variant of `swallow` — yields
  # `[abspath, bytes]` one file at a time so only one file's bytes are
  # resident at any moment. Same eager glob expansion (full qualifier
  # support, hard-fail on non-regular up-front), but file reads are
  # deferred to each iteration step. For-loops over an ingest iterator
  # pull lazily (no `to_list()` materialisation); use `|>` pipes or
  # explicit `->next` driving the same way.
  for my $pair (ingest "data/**/*.bin") {
      my ($path, $bytes) = @$pair
      # process one file's bytes, then they go out of scope
  }
  my $it = ing "logs/*.log"; while (my $p = $it->next->[0]) { ... }

  # `burp` is the inverse of `swallow` — take a `{path => content}` hash
  # and write each entry. Parent directories are created automatically,
  # so the canonical swallow → mutate → burp round-trip works even when
  # the destination tree doesn't yet exist. Pass via hashref (`\%h` or
  # inline `{ ... }`); returns the integer count of files written.
  my %src = swallow "src/**/*.rs"
  for my $p (keys %src) { $src{$p} = uc $src{$p} }
  my $n = burp \%src                          # in-place update
  burp { "out/README.md" => "# Hello\n", "out/src/main.rs" => "..." }
  ```

  **Full qualifier reference** — stryke supports **every** zsh glob qualifier (`man zshexpn`, _Filename Generation > Glob Qualifiers_), inherited verbatim from `zsh::glob`:

  | Family | Qualifier | Match |
  |---|---|---|
  | type | `(/)` `(.)` `(@)` `(=)` `(p)` `(%b)` `(%c)` `(%)` `(*)` | dir / regular file / symlink / socket / FIFO / block-dev / char-dev / any-dev / executable |
  | perm (EUID) | `(r)` `(w)` `(x)` | readable / writable / executable |
  | perm (other) | `(R)` `(W)` `(X)` `(A)` `(I)` `(E)` | world r/w/x · group r/w/x |
  | special | `(s)` `(S)` `(t)` | setuid / setgid / sticky |
  | mode bits | `(f<bits>)` | exact mode-bit match, e.g. `(f644)` |
  | owner | `(U)` `(G)` `(u<N>)` `(g<N>)` | EUID / EGID / numeric uid / numeric gid |
  | device | `(d<N>)` | match by device number |
  | size  | `(L[unit]±N)` | bytes default; units `p` `k` `m` `g` `t`; `+N` greater, `-N` less |
  | links | `(l±N)` | hard-link count |
  | times | `(a±N)` `(m±N)` `(c±N)` | atime / mtime / ctime; units `s` `m` `h` `d` `w` `M` |
  | sort  | `(o…)` `(O…)` | asc / desc on `n` `L` `l` `a` `m` `c` `d`; `(oN)` no-sort |
  | select | `([N])` `([N,M])` | Nth / slice; combine with sort, e.g. `(om[1])` newest |
  | flags | `(N)` `(D)` `(F)` `(M)` `(T)` `(n)` | null-glob / include dotfiles / non-empty dir / mark-dirs / list-types / numeric-sort |
  | eval | `(e'CMD')` `(+func)` | shell-eval predicate / function-as-test |
  | join | `(P…)` `(Q…)` | prefix / postfix join words around each match |
  | colon | `(:s/PAT/REPL/)` `(:e)` `(:r)` `(:t)` `(:h)` … | sed-style + tail/root/extension/head modifiers on each result path |
  | combinators | `^` `-` `,` chain | negate / toggle follow-symlinks / OR / chained-AND |

  **Blockless `|>` rules for `grep`/`filter`**: string literals test `$_ eq EXPR`, numbers test `$_ == EXPR`, regexes test `$_ =~ EXPR`, anything else (e.g. `defined`) uses standard Perl grep semantics (sets `$_`, evaluates expression).

  **Coderef-in-block-position** — wherever a `{ BLOCK }` is accepted (`grep`, `map`, `sort`, `first`, `any`, `all`, `none`, `take_while`, `drop_while`, `reject`, `partition`, `min_by`, `max_by`, plus their pipe-forward variants), a coderef-shaped expression also works directly. Runtime check: if the EXPR evaluates to a code ref, it is called with the current element(s) as positional args; otherwise the value's truthiness drives filtering (or its result becomes the mapped value, comparator integer, etc.). Eliminates the `{ $f($_) }` / `{ $f->($_) }` boilerplate.

  ```perl
  my $is_big = fn ($x) { $x > 3 }
  my @r = grep $is_big, @l                # was: grep { $is_big->($_) } @l
  my @r = @l |> grep $is_big              # pipe-forward variant
  my @r = first $is_big, @l               # tier-2 builtin, no parens, no block
  my @r = take_while $is_big, @l

  # Sort comparators receive ($a, $b) positionally — no $a/$b global magic:
  my $cmp = fn ($a, $b) { $b <=> $a }     # or fn { _0 <=> _1 } using positional aliases
  my @s = sort $cmp @l                    # descending
  ```

  **Threading (`~>`) excluded** — whitespace-delimited stages can't disambiguate `~> @l grep $f` from "two stages", so threading still requires `{ $f(_) }`. Use `|>` for the bare-coderef form, or stay with `{ }` blocks under `~>`.

  Under `--compat`: dispatch is skipped, restoring Perl's "evaluate EXPR per element, filter by truthiness" semantics. A coderef value is always truthy, so `grep $f, @l` keeps every element under `--compat`.

  Precedence: `|>` binds **looser** than `||` but **tighter** than `?:` / `and`/`or`/`not` — the slot sits between `parse_ternary` and `parse_or_word` in the parser stack. So `$x + 1 |> f` parses as `f($x + 1)`, and `0 || 1 |> yes` parses as `yes(0 || 1)`. The RHS must be a call, builtin, method invocation, bareword, or coderef expression; bare binary expressions / literals on the right are a parse error (`42 |> 1 + 2` is rejected).

- **`~>` macro** (`thread`, `t`, `->>`) — Clojure-inspired threading macro for clean multi-stage pipelines without repeating `|>`. Stages are bare function names, functions with blocks, parenthesized calls `name(args)` where `$_` (or bare `_`) is the threaded-value placeholder (must appear at least once in args, can sit in any position — first, last, middle, nested), or anonymous blocks (`>{}` / `fn {}`). Use `|>` after `~>` to continue piping. Blocks can use bare `_` for maximum conciseness — `map{_*2}` is equivalent to `map{$_ * 2}`.

  ```perl
  # ultra-concise — bare _ eliminates sigil noise
  ~>1:10map{_*2}fi{_>5}sum p                          # 104

  # ~> shines with multiple block-taking functions — no |> repetition
  @data = 1..20
  ~> @data grep{_ % 2 == 0} map{_ * _} sort{$_1 <=> $_0} |> join "," |> p
  # 400,324,256,196,144,100,64,36,16,4

  # Compare: same pipeline with |> requires more syntax
  @data |> grep{_ % 2 == 0} |> map{_ * _} |> sort{$_1 <=> $_0} |> join "," |> p

  # Long data processing pipeline
  @nums = 1..100
  ~> @nums grep{_ % 3 == 0} map{_ * 2} grep{_ > 50} sort{$_1 <=> $_0} |> head 5 |> join "," |> p
  # 198,192,186,180,174

  # Anonymous blocks for custom transforms
  ~> 100 >{_ / 2} >{_ + 10} >{_ * 3} p  # 180

  # Process list of hashes
  @users = ({name=>"alice",age=>30}, {name=>"bob",age=>25}, {name=>"carol",age=>35})
  ~> @users sort{$_0->{age} <=> $_1->{age}} map{_->{name}} |> join "," |> p
  # bob,alice,carol

  # String processing with unary builtins
  ~> "  hello world  " trim uc p                 # HELLO WORLD

  # Parenthesized call stages — `_` or `$_` is the threaded-value placeholder
  fn add2 { $_0 + $_1 }
  ~> 10 add2(_, 5) p                              # add2(10, 5)        => 15
  ~> 10 add2(5, _) p                              # add2(5, 10)        => 15  (any position)
  ~> 10 add2(_, 5) add2(_, 100) p                 # chains: 15 then 115
  fn add3 { $_0 + $_1 + $_2 }
  ~> 10 add3(5, _, 10) p                          # add3(5, 10, 10)    => 25
  # `_` works inside nested expressions too:
  fn mul { $_0 * $_1 }
  ~> 10 mul(_ + 1, 2) p                           # mul(11, 2)         => 22

  # Reduce with $_0/$_1
  ~> (1..10) reduce { $_0 + $_1 } p              # 55

  # Sort and unique
  @data = (3,1,4,1,5,9,2,6,5,3)
  ~> @data sort { $_0 <=> $_1 } uniq |> join "," |> p   # 1,2,3,4,5,6,9
  ```

  **When to use `~>` vs `|>`:**
  - **`~>`**: Best for chains of block-taking functions (`map { }`, `grep { }`, `sort { }`, `reduce { }`)
  - **`|>`**: Best for blockless expressions (`map $_ * 2`, `grep $_ > 5`) and unary functions

  ```perl
  # |> with blockless expressions — cleanest for simple transforms
  1..20 |> grep $_ % 2 == 0 |> map $_ * $_ |> grep $_ > 50 |> join "," |> p
  # 64,100,144,196,256,324,400

  # ~> with blocks — cleanest when every stage needs a block
  ~> @data map { complex($_) } grep { validate($_) } sort { $_0 cmp $_1 } |> p
  ```

  **Stage types:**
  - **Bare function**: `~> "hello" uc trim` — applies unary builtins in sequence
  - **Function with block**: `~> @data map{_ * 2} grep{_ > 5}` — block-taking functions (bare `_` or `$_`)
  - **Anonymous block**: `~> 5 >{_ * 2}` or `fn { }` — custom transforms

  **Termination:** `|>` ends the `~>` macro: `~> @l f1 f2 f3 |> f4` parses as `(~> @l f1 f2 f3) |> f4`.

  **Numeric/statistical pipelines:**

  ```perl
  # Sum of squares of even numbers 1-10
  ~> (1..10) grep{_ % 2 == 0} map{_ * _} sum p                # 220

  # Mean of squares
  ~> (1..10) map{_ * _} mean p                                 # 38.5

  # Multiples of 7 up to 100, doubled, summed
  ~> (1..100) grep{_ % 7 == 0} map{_ * 2} sum p               # 1470

  # Sum of odd squares, sqrt, truncate
  ~> (1..50) grep{_ % 2 == 1} map{_ ** 2} sum sqrt int p      # 144

  # Factorial via product
  ~> (1..10) product p                                        # 3628800

  # Remove duplicates, then sum
  ~> (1,1,2,2,3,3,4,5,5) uniq sum p                           # 15

  # Shuffle, dedupe, sum (same result, random order internally)
  ~> (1..20) shuffle uniq sum p                               # 210

  # Statistical measures
  ~> (1..10) mean p                                           # 5.5
  ~> (1..10) median p                                         # 5.5
  ~> (1..10) stddev p                                         # 2.87228...
  ```

  **String pipelines:**

  ```perl
  # Full transformation
  ~> " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p
  # "d-lrow-olleh"

  # String list operations
  ~> ("apple","banana","cherry","date") shuffle rev minstr p  # apple
  ~> ("apple","banana","cherry","date") shuffle rev maxstr p  # date
  ```

  **Sorting and aggregation:**

  ```perl
  # Sort then get min/max
  ~> (5,2,8,1,9,3) sort { $_0 <=> $_1 } min p                 # 1
  ~> (5,2,8,1,9,3) sort { $_0 <=> $_1 } max p                 # 9

  # Pairs: extract keys and values
  ~> (1,2,3,4,5,6) pairkeys |> join "," |> p                  # 1,3,5
  ~> (1,2,3,4,5,6) pairvalues |> join "," |> p                # 2,4,6
  ```

  **Compare with `|>` syntax (same result, more typing):**

  ```perl
  # ~> version (bare _)
  ~> (1..10) grep{_ % 2 == 0} map{_ * _} sum p

  # |> version
  (1..10) |> grep{_ % 2 == 0} |> map{_ * _} |> sum |> p
  ```

  **Language comparison — the same 10-stage pipeline:**

  ```perl
  # stryke: 1 line, reads left-to-right, no noise
  ~> " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p
  ```

  ```perl
  # Perl 5: needs CPAN modules, verbose method chains
  use String::CamelCase qw(camelize decamelize)
  use JSON
  my $s = " hello world "
  $s =~ s/^\s+|\s+$//g  # trim
  $s = uc($s)
  $s = reverse($s)
  $s = lc($s)
  $s = ucfirst($s)
  $s =~ s/([A-Z])/_\l$1/g; $s =~ s/^_//  # snake_case (manual)
  $s = camelize($s)  # camel_case (CPAN)
  $s =~ s/([A-Z])/-\l$1/g; $s =~ s/^-//  # kebab_case (manual)
  print encode_json($s), "\n"
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

  **stryke: 1 line. Perl 5: 10+ lines + CPAN. JavaScript: 15+ lines. Python: 15+ lines.**

  **Lisp hell** — without `|>`, the same pipeline becomes unreadable:

  ```perl
  # stryke with |> : reads left-to-right
  " hello world " |> trim |> uc |> rev |> lc |> ucfirst |> rev |> snake_case |> camel_case |> kebab_case |> rev |> uc |> lc |> trim |> to_json |> p
  # "d-lrow-olleh"

  # Without |> : nested calls, reads inside-out (lisp hell)
  p(to_json(trim(lc(uc(rev(kebab_case(camel_case(snake_case(rev(ucfirst(lc(rev(uc(trim(" hello world ")))))))))))))))
  ```

  The pipe-forward operator eliminates the cognitive overhead of matching parentheses and reading inside-out.

- **Short aliases** — 1-3 character aliases for common functions, designed for `~>`/`|>` pipelines:

  ```perl
  # Long form
  ~> " hello world " trim uc rev lc ucfirst snake_case camel_case kebab_case to_json p

  # Short form (same result)
  ~> " hello world " tm uc rv lc ufc sc cc kc tj p
  ```

  | Alias | Function | Alias | Function | Alias | Function |
  |-------|----------|-------|----------|-------|----------|
  | **Thread/Pipe** | | **String** | | **Case** | |
  | `~>` | `thread` | `tm` | `trim` | `sc` | `snake_case` |
  | `p` | `len` | `length` | `cc` | `camel_case` |
  | `pr` | `print` | `ufc` | `ucfirst` | `kc` | `kebab_case` |
  | | | `lfc` | `lcfirst` | `qm` | `quotemeta` |
  | **List** | | `rev` | |
  | `gr` | `grep` | `ch` | `chars` | **Serialize** | |
  | `so` | `sort` | `ln` | `lines` | `tj` | `to_json` |
  | `rd` | `reduce` | `wd` | `words` | `ty` | `to_yaml` |
  | `hd` | `head/take` | | | `tt` | `to_toml` |
  | `tl` | `tail` | **Unique/Dedup** | | `tc` | `to_csv` |
  | `drp` | `drop/skip` | `uq` | `uniq` | `tx` | `to_xml` |
  | `fl` | `flatten` | `dup` | `dedup` | `th` | `to_html` |
  | `cpt` | `compact` | `shuf` | `shuffle` | `tmd` | `to_markdown` |
  | | | | | `dd` | `ddump` |
  | | | | | `xo` | `xopen` |
  | `cat` | `slurp` | | | **Deserialize** | |
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
  | `swa` | `swallow` | | | | |
  | `ing` | `ingest` | | | | |
  | | `burp` | | | | |
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
  | `rss` | `proc_mem` | | | | |

- **`fn` keyword** — alias for `sub`. Both `fn name { }` and `fn { }` work identically to `sub`.

  ```perl
  fn double($x) { $x * 2 }
  p double(21)                    # 42

  my $f = fn { _ * 2 }
  p $f->(21)                      # 42

  # implicit zero-arg coderef — at top-level, RHS starting with bare `_` / `_N`
  # auto-wraps as `fn { ... }`. Inside any block, `_` is still the topic.
  my $g = _ * 2
  p $g->(21)                      # 42
  my $h = _ + _1
  p $h->(3, 4)                    # 7
  ```

- **Closure arguments `$_0`, `$_1`, ... `$_N`** — numeric closure arguments inspired by Swift. All arguments passed to any fn (named or anonymous) are available as `$_0` (first), `$_1` (second), `$_2` (third), up to `$_N` for any number of arguments. These work alongside or instead of Perl's `@_`, `$_`, `$a`, `$b`. Both `$_`, bare `_`, and `$_0` refer to the first argument — `_ * 2`, `$_ * 2`, and `$_0 * 2` are all equivalent. Use bare `_` for maximum conciseness in blocks.

  ```perl
  # $_0 in |> pipes (single-arg: $_0 == $_)
  (1..5) |> map { $_0 * 2 } |> join "," |> p           # 2,4,6,8,10
  (1..10) |> grep { $_0 % 2 == 0 } |> sum |> p         # 30

  # $_0/$_1 in |> pipes (two-arg: $_0/$_1 == $a/$b)
  (5,2,8,1) |> sort { $_0 <=> $_1 } |> join "," |> p   # 1,2,5,8
  (1..5) |> reduce { $_0 + $_1 } |> p                  # 15
  (1..5) |> reduce { $_0 * $_1 } |> p                  # 120 (factorial)
  ("banana","apple","cherry") |> sort { length($_0) <=> length($_1) } |> join "," |> p  # apple,banana,cherry

  # $_0/$_1 in ~> macro
  ~> (1..5) map { $_0 * 2 } sum p                  # 30
  ~> (1..5) reduce { $_0 + $_1 } p                 # 15
  ~> (1..5) reduce { $_0 * $_1 } p                 # 120
  ~> (5,2,8,1) sort { $_0 <=> $_1 } |> join "," |> p  # 1,2,5,8
  ~> (1..10) grep { $_0 % 2 == 0 } map { $_0 * $_0 } sum p  # 220

  # Multi-arg anonymous subs: $_0, $_1, ... $_N
  my $add3 = fn { $_0 + $_1 + $_2 }
  p $add3->(1, 2, 3)  # 6

  my $mul5 = fn { $_0 * $_1 * $_2 * $_3 * $_4 }
  p $mul5->(1, 2, 3, 4, 5)  # 120

  my $concat = fn { "$_0-$_1-$_2-$_3" }
  p $concat->("a", "b", "c", "d")  # a-b-c-d

  # Direct access via @_ still works
  my $join_args = fn { join("-", @_) }
  p $join_args->("x", "y", "z")  # x-y-z

  # Using $_0 closures with |> pipes
  my $double = fn { $_0 * 2 }
  my $triple = fn { $_0 * 3 }
  5 |> $double |> $triple |> p               # 30

  # Using $_0/$_1 closures in reduce
  my $add = fn { $_0 + $_1 }
  (1..5) |> reduce { $add->($_0, $_1) } |> p # 15

  # Using $_0/$_1/$_2 closure
  my $mul3 = fn { $_0 * $_1 * $_2 }
  p $mul3->(2, 3, 4)  # 24

  # Using $_0/$_1 closure as comparator
  my $cmp = fn { $_0 <=> $_1 }
  (5,2,8,1) |> sort { $cmp->($_0, $_1) } |> join "," |> p  # 1,2,5,8

  # User-defined functions in ~> (bare stage, no block needed)
  fn double { $_0 * 2 }
  fn triple { $_0 * 3 }
  fn add5   { $_0 + 5 }
  fn square { $_0 ** 2 }
  fn half   { $_0 / 2 }
  ~> 2 double triple add5 square half p  # 144.5

  fn inc  { $_0 + 1 }
  fn dec  { $_0 - 1 }
  fn dbl  { $_0 * 2 }
  fn neg  { -$_0 }
  fn abs_ { abs($_0) }
  ~> 5 inc dbl dec neg abs_ dbl inc p    # 23

  fn wrap  { "[$_0]" }
  fn upper { uc($_0) }
  fn trim_ { trim($_0) }
  fn rev_  { rev($_0) }
  fn bang  { "$_0!" }
  ~> "  hello  " trim_ upper rev_ wrap bang p  # [OLLEH]!

  # User-defined functions inside blocks
  fn is_even { $_0 % 2 == 0 }
  ~> (1..10) grep{is_even(_)} sum p  # 30

  ~> (1..5) map{square(_)} sum p     # 55

  # Multi-arg user-defined functions
  fn add  { $_0 + $_1 }
  fn mul3 { $_0 * $_1 * $_2 }
  p add(3, 4)                                # 7
  p mul3(2, 3, 4)                            # 24

  # Inline transforms with >{ } (arrow block)
  ~> 5 >{_ * 2} >{_ + 10} p               # 20
  ~> 100 >{_ / 2} >{_ + 10} >{_ * 3} p    # 180
  ```

- **Block params `{ |$var| body }`** — name the block's implicit arguments with Ruby-style `|$params|` at the start of a block. For single-param blocks (`map`, `grep`, `each`), the param aliases `$_`. For two-param blocks (`sort`, `reduce`), they alias `$a`/`$b`. For N≥3 params, they alias `$_`, `$_1`, `$_2`, etc.

  ```perl
  # Single param — aliases $_
  map { |$n| $n * $n }, 1..5                         # 1 4 9 16 25
  grep { |$x| $x > 3 }, 1..6                         # 4 5 6
  (1..3) |> map { |$n| $n + 10 } |> join ","         # 11,12,13

  # Two params — aliases $a/$b
  sort { |$x, $y| $y <=> $x }, 3, 1, 4, 1, 5        # 5 4 3 1 1
  reduce { |$acc, $val| $acc + $val }, 1..10         # 55
  ```

`stryke` is **not** a full `perl` replacement: many real `.pm` files (especially XS modules) will not run. See [`PARITY_ROADMAP.md`](parity/PARITY_ROADMAP.md).

---

## [0x08a] `--no-interop` MODE

`--no-interop` is the **idiomatic-stryke-only** mode: every Perl-ism that has a stryke replacement becomes a parse-time error so codebases stay on the stryke side of the language. Cargo-cult Perl idioms can't sneak in, and grep'ing for `\bscalar\b` / `\bsub\b` / `\bsay\b` in your sources stays signal-only.

| Perl-ism | Rejected with | Use instead |
|---|---|---|
| `sub NAME { … }` / `sub { … }` | `stryke uses 'fn' instead of 'sub' (--no-interop)` | `fn NAME { … }` / `fn { … }` |
| `say EXPR` | `stryke uses 'p' instead of 'say' (--no-interop)` | `p EXPR` |
| `reverse EXPR` | `stryke uses 'rev' instead of 'reverse' (--no-interop)` | `rev EXPR` (works for both strings and lists) |
| `$a` / `$b` outside `sort`/`reduce` blocks | `stryke uses '$_0' / '$_1' instead of '$a' (--no-interop)` | `$_0` / `$_1` positional block params |
| `scalar EXPR` (any form) | `stryke uses 'len' (also 'cnt' / 'count') instead of 'scalar' (--no-interop)` | see the `scalar` mapping below |

### `scalar` mapping under `--no-interop`

`scalar` was overloaded with at least four distinct semantics in Perl. Under `--no-interop` each idiom has its own verb so the meaning is explicit at the call site:

| Perl idiom | What it does | Stryke spelling |
|---|---|---|
| `scalar @a` / `scalar(@$ref)` / `scalar @{$r}` | element count | `len @a` / `len(@$ref)` / `len @{$r}` (aliases `cnt`, `count`) — compiles to `Op::ArrayLen` / `Op::ArrayDerefLen` fast path |
| `scalar keys %h` / `scalar values %h` | key / value count | `len keys %h` / `len values %h` |
| `scalar grep { … } @a` / `scalar split(…)` / `scalar qw(…)` | match / part / element count | `len grep { … } @a` / `len split(…)` / `len qw(…)` |
| `scalar reverse $s` | string reverse (vs. list reverse) | `rev $s` |
| `scalar unpack(FMT, STR)` | first decoded element | `unpack_first(FMT, STR)` (aliases `unpack1`, `up1`) — equivalent to `head(unpack(…))` |
| `scalar splice(@a, off, n)` | last removed element | `splice_last(@a, off, n)` (aliases `splice1`, `spl_last`) — desugars to `tail(splice(@a, off, n))`, mutates in place |
| `scalar \`cmd\`` | joined stdout as string | already the default — stryke backticks return a single string regardless of context, so no spelling change needed |
| `scalar %h` (Perl's hash-fill diagnostic, e.g. `"3/8"`) | dead semantics — never load-bearing | use `len keys %h` for the count |

Default mode (no `--no-interop`) still accepts every Perl-ism listed above for compat with stock `.pm` / `.pl` sources.

---

## [0x08b] STRING COORDINATES — BYTES VS CODEPOINTS

Stryke runs string code in two coordinate systems. Perl 5 builtins stay byte-indexed for binary-protocol / `.pm`-source compat. Stryke extensions are codepoint-indexed so search positions and slice bounds line up. They are never auto-converted — pick one coordinate system per expression, keep it consistent, and never mix a byte-position output into a codepoint-position input.

| Operation | Stryke (codepoints) | Perl 5 (bytes) |
|---|---|---|
| Length | `len $s` | `length $s` |
| Index | `$s[$i]` | `substr $s, $i, 1` |
| Slice | `$s[$a:$b]` (inclusive both ends) | `substr $s, $start, $len` |
| Search forward | `cindex $s, $needle [, $from]` | `index $s, $needle [, $from]` |
| Search backward | `crindex $s, $needle [, $from]` | `rindex $s, $needle [, $from]` |
| Match position | — | `pos $s` (regex `\G` anchor) |

Concrete example:

```perl
my $s = "hello ─ world"     # `─` is 3 bytes / 1 codepoint
length $s                    # 15 (bytes)
len    $s                    # 13 (codepoints)
index  $s, "world"           # 9  (byte position — past the 3-byte `─`)
cindex $s, "world"           # 7  (codepoint position)
$s[7]                        # "w"
substr $s, 9, 1              # "w"
$s[7:11]                     # "world"  — codepoint slice
substr $s, 9, 5              # "world"  — byte substr
```

**Rule:** never feed an `index` / `pos` / `length` result into a `[$a:$b]` slice, and never feed a `cindex` / `crindex` / `len` result into `substr` / `index`. The coordinate systems silently misalign on any string containing non-ASCII bytes.

`--no-interop` does **not** force this split — both systems remain available because Perl 5 binary-protocol code legitimately needs byte positions. The split is a coordinate-system choice, not a stylistic one.

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
 │                    ┌───────────────┴───────────┐    │
 │                    ▼                           ▼    │
 │               VM (vm.rs)                 Cranelift  │
 │                    │                      Block JIT │
 │                    ▼                                │
 │       Rayon work-stealing scheduler                 │
 │       CORE 0 │ CORE 1 │ ... │ CORE N                │
 └─────────────────────────────────────────────────────┘
```

- **Lexer** ([`strykelang/lexer.rs`](strykelang/lexer.rs)) — context-sensitive tokenizer for Perl's ambiguous syntax (regex vs division, hash vs modulo, heredocs, interpolation).
- **Parser** ([`strykelang/parser.rs`](strykelang/parser.rs)) — recursive descent + Pratt precedence climbing.
- **Compiler / VM** ([`strykelang/compiler.rs`](strykelang/compiler.rs), [`strykelang/vm.rs`](strykelang/vm.rs)) — 100% lowered to bytecode. Compiled subs use slot ops for frame-local `my` scalars (O(1)). Lowering covers `BEGIN`/`UNITCHECK`/`CHECK`/`INIT`/`END` with `Op::SetGlobalPhase`, `mysync`, `tie`, scalar compound assigns via `Scope::atomic_mutate`, regex values, named-sub coderefs, folds, `pcache`, `pselect`, `par_lines`, `par_walk`, `par_sed`, `pwatch`, `each`, four-arg `substr`, dynamic `keys`/`values`/`delete`/`exists`, etc.
- **Block JIT** ([`strykelang/jit.rs`](strykelang/jit.rs)) — Cranelift Block JIT with cached `OwnedTargetIsa`, tiered after `STRYKE_JIT_SUB_INVOKES` (default 50) VM invocations. Block JIT validates a CFG, joins typed `i64`/`f64` slots at merges, and compiles hot loops to native code. Disable with `--no-jit` / `STRYKE_NO_JIT=1`.
- **Feature work policy** — prefer **new VM opcodes** in [`bytecode.rs`](strykelang/bytecode.rs), lowering in [`compiler.rs`](strykelang/compiler.rs), implementation in [`vm.rs`](strykelang/vm.rs). Do **not** add new `ExprKind`/`StmtKind` variants for new behavior.
- **Parallelism** — each parallel block spawns an isolated VM with captured scope; Rayon does work-stealing across all cores.

---

## [0x0A] EXAMPLES

`examples/` ships **778 top-level .stk programs** plus **1648 Rosetta-Code tasks** and **347 Exercism solutions** — 2.7k working programs in all. Run any of them directly, run the CI sweep with `stryke examples/run_all_ci.stk`, or run all Exercism solutions with `stryke examples/exercism_run_all.stk`.

```sh
stryke examples/fibonacci.stk
stryke examples/text_processing.stk
stryke examples/parallel_demo.stk
stryke examples/run_all_ci.stk                # validate every example in one pass
```

### Worked examples — long-form

These are full programs (not snippets) that exercise stryke's parallel, AOP, parser, and AI primitives. Each ships with assertions that run on `stryke <file>` and pass under `--no-interop`.

```sh
# Data + ML
stryke examples/tfidf_search_engine.stk         # TF-IDF inverted index + cosine ranking
stryke examples/gradient_descent_linreg.stk     # mini-batch SGD, pmap_reduce gradient sum
stryke examples/kalman_filter_tracking.stk      # 1D Kalman filter, hand-rolled 2×2 matrix algebra
stryke examples/markov_chain_analysis.stk       # transition matrix + stationary distribution

# Scientific computing — single-file toolkit (95 sections, 18,820 lines, 2,120 hand-crafted die invariants)
stryke --no-interop examples/scientific_compute_no_interop.stk   # vectors, matrices, LU/QR, eigenvalues, root-finding,
                                                                 # quadrature, ODE/PDE, FFT, optimization, statistics,
                                                                 # RNG, graph algos (BFS/DFS/Dijkstra/Tarjan/Kuhn),
                                                                 # string algos (KMP/Z/BM/Aho-Corasick/SA/LCP/Manacher),
                                                                 # DP suites, geometry, number theory, simulation

# Parsers + interpreters
stryke examples/mini_sql_executor.stk           # SELECT/WHERE/ORDER BY/LIMIT recursive-descent
stryke examples/tiny_lisp_parser.stk            # S-expression tokenizer + recursive parser
stryke examples/binary_parser_simulation.stk    # pack/unpack binary protocol with checksum

# Concurrency
stryke examples/pubsub_message_bus.stk          # topic-filtered fan-out over pchannel + spawn
stryke examples/multi_threaded_channels.stk     # bounded pchannel producer/consumer
stryke examples/parallel_pi_monte_carlo.stk     # chunk-parallel Monte Carlo with pmaps + sum
stryke examples/parallel_prime_finder.stk       # pgreps + sort

# Web + protocols
stryke examples/http_router_middleware.stk      # Express-style router, path params, middleware chain
stryke examples/webhook_signature_verifier.stk  # HMAC-SHA256 + replay set + max-age window
stryke examples/network_utilities.stk           # IP sorting / CIDR matching / private-IP check
stryke examples/raft_election_simulator.stk     # discrete-time Raft leader election

# ETL + I/O
stryke examples/csv_etl_parallel.stk            # parse → enrich → ~p> chunk-parallel aggregate
stryke examples/parallel_file_hasher.stk        # ~> glob → head → pmaps sha256 → collect
stryke examples/idiomatic_power_user.stk        # pmaps + hll_count + ai mocked summarize
stryke examples/idiomatic_systems_ops.stk       # AOP `before` audit log + spurt

# AI primitives (mock-mode, no API key required)
STRYKE_AI_MODE=mock-only stryke examples/idiomatic_ai_workflow.stk
STRYKE_AI_MODE=mock-only stryke examples/ai_rag_simple.stk
```

### Idiomatic one-liners + quick demos

```sh
# Sets: dedupe + union / intersection
stryke 'my $a = set(1,2,2,3); my $b = set(2,3,4); p len($a | $b), " ", len($a & $b)'

# Threaded pipeline — count primes in 1..100 in parallel
stryke '~> (1:100) pgreps { is_prime _ } collect sort { _ <=> _1 } |> ep'

# File hashes in parallel
stryke '~> glob("*.stk") pmaps { [ _ => sha256(c"#{_}") ] } collect |> ep'

# Sketch algebra — Bloom union
stryke 'my $a = bloom_new(1024); my $b = bloom_new(1024);
        bloom_add($a, $_) for 1:50;
        bloom_add($b, $_) for 30:80;
        p bloom_count($a + $b)'
```

### Rosetta-Code coverage

`examples/rosetta/` mirrors the [Rosetta Code](https://rosettacode.org) catalog as fully runnable stryke programs (one file per task) — 1648 tasks at last count. Run a single task with `stryke examples/rosetta/<task>.stk` or use the project-wide CI in `examples/run_all_ci.stk` to validate the entire corpus.

### Exercism

`examples/exercism/` carries 173 idiomatic stryke solutions to public Exercism problems. `stryke examples/exercism_run_all.stk` runs every solution against its embedded test suite and reports per-track pass counts.

---

## [0x0B] BENCHMARKS

### stryke vs perl5 vs python3 vs ruby vs julia vs raku vs luajit

`bash bench/run_bench_all.sh` — stryke vs perl 5.42.2 vs Python 3.14.4 vs Ruby 4.0.2 vs Julia 1.12.6 vs Raku vs LuaJIT on Apple M5 18-core. Mean of 10 hyperfine runs with 3 warmups; **includes process startup** (not steady-state). Values <1.0x mean stryke is faster.

```
 stryke benchmark harness (multi-language)
 ──────────────────────────────────────────────
  stryke:  stryke v0.7.7
  perl5:   perl 5.42.2 (darwin-thread-multi-2level)
  python:  Python 3.14.4
  ruby:    ruby 4.0.2 +PRISM [arm64-darwin25]
  julia:   julia 1.12.6
  raku:    Rakudo Star v2026.03
  luajit:  LuaJIT 2.1.1774896198
  cores:   18
  warmup:  3 runs
  measure: hyperfine (min 10 runs)

  bench        stryke ms  perl5 ms  python3 ms  ruby ms  julia ms  raku ms  luajit ms  vs perl5  vs python  vs ruby  vs julia  vs raku  vs luajit
  ---------    ---------  --------  ----------  -------  --------  -------  ---------  --------  ---------  -------  --------  -------  ---------
  startup            3.3       2.3        14.3     23.8      68.3     71.4        1.5     1.43x      0.23x    0.14x     0.05x    0.05x      2.20x
  fib                6.7     184.0        60.1     56.6      76.4    261.3        4.7     0.04x      0.11x    0.12x     0.09x    0.03x      1.43x
  loop               3.2      91.2       191.4     77.8      78.1    159.4        4.3     0.04x      0.02x    0.04x     0.04x    0.02x      0.74x
  string             4.0      10.2        26.8     44.7      83.2    124.2        3.3     0.39x      0.15x    0.09x     0.05x    0.03x      1.21x
  hash               6.8      24.6        25.5     32.6     105.7    143.7        2.0     0.28x      0.27x    0.21x     0.06x    0.05x      3.40x
  array              9.8      24.8        33.2     39.4      88.2    843.9       59.0     0.40x      0.30x    0.25x     0.11x    0.01x      0.17x
  regex             12.6      89.7       264.0    234.3      94.4  25043.8      178.2     0.14x      0.05x    0.05x     0.13x    0.00x      0.07x
  map_grep          13.9      48.8        35.9     48.8      90.5    492.4        3.3     0.28x      0.39x    0.28x     0.15x    0.03x      4.21x
```

**stryke vs perl5** — faster on all 8 benches: `fib` 27x, `loop` 29x, `regex` 7.1x, `hash` 3.6x, `map_grep` 3.5x, `array` 2.5x, `string` 2.6x, `startup` 1.4x.

**stryke vs python3** — faster on all 8 benches: `loop` 60x, `regex` 21x, `string` 6.7x, `fib` 9.0x, `startup` 4.3x, `hash` 3.8x, `array` 3.4x, `map_grep` 2.6x.

**stryke vs ruby** — faster on all 8 benches: `regex` 19x, `loop` 24x, `string` 11x, `fib` 8.4x, `startup` 7.2x, `hash` 4.8x, `array` 4.0x, `map_grep` 3.5x.

**stryke vs julia** — faster on all 8 benches: `loop` 24x, `startup` 21x, `string` 21x, `hash` 16x, `fib` 11x, `array` 9.0x, `regex` 7.5x, `map_grep` 6.5x. Julia timings include LLVM JIT compilation cost — in long-running sessions Julia compiles to native code and would match C on numeric work. These benchmarks measure **scripting use cases** where startup + single-shot execution matters.

**stryke vs raku** — faster on all 8 benches by 20-2000x. Raku's `regex` is 25044ms vs stryke's 12.6ms (1988x). Raku (Perl 6) runs on MoarVM with heavy startup (~70ms+). Raku's strengths are language features (grammars, gradual typing, junctions), not runtime speed.

**stryke vs luajit** — LuaJIT is the fastest dynamic language runtime ever built (tracing JIT by Mike Pall). **stryke beats LuaJIT on 3 of 8 benchmarks**: `loop` (0.74x), `array` (0.17x), `regex` (0.07x). Near-parity on `string` (1.21x) and `fib` (1.43x). LuaJIT wins on `hash` (3.4x) and `map_grep` (4.2x) where its tracing JIT eliminates all dispatch overhead. LuaJIT uses Lua patterns (not PCRE) for the regex bench. stryke offers what LuaJIT cannot: `$_`, `-ne`, regex literals, PCRE, parallel primitives (`pmap`, `pmaps`, `pgrep`), streaming iterators, and one-liner ergonomics.

### stryke vs perl5 (detailed)

`bash bench/run_bench.sh` — includes noJIT and perturbation columns for honesty verification. Re-run to get current numbers on your hardware.

#### Parallel & streaming speedup (100k items, `$_ * 2`)

```
  map   (eager, sequential):     0.01s  — inline execution, zero per-item overhead
  maps  (streaming, sequential): 0.11s  — lazy iterator, single interpreter reused
  pmap  (eager, MAX cores):      0.14s  — pre-built interpreter pool, rayon par_iter
  pmaps (streaming, MAX cores):  0.49s  — background worker threads, bounded channel
```

`maps`/`pmaps` are **streaming** — they return lazy iterators that never materialize the full result list. Use `pmaps` for pipelines over billions of items where holding all results in memory is impractical, or with `take` for early termination: `range(0, 1e9) |> pmaps { expensive($_) } |> take 10 |> ep`.

---

## [0x0C] DEVELOPMENT & CI

Pull requests and pushes to `main` run [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (Check, Test, Format, Clippy, Doc, Parity, Release Build).

```sh
cargo test --lib                # parser smoke, lexer/value/error/scope, interpreter, vm, jit
cargo test --test integration   # tests/suite/* (runtime, readline list context, line-mode stdin, …)
cargo bench --bench jit_compare # JIT vs interpreter on the same bytecode
bash bench/run_bench.sh         # perl5 vs stryke suite (needs hyperfine)
bash bench/run_bench_all.sh     # stryke vs perl5 vs python3 vs ruby vs julia vs raku vs luajit (needs hyperfine)
bash parity/run_parity.sh       # exact stdout/stderr parity vs system perl (20 000+ cases)
```

- `Cargo.lock` is committed (CI uses `--locked`). If your global gitignore strips it, force-add updates: `git add -f Cargo.lock`.
- Disable JIT: `STRYKE_NO_JIT=1` or `stryke --no-jit`.
- Parity work is tracked in [`PARITY_ROADMAP.md`](parity/PARITY_ROADMAP.md).

---

## [0x0C-test] TEST RUNNER — WORKER POOL ARCHITECTURE

`stryke test t/` (or `s t t/`, or the `test()` builtin) runs every `test_*.stk` / `t_*.stk` file under a directory. The default architecture is a **persistent worker pool with fork-on-receive**, modeled on `cargo-nextest`. Process-per-test isolation, no per-test dyld cost, ~5–7× faster than the legacy `posix_spawn`-per-test path on big corpora.

```sh
s t t/                                        # default: worker pool
s t -j 8 t/                                   # 8 worker processes (default = num_cpus)
s t -q t/                                     # quiet — suppress per-file output
s t --no-interop t/                           # forward --no-interop to every test
s t --fork t/                                 # legacy posix_spawn-per-test (slower, fully isolated)
s t --inproc t/                               # single-process VM-per-test (fastest, hermetic only)
test("t/")                                    # builtin form, same default
test("t/", { fork => 1, quiet => 1 })         # builtin opt-out / opts
test_no_interop("t/")                         # builtin variant, --no-interop pinned per worker thread
```

**Topology** at peak with `-j 18`: **1 parent + 18 worker processes + up to 18 grandchildren = up to 37 PIDs**, each visible in `ps`. The 18 OS threads in the parent are not workers themselves — they are **pump threads** dedicated to one worker process each, just shuttling JSON over stdin/stdout.

```
parent runner (1 PID)
├── 18 OS pump threads (read JSON results, dispatch from shared crossbeam queue)
├── 18 worker processes (`stryke --test-worker`, persistent, no test code ever runs here)
│   └── on each request: fork() → grandchild
│       └── runs test in fresh VMHelper, writes JSON to saved-stdout fd, _exit
└── result aggregation under `print_lock` (per-block, no line tearing)
```

**Why three modes:**

| Mode | Per-test cost | Isolation | When to use |
|---|---|---|---|
| **`--pool`** (default) | ~1 ms (fork-only) | full (own address space) | normal day-to-day |
| `--fork` | ~9 ms (`posix_spawn` + dyld + crate static-init) | full | parity / debug |
| `--inproc` | ~0 ms | shared parent process | hermetic corpora; fastest but tests can corrupt each other |

**Wire protocol** ([`src/cli_runners.rs`](strykelang/cli_runners.rs)) — line-delimited JSON over stdin/stdout pipes:

```
parent → worker stdin : {"path":"/abs/path/test_foo.stk","no_interop":false,"chdir":"/abs/project_root"}
worker → parent stdout: {"name":"test_foo.stk","passes":18,"fails":0,"failed":false,"detail":null,"stderr":"…"}
```

**Per-test fd dance in the grandchild** (so test output never corrupts the wire):
1. `dup(1)` saves the parent-bound JSON pipe to a private fd.
2. `dup2(devnull, 1)` — test code's `print` / `say` go to `/dev/null` (would otherwise be parsed by the parent as a malformed JSON response and desync the pool).
3. `mkstemp("/tmp/stryke-test-XXXXXX")` + `unlink` + `dup2(tmp, 2)` — test stderr (the `✓`/`✗` checkmark lines from `test_run`) is captured to an anonymous tmp file (auto-deleted on `close`).
4. Run the test in a fresh `VMHelper`; pass/fail counters live on the per-VM `test_pass_total` / `test_fail_total` atomics.
5. `lseek` + `read` the tmp file into the JSON `stderr` field.
6. `write` the JSON to the saved stdout fd, `_exit(0)`.

**Counter accumulation:** the `test_run` builtin resets per-block counters after printing (so multiple `test_run` calls in one file work), so embedders read **`test_pass_total` + `test_pass_count`** — totals roll up before the reset; the residual `_count` is anything that never reached a `test_run`. Both live on the per-VM `VMHelper`.

**`$0`:** in fork mode the OS sets `argv[0]` to the test path; in pool mode the runner sets `interp.program_name = file_str` so tests like `slurp($0)` (used by `test_narcissist.stk`) keep working.

**Multi-root invocations** (`s t a/t b/t c.stk`): the runner groups tests by `project_root` (parent of `t/`), `chdir`s once per group, runs each group's tests in parallel, then moves to the next group. This keeps `require "./lib/Foo.stk"` working while staying parallel within a single project.

**Stragglers:** `stryke test` uses a shared work-stealing queue, so workers naturally rebalance — no static partitioning. The wall-clock floor is the longest-running single test in the slowest worker's tail. Alphabetic ordering tends to put heavy tests late in the queue, so the visible "slowdown at the end" is just the queue draining unevenly. Shortest-job-first ordering would smooth this out but needs a per-test profile pass first.

---

## [0x0D] STANDALONE BINARIES (`stryke build`)

Compile any Perl script to a single self-contained native executable. The output is a copy of the `stryke` binary with the script source embedded as a zstd-compressed trailer. `scp` it to any compatible machine and run it — **no `perl`, no `stryke`, no `@INC`, no CPAN**.

```sh
stryke build app.stk                         # → ./app
stryke build app.stk -o /usr/local/bin/app   # explicit output path
./app --any --script --args             # all argv reach the embedded script's @ARGV
```

**What's in the box:**

- Parse / compile errors are surfaced **at build time**, not when users run the binary.
- The embedded script is detected at startup by a 32-byte trailer sniff (~50 µs), then decompressed and executed by the embedded VM. A script with no trailer runs normally as `stryke`.
- Builds are idempotent: `stryke build app.stk -o app` followed by `stryke --exe app build other.stk -o other` strips the previous trailer first, so binaries never stack.
- Unix: the output is marked `+x` automatically. macOS: unsigned — `codesign` before distribution if your environment requires it.
- Current AOT runtime sets `@INC = (".")`; modules outside the embedded script have to be inlined. (`require` of a local `.pm` next to the running binary still works.)

**Under the hood** ([`strykelang/aot.rs`](strykelang/aot.rs)): trailer layout is `[zstd payload][u64 compressed_len][u64 uncompressed_len][u32 version][u32 reserved][8B magic b"STRYKEAOT"]`. ELF / Mach-O loaders ignore bytes past the mapped segments so the embedded payload is invisible to the OS loader. The `b"STRYKEAOT"` magic plus version byte lets a future pre-compiled-bytecode payload ship alongside v1 without breaking already-shipped binaries.

```sh
# 13 MB binary, no external runtime required:
$ stryke build hello.stk -o hello
stryke build: wrote hello
$ file hello
hello: Mach-O 64-bit executable arm64
$ ./hello alice
hi alice
```

---

## [0x0E] INLINE RUST FFI (`rust { ... }`)

Drop a block of Rust directly into a Perl script. On first run, stryke compiles it to a cdylib (cached at `~/.cache/stryke/ffi/<hash>.{dylib,so}`), `dlopen`s it, and registers every exported function as a regular Perl-callable sub.

```perl
rust {
    pub extern "C" fn add(a: i64, b: i64) -> i64 { a + b }
    pub extern "C" fn mul3(x: f64, y: f64, z: f64) -> f64 { x * y * z }
    pub extern "C" fn fib(n: i64) -> i64 {
        let (mut a, mut b) = (0i64, 1i64)
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

**How it works** ([`strykelang/rust_sugar.rs`](strykelang/rust_sugar.rs), [`strykelang/rust_ffi.rs`](strykelang/rust_ffi.rs)): the source-level pre-pass desugars every top-level `rust { ... }` into a `BEGIN { __stryke_rust_compile("<base64 body>", $line); }` call. The `__stryke_rust_compile` builtin hashes the body, compiles via `rustc --edition=2021 -O` if the cache is cold, `libc::dlopen`s the result, `dlsym`s each detected signature, and stores the raw symbol + arity/type tag in a process-global registry. Calls from Perl flow through a fallback arm in [`crate::builtins::try_builtin`] that dispatches on the signature tag via direct function-pointer transmute — no libffi dep, no per-call alloc, no marshalling overhead beyond the `StrykeValue::to_int` / `to_number` / `to_string` calls you'd do for any builtin.

**Combine with AOT for zero-friction deployment:** `stryke build script.stk -o prog` bakes the Perl source — which includes the `rust { ... }` block — into a standalone binary. The FFI compile still happens on first run of `./prog`, but the user only needs `rustc` once, then the `~/.cache/stryke/ffi/` entry is permanent.

**Limitations (v1):**

- Unix only (macOS + Linux). Windows support is a dlopen-equivalent swap away but isn't wired.
- Signatures beyond the table above are silently ignored (the function still exists in the cdylib, just not Perl-callable).
- Body must be self-contained Rust with `std` only — no `Cargo.toml` / external crate deps. If you need `regex` or similar, vendor the minimal code into the block.
- The cdylib runs with the calling process's privileges. Trust model is equivalent to `do FILE`.

---

## [0x0F] BYTECODE CACHE (rkyv)

stryke stores compiled bytecode in a single rkyv shard at `~/.stryke/scripts.rkyv`. The first run of a script parses + compiles + persists into the shard. Every subsequent run `mmap`s the shard, validates the archived root once, looks up the entry by canonical path in the zero-copy `ArchivedHashMap`, and skips **lex, parse, and compile** entirely.

```sh
stryke my_app.stk              # cold: parse + compile + write into the shard
stryke my_app.stk              # warm: mmap shard + lookup + dispatch (skips lex/parse/compile)
```

**Cache invalidation:** four conditions all evict a stored entry — no stale bytecode is ever served.

| Condition | Trigger |
|---|---|
| Source `mtime` mismatch | Edit the `.stk` file → cache miss → recompile |
| `stryke_version` mismatch | Cargo version bump in `Cargo.toml` |
| Pointer-width mismatch | Cross-build between 32- and 64-bit targets |
| Binary `mtime` newer than cached entry | Rebuild stryke (any `cargo build` advances `target/debug/stryke`'s mtime) → every cached script invalidates automatically. Catches edits to `compiler.rs` / `parser.rs` / `vm.rs` that don't bump `CARGO_PKG_VERSION` |

**Built-in inspection:**

```stk
cacheview()                    # list all cached scripts with stats
cacheview("pattern")           # filter by path pattern
cacheview("--count")           # just the count

cache_stats()                  # returns {count, bytes, path, enabled}
cache_exists("script.stk")     # 1 if cached, 0 if not
cache_clear()                  # wipe the cache
```

**Example output:**

```
$ stryke -e 'cacheview()'
stryke bytecode cache
  path: ~/.stryke/scripts.rkyv
  scripts: 103 (612.45 KB)

PATH                                                      PROG KB    BC KB
/Users/me/project/lib/heavy_module.stk                       8.57     19.48
/Users/me/project/bin/main.stk                               2.45      5.84
...
```

**Tuning:**

- `STRYKE_CACHE=0` — disable caching entirely
- Cache is enabled by default for file-based scripts
- Bypassed for `-e` / `-E` one-liners (overhead > benefit for tiny scripts)
- Bypassed for `-n` / `-p` / `--lint` / `--check` / `--ast` / `--fmt` / `--profile` modes

**Format:** rkyv-archived `ScriptShard { header, entries: HashMap<path, ScriptEntry> }`. Entries hold per-script `(mtime_secs, mtime_nsecs, binary_mtime_at_cache, cached_at_secs, program_blob, chunk_blob)`. Inner blobs use bincode for now (`StrykeValue`'s `Arc`-shared graph isn't trivially zero-copy archivable yet — phase 2 will derive `Archive` directly on `Chunk` / `Program` for full zero-copy load). Writes go through `flock` on `scripts.rkyv.lock` and atomic rename of a tmp file.

**Aligned with zshrs:** same rkyv shard pattern (`zshrs/src/daemon/shard.rs`) — `mmap` + `check_archived_root` + zero-copy `ArchivedHashMap` lookup. zshrs uses per-source-tree shards with a daemon; stryke uses a single global shard since scripts are individually invoked.

**Migration rationale:** see [`docs/CACHE_RKYV_MIGRATION.md`](docs/CACHE_RKYV_MIGRATION.md) for the full story — measured 11x speedup on the per-process workload (`s test t`), tradeoffs, and what's deferred to phase 2.

---

## [0x10] DISTRIBUTED `pmap_on` / `~d>` OVER SSH (`cluster`)

Distribute a `pmap`-style fan-out across many machines via SSH. The dispatcher spawns one persistent `stryke --remote-worker` process per slot, performs a HELLO + SESSION_INIT handshake **once** per slot, then streams JOB frames over the same stdin/stdout. Pairs perfectly with `stryke build`: ship one binary to N hosts, fan the workload across them.

```perl
# Build the worker pool. Each spec maps to one or more `ssh HOST STRYKE --remote-worker` lanes.
my $cluster = cluster([
    "build1:8",                          # 8 slots on build1, default `stryke` from PATH
    "alice@build2:16",                   # 16 slots, ssh as alice
    "build3:4:/usr/local/bin/stryke",        # 4 slots, custom remote stryke path
    { host => "data1", slots => 12, stryke => "/opt/stryke" },  # hashref form
    { timeout => 30, retries => 2, connect_timeout => 5 },  # trailing tunables
])

my @hashes = @big_files |> pmap_on $cluster { slurp_raw |> sha_256) }

# pflat_map_on for one-to-many mapping
my @lines = @log_paths |> pflat_map_on $cluster { split /\n/, slurp }
```

#### Distributed thread macro `~d>`

`~d>` is `~p>` (the parallel-chunk thread-first macro) but with each chunk shipped to a cluster worker instead of a local rayon thread. Same chunk-block surface — stages operate on `@_` (the chunk's elements) and the results merge in source order via the existing `pmap_on` dispatcher (one persistent ssh process per slot, JOB frames over a shared work queue, per-job retry budget).

```perl
my $cluster = cluster("build1:8", "build2:16")

# Distributed equivalent of `~p> @big_files map { sha_256(slurp_raw($_)) }`:
my @hashes = ~d> on $cluster @big_files map { sha_256(slurp_raw($_)) }

# Source-order is preserved even though chunks finish out of order on remote
# workers — the dispatcher tracks per-chunk seq numbers and merges by index.
my @doubled = ~d> on $cluster 1:1_000_000 map { $_ * 2 }
say "$doubled[0] .. $doubled[-1]"        # 2 .. 2000000
```

The `on $cluster` operand is required — no implicit default cluster in v1. Trailing `||>` / `|then|` boundary marker switches back to a sequential `~>` continuation operating on the auto-merged result, identical to `~p>`'s split-boundary semantics:

```perl
~d> on $cluster @urls map { fetch($_) } ||> uniq sort
```

For tests / debugging without an SSH host, `STRYKE_CLUSTER_LOCAL_BIN=/path/to/stryke` makes the cluster dispatcher spawn the worker locally instead of going through `ssh`. Slot `host` fields are ignored when this is set; useful for CI fixtures and single-machine end-to-end smoke tests.

#### Cluster syntax

Each list element to `cluster([...])` is one of:

| Form | Meaning |
|------|---------|
| `"host"` | One slot on `host`, remote `stryke` from `$PATH` |
| `"host:N"` | `N` slots on `host` |
| `"host:N:/path/to/stryke"` | `N` slots, custom remote `stryke` binary |
| `"user@host:N"` | `ssh` user override (kept verbatim, passed through to ssh) |
| `{ host => "...", slots => N, stryke => "..." }` | Hashref form with explicit fields |
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

Every message is `[u64 LE length][u8 kind][bincode payload]`. The single-byte `kind` discriminator lets future revisions extend the protocol without breaking older workers — an unknown kind is a hard error so version skew is loud. See [`strykelang/remote_wire.rs`](strykelang/remote_wire.rs).

```text
dispatcher                    worker
    │                            │
    │── HELLO ─────────────────►│   (proto version, build id)
    │◄───────────── HELLO_ACK ──│   (worker stryke version, hostname)
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

#### `stryke --remote-worker`

The worker subprocess. Reads a HELLO frame from stdin, parses subs prelude + block source from SESSION_INIT exactly once, then handles JOB frames in a loop until SHUTDOWN or stdin EOF. Started by the dispatcher via `ssh HOST FO_PATH --remote-worker`. Also reachable directly for local testing:

```sh
echo "..." | stryke --remote-worker      # reads framed wire protocol from stdin
stryke --remote-worker-v1                # legacy one-shot session for compat tests
```

#### Limitations (v1)

- **Unix only** — hardcoded `ssh`, hardcoded POSIX dlopen path. Windows would need a similar shim.
- **JSON-marshalled values** — `serde_json` round-trip loses bigints, blessed refs, and other heap-only `StrykeValue` payloads. The supported types are: undef, bool, i64, f64, string, array, hash. Anything outside that returns an error from `pmap_on`.
- **`mysync` / atomic capture is rejected** — shared state across remote workers can't honour the cross-process mutex semantics in v1. Use the result list and aggregate locally.
- **No streaming results** — the dispatcher buffers the full result vector before returning. For huge fan-outs this is the next thing to fix (likely via `pchannel` integration).
- **No SSH connection pool across calls** — each `pmap_on` invocation builds fresh sessions. Subsequent `pmap_on` calls in the same script reconnect from scratch.

---

## [0x10a] INFRASTRUCTURE LOAD TESTING

> *"The hottest language ever created. Literally."*

stryke is a **server farms first** language — the first programming language designed from the ground up for distributed infrastructure load testing. Not HTTP load testing. Not API benchmarks. **Bare metal heat.**

### Stress Testing Builtins

All stress functions pin **ALL cores to 100% TDP** simultaneously:

```stk
stress_cpu(10)           # 10 seconds, SHA256 across ALL cores
stress_mem(1e9)          # 1GB allocated + touched across ALL cores
stress_io("/tmp", 100)   # parallel file I/O across ALL cores
stress_test(60)          # combined CPU + memory + IO stress
heat(60)                 # 🔥 maximum thermal assault
```

### The `heat` Function

The hottest function in any programming language:

```stk
heat(60)
```

Output:
```
🔥 HEAT: Pinning MAX cores to 100% TDP for 60s (Ctrl-C to stop early)
🔥 HEAT: 3,116,320,000 hashes in 60.00s (51.9M/s)
```

### Measured Performance (Apple M3 Max)

| Function | Result | CPU Usage |
|----------|--------|-----------|
| `stress_cpu(3)` | 154M hashes | 1117% (all cores) |
| `stress_mem(1e9)` | 1GB touched | 452% (parallel) |
| `heat(60)` | 3.1B hashes | 1800% (max TDP) |

### What stryke Tests

This isn't application performance testing. This is **infrastructure validation**:

| Layer | What You Test |
|-------|---------------|
| **Cooling** | Can CRAC units handle sustained full load? |
| **Power** | PDU rated for 100% simultaneous draw? |
| **UPS/Generator** | Backup power actually works? |
| **Hardware** | Which blade has the failing fan? |
| **Ops** | Does the NOC notice? How fast? |

### Distributed Load Testing

Combine with `cluster` + `pmap_on` for fleet-wide stress:

```stk
my $c = cluster(["node1:16", "node2:16", "node3:16"])

# Pin 48 cores across 3 servers for 60 seconds
1:48 |> pmap_on $c { heat(60) }
```

Or use the built-in `stress_test` with cluster:

```stk
my $r = stress_test($c, 60)
p "Total hashes: $r->{cpu_hashes}"
p "Workers: $r->{workers}"
```

### Use Cases

- **BCP/DR exercises** — stress primary datacenter, validate failover
- **Capacity planning** — prove infrastructure handles peak load
- **Burn-in testing** — validate new hardware before production
- **Cooling validation** — find thermal limits before summer hits
- **Compliance** — demonstrate resilience for SOC 2, PCI DSS, FedRAMP

---

## [0x10b] AGENT/CONTROLLER ARCHITECTURE

stryke includes a complete distributed load testing system with persistent agents and interactive control.

### Controller (Master REPL)

```sh
stryke controller                    # listen on 0.0.0.0:9999
stryke controller --port 8888        # custom port
stryke controller --bind 10.0.0.1    # specific interface
```

**Commands:**

| Command | Description |
|---------|-------------|
| `status` | List connected agents with cores, memory, state |
| `fire [SECS]` | Start stress test on all agents (default: 10s) |
| `eval CODE` | Run arbitrary stryke source on **every** agent **in parallel**. Each agent parses & executes against its own persistent `VMHelper`, so `sub` definitions and `$main::name` globals carry across calls (lexical `my`/`our` are per-frame, like a Perl `-de0` session). Controller fans out via a two-pass loop — pass 1 writes all EVAL frames (kernel sends, no waiting), pass 2 collects each `EvalResult` in sorted name order. Wall time = max(per-agent latency), not sum. Output: `[agent/ok\|ERR] <result>` per agent, each output line tagged. 30s read timeout per agent. |
| `@CODE` | Shorthand for `eval CODE`. Any line in the REPL starting with `@` ships the rest as stryke source to every agent — `@1+1`, `@sub bump { ... } bump()`, `@$main::counter += 5` all work. Saves four keystrokes vs the explicit verb. |
| `terminate` | Stop stress test immediately |
| `shutdown` | Disconnect all agents and exit |

**`eval` example session:**

```text
stryke controller v0.14.30
> status
node-01    16   64GB         idle      120s
node-02    16   64GB         idle      118s
> @sub greet { "hello from " . $main::ENV{HOSTNAME} } greet()
[node-01/ok] hello from node-01
[node-02/ok] hello from node-02
> @$main::counter = 0
[node-01/ok] 0
[node-02/ok] 0
> @$main::counter += 5; $main::counter
[node-01/ok] 5
[node-02/ok] 5
> @p "a"; p "b"; p "c"; 0
[node-01/ok] a
[node-01/ok] b
[node-01/ok] c
[node-01/ok] 0
[node-02/ok] a
[node-02/ok] b
[node-02/ok] c
[node-02/ok] 0
> @die "boom"
[node-01/ERR] boom at -e line 1
[node-02/ERR] boom at -e line 1
> eval $main::counter             # explicit verb form still works
[node-01/ok] 5
[node-02/ok] 5
```

Wire-level: a new pair of frame kinds is added to the agent protocol — `EVAL` (controller → agent, payload = bincode `EvalCommand { code }`) and `EVAL_RESULT` (agent → controller, payload = `EvalResult { ok, output }`). `AGENT_PROTO_VERSION` is bumped to 2 so a v1 agent refuses the handshake against a v2 controller rather than silently hanging on an unrecognised frame kind.

### Builtins: `controller(...)` and `agent(...)` — go into either mode from a script

The CLI subcommands `stryke controller` and `stryke agent` are mirrored by the **`controller`** and **`agent`** builtins so any `.stk` script can drop itself into either mode without going through the CLI dispatch. Both block (the controller on its REPL stdin loop, the agent on its frame loop) and return the exit code as an integer (`0` clean shutdown, `1` bind / connect / handshake failure).

```perl
# examples/agent_become.stk — script equivalent of `stryke agent`
my $addr = $ENV{CONTROLLER_ADDR} // "localhost:9999"
my $name = $ENV{AGENT_NAME}      // $ENV{HOSTNAME} // "anonymous"
exit agent($addr, $name)

# examples/controller_with_local_agent.stk — controller + local agent in one process
spawn {
    sleep 1                          # let controller bind
    agent("localhost:9999", "local-worker")
}
exit controller("127.0.0.1", 9999)   # blocks on REPL
```

**Signatures**:

| Builtin | Args | Defaults |
|---|---|---|
| `controller(bind?, port?)` | `bind`: bind address, `port`: TCP port | `bind="0.0.0.0"`, `port=9999` |
| `agent(addr?, name?)` | `addr`: `host` or `host:port`, `name`: display label | `addr="localhost:9999"`, `name=hostname` |

Wrap either in `spawn { ... }` to run in the background while the calling script continues. Useful for in-process integration tests and local REPL development against a real agent without bringing up a second machine.

## [0x10c] SCRIPTABLE DISTRIBUTED COMPUTE — `congregation` / `pray` / `annex`

The controller/agent REPL at [0x10b] is for human-typed interactive load testing. Scripts that need programmatic distributed compute — scatter work to N workers, gather results, manage worker state — use the **28-verb religious-themed API** layered on top of the same TCP+bincode controller infrastructure. Each verb is a stryke builtin; the whole pipeline lives in a `.stk` file with no REPL, no manual orchestration, no external infrastructure.

### Minimum-viable use

```perl
my @workers = congregation(4);                # fork 4 agents locally, wait for them to register
my $div     = pray "compute()", @workers;     # scatter EVAL frames in parallel, return divination
my %results = annex $div;                     # block-and-gather hash keyed by session-id
excommunicate(@workers);                      # clean shutdown
```

`pray` accepts a string OR a coderef (closure body is deparsed and shipped — closure captures not supported in v1):

```perl
my $div = pray sub { 2 + $_ }, @workers;       # coderef form
my $div = pray "2 + 3", @workers;              # string form
```

### Full verb taxonomy

| Verb | Side | Effect |
|---|---|---|
| **Lifecycle** | | |
| `congregation($n)` | master | fork N stryke agent children, auto-register them, return handle array |
| `anoint($n)` | master | like `congregation` but doesn't take over as current controller (multi-cong scripts) |
| `ordain([$name, [$bind, [$port]]])` | master | bare controller (no agents), register name in cathedral for remote `profess` |
| `muster([$controller_id])` | master | enumerate currently-connected agent handles |
| `welcome($n [, $timeout_ms])` | master | block until N agents have joined |
| `excommunicate(@handles)` | master | SHUTDOWN frame to subset, drop from roster |
| `bow()` | slave | enter agent receive loop (alias for `agent()`) |
| `profess($name)` | slave | look up congregation in cathedral, connect as agent |
| `apostatize($name)` | local | unregister congregation from cathedral |
| `cathedral()` | local | enumerate registered congregation names |
| **Scatter / gather** | | |
| `pray($code, @handles)` | master | scatter, return divination id (closure or string) |
| `annex($div [, $timeout_ms])` | master | block-and-gather, consume divination, return hash |
| `harvest($code, @handles [, $timeout_ms])` | master | fused `pray + annex` one-shot |
| `chant($code, @handles)` | master | continuous rescatter — fires at current AND future joiners; returns chant_id |
| `amen($id)` | master | release pending divination OR stop active chant |
| `pilgrimage($code, @handles [, $timeout_ms])` | master | scatter+gather barrier; returns 1 if all rendezvous, 0 otherwise |
| **State inspection** | | |
| `lick(@handles)` | master | non-destructive snapshot of every worker's `%soul` via `to_json(\%soul)` |
| `peruse(@handles)` | master | deep `%soul` walk (Tier 3 alias of lick; god-style traversal in Tier 5) |
| `interrogate($pid_or_handles)` | master | polymorphic dump — `$pid` (OS process via sysinfo) OR `@handles` (agent VM state) |
| **State mutation** | | |
| `bestow(\%hash, @handles)` | master | push hash to each worker's `%gift` via JSON round-trip |
| `smite(@handles)` | master | reset workers' `%soul` and `%gift` without disconnecting |
| `recant(@keys)` | slave | partial self-erasure of own `%soul` |
| **Persistence** | | |
| `enshrine(\%hash, $path)` | local | write hash to disk as JSON |
| `exhume($path)` | local | read enshrined JSON back as hash |
| `smother(\%hash)` | local | securely zero a local hash (overwrite + clear) |
| `martyr($path)` | slave | exit(0) after caller has enshrined |
| `resurrect($enshrine_path)` | master | exhume + anoint(1) + bestow → new agent with restored state |
| **Security** | | |
| `cloister($token)` | master | toggle :cloistered mode — agents must send `STRYKE_AGENT_TOKEN` matching `$token` |
| `divine($handler)` | slave | register closure handler for incoming petitions (Tier 5 wires dispatch through it) |

### Worker-side `%soul` convention

Workers maintain `our %soul` as their externally-visible state. `lick`/`peruse`/`annex` serialize it; `smite` clears it; `bestow` populates a peer hash `our %gift` for pushed data:

```perl
# Slave script (run on each compute node):
profess "renderfarm";
bow {                                          # enter receive loop
    our %soul;
    our %gift;
    our $frames_rendered = 0;
};
# Master can later: lick(@workers) sees the soul state; smite(@workers) resets it
```

### `:cloistered` ACL

Open by default. To restrict membership to anointed-only workers, the master calls `cloister($token)` and workers set `STRYKE_AGENT_TOKEN` matching that token before calling `agent`/`bow`/`profess`. Wire-level: a new `AGENT_AUTH` frame (`frame_kind::AGENT_AUTH = 0x1B`) is sent post-HELLO when the env var is set; cloistered controllers reject any agent without a valid token within 500 ms.

### Continuous rescatter via `chant`

`chant` is the fire-and-forget cousin of `pray` — it registers an ongoing prayer that fires at every current agent AND at every new agent that joins later (via the accept-loop hook). Use for state distribution (`bestow`-like config push to current + future workers):

```perl
my $vigil = chant("our %config = (max_depth => 8); 'ok'", @workers);
# ... new workers can join via profess(); they auto-receive the chant on join ...
amen($vigil);                                  # stop the rescatter
```

### Cathedral (in-process v1)

`ordain($name, ...)` registers `name → master_endpoint` in an in-process registry. `profess($name)` resolves the endpoint and connects. Tier 4 ships in-process only — Tier 5 promotes the cathedral to a standalone `stryked` daemon for cross-host name resolution.

```perl
# Master:
ordain "renderfarm", "0.0.0.0", 9999;
welcome 4, 30_000;                             # wait for 4 slaves
my %frames = harvest "render_frame()", muster();

# Slave (in any process, including a forked child):
profess "renderfarm";                          # blocks in agent loop
```

### Live demos

13 demos under `examples/`, all CI-safe (loopback fork only, no network) and clean under `--no-interop`:

| Demo | What it shows |
|---|---|
| `distributed_congregation.stk` | Tier 0 minimum-viable scatter-gather (2 workers, 1 prayer) |
| `congregation_100x_scale.stk` | Large-fleet scatter-gather; tested clean at **100 and 250 workers** |
| `distributed_prime_sieve.stk` | Real CPU work — shard 0..10000 across N workers, verify π(10000)=1229 |
| `distributed_wordcount.stk` | MapReduce shape — every worker counts words, master verifies agreement |
| `distributed_log_aggregation.stk` | Telemetry pattern — per-event JSON counts, master sums fleet-wide |
| `harvest_oneshot.stk` | The ergonomic shape (`harvest $code, @workers` = `pray + annex` fused) |
| `bestow_then_lick.stk` | Master pushes config via `bestow`, reads worker state back via `lick` |
| `pilgrimage_barrier.stk` | 3-stage BSP barrier across 4 workers |
| `chant_late_joiners.stk` | `chant`/`amen` continuous-rescatter lifecycle |
| `smite_state_reset.stk` | Reset workers' `%soul` without disconnecting (vs `excommunicate`) |
| `enshrine_exhume_roundtrip.stk` | Persist hash to disk as JSON, exhume back, verify identity |
| `cloistered_acl_demo.stk` | `:cloistered` ACL + cathedral inspection + `apostatize` cleanup |
| `interrogate_pids.stk` | Polymorphic `interrogate($pid)` (OS) vs `interrogate(@handles)` (agent VM state) |
| `multi_congregation.stk` | `anoint` spawns secondary congregation alongside primary |

Sample run (the original Tier 0 demo):

```text
$ stryke examples/distributed_congregation.stk 2 "5 * 8"
── distributed_congregation: N=2 code=5 * 8 ──
spawned 2 workers: 1,2
divination id: 1
  worker 1 → 40
  worker 2 → 40
✓ all 2 workers returned: 40
```

100-worker scale demo:

```text
$ stryke examples/congregation_100x_scale.stk 100
── 100x scale: spawning 100 worker processes ──
spawned 100 / 100 workers (fork + connect + register)
harvested 100 / 100 replies
  matched:  100 / 100 replies
  diverged: 0
✓ every spawned worker returned the correct answer
excommunicated 100 workers
```

### Project-bar position

Scriptable in-language single-keyword scatter-gather + distributed state harvest + secure-erase as a language primitive does not exist anywhere as of 2026-05-27. MPI is a C library requiring `mpirun`; Erlang has process groups but no destructive harvest or peek/commit pair; Spark/Hadoop require cluster bootstrap and JVM; nothing in shell-language space comes close. The `lick`/`annex`/`smother` triple and the `chant`/`amen` continuous-rescatter pair are genuinely empty territory — world-first language-keyword primitives, not yet-another-rewrite of an existing framework.

See [`docs/killer-features-brainstorm.md`](docs/killer-features-brainstorm.md) "Scriptable Master/Slave" for the full design rationale, shipped/deferred status, and the two stryke language bugs fixed during this work (`\%our-hash` ref deref + `our %hash` cross-EVAL persistence).

### Builtins: `mark(...)` / `provenance(...)` / `unmark(...)` — value lineage as a first-class feature

**World-first** for scripting languages: no major scripting language (Perl, Python, Ruby, JavaScript, Lua, PHP) ships automatic value-lineage tracking as a first-class builtin. Closest analogs are research dataflow languages (LIO, Adapton) which surface lineage as a type-system feature, never as a plain `provenance($x)` call. Stryke ships the trio as a normal dispatch builtin with O(1) HashMap-keyed lookup and zero overhead until any value is marked.

```perl
my $config = mark({ host => "prod", retries => 3 })       # tag the Arc
my $p      = provenance($config)
# { origin => "HASH entries=2", origin_line => 1, ops => [] }
unmark($config)                                            # reclaim ledger entry
```

| Builtin | Signature | Returns |
|---|---|---|
| `mark($val)` | tag a heap value's Arc; idempotent | `$val` unchanged (pipeline-friendly) |
| `provenance($val)` | look up `$val`'s lineage | `{ origin, origin_line, ops => [...] }` or `undef` |
| `unmark($val)` | drop the ledger entry | `$val` unchanged |

**Cost model**: zero when unused. A process-global `LEDGER_ACTIVE` `AtomicBool` flips to `true` on the first `mark(...)` call. Until then every dispatch's post-hook elides via a single inlined `load(Relaxed)` branch.

**v1 scope** (intentional — documented in `strykelang/provenance.rs` and the LSP hover doc):
- Tracks builtin-call op chains only. User-sub call boundaries aren't recorded yet.
- Keys on heap Arc pointer — two structurally-equal values with different origins have independent lineages; two refs to the SAME Arc share lineage (the aliasing model `god` already uses).
- Immediates (integers, floats, undef) have no Arc — `provenance` on them returns `undef`. Wrap in a hashref if you need to track a scalar.
- String results from builtins (`to_json`, `sha256`, `base64_*`, ...) lose Arc identity in the VM's scalar-return path. Workaround: wrap the string in a one-key hashref so the container's stable Arc carries the lineage. Heap-container results (arrays, hashes, atomics, sets, deques, byte buffers) propagate correctly.
- Ledger entries persist until `unmark($val)`. A v2 sweep based on Arc weak refs is on the roadmap.

Demo: [`examples/provenance_basics.stk`](examples/provenance_basics.stk) walks through each shape end-to-end.

### Builtins: `kick(...)` / `udp_send(...)` — TCP knock + UDP multi-shot

Convenience builtins over standard socket calls. Both capabilities exist in every language's stdlib; stryke ships them as bare builtins so service probes / Wake-on-LAN scripts / NAT keepalives don't need a socket import. (`punch` — full NAT hole-punching with STUN discovery for peer-to-peer UDP between stryke instances behind NAT — lands in a follow-on commit.)

```perl
# TCP service-health sweep — 250 ms per probe, returns 1 / 0.
for my $pair (([ "db",  5432 ], [ "redis", 6379 ], [ "api", 8080 ])) {
    printf "  %-6s :%d  %s\n", $pair->[0], $pair->[1],
        kick($pair->[0], $pair->[1], 250) ? "UP" : "down"
}

# Wake-on-LAN magic packet, 3× for reliability.
my @mac_bytes = map { hex($_) } split /:/, "aa:bb:cc:dd:ee:ff"
my @packet = (0xff) x 6
push @packet, @mac_bytes for 1:16
udp_send("255.255.255.255", 9, pack("C*", @packet), 3)
```

| Builtin | Signature | Returns |
|---|---|---|
| `kick($host, $port [, $timeout_ms])` | TCP connect with timeout (default 1000 ms) | `1` on success, `0` on any failure |
| `udp_send($host, $port, $payload [, $retries=1, $interval_ms=20])` | bind ephemeral, `sendto` the payload N times | count of successful sends |

Both fail soft — bad host, closed port, DNS failure, invalid port all return 0 without raising, so callers can write `if (kick(...))` and `if (udp_send(...))` idiomatically.

Demo: [`examples/kick_probe.stk`](examples/kick_probe.stk).

### Builtins: `udp_open` / `stun` / `punch` / `udp_send_to` / `udp_recv` / `udp_close` — P2P over the open internet

Stryke-to-stryke communication between two hosts behind arbitrary NATs, no infrastructure required. STUN client + UDP hole-punching state machine, both rolled in-tree (~500 lines of Rust, no third-party network crate dependency). Bytecode-VM-direct dispatch — same path as the rest of the builtins.

```perl
# Side A (any host, runs first):
my $sock = udp_open()
my $info = stun($sock)                       # → { public_ip, public_port }
printf "Tell peer to: stryke peer.stk %s %d\n",
    $info->{public_ip}, $info->{public_port}
my $first = udp_recv($sock, 60_000)          # wait for peer's bombards
p "Peer connected: $first"
udp_close($sock)

# Side B (peer host):
my $sock = udp_open()
my $r = punch($sock, $peer_ip, $peer_port, { payload => "hello!" })
if ($r->{established}) {
    printf "Connected in %d bombards (%dms)\n", $r->{bombards}, $r->{latency_ms}
    p "Peer replied: $r->{peer_msg}"
}
udp_close($sock)
```

| Builtin | Signature | Returns |
|---|---|---|
| `udp_open([$bind_host, $bind_port])` | bind a UDP socket, register in pool | integer handle (0 on bind failure) |
| `udp_send_to($id, $host, $port, $payload)` | send via pool socket | bytes sent (0 on failure) |
| `udp_recv($id [, $timeout_ms=1000])` | receive one datagram (payload only) | payload string / bytes / undef on timeout |
| `udp_recv_from($id [, $timeout_ms=1000])` | receive one datagram + source address | `{ payload, src_ip, src_port }` or undef |
| `udp_close($id)` | release socket from pool | 1 if present, 0 if unknown |
| `stun($id [, $stun_host, $stun_port, $timeout_ms])` | query STUN server via socket | `{ public_ip, public_port }` or undef |
| `stun_classify($id [, $opts])` | query multiple STUN servers, detect symmetric NAT | `{ nat_type, public_ip, queried, succeeded, observations }` |
| `punch($id, $peer_ip, $peer_port [, $opts])` | hole-punching state machine | `{ established, latency_ms, bombards, peer_msg, peer_addr }` |

**Protocol details**: RFC 8489 STUN Binding Request with XOR-MAPPED-ADDRESS parsing (modern form) + MAPPED-ADDRESS fallback (legacy servers). Default STUN server is `stun.l.google.com:19302` — Google's public, free, reliable. The same socket handle MUST flow through STUN → punch → application traffic because the NAT mapping is tied to the socket's `(local_ip, local_port)`.

**Real-world success rate is ~70-80%**. Failures (no language-level workaround):
- **Symmetric NATs** — some mobile carriers, some corporate firewalls assign a different public port per destination, so the port revealed by STUN ≠ the port the peer's traffic arrives on.
- **UDP-blocking firewalls** — drop all UDP outright.
- **Timing misalignment** — if one peer punches 30s before the other, the first peer's NAT mapping may time out before the second peer starts.

For guaranteed delivery use the **TURN relay** builtins below (`turn_allocate`, `turn_permission`, `turn_send`, `turn_recv`, `turn_refresh`) — relay every packet through a public coturn server, works regardless of NAT type.

Demo: [`examples/p2p_chat.stk`](examples/p2p_chat.stk) — runnable two-process flow.

### Builtins: `turn_allocate` / `turn_permission` / `turn_send` / `turn_recv` / `turn_refresh` — TURN relay fallback (RFC 8656)

When `stun_classify` reports `symmetric` (~20-30% of real-world NAT scenarios — mobile carriers, some corporate firewalls), pure hole-punching cannot succeed. TURN routes every packet through a public relay server you trust, working regardless of NAT type at the cost of one network hop and the bandwidth budget of whoever runs the TURN server.

RFC 8656 client implemented in-tree (~600 lines, no third-party network crate — reuses the existing `hmac` + `sha1` + `md-5` from RustCrypto for MESSAGE-INTEGRITY auth). Builds on the STUN binary protocol from `nat_punch.rs`; TURN messages are STUN-formatted frames with different message types and attributes.

```perl
# Side A (the listener):
my $sock = udp_open()
my $alloc = turn_allocate($sock, "turn.example.com", 3478, "alice", "hunter2")
printf "Tell peer my relay is: %s:%d\n", $alloc->{relay_ip}, $alloc->{relay_port}
while (defined(my $msg = turn_recv($sock, 0))) {
    p "from $msg->{peer_ip}:$msg->{peer_port}: $msg->{payload}"
    turn_send($sock, $msg->{peer_ip}, $msg->{peer_port}, "ack")
}

# Side B (the sender):
my $sock = udp_open()
my $alloc = turn_allocate($sock, "turn.example.com", 3478, "bob", "trustno1")
turn_permission($sock, $peer_relay_ip)
turn_send($sock, $peer_relay_ip, $peer_relay_port, "hello via turn!")
my $reply = turn_recv($sock, 5_000)
```

| Builtin | Signature | Returns |
|---|---|---|
| `turn_allocate($id, $server, $port, $user, $pass [, $timeout_ms])` | two-roundtrip auth + Allocate | `{ relay_ip, relay_port, lifetime_secs, realm, nonce_len }` or undef |
| `turn_permission($id, $peer_ip [, $timeout_ms])` | install CreatePermission for peer | 1 / 0 |
| `turn_send($id, $peer_ip, $peer_port, $payload)` | wrap in SEND-INDICATION, ship via relay | bytes sent to TURN server / 0 |
| `turn_recv($id [, $timeout_ms])` | parse next DATA-INDICATION | `{ payload, peer_ip, peer_port }` or undef |
| `turn_refresh($id [, $lifetime_secs, $timeout_ms])` | extend allocation (or release with `lifetime_secs=0`) | new lifetime / 0 |

**Protocol scope** (RFC 8656 core, deliberately bounded):
- ✅ Allocate with long-term HMAC-SHA1 auth + nonce/realm exchange
- ✅ CreatePermission
- ✅ SendIndication / DataIndication
- ✅ Refresh
- ✅ IPv4 + IPv6 XOR address parsing
- ❌ ChannelBind / ChannelData — bandwidth optimization, not v1
- ❌ TLS/DTLS transport — plain UDP for v1
- ❌ SHA-256 / SHA-384 MESSAGE-INTEGRITY — coturn defaults to SHA1
- ❌ ALTERNATE-SERVER redirect handling

Verified end-to-end via an in-process mock TURN server (8 unit + 3 integration pins) — for real coturn interop you should test against your own server.

Demo: [`examples/turn_relay_chat.stk`](examples/turn_relay_chat.stk) — three modes (classify NAT type / listen for messages via relay / send via relay).

### Active probes — `tcp_probe` / `tcp_banner` / `whois_query`

Sibling to `kick` (which returns `1`/`0`) — pick by what you need:

| Builtin | Returns | When to use |
|---|---|---|
| [`kick($host, $port)`](#builtins-kick--udp_send--tcp-knock--udp-multi-shot) | `1` / `0` | Fast service-mesh sweep, don't care about latency |
| `tcp_probe($host, $port [, $timeout_ms])` | `{ alive, latency_ms }` | Same probe, but with the RTT measurement |
| `tcp_banner($host, $port [, $timeout_ms, $max_bytes])` | `{ alive, latency_ms, banner }` | Service fingerprint (SSH version, HTTP server header, SMTP greeting) |
| `whois_query($domain [, $server, $timeout_ms])` | response string or `undef` | Domain registration / IP-ownership investigation (RFC 3912) |

```perl
# SSH version sniff across a fleet.
for my $host (qw(web1 web2 db1)) {
    my $r = tcp_banner($host, 22, 500)
    printf "  %-8s %s\n", $host, $1 if $r->{alive} && $r->{banner} =~ /^SSH-(\S+)/
}

# Find the authoritative WHOIS server for a TLD, then chase the refer:
my $iana = whois_query("example.com")
my ($registry) = $iana =~ /^refer:\s+(\S+)/m
my $detail = whois_query("example.com", $registry // "whois.verisign-grs.com")
```

Demo: [`examples/network_recon.stk`](examples/network_recon.stk) chains `tcp_probe` + `tcp_banner` via `pmap` for parallel asset discovery + service fingerprinting.

### Builtins `teleport` / `arrive` — multi-target SHM IPC

Broadcast a value to N stryke processes via one POSIX shared-memory segment plus per-receiver Unix-domain-socket notification. For big payloads, only one process copies the bytes into kernel memory; each receiver `mmap`s the same backing pages.

**World-first**: no other scripting language ships single-call zero-copy(ish) multi-target value teleportation as a primitive. Closest analogs — Python `multiprocessing.shared_memory`, Perl `IPC::Shareable`, Ruby `mmap` — are single-target, require manual segment lifecycle + per-process attach plumbing, and don't bundle a notification primitive. `teleport` is one call.

| Builtin | Signature | Returns |
|---|---|---|
| `teleport` | `teleport($val, @receiver_pids [, { hold_ms => 500 }])` | count of receivers whose UDS accepted the notify |
| `arrive` | `arrive([$timeout_ms=5000])` | the teleported value (deeply ref-wrapped) or `undef` on timeout |

**Surfaces** — all four forms are supported:

```perl
# Standalone with literal PIDs:
teleport($payload, $pid1, $pid2, $pid3)

# Arrayref of PIDs:
teleport($payload, [@worker_pids])

# Opts hash for hold window:
teleport($payload, @pids, { hold_ms => 1500 })

# Prefix thread macro (data-first signature makes this natural):
~> $payload teleport(@worker_pids)
```

**Wire path**: `StrykeValue → serde_json::Value → JSON bytes → POSIX SHM segment named `/stryke_tp_PID_SEQ` → 40-byte notify over UDS at `/tmp/stryke_teleport_PID.sock` per receiver`. Receivers reverse this. Same lossy-but-portable shape as `cluster` / `dist_thread` IPC — scalars, arrays, hashes, and nested combos round-trip; closures and blessed objects don't.

**Top-level + nested containers always arrive as refs** (matching `decode_json`'s convention) so `$msg->{key}->[i]->{inner}` Just Works without re-wrapping.

**Wire format limits** (v1, intentional):

- Receivers must be stryke processes running an `arrive()` loop. Non-stryke processes or stryke processes without `arrive()` have no bound UDS and count as unreachable.
- macOS POSIX SHM names cap at 30 chars; `/stryke_tp_99999_99` is 19 chars so we have headroom for billions of segments per PID.
- Sender holds the SHM segment alive for `hold_ms` (default 500), not ack-based. Receivers slower than `hold_ms` get a stale-name `shm_open` failure.
- No encryption — receivers are cooperating processes; payload is visible to anything that knows the SHM name.

**Fork-safety**: the receiver's UDS socket is PID-aware. After `fork()`, the child rebinds at its own `/tmp/stryke_teleport_PID.sock` on first `arrive()` call (the parent's inherited fd is dropped). So fork-then-arrive in the child Just Works without manual reinitialization.

Demo: [`examples/teleport_broadcast.stk`](examples/teleport_broadcast.stk) — parent forks N workers, builds a sharded corpus hashref, teleports the whole thing once; each worker reconstructs and processes its slice.

### Common pitfalls — friction points worth knowing upfront

These are real bumps I hit while building the NAT-traversal stack — surfacing them so you don't waste time on the same investigations.

| Pitfall | Symptom | What to do instead |
|---|---|---|
| **Postfix `for` after `printf`/`print`** | `printf "..." for @arr` fails with `Expected LParen, got ArrayVar(...)` | Use explicit block form: `for my $x (@arr) { printf "...", $x }` |
| **Postfix `if` after `printf`** | Same parse failure as above | Same fix — wrap in explicit `if (...) { ... }` block |
| **`$tx->clone` on pchannel** | "Can't call method on non-object" | Multi-producer channels aren't supported; use a different shape (multiple receivers, or one producer fanning to N consumers) |
| **String return → provenance lost** | `mark({...}); my $j = to_json(...); provenance($j) → undef` | Wrap the string in a one-key hashref: `mark({ payload => to_json(...) })`. VM re-Arcs scalar string returns, breaking ptr-keyed lookup. Document in [provenance v1 limits](strykelang/provenance.rs#L40). |
| **`grep { Pkg::fn }` doesn't auto-bind `$_`** | All elements pass / fail uniformly (predicate sees `$path = undef`) | Pass explicitly: `grep { Pkg::fn($_) } @list` |
| **`fn($a, @b, @c)` slurps** | Second `@arr` param always empty; first `@arr` contains both | Pass arrayrefs + deref inside: `fn ... ($a, $b, $c) { my @b = @$b; my @c = @$c; ... }` |
| **`mark(\@arr)` vs `mark([...])`** | `\@arr` produces a fresh SCALARREF Arc per access → provenance lookup misses | Use anonymous arrayrefs (`mark([10, 20])`) or hashrefs — they have stable Arc identity. `\@arr` operator semantics are subtle |
| **`pack "a*"` for variable-length string** | `pack: 'A' and 'a' do not support '*'` | Concat: `pack("a4 n", $magic, $len) . $payload` — stryke's pack is stricter than Perl's |
| **`par { Pkg::fn }` is chunked, not 1:1** | Result count = worker thread count, not input count | Use `pmap { Pkg::fn($_) } @list` for 1-result-per-input. `par` runs BLOCK once per chunk with `_` = whole chunk list |
| **Tests that bind+drop a port then probe it** | Flaky under parallel-test load — another test grabs the freed port between drop and probe | Use port 1 (privileged, never auto-assigned) for "guaranteed closed" probes; or hold the listener for the test duration |
| **Test file reads with CWD-relative paths** | Pass when `cargo test` is run from repo root, NotFound when run from elsewhere (IDE runner, `cargo test --manifest-path ...`) | Build absolute paths via `env!("CARGO_MANIFEST_DIR")`. See [tests/suite/examples_strict_lint.rs](tests/suite/examples_strict_lint.rs) for the pattern |

### NAT traversal — quick reference

Compact decision sheet. Each rung has a different cost / success / failure profile; pick by `stun_classify` outcome + your tolerance for infrastructure.

**Rungs ranked by cost (low → high) and success rate (network-dependent):**

| Rung | Cost | Success rate | Fails when | Use when |
|---|---|---|---|---|
| 1. host (direct UDP) | 0 (just `udp_send_to`) | LAN: 100% · public IP: 100% · NAT: 0% | both peers behind any NAT | LAN or one-side-public-IP |
| 2. server-reflexive (`punch`) | 1 STUN RTT + ~5-30 hole-punch bombards | cone NAT: ~95% · symmetric NAT: 0% · UDP-blocked: 0% | symmetric NAT or UDP-blocked firewall | `stun_classify` returned `cone` |
| 3. relayed (`turn_*`) | TURN allocation + 1 hop per packet | ~100% (assuming TURN reachable) | TURN server unreachable / unauthenticated | symmetric NAT, UDP-blocked firewall, OR cone-NAT fallback when rung 2 fails |

**Decision tree** (codifies the same logic `ice::connect` automates):

```
                  stun_classify($sock)
                          ↓
        ┌─────────────────┼─────────────────┐
        ↓                 ↓                 ↓
     "cone"           "symmetric"        "unknown"
        ↓                 ↓                 ↓
   try host first      skip punch       try host, then
   then punch          go straight      punch, then
                       to turn          turn (probe-best)
```

**Per-step latency** (rough, on a normal home connection):

| Operation | Typical wall time |
|---|---|
| `udp_open` | < 1 ms |
| `stun_classify` (3 servers in parallel) | ~30-80 ms (slowest STUN RTT + handshake) |
| `stun` (single server) | ~10-50 ms |
| `punch` (cone-to-cone) | 100 ms - 5 s (timeout governs) |
| `turn_allocate` (incl. 401 + auth retry) | ~40-100 ms |
| `turn_send` / `turn_recv` data path | +5-30 ms vs direct (TURN relay hop) |

**What can go wrong + what to do**:

| Symptom | Likely cause | Fix |
|---|---|---|
| `stun_classify` returns `"unknown"` | < 2 STUN servers reached | Check internet, add more servers in `$opts->{servers}` |
| `punch` returns `established=0` after timeout | One side behind symmetric NAT, OR misaligned bombard timing | Fall back to TURN; or have peers retry simultaneously |
| `turn_allocate` returns undef | Bad credentials, server unreachable, or non-RFC-8656 server | Test with `examples/turn_health_check.stk` against your server |
| Data arrives but `udp_recv_from` returns wrong `src_ip` | NAT rewrote the source — normal | Use the address as-is; that's the public address the peer's NAT mapped to |

Demos showing each shape: [`p2p_chat.stk`](examples/p2p_chat.stk) (raw primitives) · [`p2p_chat_v2.stk`](examples/p2p_chat_v2.stk) (`ice::connect`) · [`turn_relay_chat.stk`](examples/turn_relay_chat.stk) (TURN path) · [`turn_health_check.stk`](examples/turn_health_check.stk) (TURN server probe) · [`port_scanner.stk`](examples/port_scanner.stk) (`kick`+`pmap`) · [`network_introspect.stk`](examples/network_introspect.stk) (reflection-driven builtin discovery)

### P2P walkthrough: pick the right rung for YOUR network

Decision tree for stryke-to-stryke P2P over the open internet — what to call, in what order, what each return value means:

```perl
my $sock = udp_open()                          # 1. Bind a UDP socket once
                                                #    (used for STUN + the data path)

my $nat = stun_classify($sock)                 # 2. Detect NAT type (~3-server query)
if ($nat->{nat_type} eq "symmetric") {         #    Symmetric → hole-punching can't work
    die "need TURN — see turn_allocate below"  #    (room for a TURN fallback here)
}

my $me = stun($sock)                           # 3. Discover OWN public address
printf "my address: %s:%d — send this to peer\n",
    $me->{public_ip}, $me->{public_port}
# ... exchange addresses with peer via email/paste/IRC/anything ...
my $peer_ip   = "203.0.113.45"
my $peer_port = 51234

my $r = punch($sock, $peer_ip, $peer_port,     # 4. Hole-punch the peer
    { timeout_ms => 5000 })
if (!$r->{established}) {
    die "punch failed — peer NAT too restrictive; need TURN"
}
printf "connected: %s\n", $r->{peer_msg}      # 5. Bidirectional flow established

# Now send/recv as normal UDP via the same socket:
udp_send_to($sock, $peer_ip, $peer_port, "hello")
my $reply = udp_recv_from($sock, 5000)        # → { payload, src_ip, src_port }
p "peer said: $reply->{payload}"
udp_close($sock)
```

| If `stun_classify` returns | Then you can | Otherwise |
|---|---|---|
| `cone` | use `punch` directly (~70-80% success) | — |
| `symmetric` | skip `punch` — it WILL fail | use `turn_allocate` + `turn_send` / `turn_recv` |
| `unknown` | try `punch` first, fall back to TURN on failure | — |

The orchestrator at [`examples/ice_orchestrator.stk`](examples/ice_orchestrator.stk) automates this whole ladder — see the next subsection. The two runnable demos pair like this:

- [`examples/p2p_chat.stk`](examples/p2p_chat.stk) — uses raw primitives (`udp_open`, `stun`, `punch`, `udp_send_to`, `udp_recv_from`). Read this first to understand what's happening on the wire.
- [`examples/p2p_chat_v2.stk`](examples/p2p_chat_v2.stk) — uses `ice::connect` from the orchestrator. Copy this into your own code.

### ICE-lite: orchestrating direct → punch → relay in stryke source

The v1.3 primitives (`udp_open` / `stun` / `stun_classify` / `punch` / `turn_*`) compose into a complete NAT-traversal orchestrator. The orchestration algorithm — which transport to try first, when to fall back — is intentionally **stryke source** rather than a Rust builtin: the wire-protocol code is already builtins, and keeping the ladder logic as user-editable source means you can read it, adapt it, and inline custom rules without recompiling.

[`examples/ice_orchestrator.stk`](examples/ice_orchestrator.stk) implements the three-rung ladder per RFC 8445 §5 (the connectivity-check core):

| Rung | Transport | Works when |
|---|---|---|
| 1. host | `udp_send_to` direct to peer's host:port | peer has no NAT (public IP / same LAN) |
| 2. server-reflexive | `punch` using STUN-discovered address | both sides behind cone NAT (~70% of pairs) |
| 3. relayed | `turn_*` via TURN server | always (assuming TURN is reachable) |

```perl
require "examples/ice_orchestrator.stk"

my $conn = ice::connect({
    peer_host_addr   => "192.0.2.50:9000",      # try direct first
    peer_srflx_addr  => "203.0.113.45:51234",   # peer's STUN-discovered
    peer_relay_addr  => "198.51.100.99:49000",  # peer's TURN allocation
    turn_server      => "turn.example.com:3478",
    turn_user        => "alice",
    turn_pass        => "hunter2",
    timeout_ms       => 3000,
})
die "no transport: $conn->{reason}" unless $conn->{ok}
# $conn->{method} is "direct" | "punch" | "relay"; $conn->{socket} is the
# bound socket to use for subsequent send/recv (udp_send_to OR turn_send
# per method).
```

`ice::gather_candidates({ turn_server, turn_user, turn_pass })` gathers the local side's three candidates so you can publish them via signaling (email, paste, IRC, anything) for the peer to use.

What this ICE-lite does NOT cover (RFC 8445 proper):
- Full priority math + role tie-breakers (8445 §5.1.2 / §5.2)
- Regular vs aggressive nomination (§8)
- Conflict resolution when both peers think they're the controller (§7.1.3.3)
- TCP candidates (RFC 6544)
- Trickle ICE (RFC 8838)

These are useful in WebRTC where the orchestrator runs autonomously; for stryke's typical "two peers with manual signaling" case the three-rung ladder + first-success wins is enough and stays in ~200 lines of inspectable code.

### Agent (Worker Daemon)

```sh
stryke agent                              # use config file
stryke agent --controller 10.0.0.1        # connect to specific host
stryke agent --port 9999                  # specific port
```

**Config file:** `~/.config/stryke/agent.toml`

```toml
[controller]
host = "controller.example.com"
port = 9999

[limits]
max_temp = 85       # auto-terminate if CPU temp exceeds
max_duration = 3600 # max seconds per session

[agent]
name = "node-01"    # optional, defaults to hostname
```

### Example Session

```
$ stryke controller
stryke controller listening on 0.0.0.0:9999
[agent connected] node-01 (cores=64, session=1)
[agent connected] node-02 (cores=64, session=2)
[agent connected] node-03 (cores=64, session=3)

controller> status
AGENT                 CORES     MEMORY        STATE       UPTIME
------------------------------------------------------------
node-01                  64       256GB         idle         42s
node-02                  64       256GB         idle         38s
node-03                  64       256GB         idle         35s

Total: 3 agents, 192 cores, 0 firing

controller> fire 60
[fire] 3 agents, duration=60s

controller> terminate
[terminate] 3 agents
```

### Wire Protocol

Framed binary protocol over TCP:

```
CONTROLLER                    AGENT
    │                           │
    │◄──── AGENT_HELLO ─────────│  (hostname, cores, memory)
    │───── AGENT_HELLO_ACK ────►│  (session_id)
    │                           │
    │───── FIRE ───────────────►│  (workload, duration)
    │◄──── METRICS ─────────────│  (cpu%, hashes/sec)
    │───── TERMINATE ──────────►│
    │◄──── TERM_ACK ────────────│  (final stats)
```

### Deployment

**Single binary, zero dependencies:**

```sh
# Build self-contained agent binary
stryke build agent.stk -o stryke-agent

# Deploy to any Linux server
scp stryke-agent node1:/usr/local/bin/
ssh node1 'stryke-agent --controller controller:9999'
```

**Kubernetes DaemonSet:**

```yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: stryke-agent
spec:
  template:
    spec:
      containers:
      - name: agent
        image: ghcr.io/menketechnologies/stryke:latest
        args: ["agent", "--controller", "stryke-controller:9999"]
```

### Why This Matters

| Other Tools | stryke |
|-------------|--------|
| External load generators | Agents inside cluster |
| Config files, YAML, XML | `fire 60` — two words |
| Batch jobs, wait for results | Interactive REPL |
| Install runtime on every node | Single binary, no deps |
| Test application performance | Test infrastructure: cooling, power, fabric |

**stryke is the ultimate load testing tool for distributed computing clusters.**

---

## [0x11] LANGUAGE SERVER (`stryke lsp`)

`stryke lsp` (or `stryke --lsp`) runs an LSP server over stdio. Hooks into the existing parser, lexer, and symbol table — no separate analyzer to maintain. Surfaces:

- **Diagnostics** on every keystroke. Strict-vars is **on by default** in the IDE (CLI `stryke check` stays lenient — gated on explicit `use strict;`), so undefined `$scalar` / `@array` / `%hash` typos surface inline without per-file opt-in. Skipped: bare sigil-vars inside double-quoted string interpolations (`"got $fh ..."` is template/description text — no false positives on test descriptions), Perl `$^X`/`$^O`/`$^V`/`$^W` family and bare `$$` (process id), AOP advice-body context vars (`$INTERCEPT_NAME`, `@INTERCEPT_ARGS`, `$INTERCEPT_RESULT`, `$INTERCEPT_MS`, `$INTERCEPT_US`), the full block-param grammar (`_`, `_N`, `_<`, `_<N`, `_<<<<<`, `_N<<<<<`, `_N<M`), `$#arr` last-index-of-array references resolved via `@arr`, `open(my $fh, …)` lexical filehandle declarations, `exists(&Pkg::sub)` introspection, and parser-internal `_thread_par_run` desugaring targets emitted by `~p>` / `~s>` thread macros.
- **OOP-aware checks**. Constructor calls (`Class(field => v, …)` and `Class->new(...)`) validate keys against declared fields, walking parent classes via `extends` and implemented traits via `impl` for inherited fields. Positional constructor calls (`Task(1, "title", Priority::High)`) are auto-detected and pass through unchanged. `$self->method` / `$obj->method` checks walk the same `extends + impl` chain, plus a universal-method whitelist (`isa`, `can`, `DOES`, `does`, `VERSION`, `new`, `BUILD`, `DESTROY`, `clone`, `with`, `to_hash`, `to_hash_rec`, `to_hash_deep`, `fields`, `methods`, `superclass`). Match-arm enum-variant typos (`Sig::Term2 =>` when `Sig` has no `Term2`) flag with the available-variant list.
- **Cross-file require resolution**. `require "./lib/Foo/Bar.stk"` walks up from the source file looking for the project root (any ancestor with a sibling `lib/` directory) — the classic Perl/CPAN layout. Subs, classes, traits, enums, constants declared in required files all join the active completion + diagnostics index.
- **Hover docs** for builtins (`pmap`, `cluster`, `fetch_json`, `dataframe`, …) — including the parallel and cluster primitives from sections [\[0x03\]](#0x03-parallel-primitives) and [\[0x10\]](#0x10-distributed-pmap_on--d-over-ssh-cluster), every Perl special variable (`$!`, `$@`, `$_`, `@_`, `@ARGV`, `%ENV`, `$1`..`$9`, `$^A`..`$^X`, `@+`/`@-`, …), the `__NAME__` compile-time tokens (`__END__`, `__DATA__`, `__FILE__`, `__LINE__`, `__PACKAGE__`, `__SUB__`), every phase block (`BEGIN`/`UNITCHECK`/`CHECK`/`INIT`/`END`/`BUILD`), and the reflection-hash short aliases (`%a`/`%b`/`%c`/`%d`/`%e`/`%k`/`%o`/`%p`/`%pc`/`%v`, `%parameters`, `%limits`, `%term`). Hash-returning builtins like `pool_info()` ship full key tables in their hover doc. Hover is suppressed inside string literals (`"length"` doesn't pop the `length` builtin) but still fires inside `#{ EXPR }` interpolations.
- **Completion** covering every identifier category the editor expects to see:
  - Sigil variables (`$scalar`, `@array`, `%hash`) — declared via `my` / `our` / `state` / `local` / `mysync` / `oursync`, `foreach my $x`, sub signature params, `open(my $fh, …)` filehandle decls
  - Subs — bare and qualified, with **suffix-only `insertText`** for qualified completions (typing `Demo::│` produces `Demo::handle`, not `Demo::Demo::handle`)
  - User-declared **Types** — classes (`CompletionItemKind::CLASS`), structs (STRUCT), enums (ENUM), traits (INTERFACE), plus every enum variant as a qualified `EnumName::Variant`
  - Constants from `use constant NAME => …` and the `use constant { A => 1, B => 2 }` hash form
  - **Loop labels** for `last LOOP` / `next LOOP` / `redo LOOP` references
  - **Hash-key completion driven by builtin return schemas** — `my $info = pool_info(); $info->{<tab>}` lists the actual keys `pool_info` returns (`cpus`, `rayon_threads`, `arch`, `os`, `perf_cores`, `eff_cores`). Same registry covers `par_bench`, `stress_test`, `cache_stats`, `uname`, `audio_info`, `id3_read`, `git_log`, `git_show`, `git_status`, `git_branches`, `git_blame`, `git_authors`, `du_tree`, `process_list`, `net_interfaces`, `perfview`, `mounts`, `html_parse`, `css_select`, `xml_parse`, `xpath`. `foreach my $row (git_log()) { $row->{<tab>} }` also resolves through the loop-var binding.
  - Stryke keywords (`fn`, `class`, `struct`, `enum`, `trait`, `match`, `mysync`, `frozen`, …) and ~10k builtins from `%all`
  - **In-progress parse recovery** — when the cursor sits inside a fragment that breaks the parse (`Demo::│`), the LSP retries with the cursor's line blanked so completion still indexes the rest of the file
  - **Trigger characters** include `{` so `$h->{` / `$h{` auto-popup the relevant key set
- **Semantic tokens** for server-driven syntax coloring — keywords, builtins (with `defaultLibrary` modifier), sigil variables, pipe operators, regex literals, numbers, strings, comments — beyond what any client-side lexer can know
- **Goto / References / Rename** — package-aware. Rename of struct/class/enum/trait fields and methods is AST-based with no textual fallback (so `my %h = (width => 1)` is not mistaken for a field reference when renaming `width`). The server defensively strips a `::` qualifier from `newName` so clients that send the full prefilled identifier (e.g. `TrafficLight::Stop` when renaming a variant) still produce the correct bare replacement. Cross-file rename walks the require graph BFS-style.
- **Code actions** — *Extract Variable / Constant / Parameter / Function* (Cmd-Opt-V / Cmd-Opt-C / Cmd-Opt-P / Cmd-Opt-M), Wrap-in-`p`, toggle line comment. Extract works on caret-only (no manual selection needed) and inside double-quoted strings / backticks.
- **Signature help** — parameter hints derived from the same doc strings that drive hover; active-parameter tracking as you type past commas

Wire it into VS Code, JetBrains, or any LSP-aware editor by pointing the client at `stryke lsp` (or `stryke --lsp`) as the language-server command. There is no separate `stryke-lsp` binary — the same `stryke` you run scripts with also acts as its own language server.

```jsonc
// .vscode/settings.json
{
  "stryke.serverPath": "/usr/local/bin/stryke",
  "stryke.serverArgs": ["--lsp"]
}
```

For JetBrains IDEs (RustRover, IDEA Ultimate, GoLand, PyCharm Pro, WebStorm, RubyMine, PhpStorm, CLion, Rider, DataGrip, Aqua) there is a first-class plugin under [`editors/intellij/`](editors/intellij/):

- `.stk` file association, **44-slot color scheme**, hand-rolled lexer with finer-grained token categories (declaration vs control keywords, sigil variants, topic / block-param / special-var splits, pipe / arrow / range / regex-bind operators). The lexer correctly handles every Perl regex-operator form as one atomic REGEX token — `s/PATTERN/REPL/FLAGS`, `tr/.../.../`, `y/.../.../`, `m/.../`, `qr/.../` — including embedded quote chars (`s/"/""/g`), paired-bracket delimiters (`s{foo}{bar}`), and mixed delimiters. Keyword spellings used as method names (`fn state`, `fn transition`) tokenize as `FUNCTION_DECL`, not the matching control/decl keyword. Keyword spellings used as hash keys (`$h->{state}`, `state => 1`) tokenize as `IDENTIFIER`. Perl-style `@{[ EXPR ]}` array-ref interpolations inside double-quoted strings tokenize the interior as code. The full block-param grammar (`_`, `_<`, `_<2`, `_<<<<<`, `$_<3`, `$_2<<<`) lexes as single `BLOCK_PARAM` tokens.
- **LSP client** wired to `stryke --lsp` — completion (with 60+ snippets, hash-key completion driven by builtin return schemas, suffix-only `insertText` for qualified `Demo::<tab>`, parse-error recovery during typing), hover (full markdown cards, suppressed inside string literals so `"length"` doesn't pop builtin docs), **goto / refs / rename** (AST-based for class/struct/enum/trait fields; the rename dialog prefills with the bare segment so editing `Red → Redgg` doesn't qualify the result as `TrafficLight::Redgg`), semantic tokens, signature help, **code actions** (Wrap-in-`p`, Comment / Uncomment, *Extract to variable / constant / parameter / function* — caret-only, inside-string aware), **folding ranges** (every `{ … }` block + `=pod ... =cut` + 3+ `#`-line comment runs), **document formatting** (Cmd-Opt-L pipes through `fmt::format_program`), diagnostics (strict-vars by default with sensible exemptions — see [\[0x11\]](#0x11-language-server-stryke-lsp))
- **Run configurations** with `--no-interop` / `--disasm` / `--profile` / `--flame` / `-d` toggles; context-menu *Run with stryke* on any `.stk`
- **Debugger** over the Debug Adapter Protocol (`stryke --dap`): line + function breakpoints from the gutter, Continue / Step Over / Step Into / Step Out / Pause / Run-to-Cursor, frames with file:line, **recursive hash & array drill-down** in the Variables panel, real-time `p` / `print` output in the Console, **Evaluate dialog** with scalar prelude injection so `$a * $b` returns the right value from the paused frame. The CLI debugger `stryke -d file.stk` is a separate TTY front-end on the same `Debugger` state machine — both share breakpoint / step / scope inspection logic.
- **Reflection tool window** (*View → Tool Windows → Stryke*) — searchable trees of `%stryke::all`, `%stryke::builtins`, `%stryke::keywords`, `%stryke::operators`, `%stryke::special_vars`, `%stryke::perl_compats`, `%stryke::extensions`, `%stryke::aliases`, `%stryke::descriptions` (≈25k entries). Left-click a leaf for an ANSI-rendered docs popup; right-click for *Show Docs* / *Copy Name*.

Build with `./gradlew buildPlugin` and install the zip from `editors/intellij/build/distributions/`. Community editions are not supported (no LSP API). See [`editors/intellij/README.md`](editors/intellij/README.md) for the full feature list, settings, and architecture.

### `stryke --dap` — Debug Adapter Protocol

The same `stryke` binary that runs scripts also speaks the standard DAP protocol. Two transport modes:

- **Stdio** (`stryke --dap`) — DAP messages over stdin/stdout. Useful for shell testing and clients that drive adapters over their own stdio.
- **TCP** (`stryke --dap HOST:PORT`) — connects to a listening socket the client opened first. This is what the JetBrains plugin uses; it keeps stryke's stdout/stderr free for the program's own `p`/`print` output (which the IDE's `OSProcessHandler` reads independently).

Supported DAP requests: `initialize`, `setBreakpoints` (line), `setFunctionBreakpoints`, `setExceptionBreakpoints`, `configurationDone`, `launch`, `threads`, `stackTrace`, `scopes`, `variables` (recursive — every hash/array gets a `variablesReference` for drill-down), `continue`, `next`, `stepIn`, `stepOut`, `pause`, `evaluate`, `terminate`, `disconnect`. Supported events: `initialized`, `stopped`, `output`, `process`, `thread`, `exited`, `terminated`.

The TTY CLI debugger (`stryke -d`) and the DAP server (`stryke --dap`) share a single `Debugger` state machine — breakpoints, step modes, scope inspection, call-stack tracking. The only difference is the front-end transport.

---

## [0x12] LANGUAGE REFLECTION

stryke exposes its own parser and dispatcher state as plain Perl hashes, so
you can enumerate, look up, filter, and pipe over everything the interpreter
knows about — no separate API surface to learn, just standard hash ops.

The data is derived at compile time by `build.rs` from the source of truth:
section-commented groups in `is_perl5_core` / `stryke_extension_name` (for
categories), `try_builtin` arm names (for aliases), and `doc_for_label_text`
in `src/lsp.rs` (for descriptions). No hand-maintained list, no stale counts.

#### Hashes

Eleven hashes; every direct lookup (`$h{name}`) is **O(1)**. Forward maps:

| Long name | Short | Key → Value |
| --- | --- | --- |
| `%stryke::builtins` | `%b` | **primary** callable name → category (`"parallel"`, `"string"`, …). Primaries-only — clean unique-op count. No keywords. |
| `%stryke::keywords` | `%k` | stryke language keyword → category (`"control"`, `"decl"`, `"exception"`, `"phase"`, `"concurrency"`, `"oo"`, `"operator"`, `"visibility"`). Disjoint from `%b`. |
| `%stryke::operators` | `%o` | symbol operator spelling → category (`"arith"`, `"compare"`, `"logical"`, `"bitwise"`, `"assign"`, `"binding"`, `"pipeline"`, …). Word operators (`and`/`or`/`eq`/`cmp`) live in `%k`. |
| `%stryke::special_vars` | `%v` | special variable spelling (sigil included) → category (`"error"`, `"regex-capture"`, `"caret"`, `"env"`, `"script"`, `"args"`, …). One hash covers every kind: `$!`, `@ARGV`, `%ENV`, `$^X`, `__FILE__`, etc. |
| `%stryke::all` | `%all` | **every name** stryke recognizes — `%a + %b + %k`. Aliases inherit their primary's tag; keywords carry their `%k` category. Use this for `scalar keys %all`. |
| `%stryke::perl_compats` | `%pc` | subset of `%b`: Perl 5 core only, name → category |
| `%stryke::extensions` | `%e` | subset of `%b`: stryke-only, name → category |
| `%stryke::aliases` | `%a` | alias → canonical primary (`$a{tj}` → `"to_json"`) |
| `%stryke::descriptions` | `%d` | name → one-line LSP summary (**sparse**) |

Inverted indexes for constant-time reverse queries:

| Long name | Short | Key → Value |
| --- | --- | --- |
| `%stryke::categories` | `%c` | category → arrayref of names (`$c{parallel}` → `[pmap, pgrep, …]`) |
| `%stryke::primaries` | `%p` | primary → arrayref of its aliases (`$p{to_json}` → `[tj]`) |

#### Examples

```sh
# O(1) direct lookups
stryke 'p $b{pmap}'              # "parallel"
stryke 'p $b{to_json}'           # "serialization"
stryke 'p $pc{map}'              # "array / list"
stryke 'p $e{pmap}'              # "parallel"
stryke 'p $a{tj}'                # "to_json"
stryke 'p $d{pmap}'              # LSP one-liner
stryke 'p $all{tj}'              # "serialization"  (alias resolved via %all)
stryke 'p $k{if}'                # "control"
stryke 'p $k{class}'             # "decl"
stryke 'p $all{while}'           # "control"        (keyword resolved via %all)
stryke 'p scalar @{$c{parallel}}'  # number of parallel ops
stryke '$p{to_json} |> e p'        # every alias of to_json

# total callable spellings (primaries + aliases), one direct count
stryke 'p scalar keys %all'

# see just Perl compats
stryke 'keys %pc |> sort |> p'

# see just stryke extensions
stryke 'keys %e |> sort |> p'

# enumerate a whole category in O(1)
stryke '$c{parallel} |> e p'
stryke '$c{"array / list"} |> e p'

# browse any of them interactively via the pager
stryke 'keys %all |> less'

# frequency table: how many ops per category?
stryke 'my %f; $f{$b{$_}}++ for keys %b; dd \%f'

# find every documented op mentioning "parallel"
stryke 'keys %d |> grep { $d{$_} =~ /parallel/i } |> sort |> p'

# catalog the full reflection surface
stryke 'for my $h (qw(b k all pc e a d c p)) {
         printf "%%%-4s %d\n", $h, scalar keys %$h
       }'
```

#### Notes

- Every direct `$h{name}` lookup is O(1). Filter queries (`grep { cond }
  keys %h`) are O(n), but the two inverted indexes (`%c`, `%p`) give you
  O(1) reverse-lookups for the two most common "find names by property"
  queries.
- Hash sigil namespace is separate from scalars and subs, so `%a`/`%b`/`%c`/`%d`/`%e`/`%k`/`%p`/`%pc`
  don't collide with `$a`/`$b` sort specials or the `e` extension sub.
- Short aliases are value copies of the long `%stryke::*` names — currently
  read-only in practice, so the copy never diverges.
- `%descriptions` is sparse: `exists $d{$name}` doubles as "is this
  documented in the LSP?". Undocumented ops still appear in `%builtins`
  with a category — they just lack a hover summary.
- A value of `"uncategorized"` in `%builtins` means the name is dispatched
  at runtime but doesn't match any `// ── category ──` section comment in
  `parser.rs` yet — a flag for "add a section header here", not an error.

## [0x14] PACKAGE MANAGER

Cargo-shaped manifest + lockfile, hash-pinned, parallel resolver. Single binary surface (`stryke ...`), no separate `cargo`-style entry point. Full design in [`docs/PACKAGE_REGISTRY.md`](docs/PACKAGE_REGISTRY.md).

```
# Lifecycle
stryke new myapp                  # scaffold project at ./myapp/
stryke init                       # scaffold project in cwd
stryke add http@^1.0 json         # write deps to stryke.toml
stryke add mylib --path=../mylib  # local path dep (works today)
stryke add http --dev             # dev-deps
stryke add criterion --group=bench
stryke remove http                # drop dep from manifest
stryke install                    # resolve + write stryke.lock
stryke install --offline          # no network; lockfile-only
stryke update [NAME]              # re-resolve and rewrite stryke.lock
stryke outdated                   # report deps drifted from their lock pin
stryke audit                      # check lockfile against advisory feed
stryke tree                       # print resolved dep graph
stryke info http                  # show lockfile entry for a dep
stryke vendor                     # snapshot store deps to ./vendor/
stryke clean [--all]              # wipe target/ (and optionally global caches)

# npm-style task runner
stryke run greet                  # execute [scripts] entry "greet"

# Global CLI tools
stryke install -g ../mytool       # link [bin] entries from a path package into ~/.stryke/bin/
stryke uninstall -g mytool
stryke list -g

# Registry surface (registry endpoint not deployed yet — stubs return diagnostics)
stryke search NAME
stryke publish [--dry-run]
stryke yank VERSION
```

Project layout (`examples/project/`):

```
myapp/
├── stryke.toml                   # manifest (name, version, deps, [scripts], [bin], [workspace], etc.)
├── stryke.lock                   # exact versions + integrity hashes (commit this)
├── main.stk                      # entry point (`stryke main.stk` or just `stryke`)
├── lib/                          # module sources, accessed via require/use
├── bin/                          # additional executables (auto-discovered)
├── t/                            # tests (`stryke test t/`)
├── benches/                      # benchmarks (`stryke bench`)
└── target/                       # build outputs (gitignored)
```

Workspaces are first-class:

```toml
# stryke.toml at workspace root
[workspace]
members = ["crates/*"]

[workspace.deps]
shared = { path = "../shared" }   # one version pinned for the whole monorepo
```

Then in any member's `stryke.toml`:

```toml
[deps]
shared = { workspace = true }     # inherit version + features from the root
```

Single `stryke.lock` at the workspace root pins every member's transitive graph.

Deps live globally in `~/.stryke/store/name@version/` — no `node_modules/`-shaped per-project dependency tree. Every dep is hash-pinned in the lockfile (Nix-style reproducibility, Cargo-style ergonomics). `stryke build --release` AOT-compiles the whole program — your code, every dep, the stdlib — through Cranelift to a single statically-linked native binary. SFTP-able. No interpreter needed on the target machine.

**Status**: path deps + workspace deps + full CLI surface (`new`/`init`/`add`/`remove`/`install`/`update`/`outdated`/`audit`/`tree`/`info`/`vendor`/`clean`/`run`/`install -g` etc.) are wired and tested today. Registry/git deps + the PubGrub semver resolver land when the registry endpoint is deployed — the CLI stubs for `search`/`publish`/`yank` already exist and return clear "registry not deployed yet" diagnostics so the surface matches the RFC end-state.

**Skipped on purpose**: install-time code execution (no `build.rs` / `postinstall`), hoisting, phantom deps, peer deps, mutable registries. The lockfile is sacred; regenerate explicitly.

---

## [0x15] WEB FRAMEWORK (`s_web`)

Rails-shaped framework that lives in the sibling crate `stryke_web/`. Generator emits `.stk` source files; framework runtime is `web_*` builtins in the main `strykelang/` crate. Full reference in [`stryke_web/README.md`](stryke_web/README.md).

**One-line full-stack app**:

```sh
s_web new mega --app everything --theme cyberpunk --auth --admin --docker --ci --pwa --migrate
cd mega && bin/server
# ~70 resources, ~490 CRUD routes, dark cyberpunk CSS, signup/login/sessions,
# admin panel at /admin, /health endpoint, Dockerfile, GitHub Actions CI, PWA
# manifest + service worker. Runs at http://localhost:3000.
```

| Component | What's wired |
|---|---|
| Routing | `web_route VERB " /path", "ctrl#act"`, `web_resources "posts"` (7-route REST), `web_root "ctrl#act"`, OpenAPI 3.0 dump auto-served at `/openapi.json`, Swagger UI at `/docs` |
| Controllers | `web_render(html\|text\|json\|template\|redirect)`, `web_params`, `web_request`, `web_set_header`, `web_status`, `web_security_headers`, default-convention render |
| Views | ERB engine (`<%= %>` / `<% %>` / `<%# %>` / `<%- -%>`), layout wrap, partials (`web_render_partial`), `web_link_to`, `web_form_with`, `web_text_field/area/check_box`, `web_csrf_meta_tag` |
| ORM (SQLite) | `class Article extends ApplicationRecord` with auto-generated `Self.all/find/where/create/update/destroy` static methods. `web_model_paginate/search/soft_destroy/count/first/last/with` for n+1 elimination, soft delete, pagination |
| Migrations | `web_create_table/drop_table/add_column/remove_column` schema DSL, `web_migrate/rollback` runner, `schema_migrations` tracking |
| Validations / strong params | `web_validate(+{title => "presence,length:1..100", email => "format:^.+@.+$"})`, `web_permit($params, "title", "body")` |
| Auth | `web_password_hash/verify`, `web_session_set/get`, signed time-limited tokens (`web_token_for`/`consume`), CSRF meta, `web_can("posts.edit", $user)` permissions |
| Filters | `web_before_action`/`web_after_action` with `only`/`except` |
| HTTP cache | `web_etag` with auto-304 short-circuit, prompt-cache headers |
| Helpers | `web_h` (HTML escape), `web_truncate`, `web_pluralize`, `web_time_ago_in_words`, `web_image_tag`, `web_button_to` |
| API | `--api` mode, `s_web g api Post` JSON controllers, JSON:API helpers (`web_jsonapi_resource/collection/error`), `web_bearer_token` |
| Themes | 9 baked-in: `simple`, `dark`, `pico`, `bootstrap`, `tailwind`, `cyberpunk`, `synthwave`, `terminal`, `matrix` |
| DevOps | `--docker` (multi-stage Dockerfile + .dockerignore), `--ci` (GitHub Actions), `--pwa` (manifest.json + service worker) |
| **Fat binary** | `s_web build --out dist && cd dist && cargo build --release` produces a single self-contained binary that include_str!s every `.stk` file plus the stryke runtime — drop on any Linux box, run, no deps |
| Generators | `s_web g {scaffold, model, migration, controller, app, auth, admin, api, mailer, job, channel, docker, ci, pwa}` |

**Presets**: `blog` (8 resources), `ecommerce` (15), `saas` (12), `social` (10), `cms` (12), `forum` (10), `crm` (10), `helpdesk` (8), plus named clones: `amazon` (25), `facebook` (23), `learning` (21 — Anki-style with SRS), and `everything` (~70 resources unioned + dedup'd).

---

## [0x16] AI PRIMITIVES

`ai` is a builtin like `print` — two letters, ubiquitous, unlimited power. Full design + phase-by-phase status in [`docs/AI_PRIMITIVES.md`](docs/AI_PRIMITIVES.md).

```stryke
my $r = ai "summarize this", $document       # bare call
my $r = ai "research X", tools => [...]      # auto-routes to agent loop
my $r = ai "describe", image => "/img.jpg"   # vision
my $r = ai "extract", schema => +{...}       # structured output
my $r = ai "...", pdf => "/contract.pdf"     # document input
for my $chunk in stream_prompt("write a haiku") { print $chunk }   # iter-context streaming
```

| Surface | Builtins |
|---|---|
| Single-shot | `ai`, `prompt`, `stream_prompt`, `chat`, `embed`, `tokens_of`, `ai_estimate` |
| Agent loop | `ai($p, tools => [...])` — Anthropic tool_use + OpenAI function-calling protocols. Multi-turn, multi-tool, max_turns/max_cost ceilings |
| `tool fn` keyword | `tool fn weather($city: string) "Get weather" { ... }` — auto-schemas signature, auto-registers, auto-attaches to bare `ai($p)` calls |
| Built-in tools | `web_search_tool` (Brave/DDG), `fetch_url_tool`, `read_file_tool`, `run_code_tool` (sandboxed Python) — drop into `tools => [...]` |
| MCP client | `mcp_connect("stdio:CMD")` and `mcp_connect("https://...")`, full `tools/resources/prompts/call` surface, auto-attach to agent loop via `mcp_attach_to_ai` |
| MCP server | `mcp_server_start("name", +{tools => [...]})` runtime, plus declarative `mcp_server "name" { tool foo($a) "..." {...} }` parser DSL |
| Sessions | `ai_session_new/send/history/reset/close` — multi-turn chat tracking |
| Collection ops | `ai_filter`, `ai_map`, `ai_classify`, `ai_match`, `ai_sort`, `ai_dedupe` — single batched LLM call across the collection |
| Memory / RAG | `ai_memory_save/recall/forget/count/clear` — sqlite-backed embedding store, cosine retrieval |
| Vector ops | `vec_cosine`, `vec_search`, `vec_topk` |
| Multimodal | `ai_vision` (image), `ai_pdf` (document) |
| Cost / cache | `ai_cost`, `ai_cache_get/set/clear`, `ai_history`, `ai_budget($usd, sub { ... })` scoped cap |
| Mock / test | `ai_mock_install`, `STRYKE_AI_MODE=mock-only` for CI |
| Convenience | `ai_summarize`, `ai_translate`, `ai_extract`, `ai_template`, `ai_last_thinking` |
| Audio | `ai_transcribe "audio.mp3"` (Whisper), `ai_speak "text", voice => "alloy"` (OpenAI TTS) |
| Image | `ai_image $prompt`, `ai_image_edit $prompt, image => $src, mask => $m`, `ai_image_variation image => $src, n => 4` — DALL-E 3 / gpt-image-1 / DALL-E 2 |
| Catalog | `ai_models("openai"\|"anthropic"\|"ollama"\|"gemini")` — live model IDs from each provider's `/models` endpoint |
| Citations | `ai_pdf $p, pdf => $f, citations => 1` and `ai_grounded $p, documents => [@paths]` — multi-doc grounding with auto-citations via `ai_citations()` |
| Files (OpenAI) | `ai_file_upload "file.bin", purpose => "user_data"`, `ai_file_list`, `ai_file_get`, `ai_file_delete` |
| Files (Anthropic) | `ai_file_anthropic_upload "file.pdf"`, `ai_file_anthropic_list`, `ai_file_anthropic_delete` — beta `files-api-2025-04-14` |
| Moderation | `ai_moderate $text` → `+{ flagged, categories, scores }` — OpenAI safety classifier (free endpoint) |
| Chunk | `ai_chunk $text, max_tokens => 500, overlap => 50, by => "chars"\|"sentences"` — RAG primitive, no API call |
| Warm / verify | `ai_warm(model => ..., provider => ...)` → `+{ ok, latency_ms, error }` — auth + reachability ping at script start |
| Compare | `ai_compare $a, $b, criteria => "...", scale => 5` → `+{ winner, reason, scores }` — single-call structured comparison |
| Dashboard | `print ai_dashboard()` — ANSI summary of cost/tokens/cache hit-ratio |
| Pricing | `ai_pricing("claude-opus-4-7")` → `+{ input, output, input_per_1m, output_per_1m }` for pre-flight cost estimates |
| Describe | `ai_describe "img.png", style => "concise"\|"detailed"\|"alt"` — vision wrapper with style presets |
| Sessions | `ai_session_new/send/history/reset/close` plus `ai_session_export($h) → $json` and `ai_session_import($json) → $h` for persistence across runs |
| Embed providers | Voyage (default), OpenAI (`text-embedding-3`), Ollama (`nomic-embed-text`/`mxbai-embed-large` — local, $0/M tokens) |
| CLI modes | `stryke ai --image PROMPT -o out.png`, `--transcribe audio.mp3 -o out.txt`, `--speak "hello" -o out.mp3` — UNIX-filter mode covers chat, image, audio |
| Batch | `ai_batch(\@prompts)` — Anthropic batch API at 50% cost |
| Cluster fanout | `ai_pmap(\@items, "instruction", cluster => $c)` — distributed AI work |
| CLI | `stryke ai "prompt"` — UNIX filter mode with `--model`, `--system`, `--stream`, `--json` |

**Providers wired**: Anthropic (full surface incl. extended thinking, prompt caching, vision, PDF, batch), OpenAI (Chat + tool calls + streaming, Whisper, TTS), Voyage (embeddings, default), Ollama (`/api/generate`), OpenAI-compatible (`openai_compat`/`compat`/`local` — LM Studio, vLLM, llama-server; configurable `STRYKE_AI_BASE_URL`), Google Gemini. In-process llama.cpp deferred — Ollama / LM Studio is the supported local path today.

```stryke
# Auto-attached: bare `ai()` sees the tool fn without `tools =>` arg.
tool fn current_user($username: string) "Look up a user" {
    User::find_by_email($username)
}

tool fn create_post($title: string, $body: string) "Create a post" {
    Post::create(+{ title => $title, body => $body })
}

my $reply = ai("create a post titled 'Hello' from alice@x.io with body 'World'");
```

---

## [0x17] EXPECT / INTERACTIVE AUTOMATION

PTY-driven interactive scripting — the modern Tcl/Expect successor. Full design + phase status in [`docs/expect-feature-idea.md`](docs/expect-feature-idea.md).

```stryke
my $h = pty_spawn("ssh user@host");
pty_expect($h, qr/password:/, 30);
pty_send($h, "hunter2\n");
pty_expect($h, qr/\$ /, 30);
pty_send($h, "uptime\n");
my $out = pty_expect($h, qr/\$ /, 30);
pty_close($h);

# Table form (Tcl `expect { ... }` block, in stryke):
my $tag = pty_expect_table($h, [
    +{ re => qr/password:/, do => sub { pty_send($h, "$pw\n"); "ok" } },
    +{ re => qr/yes\/no/,   do => sub { pty_send($h, "yes\n"); "confirmed" } },
    +{ re => qr/denied/,    do => sub { die "auth failed" } },
], 30);

# Method-form sugar (require "perl_pty_class.stk"):
my $h = PtyHandle::spawn("ssh host");
$h->expect(qr/password:/, 30);
$h->send("$pw\n");
$h->branch([+{re => qr/\$ /, do => sub { "shell ready" }}], 30);
$h->interact();   # raw-mode handoff, Ctrl-] to detach
$h->close();
```

| Builtin | Behavior |
|---|---|
| `pty_spawn(cmd)` / `pty_spawn(cmd, arg, ...)` | Allocate PTY via `nix::pty::openpty`, fork+exec child, return handle |
| `pty_send($h, "text")` | Write to master fd |
| `pty_read($h, timeout_secs)` | One-shot read, returns string or undef on EOF |
| `pty_expect($h, qr/.../, timeout?)` | Loop: try regex on buffer, else `select()` + drain, retry until match or timeout |
| `pty_expect_table($h, [+{re, do}, ...], timeout?)` | Multi-pattern dispatch — first match wins; calls the matched branch's `do` sub |
| `pty_buffer($h)` / `pty_alive($h)` / `pty_eof($h)` | Inspection |
| `pty_close($h)` | SIGTERM → 200ms grace → SIGKILL, returns exit status |
| `pty_interact($h)` | Raw-mode handoff: `tcgetattr`/`cfmakeraw`, `select()` on stdin+master, forward both directions until EOF or Ctrl-] |

Combined with `pmap_on` cluster dispatch you get parallel SSH automation across N hosts:

```stryke
my $cluster = cluster(["host1:8", "host2:8", "host3:8"]);
pmap_on $cluster @hosts -> $host {
    my $h = pty_spawn("ssh $host");
    pty_expect($h, qr/password:/, 10);
    pty_send($h, "$passwords{$host}\n");
    pty_expect($h, qr/\$ /, 30);
    pty_send($h, "apt update && apt upgrade -y\n");
    pty_expect($h, qr/\$ /, 1800);
    pty_close($h);
}
```

Unix-only for v0; Windows ConPTY support is a separate code path that's still pending.

---

## [0x18] DOCUMENTATION

All documentation is served via GitHub Pages at [`menketechnologies.github.io/strykelang/`](https://menketechnologies.github.io/strykelang/).

| Document | Description |
|----------|-------------|
| [`Docs Home`](https://menketechnologies.github.io/strykelang/) | Stryke reference — quickstart, builtins, parallel primitives, pipe-forward syntax, reflection hashes |
| [`Full Reference`](https://menketechnologies.github.io/strykelang/reference.html) | Complete language reference — every builtin, operator, special variable, and regex feature |
| [`Engineering Report`](https://menketechnologies.github.io/strykelang/report.html) | strykelang internals — Rust lines, callable builtins, VM opcodes, AST variants, Cranelift JIT, rayon-backed parallel runtime, perl-parity cases |
| [`PACKAGE_REGISTRY.md`](docs/PACKAGE_REGISTRY.md) | Package manager design — manifest, lockfile, resolver, global store, build outputs |
| [`stryke_web/README.md`](stryke_web/README.md) | Web framework — generators, presets, themes, builtins, fat-binary build |
| [`AI_PRIMITIVES.md`](docs/AI_PRIMITIVES.md) | AI primitives — agent loop, MCP, tool fn, RAG, vector search, providers, phase-by-phase status |
| [`expect-feature-idea.md`](docs/expect-feature-idea.md) | Interactive automation — PTY runtime, expect tables, cluster fanout |
| [`STRESS_TESTING.md`](docs/STRESS_TESTING.md) | Distributed load testing — `stress_*` builtins, agent/controller, hardware/cooling validation |
| [`WEB_FRAMEWORK.md`](docs/WEB_FRAMEWORK.md) | Original web-framework design RFC |
| [`ROADMAP.md`](docs/ROADMAP.md) | Forward-looking work and open questions |

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
