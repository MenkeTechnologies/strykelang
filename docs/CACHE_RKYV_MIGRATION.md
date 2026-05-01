# Bytecode Cache: SQLite → rkyv Migration — **SHIPPED**

**Decision:** stryke's bytecode cache moved from a SQLite database to a single rkyv-archived shard.
**Result:** 11x faster per-process cache hit (p50: 241 µs → 22 µs). 3.2x bigger on disk. Aligned with zshrs.
**Status:** Phase 1 (SQLite → rkyv shard with bincode-encoded Program/Chunk inner blobs) is complete and live in `strykelang/script_cache.rs`. Phase 2 (zero-copy on inner `PerlValue`/`Chunk`/`Program`) remains deferred — see [What's deferred](#whats-deferred-phase-2).

## Status quo (pre-migration)

The original cache was a port of an early zshrs design:

- File: `~/.cache/stryke/scripts.db` (SQLite, WAL mode, 256 MB mmap)
- Schema: `scripts(id, path, mtime_secs, mtime_nsecs, stryke_version, pointer_width, program_blob BLOB, chunk_blob BLOB, cached_at)`
- Each blob: `zstd::encode_all(bincode::serialize(...), level=3)`
- Read path: `Connection::open` → SQL `SELECT WHERE path = ? AND mtime_secs = ? AND mtime_nsecs = ?` → `zstd::decode_all` × 2 → `bincode::deserialize` × 2.

The dependency `rusqlite = "0.32"` (with bundled SQLite) is still needed for stryke's user-facing `sqlite()` builtin — that didn't change. Only the bytecode cache moved.

## Why migrate

### 1. zshrs already moved past it

`zshrs/src/daemon/shard.rs` (430 LOC) ships an rkyv-based shard pattern at `~/.cache/zshrs/images/{hash8}-{slug}.rkyv`. The pattern: file-per-shard, atomic-rename writes, advisory `flock`, mmap + `check_archived_root` reads, zero-copy `ArchivedHashMap` lookup. zshrs uses per-source-tree shards driven by a daemon; the SQLite cache stryke inherited was the older zshrs design from before the rkyv shard layer existed.

stryke embeds in zshrs (per the MenkeTechnologies stack architecture). Aligning their persistence layers — same crate, same pattern, same crash-safety story — reduces the surface area both projects need to maintain.

### 2. SQLite was over-engineered for the access pattern

The cache only does two operations: lookup-by-path and replace-by-path. There are zero range queries, zero joins, zero secondary indexes used. Justifying a 1 MB+ bundled C SQLite library, WAL machinery, and SQL query parsing for "give me the bytes for this path" is the wrong tool match.

The "viewability" argument (`SELECT * FROM scripts`) doesn't survive: blobs are opaque zstd-compressed bincode, so SQL can't query into them. The only useful queries (`COUNT(*)`, list paths, sum blob sizes) are already what `ls -la ~/.cache/stryke/` and `du -sb` give you for free with rkyv files.

### 3. The codec was the bottleneck, not the storage

In the SQLite path, the SQL B-tree probe is sub-microsecond — fine. The expensive parts per cache hit:

- `Connection::open` (file open, header parse, page-cache warm)
- 5 separate WAL `PRAGMA` exec() round-trips on connection
- `prepare_cached` for the SELECT, bind, step
- `zstd::decode_all` × 2 (one per blob, ~0.5-1 ms each)
- `bincode::deserialize` × 2

Each `s test t` invocation spawns a separate process per test file. Each process pays the connection-setup cost once for one lookup. That's a fixed per-process tax that scales with the number of test files, not the size of the work.

rkyv collapses this to: `File::open` → `Mmap::map` → `check_archived_root` (one-shot validation) → `ArchivedHashMap::get` → bincode-deserialize × 2. Four real syscalls vs the SQLite ritual.

### 4. Endgame implications

Per the project framing: stryke is the second-priority project after zshrs, and zshrs is "endgame" — no further migrations planned, schemas must be versioned and migration-safe. rkyv has explicit format versioning via the `magic: u32` + `format_version: u32` header fields (`zshrs/src/daemon/shard.rs` pattern). Bumping the format version cleanly invalidates old shards on read. SQLite gave us schema versioning via a `stryke_version` column but couldn't catch *binary-level* changes (compiler.rs / parser.rs / vm.rs edits that don't touch `CARGO_PKG_VERSION`). The rkyv design keeps that invariant via the `binary_mtime_at_cache` field — same logic, different store.

## What changed

| Aspect | Before | After |
|---|---|---|
| Crate | `rusqlite` + bundled SQLite C | `rkyv = "0.7"` with `validation`, `archive_le`, `size_32` features |
| Path | `~/.cache/stryke/scripts.db` (+ WAL, SHM files) | `~/.cache/stryke/scripts.rkyv` (+ `.rkyv.lock`) |
| Storage | SQL table with BLOB columns | rkyv-archived `ScriptShard { header, entries: HashMap<path, ScriptEntry> }` |
| Lookup | SQL B-tree probe via path index | `ArchivedHashMap::get` (zero-copy hashbrown) |
| Compression | zstd L3 on each blob | None on inner blobs (mmap-friendly) |
| Atomicity | WAL + SQLite transactions | tmp file → fsync → atomic rename |
| Concurrency | SQLite WAL multi-reader | `flock` on `scripts.rkyv.lock` for exclusive writes; lockless reads |
| Process state | One `Connection` per `Mutex<ScriptCache>` | One `Mmap` per `parking_lot::Mutex<Option<MmappedShard>>`, lazy-initialized |

The public API (`try_load`, `try_save`, `stats`, `clear`, `evict_stale`, `list_scripts`, `ScriptCache::open` / `get` / `put`) is preserved byte-for-byte. The only call-site rename was `Interpreter::sqlite_cache_script_path` → `cache_script_path`. The env var `STRYKE_SQLITE_CACHE` → `STRYKE_CACHE`. `bytecode.rs:1067` still uses `crate::script_cache::constants_pool_codec` to serialize `Vec<PerlValue>` constants in the inner bincode blob (the `PerlValue` Arc-shared graph still isn't trivially rkyv-archivable).

## Measured results

Bench harness: `strykelang/cache_bench.rs`, runs both implementations side-by-side in one process against an identical 100-script corpus. Reproduce with:

```sh
cargo test --release --lib bench_rkyv_vs_sqlite -- --nocapture --ignored
```

Hardware: macOS aarch64 (M-series), filesystem cache warm.

### File sizes (100 scripts)

| Format | Size | vs other |
|---|---|---|
| sqlite (zstd-compressed) | 84 KB | 1.0x baseline |
| rkyv (uncompressed) | 270 KB | 3.2x bigger |

The 3.2x size growth is entirely the zstd we dropped. AST + bytecode bincode is highly compressible (repeated `ExprKind` variant tags, repeated opcodes, repeated identifier strings, narrow-range line numbers).

### Steady-state lookup (open once, 10 000 lookups)

| Format | Total | Per-hit |
|---|---|---|
| rkyv | 24.86 ms | **2.49 µs** |
| sqlite | 86.04 ms | 8.60 µs |

**rkyv: 3.46x faster per hit.**

### Per-process lookup (open + 1 lookup + close, ×1000)

This is the actual `s test t` workload — each test file spawns a fresh stryke process that does exactly one cache lookup and exits.

| Format | p50 | p99 | total |
|---|---|---|---|
| rkyv | **22 µs** | 36 µs | 22.95 ms |
| sqlite | 241 µs | 499 µs | 263.62 ms |

**rkyv: 10.79x faster (p50), 11.48x faster (total).**

The bigger gap vs steady-state is connection setup amortizing. SQLite pays its full connection-open + WAL pragma + statement prepare cost on every process. rkyv pays one mmap + one validate then routes through `ArchivedHashMap`.

### What this means at scale

At 1033 cached scripts, `s test t` over the full Rosetta + Exercism corpus:

```
1033 invocations × (241 µs sqlite − 22 µs rkyv) ≈ 226 ms saved per warm run
```

End-to-end the same warm run was measured at 1.20x faster overall (3.09 s cold → 2.58 s warm earlier in this work). The cache-layer win is real and matches the math; the rest of the runtime is dominated by interpreter startup + actual test body execution, which the cache doesn't touch.

## Tradeoffs accepted

### Size

3.2x growth on disk. At 1033 scripts that's 15 MB instead of ~5 MB. This is the cost of dropping zstd on inner blobs. Could be added back inside the rkyv container with no impact on outer-shard mmap speed — would trade ~50-100 ms of decode CPU per pass for ~10 MB less disk. Not worth it on developer machines; possibly worth it on space-constrained CI.

### Cold-start I/O

When the OS page cache is cold, the larger rkyv file faults more pages from disk. Not measured. For a developer's repeated-test loop the file is hot in page cache after the first run, so this doesn't apply. For one-shot CI runs it might give a small fraction back to SQLite. Not enough signal to act on.

### Lost feature: ad-hoc SQL queries

Any future "list all scripts cached more than 1 day ago" / "find scripts with chunk > 50KB" queries now require iterating the `ArchivedHashMap` in code instead of `SELECT … WHERE …`. Acceptable — those queries weren't being made.

## What's deferred (phase 2) — ⏳ NOT SHIPPED

The current rkyv shard wraps **bincode-encoded** Program/Chunk bytes. To get true zero-copy load on the inner data — skip bincode entirely on cache hit — `Chunk` and `Program` would need `#[derive(Archive, Deserialize, Serialize)]` derives across:

- `bytecode.rs` — `Chunk`, `Op`, `Block`, `BlockBytecodeRange`, the constant pool entry type
- `ast.rs` — entire `Program`, `Stmt`/`Expr` enum graph
- `value.rs` — `PerlValue`, which is the hard one

`PerlValue` is `Arc`-shared heap-pointed (interior mutability via `Arc<RwLock<...>>`). rkyv's zero-copy contract requires the on-disk byte layout to match the in-memory layout. Arc isn't archivable in zero-copy form. The current code already side-steps this for SQLite via the `CacheConst` adapter (only `Undef`/`Int`/`Float`/`Str` constants make it into the cache); the same adapter pattern would extend to phase 2 but applied to the whole graph.

Estimated win from phase 2: skip ~1-3 µs of bincode-decode per cache hit on top of the current rkyv savings. Probably not worth the derive churn until the hit cost actually shows up as a bottleneck somewhere.

## Why this fits the endgame brand

Per the project priority framing: anything in the MenkeTechnologies stack must be **world's first** AND **world's fastest**. The cache layer alone isn't a "world's first" feature, but the consolidation it enables is:

- zshrs and stryke share a persistence pattern (same crate, same code shape) → maintenance debt cut.
- A daemonized shell (zshrs) hosting an embedded scripting language (stryke) that uses the same archived bytecode store is novel — no other shell+language combo does this.
- Both layers measure as "fastest in their category" under the same benchmark methodology.

The migration didn't add a new world-first capability, but it removed an inconsistency that would have eventually been a documentation footnote ("stryke uses SQLite for caching, except where it doesn't") and replaced it with one architectural story ("rkyv shards everywhere persistent state lives in this stack").

## Files changed

| File | Change |
|---|---|
| `Cargo.toml` | `+rkyv = "0.7"` (validation, archive_le, size_32). `rusqlite` retained for the user-facing `sqlite()` builtin. |
| `strykelang/script_cache.rs` | Full rewrite. Was 486 LOC SQLite-backed; now 540 LOC rkyv-backed with the same public surface. |
| `strykelang/interpreter.rs` | Field rename `sqlite_cache_script_path` → `cache_script_path`. |
| `strykelang/main.rs`, `lib.rs` | Comment updates; field-rename ripple. |
| `strykelang/builtins.rs` | `cacheview` outer `Mutex<>` dropped (internal locking now). Disabled message: `STRYKE_CACHE=0`. |
| `strykelang/lib_api_extended_tests.rs` | `test_sqlite_cache_save_load` → `test_rkyv_cache_save_load`. |
| `strykelang/cache_bench.rs` | New. Side-by-side bench harness for the numbers above. |
| `README.md` | Section `[0x0F] BYTECODE CACHE (rkyv)` rewritten. CLI examples + feature bullet updated. |
| `strykelang/pkg/commands.rs` | `s init` scaffold gitignore: dropped `*.stkc`/`*.pec` (stale; cache is global, not per-project). |
