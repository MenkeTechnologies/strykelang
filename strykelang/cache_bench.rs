//! Apples-to-apples bench: rkyv vs SQLite-zstd-bincode bytecode cache.
//!
//! Run with `cargo test --release --lib bench_rkyv_vs_sqlite -- --nocapture --ignored`.
//! `--ignored` so it only fires when explicitly asked (it's a bench, not a correctness test).
//!
//! Two modes measured per format:
//!   - Steady-state: open cache once, do N lookups (within a single process).
//!     Reflects servers / long-running drivers.
//!   - Per-process:  open cache + do 1 lookup + close, repeated. Reflects the
//!     actual `s test t` workload where each test file is its own process.

#![cfg(test)]

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use tempfile::tempdir;

use crate::ast::Program;
use crate::bytecode::Chunk;
use crate::compiler::Compiler;
use crate::parse;
use crate::script_cache::ScriptCache;

// ── Old SQLite-zstd-bincode implementation (verbatim port of pre-rkyv path) ──

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

fn sqlite_get(conn: &Connection, path: &str, mtime_s: i64, mtime_ns: i64) -> Option<(Program, Chunk)> {
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

// ── Bench harness ────────────────────────────────────────────────────────────

fn build_corpus(n: usize) -> Vec<(String, Program, Chunk)> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        // A non-trivial script: arithmetic + a sub + a loop.
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

fn fmt(d: Duration) -> String {
    if d.as_secs_f64() >= 1.0 {
        format!("{:.3}s", d.as_secs_f64())
    } else if d.as_millis() >= 1 {
        format!("{:.2}ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{:.0}µs", d.as_secs_f64() * 1_000_000.0)
    }
}

fn percentile(samples: &[Duration], p: f64) -> Duration {
    let mut sorted: Vec<Duration> = samples.to_vec();
    sorted.sort();
    let idx = ((p / 100.0) * sorted.len() as f64) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
#[ignore]
fn bench_rkyv_vs_sqlite() {
    let n_scripts: usize = 100;
    let n_steady_reads: usize = 10_000;
    let n_per_process: usize = 1_000;

    println!();
    println!("Building corpus ({} scripts)…", n_scripts);
    let corpus = build_corpus(n_scripts);
    println!("Corpus ready.");

    let dir = tempdir().unwrap();
    let rkyv_path = dir.path().join("scripts.rkyv");
    let sqlite_path = dir.path().join("scripts.db");

    // ── Populate both caches ──
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

    let rkyv_size = std::fs::metadata(&rkyv_path).map(|m| m.len()).unwrap_or(0);
    let sqlite_size = std::fs::metadata(&sqlite_path)
        .map(|m| m.len())
        .unwrap_or(0);

    println!();
    println!("=== File sizes ({} scripts) ===", n_scripts);
    println!("  rkyv:   {} KB", rkyv_size / 1024);
    println!("  sqlite: {} KB", sqlite_size / 1024);

    // ── Steady-state: open once, N lookups ──
    println!();
    println!("=== Steady-state: open once, {} lookups ===", n_steady_reads);

    let rkyv_steady = {
        let rkyv = ScriptCache::open(&rkyv_path).unwrap();
        // Warm-up the mmap (first get does check_archived_root).
        let _ = rkyv.get(&corpus[0].0, 100, 200).unwrap();
        let start = Instant::now();
        for i in 0..n_steady_reads {
            let (path, _, _) = &corpus[i % corpus.len()];
            let _ = rkyv.get(path, 100, 200).unwrap();
        }
        start.elapsed()
    };

    let sqlite_steady = {
        let conn = sqlite_open(&sqlite_path);
        // Warm-up.
        let _ = sqlite_get(&conn, &corpus[0].0, 100, 200).unwrap();
        let start = Instant::now();
        for i in 0..n_steady_reads {
            let (path, _, _) = &corpus[i % corpus.len()];
            let _ = sqlite_get(&conn, path, 100, 200).unwrap();
        }
        start.elapsed()
    };

    let rkyv_per_hit = rkyv_steady.as_secs_f64() * 1_000_000.0 / n_steady_reads as f64;
    let sqlite_per_hit = sqlite_steady.as_secs_f64() * 1_000_000.0 / n_steady_reads as f64;
    println!(
        "  rkyv:   {} total, {:.2} µs/hit",
        fmt(rkyv_steady),
        rkyv_per_hit
    );
    println!(
        "  sqlite: {} total, {:.2} µs/hit",
        fmt(sqlite_steady),
        sqlite_per_hit
    );
    println!("  → rkyv is {:.2}x faster", sqlite_per_hit / rkyv_per_hit);

    // ── Per-process: open + 1 lookup + close, repeated ──
    println!();
    println!(
        "=== Per-process simulation: (open + 1 lookup + close) × {} ===",
        n_per_process
    );

    let mut rkyv_samples: Vec<Duration> = Vec::with_capacity(n_per_process);
    for i in 0..n_per_process {
        let (path, _, _) = &corpus[i % corpus.len()];
        let start = Instant::now();
        let cache = ScriptCache::open(&rkyv_path).unwrap();
        let _ = cache.get(path, 100, 200).unwrap();
        drop(cache);
        rkyv_samples.push(start.elapsed());
    }

    let mut sqlite_samples: Vec<Duration> = Vec::with_capacity(n_per_process);
    for i in 0..n_per_process {
        let (path, _, _) = &corpus[i % corpus.len()];
        let start = Instant::now();
        let conn = sqlite_open(&sqlite_path);
        let _ = sqlite_get(&conn, path, 100, 200).unwrap();
        drop(conn);
        sqlite_samples.push(start.elapsed());
    }

    let rkyv_total: Duration = rkyv_samples.iter().sum();
    let sqlite_total: Duration = sqlite_samples.iter().sum();
    let rkyv_p50 = percentile(&rkyv_samples, 50.0);
    let rkyv_p99 = percentile(&rkyv_samples, 99.0);
    let sqlite_p50 = percentile(&sqlite_samples, 50.0);
    let sqlite_p99 = percentile(&sqlite_samples, 99.0);

    println!(
        "  rkyv:   total {}, p50 {} / p99 {}",
        fmt(rkyv_total),
        fmt(rkyv_p50),
        fmt(rkyv_p99)
    );
    println!(
        "  sqlite: total {}, p50 {} / p99 {}",
        fmt(sqlite_total),
        fmt(sqlite_p50),
        fmt(sqlite_p99)
    );
    let p50_ratio = sqlite_p50.as_secs_f64() / rkyv_p50.as_secs_f64();
    let total_ratio = sqlite_total.as_secs_f64() / rkyv_total.as_secs_f64();
    println!(
        "  → rkyv is {:.2}x faster (p50), {:.2}x faster (total)",
        p50_ratio, total_ratio
    );

    println!();
}
