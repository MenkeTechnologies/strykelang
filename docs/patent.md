---
name: zshrs+stryke+fusevm patent strategy — eight omnibus claims
description: Eight independently assertible omnibus claims covering the full zshrs+stryke+fusevm portfolio. (A) Unified-AOT (shell binary + script-AOT trailer-format track). (B) zshrs companion-daemon architecture. (C) fusevm three-stage-JIT runtime + no-GC + Rayon-parallel + AOT + embeddable. (D) stryke language design — 20 dependent claims, meta-claim, three-axis universal-access protocol, 20,000-test Perl-parity corpus. (E) Distributed orchestration + stress-testing infrastructure (agent/controller, cluster() SSH worker pool, heat/stress_cpu/stress_mem/stress_io/stress_test, mTLS, auto-terminate guards). (F) AI primitives as language builtin (`ai` 2-letter primitive, tool fn with build-time JSON schema, mcp_server/mcp_connect declarative DSL, ai_filter/map/sort/classify collection builtins, provider-agnostic with local fallback, cost-aware runtime). (G) Web framework (Rails-grade DSL + thread-per-core io_uring + single-binary deploy + radix-trie compile-time routing + per-request arena). (H) Package registry/manager (Cargo+uv+Nix+Bundler synthesis + AOT-compile-everything-including-deps via Cranelift). Each combination + domain application is patentable novelty. Filing strategy: 8 provisionals at same priority date for ~$520 micro-entity / ~$2400 small-entity USPTO fees.
type: project
originSessionId: 5439464a-01d8-4af4-a95a-c4942b441226
---

Patent direction locked 2026-04-28: file the combination claim as the omnibus, not individual primitive claims. Surface expanded 2026-04-30 to eight omnibuses covering the full zshrs+stryke+fusevm portfolio after design-doc audit (`AI_PRIMITIVES.md`, `WEB_FRAMEWORK.md`, `PACKAGE_REGISTRY.md`, `STRESS_TESTING.md`) revealed claim surface beyond the original four-omnibus framing.

**Three structural moats unify the portfolio:**

1. **Three-axis universal-access protocol** (Patent D, claim #16) — pipeline substrate covers (callable × value-class × reflection-metadata) cross-product with zero categorical exclusion. Defeats design-arounds because missing any cell collapses the imitator into a categorical-exclusion regime that #16 escapes.
2. **20,000-test Perl-parity corpus** (Patent D, claim #20) — empirical specification of `--compat` mode behavior. Defeats "we have Perl-compat too" claims because the corpus IS the operational spec; competitors must reproduce 20,000 tests passing to make a comparable claim.
3. **Systematic-absorption-of-userspace-tooling meta-claim** (Patent D meta + Patents F/G/H) — first scripting language to absorb 25+ categories (git, jq, parallel, visualization, crypto, stats, linalg, networking, compression, serialization, HTTP-server, SQLite, DataFrame, PDF, testing, file-watcher, profiler, formatter, LSP, REPL, dep-manager, docs, AOP, AI primitives, web framework, package manager) as core builtin verbs and language subsystems. Inverts "core minimal, libraries optional" → "core encyclopedic, libraries unnecessary." Defeats "we have feature X too" claims because absorbing one category is trivial; absorbing 25+ as a unified architectural commitment is not.

---

## Patent A: Unified-AOT (shell binary + script-AOT trailer-format track)

A first omnibus claim covering AOT compilation of dynamic-scripting source into self-contained native executables, with two distinct application tracks: (Track 1) zshrs shell-binary AOT, (Track 2) stryke script-AOT.

**The omnibus claim shape (engineering sketch, attorney drafts the legal language):**

A method comprising — (a) AOT-compile shell/Perl source + plugin trees + completion defs into one native executable; (b) embed lookup structures (perfect-hash completion tables, zstyle config, bindkeys) as instruction-accessible constants in `.rodata`; (c) weave AOP advice into machine code at compile time with glob-matched command-name patterns; (d) emit hardware-timestamp-counter-based (rdtscp / CNTVCT_EL0) per-function timing in prologues/epilogues; (e) persist runtime mutable state to a writable section of the executable; (f) atomically replace function chunks via memory-page patching at command-boundary safepoints in response to source-edit events. **Wherein** said executable functions simultaneously as interactive shell, script-deployment artifact, plugin host, completion engine, AOP weaver, and persistent state store.

The "wherein" clause locks the claim to the unified-product novelty.

**Why combination, not individual primitives:**

| Primitive | Prior art (cross-domain) |
|-----------|---------------------------|
| Perfect-hash data in `.rodata` | gperf (1989); RE2 / Hyperscan compiled DFAs; Linux kallsyms |
| Live patching of native AOT code | Linux kpatch / ksplice / kgraft / livepatch (2008-2014); Microsoft Detours (1999); DTrace SDT; ftrace; kprobes |
| Compile-time AOP weaving | AspectJ (2001); gcc `-finstrument-functions`; LLVM sanitizers; GHC `-prof` |
| Image-as-binary persistence | Pharo/Squeak Smalltalk (30+ yrs); SBCL `save-lisp-and-die`; HyperCard |
| Hardware-counter inline timing | DTrace; perf events; rdpmc-based profiling |
| AOT shell compilation | None in shell domain; closest is zsh `.zwc` (bytecode, not native) |

**Why non-obvious (KSR v. Teleflex 2007 test):** the six primitives live in six separate fields. A PHOSITA in any one field has zero motivation to reach for the others. Every shell for 50+ years has been C with manual memory, no VM, no image, no AOP — the invention runs counter to the entire teaching of the field.

### Track 2: Script-AOT trailer-format dependent claims

The script-AOT track applies the same AOT philosophy to standalone stryke scripts (vs. zshrs's shell-as-binary). The output is a copy of the stryke binary with the script source embedded as a zstd-compressed trailer.

```
[zstd payload][u64 compressed_len][u64 uncompressed_len][u32 version][u32 reserved][8B magic b"STRYKEAOT"]
```

| Sub-claim | What it covers |
|---|---|
| **A.x: trailer-format binary appending** | Trailer format with OS-loader-invisible placement past mapped segments — runtime detects via 32-byte magic-suffix sniff at ~50µs |
| **A.y: versioned forward-compat trailer** | `[u32 version][u32 reserved]` fields permit future payload formats (v1 source, v2 bytecode, v3 fully-JIT'd native) to coexist in already-shipped binaries with version-dispatch at load time |
| **A.z: idempotent rebuild semantics** | `stryke --exe foo build other.stk -o foo` strips previous trailer first; trailers never stack |
| **A.w: convention-based multi-file project bundling** | `stryke build --project DIR` bundles `main.stk` + `lib/*.stk` (excludes `t/` test dir by default) into single binary, no manifest required |

**Comparison to existing bundlers:**

| Tool | Output size (hello-world) | Forward-compat versioned format? | Idempotent rebuild? | Detection time |
|---|---|---|---|---|
| Go `go build` | 2-5 MB | n/a (compiled native) | yes | n/a |
| PyInstaller | 10-50 MB | no | partial | ~100-500ms extract |
| `pkg` (Node) | 30-80 MB | no | partial | ~50-200ms |
| PAR::Packer (`pp`) | 5-30 MB | no | no | ~100-500ms |
| GraalVM native-image | 30-100 MB | no | yes | n/a |
| Nuitka | 5-15 MB | n/a (compiled C) | yes | n/a |
| **`stryke build`** | **~13 MB** | **yes** | **yes** | **~50µs** |

Three structural differentiators: (i) versioned magic with reserved fields for payload-format evolution; (ii) sub-millisecond detection (vs PyInstaller's ~100-500ms extraction); (iii) idempotent rebuild semantics. Each is engineering work most bundlers don't attempt because they assume "one payload format forever."

---

## Patent B: zshrs companion-daemon architecture

A second omnibus claim covering the singleton companion daemon for a Unix command shell.

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a singleton daemon process that owns all bytecode-cache mutation, supervises long-running detached jobs surviving originating-shell exit, brokers cross-shell publish/subscribe over a Unix domain socket, and federates with peer daemons over secure remote channels (e.g. SSH multiplex); (b) thin shell-client processes that read bytecode from sharded mmap'd image files via direct user-space pointer dereference (data plane), and signal the daemon via JSON-over-socket IPC for all configuration changes, job submissions, cross-shell dispatch, and event subscriptions (control plane); (c) atomic shard-rename followed by index update with generation counters, allowing clients to detect stale mmap regions and re-mmap without coordination; (d) shell-process registry maintained authoritatively by the daemon via socket-connect enrollment, replacing filesystem-based registries used by prior cross-shell coordination plugins. **Wherein** said daemon is dedicated to a Unix command shell — distinct from text-editor daemons (emacs --daemon), credential-cache daemons (ssh-agent / gpg-agent), terminal multiplexers (tmux / screen), batch-job daemons (pueue), and language runtime images (Pharo / SBCL) — and serves N concurrent shell clients with the data plane operating without per-call IPC.

**Stacked world-firsts the daemon claim secures (dependent claims):**

1. **Shell with a dedicated companion daemon spanning bytecode cache + supervised jobs + IPC + federation.** No prior art in any shell.
2. **Native session-persistent shell-job supervision.** No prior art in any shell.
3. **Native cross-shell pub/sub + dispatch as first-class primitives** (`zsubscribe shell:N.commands`, `zsend --tag prod ...`, federation via daemon-to-daemon).

**Per-shell prior-art comparison:**

| Primitive | Prior art (in any shell) |
|-----------|---------------------------|
| Companion daemon | fish 1.x–2.0.x had `fishd` for **universal-variable sync only**; removed in fish 2.1.0 (2014). No other shell ever shipped one. |
| Shared bytecode cache between shell instances | Zero. zsh `.zwc` is per-file static, not daemon-managed. |
| Daemon-supervised detached jobs | Zero. `nohup`/`disown`/`setsid`/`&` detach but don't supervise. |
| Cross-shell pub/sub | zconvey (zsh plugin, filesystem-IPC + per-prompt polling); not built into any shell. |
| Cross-machine shell federation | Atuin syncs history only, runs as separate REST server, not a shell daemon. |

The daemon-architecture combination as a whole, applied to a Unix command shell, has zero prior art. (zshrs-specific; this strykelang docs slice retains it for portfolio completeness.)

---

## Patent C: fusevm runtime architecture

A third omnibus claim covering the **fusevm three-stage JIT runtime** that powers both zshrs and stryke. Splits off from claims A and B because fusevm is **independently licensable for non-shell uses** (other languages, embedded scripting in non-shell tools).

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a bytecode virtual machine for a dynamic scripting language; (b) a tiered just-in-time compilation system spanning at least three optimization regimes simultaneously: (i) a **linear/template/baseline JIT tier** that compiles basic blocks immediately upon first execution with minimal optimization for fast warmup, (ii) a **tracing JIT tier** that records hot linear execution paths through loops and emits type-specialized native code with inlining and unrolling, (iii) a **block/method optimizing JIT tier** that compiles entire functions with inlining + escape analysis + dead-code elimination + polymorphic-call-site specialization; (c) all three tiers coexisting in a single runtime with a tier-promotion policy that escalates code from baseline → tracing or baseline → block based on observed execution profile; (d) a no-garbage-collection memory model based on Rust ownership semantics + atomic reference counting (Arc), eliminating GC pauses entirely; (e) a Rayon-style work-stealing parallel execution substrate exposed as language primitives (e.g., parallel pipelines, parallel directory walks, parallel field-substitution operations); (f) an AOT compilation path producing self-contained native binaries from the same bytecode source; (g) an embeddable substrate API that allows the runtime to be hosted inside other processes (notably a Unix command shell) with zero process-spawn overhead. **Wherein** said runtime, in combination, achieves per-loop overhead within an order of magnitude of statically-compiled native code while supporting dynamic scripting semantics, native parallelism without a global interpreter lock, and zero-startup invocation when embedded.

**No prior runtime ships all three JIT tiers simultaneously:**

| Runtime | Tracing | Linear/baseline | Block/optimizing |
|---------|---------|-----------------|-------------------|
| LuaJIT | ✓ | — | — |
| PyPy | ✓ | — | — |
| V8 (modern) | — | ✓ (Sparkplug) | ✓ (TurboFan) |
| JSC | — | ✓ (Baseline) | ✓ (DFG, FTL) |
| HotSpot | — | ✓ (C1) | ✓ (C2) |
| GraalVM/Truffle | partial (via partial eval) | — | ✓ |
| **fusevm** | **✓** | **✓** | **✓** |

**Stacked world-firsts (dependent claims):**

1. First runtime combining tracing + linear + block JIT in one substrate.
2. First production no-GC dynamic-scripting runtime with JIT (Roc is research; nothing else qualifies).
3. First runtime exposing parallelism as language primitives (par_pipeline, par_sed, par_walk) rather than library calls.
4. First runtime achieving per-iteration overhead within ~100ns of statically-compiled native Rust while preserving dynamic-scripting semantics. (Empirical, supportable with measurement evidence.)
5. **rkyv-backed mmap'd bytecode cache for warm-start script reruns** — first run of `stryke app.stk` parses + JIT-compiles + serializes bytecode to `~/.cache/stryke/scripts.rkyv` via [rkyv](https://crates.io/crates/rkyv) zero-copy serialization; subsequent runs skip parse + compile entirely, mmap the cache and resume execution directly. Empirically delivers **~11× faster warm-start script reruns** vs cold-start. Cache invalidation is content-hash-driven (source mtime + hash); stale entries silently re-compile. The cache architecture parallels zshrs's daemon-managed rkyv image cache (Patent B) but applies to standalone scripts without requiring a daemon — the script binary itself manages cache read/write, the OS page cache shares pages across concurrent invocations. **No prior dynamic-scripting language ships zero-copy mmap'd compiled-bytecode caching with this architecture** — Python's `__pycache__/.pyc` is a marshaled-bytecode format requiring per-import deserialization (no zero-copy, no mmap-direct-execution); Ruby has no script-level bytecode cache; Lua's `string.dump` is a serialization primitive, not a managed cache. Stryke's combination of (a) rkyv zero-copy archive format, (b) mmap-direct-execution from cache, (c) content-hash invalidation, (d) shared-via-OS-page-cache across concurrent invocations is novel for the dynamic-scripting category.

**Empirical positioning** (from `examples/rosetta/README.md`): "2nd fastest dynamic language runtime ever benchmarked for singlethreaded — behind only Mike Pall's LuaJIT, and beating it on 3 of 8 benchmarks. The fastest on all multithreaded benchmarks. Faster than perl5, Python, Ruby, Julia, and Raku on every benchmark." Combined with the rkyv warm-start cache (#5), repeated invocations of the same stryke script approach native-binary cold-start performance — relevant for shell-embedded one-liner workloads where the same `stryke -e '...'` or `stryke app.stk` invocation runs hundreds of times across a session.

**Closest analog in patent literature:** Sun's HotSpot JIT patents (1999-2005) cover C1+C2 tiered compilation but for a strongly-typed JVM; Oracle's GraalVM/Truffle patents cover partial-evaluation-based JIT for dynamic languages but only one optimization tier. **No patent covers the three-tier combination + no-GC + parallel + dynamic-scripting + embeddable composition.**

---

## Patent D: stryke language design

A fourth omnibus claim covering the **stryke language design** as a unified syntactic + semantic synthesis. Splits off from C because language design is **independently licensable from the runtime substrate** — stryke's syntactic primitives could be implemented on a different runtime; fusevm could host a different language design.

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a programming language for shell-embedded scripting and one-liner workloads, comprising at least: (b) **two-axis positional-argument access syntax** of the form `_N<...<` where N is a positional-slot index (0, 1, 2, ...) and each `<` character represents one level of closure-nesting outward from the current frame, allowing reference to positional arguments of arbitrarily-deep enclosing closures or function frames without named-binding fallback; including (b.i) **four-way cross-tradition aliasing for the default slot 0** wherein `_`, `$_`, `_0`, and `$_0` are accepted as equivalent spellings (Scala bare-`_` ≡ Perl topic-var `$_` ≡ Mathematica/Swift positional ≡ sigil-positional synthesis); (b.ii) **two-way aliasing for indexed slots N≥1**; (b.iii) **orthogonal depth-encoding** wherein the `<...<` depth marker composes uniformly with any spelling choice; (c) **polymorphic literal-typed range syntax** of the form `<literal>:<literal>[:<step>]` where the literal type is inferred from the literal form (integer → integer range, character → character range, roman-numeral → roman range, ISO-8601 date → date range, RFC3339 datetime → datetime range, dotted-quad IP → IP range) and the step semantics are inferred from the inferred type; (d) **encyclopedic bundled standard library** of 3000+ built-in functions covering cryptographic primitives, statistical functions, linear-algebra primitives, networking, date-time arithmetic, compression, structured-data formats, shell-process primitives, type predicates (≥150 distinct is_* predicates), ANSI/terminal styling, and tools-typically-invoked-as-subprocesses-elevated-to-language-builtins (notably git operations and jq queries) — all available without import or package-load step; (d.i) said standard library further providing **parallel short-name aliases** (2-3 character forms) for long-name builtins systematically across the encyclopedic corpus; (e) **a unified threading/pipe operator family** comprising (e.i) pipe-forward operator (`|>`); (e.ii) Clojure-tradition compile-time AST-rewriting threading macros (`->`, `->>`); (e.iii) Racket-tradition threading macros (`~>`, `~>>`); coexisting as semantically-distinct first-class operators; (e.iv) said threading operators supporting **three notation forms per pipeline stage**: bare-function, inline arrow-block, and **anonymous-positional-placeholder** (`f(_, 5)`, `f(5, _)`, `f(5, _, 10)`); (e.v) wherein the **arrow-block sigil `>{ ... }`** is a syntactic primitive distinct from regular closure-block; (f) **Perl-style sigils** (`$`, `@`, `%`, `&`, `*`); (g) **Ruby-derived ultra-terse output verbs** (`p`, `ep`); (h) **parallelism-as-syntactic-primitive** via builtins (`par_pipeline`, `par_sed`, `par_walk`, `par_csv_read`, `pmap`, `pgrep`, `psort`, `pfor`, `pmaps`, `pgreps`, `fan`, `fan_cap`, `pchannel`, `pselect`, `barrier`, `ppool`); (i) **terminal-ASCII data visualization-as-syntactic-primitive** via builtins (`histo`, `sparkline`, `barchart`, `table`); and (j) **systematic absorption of userspace tooling categories into the core language** including version control (git operations as builtins), structured-data query (jq absorbed as builtin verb), parallel processing primitives, terminal visualization, cryptography, statistics, linear algebra, networking, compression, structured-data serialization, and (per Patents F/G/H) AI primitives, web-framework primitives, and package-management primitives. **Wherein** said language is invocable both as a standalone interpreter and as a shell-embedded sublanguage with zero process-spawn cost when embedded; **wherein** the threading-operator family of (e), with its three notation forms (bare-function, arrow-block, anonymous-positional-placeholder), collapses what Clojure achieves with three separate macros (`->`, `->>`, `as->`) into one operator family with richer per-stage notation; **wherein** the systematic absorption of (j) inverts the prevailing scripting-language design philosophy of "core minimal, libraries optional" to "core encyclopedic, libraries unnecessary"; and **wherein** said threading-operator family of (e) operates as a **three-axis universal-access protocol** uniformly covering: (k.i) **the callable axis** — every function-class expression in the language (built-in functions, user-defined functions, closures, lambdas, arrow-block expressions, operator expressions wrapped via arrow-block, and placeholder-substituted call sites) is uniformly addressable as a pipeline stage with zero adapter code, zero registration requirement, and zero type-system constraint; (k.ii) **the value axis** — every value-class in the language (primitive scalars; collections including arrays, hashes, sets; user-defined structures and class instances providing object-oriented programming; nested-structures of arbitrary depth; mixed-type heterogeneous collections) flows through threading uniformly without type-system adapters or "stream-compatible" interface implementations; (k.iii) **the reflection axis** — language metadata including method enumeration, field enumeration, class-hierarchy inspection, dynamic method dispatch by name-string, and dynamic field access/mutation by name-string is itself reified as threadable data and composes with the callable and value axes at no syntactic boundary; and **wherein** the absence of categorical exclusion across all three axes is itself a patentable architectural commitment, in that competitor languages whose pipeline paradigm excludes even one cell from the (callable × value-class × reflection-metadata) cross-product fall outside this claim.

**Stacked world-firsts (20 dependent claims):**

1. **First syntax for two-axis positional-argument access (index × depth) across closure nesting and function boundaries** — `_N<...<` family.
2. **First language to ship four-way cross-tradition aliasing for the default closure slot:** `_` ≡ `$_` ≡ `_0` ≡ `$_0`.
3. **First language with orthogonal depth-encoding decoupled from spelling choice.**
4. **First polymorphic literal-typed range syntax** spanning int + char + roman + date + datetime + IP without trait/protocol boilerplate.
5. **First scripting language with encyclopedic no-import bundled stdlib (3000+) outside Mathematica.**
6. **First general-purpose scripting language with git operations as language builtins.**
7. **First language with parallelism (par_pipeline, par_sed, par_walk) as syntactic-primitive verbs rather than library calls.**
8. **First scripting language synthesizing Clojure threading + Racket arrows + Scala underscore + Perl sigils + Ruby p/string-interp into a unified syntax.**
9. **First scripting language with absorbed jq DSL as a single builtin verb.**
10. **First non-Lisp language to ship pipe operator (`|>`) AND threading macros (`->`, `->>`, `~>`, `~>>`) as semantically-distinct first-class operators.**
11. **First language with three-form threading in one operator** — bare-function, inline arrow-block, anonymous-positional-placeholder.
12. **First `>{ ... }` arrow-block sigil** for inline anonymous transforms in threaded pipelines.
13. **First anonymous-positional-placeholder threading** (`f(_, 5)`, `f(5, _)`, `f(5, _, 10)`).
14. **First scripting language to systematically ship parallel short/long namespaces** (`tm`/`trim`, `rv`/`reverse`, etc.).
15. **First scripting language to ship terminal-ASCII data visualization as language-level builtins** (`histo`, `sparkline`, `barchart`, `table`).
16. **First pipeline substrate operating as a three-axis universal-access protocol** (callable × value-class × reflection-metadata) with zero categorical exclusion. The exclusion-free property is the architectural commitment defended against design-arounds: any imitator who excludes even one cell falls outside the claim.
17. **First scripting language to ship eight purpose-built globally-named introspection hashes** (`%b`/`%all`/`%pc`/`%e`/`%a`/`%d`/`%c`/`%p`) populated at compile time from source-of-truth parsing, providing O(1) bidirectional indexing.
18. **First language to support intra-expression composition of pipe-forward and threading-macro operators** — `~> seed stage1 stage2 |> stage3` mixes `~>` and `|>` in a single expression.
19. **First scripting language to ship bidirectional source-level conversion to its parent-language tradition (Perl) as a built-in subcommand** — `stryke convert` (Perl → stryke) and `stryke deconvert` (stryke → Perl).
20. **First scripting language to ship empirically-validated full Perl 5 compatibility (`--compat` mode) on a JIT'd runtime** — verified by a 20,000-test parity corpus pinning behavior against upstream Perl 5. The 20,000-test parity corpus IS the operational specification of Perl-compat — competitors must reproduce that corpus to make a comparable claim.

**META-CLAIM** (the architectural pattern unifying claims #5, #6, #7, #9, #15, plus Patents F/G/H, plus ~1500 of the 3244 builtins):

> **First scripting language to systematically absorb userspace tooling categories — version control, structured-data query, parallel processing, terminal visualization, cryptography, statistics, linear algebra, networking, compression, structured-data serialization, HTTP-server, SQLite, DataFrame, PDF generation, testing framework, file watcher, profiler, formatter, language server, REPL, dependency manager, documentation system, AOP, AI primitives, web-framework primitives, package-management primitives — into the core language as builtin verbs and language subsystems, rather than library imports or subprocess invocations.**

The category-absorption pattern is itself a defensible meta-claim: a competitor adding ONE absorbed category trivially imitates one feature without infringing the meta. The systematic absorption across 25+ categories is what's claimed.

---

## Patent E: Distributed orchestration + stress-testing infrastructure

A fifth omnibus claim covering language-level distributed fleet orchestration, with `cluster()` SSH worker pools, agent/controller architecture, and bare-metal stress-testing builtins.

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a built-in distributed-execution primitive `cluster()` that opens persistent SSH connections to remote hosts, spawns persistent worker processes (`stryke --remote-worker`), and exposes the resulting worker pool as a language-level value, with `pmap_on $cluster { ... }` distributing work across all slots with work-stealing; (b) an **agent/controller orchestration architecture** wherein `stryke agent --controller HOST:PORT` runs on each fleet node and registers with a central controller, while `stryke controller` provides an interactive REPL with fleet-wide commands (`status`, `fire SECS`, `terminate`, `shutdown`); (c) **bare-metal stress-testing builtins** (`stress_cpu`, `stress_mem`, `stress_io`, `stress_test`, `heat`) that pin every CPU core to 100% TDP for a specified duration, exposed as language-level verbs composable with the threading-operator family; (d) **mTLS controller-agent communication** for production deployment; (e) **audit log of all controller commands** for compliance; (f) **auto-terminate guards** including max temperature thresholds (Celsius), max session duration, and ack timeout fallback. **Wherein** said language is positioned as a "server farms first" language — the first programming language designed from the ground up for distributed infrastructure load testing, capacity validation, and BCP/DR exercises — and **wherein** the same single binary functions as: standalone interpreter, fleet agent, fleet controller, stress-testing tool, and AOT-compile target, with mode selection via subcommand (`stryke`, `stryke agent`, `stryke controller`, `stryke build`, `stryke --remote-worker`).

**Why this is its own omnibus, not a sub-claim of A-D:**

| Existing claim | Covers | Does NOT cover |
|---|---|---|
| Patent A (AOT) | single-binary AOT compile | fleet deployment of those binaries |
| Patent B (zshrs daemon) | per-machine shell coordination + cross-host federation hint | language-level fleet REPL, agent/controller protocol, bare-metal stress builtins |
| Patent C (fusevm) | runtime substrate | distributed execution model |
| Patent D (stryke language) | syntactic + semantic primitives | remote/distributed extension as architectural commitment |

Patent E is the strykelang-side equivalent of zshrs's daemon (Patent B) extended to fleet scale — and adds the bare-metal stress-testing dimension that no shell daemon provides.

**Prior-art landscape:**

| Tool / framework | Year | Language-level orchestration? | Single-binary deploy? | REPL-as-controller? | Stress-testing builtins? |
|---|---|---|---|---|---|
| Puppet | 2005 | no — YAML DSL | no — Ruby runtime | no | no |
| Salt | 2011 | no — YAML DSL | no — Python runtime | partial | no |
| Ansible | 2012 | no — YAML DSL | no — Python runtime | no | no |
| Chef | 2009 | partial (Ruby DSL) | no — Ruby runtime | no | no |
| Spark | 2014 | partial (Scala/Python API) | no — JVM | partial (spark-shell) | no |
| Dask distributed | 2014 | partial (Python API) | no — Python runtime | partial | no |
| Ray | 2017 | partial (Python API) | no — Python runtime | partial | no |
| Kubernetes | 2014 | no — YAML manifests | no — container images | no | no |
| Nomad | 2015 | no — HCL DSL | yes (Go binary) | no | no |
| Fabric (SSH) | 2008 | partial (Python API) | no — Python on remote | no | no |
| stress-ng | 2016 | n/a — standalone tool | yes (binary) | no | yes (sole purpose) |
| **stryke distributed** | **2026** | **yes — orchestration IS the language** | **yes — AOT'd binaries, no runtime needed** | **yes — controller is stryke REPL** | **yes — `heat`, `stress_*` as language builtins** |

**No prior tool ships all five: language-level orchestration + single-binary agents + REPL-as-controller + bare-metal stress builtins + AOT-compile-everything.** Each existing tool fails at least one cell:

- Spark/Ray/Dask: have language-level APIs but require runtime on every node (kills "scp anywhere" deploy)
- Nomad/Consul/HashiCorp: have single-binary deployment but use YAML/HCL DSL, not host language
- Fabric: REPL-driven for SSH but requires Python on remote and isn't language-level
- Ansible/Puppet/Chef/Salt: YAML/DSL configuration languages, not general-purpose
- stress-ng: dedicated stress tool but isn't a programming language

**Stacked world-firsts (dependent claims):**

1. **First scripting language with `cluster()` SSH worker pool as a built-in primitive** — opens persistent connections, spawns `--remote-worker` processes, exposes pool as language value.
2. **First scripting language with `pmap_on $cluster` work-stealing distribution** — composes with the standard parallel-builtin family.
3. **First scripting language with agent/controller fleet REPL** — `stryke controller` is a stryke REPL with fleet-wide commands.
4. **First scripting language with bare-metal stress-testing builtins** (`stress_cpu`, `stress_mem`, `stress_io`, `stress_test`, `heat`) that pin CPU cores to 100% TDP.
5. **First scripting language with mTLS controller-agent communication + audit log + auto-terminate guards** as a unified production-grade orchestration architecture.
6. **First scripting language whose same binary functions as interpreter / AOT target / fleet agent / fleet controller / stress tool** with mode selection via subcommand.

---

## Patent F: AI primitives as language builtin

A sixth omnibus claim covering **AI as a language primitive, not a library**. The simplest invocation is `ai $prompt`; the complex form composes from the same primitive — there is no separate "advanced API."

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a built-in `ai` primitive (two-letter, no-import, always-available) that accepts a prompt and optional context value and runs an agent loop to completion (tool call → tool result → next call → final answer); (b) said `ai` primitive supporting **three syntactic invocation forms**: function call (`ai "summarize", $doc`), threading-macro (`~> $doc ai "summarize"`), and pipe-forward (`$doc |> ai "summarize"`); (c) a **`tool fn` declaration** that marks a function as agent-callable, with build-time generation of (c.i) JSON schema from the function signature and parameter types, (c.ii) tool description from the function docstring, (c.iii) automatic registration of all in-scope `tool fn`s with subsequent `ai` calls; (d) a **declarative `mcp_server { ... }` block** that compiles to a spec-compliant MCP (Model Context Protocol) server with one or more tool/resource/prompt declarations, exposable via stdio/websocket/HTTP transports, with a `stryke build --mcp-server` flag emitting a standalone binary; (e) an **`mcp_connect` client** that connects to remote MCP servers via stdio/HTTP/websocket transports, with discovered tools/resources/prompts auto-attaching to subsequent `ai` calls; (f) **AI collection builtins** (`ai_filter`, `ai_map`, `ai_classify`, `ai_sort`, `ai_match`, `ai_dedupe`) that compile collection operations to single batched LLM calls where possible; (g) **provider-agnostic with namespaced extensions** — same `ai` call hits Claude, GPT, Gemini, or local llama.cpp, with provider chosen by `stryke.toml` config and swappable at runtime; (h) **local-fallback-always-works** semantics — stryke binaries can ship with a quantized model linked statically, so `ai` works offline with degraded quality; (i) **cost-aware-by-default** runtime including result cache, automatic batching, parallel rate-limit-aware execution, hard cost ceilings (`max_cost_run`), cost introspection (`ai_cost`), and pre-flight token estimation (`tokens_of`); (j) **deterministic-in-tests** semantics via `ai_mock { ... }` blocks that intercept every AI primitive in scope, with pattern-matching on prompts and structured/string/generator responses; **wherein** the AI subsystem composes with the rest of the language: web-framework controllers (per Patent G), package-manager dependency declarations, cluster-dispatch (per Patent E), effect handlers, capability-based access control. **Wherein** said language is positioned as the first AI-native general-purpose scripting language where AI is a language primitive rather than a third-party library, framework, or SDK.

**Why combination, not individual primitives:**

| Primitive | Prior art |
|-----------|-----------|
| LLM API call | OpenAI Python SDK (2020); Anthropic Python SDK (2022); LangChain (2022); LlamaIndex (2022); every-framework-and-its-cousin |
| Tool/function calling with JSON schema | OpenAI function calling (2023); Anthropic tools API (2024); LangChain Tools |
| MCP (Model Context Protocol) | Anthropic published spec (2024-11) |
| Agent loop | LangChain agents; AutoGPT (2023); BabyAGI (2023); CrewAI (2024) |
| Streaming responses | every modern LLM SDK |
| Cost tracking | LangChain callbacks; OpenAI usage API; manual instrumentation |
| Test mocking | unittest.mock; vcr.py; every-test-framework |
| Local model inference | llama.cpp (2023); ollama (2023); LM Studio (2023) |
| Multi-provider abstraction | LangChain; LiteLLM (2023); aisuite (2024) |

**Why combination is non-obvious:**

1. **AI-as-builtin teaching.** Every existing language treats AI as a library/SDK that must be imported. The accepted convention is "language is general-purpose, AI is a domain library." Stryke's commitment to `ai` as a built-in primitive (no import, always available, two letters) inverts the convention. PHOSITAs in language design are taught to keep stdlibs minimal; this claim explicitly violates that teaching.
2. **MCP-native combined with provider-agnostic abstraction.** No prior language has both (a) a declarative `mcp_server { ... }` block AND (b) automatic MCP-server-to-`ai`-call attachment — they exist as separate library layers if at all.
3. **Build-time JSON schema from function signatures.** TypeScript/Python practice requires manual schema declaration alongside function definition (see LangChain, OpenAI Python SDK). Build-time schema-generation-from-signature is novel.
4. **AI collection builtins compile to batched calls.** Every other framework treats `[f(x) for x in items]` as N separate LLM calls. `ai_map` compiles to a single batched prompt with cost-aware batching. No prior framework ships this as a language-level primitive.
5. **Cost-as-runtime-concern.** Cost tracking exists in libraries; **cost-aware execution as a runtime guarantee** (with hard ceilings that abort the program) is novel. PHOSITAs treat cost as a deployment/billing concern, not a language-runtime concern.
6. **Composition with web framework / package manager / cluster.** No prior AI library composes natively with HTTP frameworks, package managers, and distributed-execution primitives at the language level — they're all separate library imports requiring glue code. Stryke's design has them all touch the same `ai` primitive.
7. **Cross-tradition synthesis.** Combining (a) language-builtin AI, (b) MCP-native, (c) `tool fn` build-time schema, (d) AI collection builtins, (e) local-fallback-always, (f) cost-aware ceilings, (g) deterministic test mocking, (h) cluster-dispatch composition — eight independent AI-architecture commitments unified in one omnibus.

**Stacked world-firsts (dependent claims):**

1. **First general-purpose scripting language with `ai` as a no-import language builtin.** Two-letter primitive, always available.
2. **First language with three-form AI invocation** — function call, threading-macro, pipe-forward all compile to the same primitive.
3. **First language with `tool fn` declaration generating JSON schema + description at build time** from function signature and docstring, with automatic in-scope tool registration.
4. **First language with declarative `mcp_server { ... }` block** compiling to spec-compliant MCP server, with `stryke build --mcp-server` standalone-binary emission.
5. **First language with `mcp_connect` clients that auto-attach discovered tools/resources/prompts to subsequent `ai` calls** with no re-registration step.
6. **First language with AI collection builtins (`ai_filter`/`ai_map`/`ai_classify`/`ai_sort`/`ai_match`/`ai_dedupe`)** compiling to single batched LLM calls.
7. **First language with provider-agnostic AI calls + namespaced provider extensions** — `ai` is one builtin; `[ai]` config picks provider; provider-specific options via namespaced extensions.
8. **First language with statically-linked local-LLM fallback** — `ai` works offline with degraded quality regardless of API connectivity.
9. **First language with cost-aware runtime** — result cache + batching + parallel + hard ceiling (`max_cost_run`) + introspection (`ai_cost`) + pre-flight estimation (`tokens_of`) as runtime guarantees, not library APIs.
10. **First language with `ai_mock { ... }` deterministic test interception** — patterns match prompts; CI fails build on unmatched live calls (`STRYKE_AI_MODE=mock-only`).
11. **First language whose AI primitive composes natively with web framework, package manager, cluster dispatch, and (planned) effect-system + capability-system** at the language level.

**Closest analog in patent literature:** OpenAI's function-calling and Anthropic's tools API have product implementations but limited issued patents. LangChain is open-source library, no patent claims. MCP spec is open-source from Anthropic, no patent claims. **No prior patent covers the eleven-primitive combination applied to a general-purpose scripting language.**

---

## Patent G: Web framework

A seventh omnibus claim covering **Stryke Web** — Rails-grade developer experience compiled to single statically-linked binary with thread-per-core io_uring runtime.

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a web application framework hosted by a dynamic-scripting language runtime (fusevm per Patent C) wherein (a.i) routes are declared via Rails-style DSL (`route :GET, "/", home#index; resources :posts; namespace :api ...`) parsed at build time and **compiled into a radix trie with parameter-capture indices serialized into the binary as a static lookup table**, with match resolving in 50-200ns with zero allocation; (a.ii) controller methods receive request/response/params/session/cookies/flash as in-scope methods (Rails-style ergonomics, explicit method-table entries on `Controller` base class, no method_missing magic); (a.iii) ORM models compose chainably (`Post.published.recent.limit(20)`) with N+1 detection at compile time; (a.iv) view templates compile to native code; (b) a runtime model of **thread-per-core with io_uring on Linux** (glommio-backed, one executor per core, pinned, with `SO_REUSEPORT` for kernel-side load balancing, per-core memory pools, per-core connection state, per-core arena allocator) and tokio M:N fallback on macOS/Windows; (c) **per-request arena allocator** wherein every request allocates from a bumpalo-style arena, all parsed headers + parameters + response body bump a pointer in the arena, response completion drops the entire arena in one `free()` (zero individual deallocations on the hot path); (d) **HTTP/1.1, HTTP/2, HTTP/3 in the same binary** (httparse + custom fast-path for HTTP/1; h2 crate for HTTP/2; quinn for HTTP/3 over QUIC, opt-in via config) sharing the same handler API, with version negotiation via ALPN and Alt-Svc; (e) **rustls + kTLS on Linux ≥ 4.13** for ~2x OpenSSL throughput; (f) WebSocket (`ws "/chat", chat#stream`) and Server-Sent Events (`sse "/events", events#stream`) as first-class route declarations; (g) deployment via single statically-linked native binary produced by `s build --release` (per Patent A.script-AOT track) — `scp target/release/myapp prod:` is the entire deploy pipeline, no PaaS, no Docker, no nginx required, no `bundle install` / `node_modules` / `pip install`; (h) zero install-time code execution on target machine — the binary is the app, the OS is the runtime, nothing else; (i) `s new myapp --web` scaffolding generators producing working CRUD app in under 30 seconds. **Wherein** said framework targets top-3 TechEmpower throughput (>3M req/s plaintext at maturity), <5ms cold start, <15MB idle memory, <4KB per concurrent connection — public commitments tracked in CI with regressions blocking merge.

**Why combination, not individual primitives:**

| Primitive | Prior art |
|-----------|-----------|
| Rails-style routing DSL | Rails (2004); Phoenix (2014); Rocket (Rust, 2017); Sinatra (2007) |
| Thread-per-core with io_uring | seastar (ScyllaDB, 2014); glommio (2020); monoio |
| Compile-time routing trie | Rocket; some actix-web versions; matchit crate |
| Per-request arena allocator | hyper (partial); some C++ frameworks (drogon, lithium) |
| HTTP/1+2+3 in one binary | actix-web, axum (with separate features) |
| Single-binary deploy | Go web frameworks (Gin, Echo, Fiber); Phoenix releases |
| Rust-runtime web framework | actix-web (2017); axum (2021); rocket; warp |
| Rails-DX ergonomics | Phoenix (Rails-inspired but Elixir); Buffalo (Go); Loco (Rust, 2024) |

**Why combination is non-obvious:**

1. **Rails-DX + native compilation teaching.** Every prior framework forces a choice: Rails-DX (Ruby/Phoenix/Buffalo) with runtime overhead, OR native compilation (actix-web/axum) with much-coarser DSL. The PHOSITA in web framework design treats these as a tradeoff curve with no Pareto-dominant point. Stryke web claims to land both: Rails-DX + AOT-compiled native code. The combination requires a host language that has both (a) a Rails-quality DSL surface and (b) a native AOT compile path — almost no language has both.
2. **Single-binary deploy + Rails-DX.** Go has single-binary deploy but no Rails-DX (Gin/Echo/Fiber are explicit, low-magic). Phoenix has Rails-DX but ships a BEAM runtime (not a true single-binary). The combination has no prior art.
3. **Convention-over-configuration in a strongly-typed-runtime context.** Most "convention" frameworks (Rails, Django, Phoenix) live in dynamically-typed languages where convention emerges from runtime introspection. Stryke web has gradual typing (per Patent D) AND convention-over-configuration AND build-time route compilation — three commitments most frameworks pick one or two of.
4. **Thread-per-core io_uring + script-language runtime.** No prior dynamic-scripting-language web framework runs on glommio/io_uring at the runtime level. Python/Ruby/Node runtimes have fundamentally incompatible threading models. fusevm (Patent C) makes thread-per-core feasible because it has no GIL and Rayon-native parallelism.
5. **Deployment story dominance.** Single-binary scp = `s build --release && scp target/release/myapp prod:` — every Rails-DX framework requires more (Rails: bundler+Ruby+Passenger; Django: gunicorn+Python+nginx; Phoenix: BEAM+release-tooling; Express: node+npm+pm2). Stryke web's deploy story is Go-grade.

**Stacked world-firsts (dependent claims):**

1. **First web framework combining Rails-grade DX + AOT-compiled-to-native-code single-binary deploy.** Goes beyond what Rails/Phoenix/actix-web/axum each ship individually.
2. **First scripting-language web framework on thread-per-core io_uring.** No GIL, no Python/Ruby threading limitations.
3. **First web framework with build-time radix-trie routing + Rails-style `resources :posts` DSL** — the DSL expands at compile time to the seven CRUD routes serialized as static lookup tables.
4. **First web framework with WebSocket and SSE as first-class route declarations** in a Rails-style DSL.
5. **First web framework whose entire dependency graph (framework + user code + stdlib) compiles to native machine code via Cranelift** (per Patent C + Patent A.script-AOT).
6. **First web framework explicitly committing to top-3 TechEmpower performance** with public CI-tracked benchmarks — public-commitment performance targets are a deliberate marketing/architectural claim.
7. **First web framework whose `s new myapp --web` produces working CRUD app in <30 seconds with zero PaaS / Docker / nginx / bundle / npm / pip dependencies.**

**Closest analog in patent literature:** Web framework patents are sparse — Rails/Django/Phoenix/actix-web are open-source with no patent claims. Microsoft has ASP.NET patents (mostly process patents around request lifecycle). Google has gRPC patents. **No prior patent covers the seven-primitive combination of Rails-DX + AOT-native-compile + io_uring + radix-trie-build-time + per-request-arena + HTTP-1/2/3-unified + zero-target-runtime applied to a dynamic-scripting language.**

---

## Patent H: Package registry/manager

An eighth omnibus claim covering the **stryke package manager** — Cargo's model + uv's execution speed + Nix's reproducibility + Bundler's lockfile sacredness + npm's `[scripts]` table — with the "kill feature" that `s build --release` AOT-compiles **the entire program (user code + every dep + stdlib) through Cranelift to native machine code**, producing a single statically-linked binary.

**The omnibus claim shape (engineering sketch):**

A method comprising — (a) a TOML-based manifest (`stryke.toml`) with sections for package metadata, dependencies (with semver ranges, exact pins, path-deps, git-deps), dev-deps, arbitrary-named groups (Bundler-style beyond dev/prod), features (per-package scoped, not unified workspace-wide), `[scripts]` table for project-local task running (npm's one good idea), `[bin]` entries for executable targets, and `[workspace]` for first-class multi-package workspaces; (b) a deterministic lockfile (`stryke.lock`) with hash-pinned integrity (Nix-style reproducibility) — every dependency is hash-pinned, two `s install`s from the same lockfile on different machines produce byte-identical store contents; (c) a global content-addressable store at `~/.stryke/store/name@version/` (human-readable paths, not Nix-style hash-paths) with hash-pinning happening in the lockfile; (d) a parallel resolver and parallel fetch/extract/verify (uv-style execution speed, milliseconds not minutes); (e) a unified CLI surface — `s init`, `s new myapp`, `s build`, `s run`, `s test`, `s bench`, `s doc`, `s check`, `s fmt`, `s clean`, `s install`, `s add`, `s remove`, `s update`, `s tree`, `s outdated`, `s audit`, `s publish`, `s yank`, `s search` — one binary, one mental model; (f) **deliberate exclusions** — no per-project deps tree (no `node_modules`/`vendor`/`packages`), no install-time code execution (no `build.rs`/`postinstall`), no hoisting, no phantom deps, no peer deps, no mutable registries (no left-pad re-runs); (g) **first-class private registries** (no centralized monoculture); (h) **the killer feature** — `s build --release` AOT-compiles the entire program (user code + every transitively-resolved dep + stdlib) through Cranelift to native machine code, producing a single statically-linked ELF/Mach-O/PE binary in `target/release/`, no interpreter required on target machine, no JIT warmup, no bytecode at runtime — Perl-grade ergonomics with Go-grade binaries. **Wherein** said package manager combines (i) Cargo's manifest+lockfile+resolver model, (ii) uv's parallel-Rust-native execution, (iii) Nix's hash-pinned reproducibility, (iv) Bundler's sacred-lockfile discipline, (v) npm's `[scripts]` table — picking proven winners from a decade of design experiments while skipping the legacy mistakes (no node_modules / install-time-code / hoisting / phantom-deps / mutable-registries / centralized-monoculture).

**Why combination, not individual primitives:**

| Primitive | Prior art |
|-----------|-----------|
| TOML manifest | Cargo (Rust, 2014); Pyproject.toml (PEP 621, 2020); Bundler-Gemfile is Ruby-DSL |
| Deterministic lockfile | Cargo (Cargo.lock); Yarn (yarn.lock, 2016); Bundler (Gemfile.lock, 2010); pip-tools (requirements.txt, 2010s) |
| Hash-pinned reproducibility | Nix (2003); NPM lockfile-hashes (since 5.0); Cargo Cargo.lock with Cargo-checksums |
| Parallel resolver | uv (2024); pnpm (2017); Cargo's resolver; Renovate |
| Global content-addressable store | Nix; pnpm store; Maven local repo |
| `[scripts]` table | npm package.json scripts; just/Justfile |
| Workspace support | Cargo workspaces; npm workspaces; Yarn workspaces |
| AOT compile-everything-including-deps | GraalVM native-image (2018); Go (compiles deps as part of binary); Rust (similar); Nuitka (2007 Python) |
| AOT compile dynamic-language program-and-deps | Nuitka (Python, partial); GraalVM (JVM); **none for general-purpose scripting languages** |

**Why combination is non-obvious:**

1. **Five-tradition synthesis applied to package management.** Each of (Cargo, uv, Nix, Bundler, npm-`[scripts]`) is the canonical leader in its own dimension. Picking proven-winners from five separate package-manager traditions and unifying them is itself a deliberate cross-tradition design act. Most package managers either (a) inherit from one tradition (Bundler ≈ Gemfile-style, Cargo ≈ Rust-only), (b) accumulate ad-hoc features (npm), or (c) reinvent everything (Nix). Stryke's package manager is the first to do principled multi-tradition synthesis at the design-doc level.
2. **AOT-compile-everything-via-Cranelift for a dynamic-scripting language.** GraalVM native-image does this for JVM bytecode (typed languages); Nuitka does partial for Python. **No prior dynamic scripting language ships `package_manager build --release → native binary including all transitively-resolved deps + stdlib` with this level of completeness.** Stryke's claim is that you write Perl-flavored stryke, declare deps in `stryke.toml`, run `s build --release`, and get a Go-grade binary out.
3. **Deliberate exclusions are themselves a claim.** "No node_modules" / "no install-time code execution" / "no hoisting" / "no phantom deps" / "no peer deps" / "no mutable registries" / "no centralized monoculture" — each exclusion fixes a known package-manager footgun. The set of exclusions IS the design commitment.
4. **First-class private registries.** Centralized monocultures (npm, PyPI) have failure modes (left-pad, name-squatting, supply-chain attacks). Stryke commits to private-registry-first as a structural decision.
5. **Per-package scoped features** vs Cargo's unified-workspace-wide features. Cargo's biggest footgun (a consumer turning on `feature = "yaml"` silently flips it on for every other package in the graph) is fixed by scoping features per-package.

**Stacked world-firsts (dependent claims):**

1. **First package manager to synthesize Cargo + uv + Nix + Bundler + npm-`[scripts]` traditions** with explicit exclusion of node_modules / install-time-code / hoisting / phantom-deps / mutable-registries / centralized-monoculture.
2. **First package manager for a dynamic scripting language to AOT-compile the entire program (user code + every transitively-resolved dep + stdlib) to native machine code via Cranelift**, producing a single statically-linked binary with no target-machine runtime requirement.
3. **First package manager with per-package scoped features** (Cargo's unified-workspace-feature footgun fixed).
4. **First package manager with human-readable global-store paths AND hash-pinned lockfile reproducibility** — Nix's reproducibility without Nix's opaque paths.
5. **First package manager whose `s build` produces three artifacts from one source: standalone interpreter target, AOT-compiled native binary, MCP-server binary (per Patent F)** via flag selection.

**Closest analog in patent literature:** Package manager patents are sparse and mostly about specific algorithms (Microsoft's NuGet, Apple's CocoaPods). **No prior patent covers the multi-tradition synthesis + AOT-native-compile-everything combination applied to a dynamic scripting language.**

---

## Filing Strategy for the Eight-Omnibus Portfolio

| Omnibus | Cost (provisional, micro) | Cost (provisional, small) | Locks |
|---|---|---|---|
| A: unified-AOT (shell + script) | ~$65 | ~$300 | shell-binary + script-AOT trailer-format |
| B: zshrs daemon | ~$65 | ~$300 | shell-coordination architecture |
| C: fusevm runtime | ~$65 | ~$300 | three-stage-JIT + no-GC + parallel + AOT + embeddable |
| D: stryke language design | ~$65 | ~$300 | 20 dependent claims + meta-claim + 3-axis universality + Perl-parity |
| E: distributed orchestration + stress | ~$65 | ~$300 | language-native fleet REPL + cluster + heat builtins |
| F: AI primitives | ~$65 | ~$300 | `ai` builtin + `tool fn` + MCP-native + collection builtins + cost-aware |
| G: web framework | ~$65 | ~$300 | Rails-DX + thread-per-core io_uring + radix-trie-build-time + single-binary |
| H: package registry/manager | ~$65 | ~$300 | five-tradition synthesis + AOT-everything via Cranelift |
| **Total** | **~$520** | **~$2400** | **8 independently-assertible omnibus claims** |

**File all eight under the same priority date** when possible; cross-reference in the specifications so the dependent-claim space spans the full portfolio.

**The eight omnibuses combined create the widest possible moat** — competitors must work around all eight to ship anything architecturally similar to the zshrs+stryke+fusevm trio. Any partial overlap (e.g., a competing language that adds `ai` builtin without the web framework) hits at least one omnibus.

**PCT international filing** within 12 months of any provisional gives priority in 150+ countries — strongly recommended given that zshrs/stryke have global use cases.

**Defensive publication** (IP.com, Defensive Publications repository) for items not patented but where blocking others is desirable — free, locks prior art.

**Public-disclosure clock awareness:** US gives a 1-year grace period from public disclosure; EU and most international jurisdictions bar patent rights from the moment of public disclosure. Anything already publicly committed to the zshrs/stryke/fusevm git repos started a 1-year US clock and may have already barred international rights. **Audit first-public-disclosure dates per omnibus claim element** before deciding which claims still have international priority headroom.

---

## Three Structural Moats — Summary

1. **Three-axis universal-access protocol** (Patent D, claim #16) — pipeline substrate covers (callable × value-class × reflection-metadata) cross-product with zero categorical exclusion.
2. **20,000-test Perl-parity corpus** (Patent D, claim #20) — empirical specification of `--compat` mode behavior; competitors must reproduce 20,000 tests passing to make a comparable claim.
3. **Systematic-absorption-of-userspace-tooling meta-claim** (Patent D meta + Patents F/G/H) — first to absorb 25+ categories (git, jq, parallel, visualization, crypto, stats, linalg, networking, compression, serialization, HTTP-server, SQLite, DataFrame, PDF, testing, file-watcher, profiler, formatter, LSP, REPL, dep-manager, docs, AOP, AI primitives, web framework, package manager) as core builtin verbs and language subsystems.

Each moat is independently load-bearing; combined they defeat design-arounds across categorical (#16), empirical (#20), and architectural (meta-claim) axes simultaneously.

---

## How to Apply This Memory

- Treat each omnibus (A through H) as **independently assertible**. Engineering proposals affecting any one are patent-relevant per that omnibus's dependent-claim space.
- New `z*` builtins → dependent claims under B (zshrs daemon).
- New JIT tier optimizations or runtime architectural changes → dependent claims under C (fusevm).
- New language-level syntactic primitives → dependent claims under D.
- New AOT-binary capabilities → dependent claims under A.
- New cluster/agent/controller/stress-testing capabilities → dependent claims under E.
- New AI primitives, MCP features, tool-fn extensions → dependent claims under F.
- New web framework capabilities (routing DSL extensions, runtime improvements, deploy enhancements) → dependent claims under G.
- New package-manager capabilities → dependent claims under H.
- When a feature spans multiple omnibuses (e.g., a new parallel primitive that's both a fusevm runtime feature AND a stryke language builtin), file under both as cross-claim dependent material.
- Engineering analysis here; legal claims belong to a real patent attorney specializing in compiler/runtime/language patents.

---

**Canonical source-of-truth:** `/Users/wizard/.claude/projects/-Users-wizard-RustroverProjects-zshrs/memory/aot_patent_strategy.md` (zshrs-project memory). This file is the strykelang-repo slice, kept in sync with the canonical when significant updates land. If conflicts exist, the canonical wins; reconcile here.
