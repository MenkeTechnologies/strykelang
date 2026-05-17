//! Apples-to-apples bench: rkyv-backed `ScriptCache` vs the pre-rkyv
//! SQLite-zstd-bincode bytecode cache.
//!
//! Two scenarios measured per backend:
//!
//!   * `steady_state` — open the cache once, do one lookup per iteration.
//!     Models long-running drivers, REPL, server.
//!
//!   * `per_process` — open the cache, do one lookup, drop it. Per iteration.
//!     Models the actual `s test t` workload where each test file is its
//!     own short-lived process.
//!
//! Run with `cargo bench --bench cache_bench`.
//!
//! The pre-rkyv SQLite path is intentionally kept here as a frozen
//! comparison baseline — it doesn't appear anywhere in production code.

use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rusqlite::{params, Connection, OptionalExtension};
use tempfile::TempDir;

use stryke::ast::Program;
use stryke::bytecode::Chunk;
use stryke::compiler::Compiler;
use stryke::parse;
use stryke::script_cache::ScriptCache;

// ── Frozen baseline: pre-rkyv SQLite-zstd-bincode cache ──────────────────────

fn sqlite_open(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-64000;
         PRAGMA mmap_size=268435456;
         PRAGMA temp_store=MEMORY;",
    )
    .unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS scripts (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            mtime_secs INTEGER NOT NULL,
            mtime_nsecs INTEGER NOT NULL,
            stryke_version TEXT NOT NULL,
            pointer_width INTEGER NOT NULL,
            program_blob BLOB NOT NULL,
            chunk_blob BLOB NOT NULL,
            cached_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scripts_path ON scripts(path);
        "#,
    )
    .unwrap();
    conn
}

fn sqlite_put(conn: &Connection, path: &str, mtime_s: i64, mtime_ns: i64, p: &Program, c: &Chunk) {
    let pb = bincode::serialize(p).unwrap();
    let cb = bincode::serialize(c).unwrap();
    let pz = zstd::stream::encode_all(&pb[..], 3).unwrap();
    let cz = zstd::stream::encode_all(&cb[..], 3).unwrap();
    conn.execute("DELETE FROM scripts WHERE path = ?1", params![path])
        .unwrap();
    conn.execute(
        "INSERT INTO scripts (path, mtime_secs, mtime_nsecs, stryke_version, pointer_width, program_blob, chunk_blob, cached_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![path, mtime_s, mtime_ns, "bench", 8i64, pz, cz, 0i64],
    ).unwrap();
}

fn sqlite_get(
    conn: &Connection,
    path: &str,
    mtime_s: i64,
    mtime_ns: i64,
) -> Option<(Program, Chunk)> {
    let (pz, cz): (Vec<u8>, Vec<u8>) = conn
        .query_row(
            "SELECT program_blob, chunk_blob FROM scripts WHERE path = ?1 AND mtime_secs = ?2 AND mtime_nsecs = ?3",
            params![path, mtime_s, mtime_ns],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten()?;
    let pb = zstd::stream::decode_all(&pz[..]).ok()?;
    let cb = zstd::stream::decode_all(&cz[..]).ok()?;
    let p: Program = bincode::deserialize(&pb).ok()?;
    let c: Chunk = bincode::deserialize(&cb).ok()?;
    Some((p, c))
}

// ── Corpus ───────────────────────────────────────────────────────────────────

fn build_corpus(n: usize) -> Vec<(String, Program, Chunk)> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let src = format!(
            r#"
            sub mul_{i} {{ $_[0] * $_[1] + {i} }}
            my $sum = 0
            for my $k (1..20) {{
                $sum += mul_{i}($k, $k + 1)
            }}
            $sum
            "#,
            i = i
        );
        let prog = parse(&src).unwrap_or_else(|e| panic!("parse {}: {}", i, e));
        let chunk = Compiler::new()
            .compile_program(&prog)
            .unwrap_or_else(|e| panic!("compile {}: {:?}", i, e));
        out.push((format!("/virtual/script_{}.stk", i), prog, chunk));
    }
    out
}

// ── Fixture: populated rkyv + sqlite caches, alive for the bench's lifetime ──

struct Fixture {
    _dir: TempDir,
    rkyv_path: std::path::PathBuf,
    sqlite_path: std::path::PathBuf,
    corpus: Vec<(String, Program, Chunk)>,
}

fn build_fixture(n_scripts: usize) -> Fixture {
    let corpus = build_corpus(n_scripts);
    let dir = tempfile::tempdir().unwrap();
    let rkyv_path = dir.path().join("scripts.rkyv");
    let sqlite_path = dir.path().join("scripts.db");

    {
        let rkyv = ScriptCache::open(&rkyv_path).unwrap();
        for (path, p, c) in &corpus {
            rkyv.put(path, 100, 200, p, c).unwrap();
        }
    }
    {
        let conn = sqlite_open(&sqlite_path);
        for (path, p, c) in &corpus {
            sqlite_put(&conn, path, 100, 200, p, c);
        }
    }

    Fixture {
        _dir: dir,
        rkyv_path,
        sqlite_path,
        corpus,
    }
}

// ── Bench groups ─────────────────────────────────────────────────────────────

fn bench_steady_state(c: &mut Criterion) {
    let fx = build_fixture(100);
    let mut g = c.benchmark_group("cache_steady_state");

    g.bench_function("rkyv", |b| {
        let cache = ScriptCache::open(&fx.rkyv_path).unwrap();
        // Warm the mmap header check.
        let _ = cache.get(&fx.corpus[0].0, 100, 200).unwrap();
        let mut i = 0usize;
        b.iter(|| {
            let (path, _, _) = &fx.corpus[i % fx.corpus.len()];
            i = i.wrapping_add(1);
            black_box(cache.get(path, 100, 200).unwrap())
        });
    });

    g.bench_function("sqlite", |b| {
        let conn = sqlite_open(&fx.sqlite_path);
        let _ = sqlite_get(&conn, &fx.corpus[0].0, 100, 200).unwrap();
        let mut i = 0usize;
        b.iter(|| {
            let (path, _, _) = &fx.corpus[i % fx.corpus.len()];
            i = i.wrapping_add(1);
            black_box(sqlite_get(&conn, path, 100, 200).unwrap())
        });
    });

    g.finish();
}

fn bench_per_process(c: &mut Criterion) {
    let fx = build_fixture(100);
    let mut g = c.benchmark_group("cache_per_process");

    g.bench_function("rkyv", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let (path, _, _) = &fx.corpus[i % fx.corpus.len()];
            i = i.wrapping_add(1);
            let cache = ScriptCache::open(&fx.rkyv_path).unwrap();
            black_box(cache.get(path, 100, 200).unwrap())
        });
    });

    g.bench_function("sqlite", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let (path, _, _) = &fx.corpus[i % fx.corpus.len()];
            i = i.wrapping_add(1);
            let conn = sqlite_open(&fx.sqlite_path);
            black_box(sqlite_get(&conn, path, 100, 200).unwrap())
        });
    });

    g.finish();
}

criterion_group!(benches, bench_steady_state, bench_per_process);
criterion_main!(benches);
