//! SQLite-backed bytecode cache for scripts.
//!
//! Stores compiled bytecode indexed by (canonical_path, mtime). On 2+ runs,
//! skips lex/parse/compile entirely — just deserialize and eval into fusevm.
//!
//! Cache location: `~/.cache/stryke/scripts.db`
//!
//! Invalidation:
//!   - source mtime mismatch → recompile, update cache
//!   - stryke version mismatch → cache miss
//!   - pointer width mismatch → cache miss
//!   - cache entry older than the running stryke binary's mtime → cache miss
//!     (any rebuild of stryke invalidates every cached script — guards
//!      against stale bytecode after compiler/parser/VM changes that don't
//!      bump CARGO_PKG_VERSION).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ast::Program;
use crate::bytecode::Chunk;
use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

// ── Constant pool codec for serializing PerlValues in bytecode cache ───────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CacheConst {
    Undef,
    Int(i64),
    Float(f64),
    Str(String),
}

fn cache_const_from_perl(v: &PerlValue) -> Result<CacheConst, String> {
    if v.is_undef() {
        return Ok(CacheConst::Undef);
    }
    if let Some(n) = v.as_integer() {
        return Ok(CacheConst::Int(n));
    }
    if let Some(f) = v.as_float() {
        return Ok(CacheConst::Float(f));
    }
    if let Some(s) = v.as_str() {
        return Ok(CacheConst::Str(s.to_string()));
    }
    Err(format!(
        "constant pool value cannot be cached (type {})",
        v.ref_type()
    ))
}

fn perl_from_cache_const(c: CacheConst) -> PerlValue {
    match c {
        CacheConst::Undef => PerlValue::UNDEF,
        CacheConst::Int(n) => PerlValue::integer(n),
        CacheConst::Float(f) => PerlValue::float(f),
        CacheConst::Str(s) => PerlValue::string(s),
    }
}

/// Serde codec for serializing `Vec<PerlValue>` in bytecode Chunk.
pub mod constants_pool_codec {
    use super::*;

    pub fn serialize<S>(values: &Vec<PerlValue>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut out = Vec::with_capacity(values.len());
        for v in values {
            let c = cache_const_from_perl(v).map_err(serde::ser::Error::custom)?;
            out.push(c);
        }
        out.serialize(ser)
    }

    pub fn deserialize<'de, D>(de: D) -> Result<Vec<PerlValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v: Vec<CacheConst> = Vec::deserialize(de)?;
        Ok(v.into_iter().map(perl_from_cache_const).collect())
    }
}

/// Cached script bundle: AST + compiled bytecode.
#[derive(Debug, Clone)]
pub struct CachedScript {
    pub program: Program,
    pub chunk: Chunk,
}

/// SQLite-backed script cache.
pub struct ScriptCache {
    conn: Connection,
}

impl ScriptCache {
    /// Open (or create) the cache database.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-64000;
             PRAGMA mmap_size=268435456;
             PRAGMA temp_store=MEMORY;",
        )?;
        let cache = Self { conn };
        cache.init_schema()?;
        Ok(cache)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
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
        )?;
        Ok(())
    }

    /// Check cache for a script. Returns cached bundle if mtime matches AND the
    /// cache entry is not older than the current stryke binary (so a recompile
    /// of stryke invalidates every cached script automatically).
    pub fn get(&self, path: &str, mtime_secs: i64, mtime_nsecs: i64) -> Option<CachedScript> {
        let (program_blob, chunk_blob, version, ptr_width, cached_at) = self
            .conn
            .query_row(
                "SELECT program_blob, chunk_blob, stryke_version, pointer_width, cached_at
                 FROM scripts
                 WHERE path = ?1 AND mtime_secs = ?2 AND mtime_nsecs = ?3",
                params![path, mtime_secs, mtime_nsecs],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()?;

        if version != env!("CARGO_PKG_VERSION") {
            return None;
        }
        if ptr_width != std::mem::size_of::<usize>() as i64 {
            return None;
        }
        // Bytecode predates the running stryke binary → recompile. Catches
        // edits to compiler.rs / parser.rs / vm.rs that don't bump the
        // version string.
        if let Some(bin_mtime) = current_binary_mtime_secs() {
            if cached_at < bin_mtime {
                return None;
            }
        }

        let program_decompressed = zstd::stream::decode_all(&program_blob[..]).ok()?;
        let chunk_decompressed = zstd::stream::decode_all(&chunk_blob[..]).ok()?;

        let program: Program = bincode::deserialize(&program_decompressed).ok()?;
        let chunk: Chunk = bincode::deserialize(&chunk_decompressed).ok()?;

        Some(CachedScript { program, chunk })
    }

    /// Store a compiled script in the cache.
    pub fn put(
        &self,
        path: &str,
        mtime_secs: i64,
        mtime_nsecs: i64,
        program: &Program,
        chunk: &Chunk,
    ) -> PerlResult<()> {
        let program_bytes =
            bincode::serialize(program).map_err(|e| PerlError::runtime(e.to_string(), 0))?;
        let chunk_bytes =
            bincode::serialize(chunk).map_err(|e| PerlError::runtime(e.to_string(), 0))?;

        let program_compressed = zstd::stream::encode_all(&program_bytes[..], 3)
            .map_err(|e| PerlError::runtime(e.to_string(), 0))?;
        let chunk_compressed = zstd::stream::encode_all(&chunk_bytes[..], 3)
            .map_err(|e| PerlError::runtime(e.to_string(), 0))?;

        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute("DELETE FROM scripts WHERE path = ?1", params![path])
            .map_err(|e| PerlError::runtime(e.to_string(), 0))?;

        self.conn
            .execute(
                "INSERT INTO scripts (path, mtime_secs, mtime_nsecs, stryke_version, pointer_width, program_blob, chunk_blob, cached_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    path,
                    mtime_secs,
                    mtime_nsecs,
                    env!("CARGO_PKG_VERSION"),
                    std::mem::size_of::<usize>() as i64,
                    program_compressed,
                    chunk_compressed,
                    now,
                ],
            )
            .map_err(|e| PerlError::runtime(e.to_string(), 0))?;

        Ok(())
    }

    /// Get cache stats: (total_scripts, total_bytes).
    pub fn stats(&self) -> (i64, i64) {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM scripts", [], |r| r.get(0))
            .unwrap_or(0);
        let bytes: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(program_blob) + LENGTH(chunk_blob)), 0) FROM scripts",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (count, bytes)
    }

    /// List all cached scripts: (path, program_kb, chunk_kb, version, cached_at).
    pub fn list_scripts(&self) -> Vec<(String, f64, f64, String, String)> {
        let mut stmt = match self.conn.prepare(
            "SELECT path, LENGTH(program_blob)/1024.0, LENGTH(chunk_blob)/1024.0, stryke_version, datetime(cached_at, 'unixepoch', 'localtime')
             FROM scripts ORDER BY cached_at DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Evict stale entries (file deleted or mtime changed).
    pub fn evict_stale(&self) -> usize {
        let paths: Vec<(i64, String, i64, i64)> = {
            let mut stmt = match self
                .conn
                .prepare("SELECT id, path, mtime_secs, mtime_nsecs FROM scripts")
            {
                Ok(s) => s,
                Err(_) => return 0,
            };
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        };

        let mut evicted = 0;
        for (id, path, cached_s, cached_ns) in paths {
            let stale = match file_mtime(Path::new(&path)) {
                Some((s, ns)) => s != cached_s || ns != cached_ns,
                None => true,
            };
            if stale {
                let _ = self
                    .conn
                    .execute("DELETE FROM scripts WHERE id = ?1", params![id]);
                evicted += 1;
            }
        }
        evicted
    }

    /// Clear entire cache.
    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM scripts", [])?;
        self.conn.execute("VACUUM", [])?;
        Ok(())
    }
}

/// Get mtime from file metadata as (secs, nsecs).
pub fn file_mtime(path: &Path) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.mtime(), meta.mtime_nsec()))
}

/// Mtime of the currently-running stryke binary, in unix epoch seconds.
/// Cached for the lifetime of the process — `current_exe()` does a syscall
/// per call and the binary doesn't move out from under us.
fn current_binary_mtime_secs() -> Option<i64> {
    static BIN_MTIME: OnceLock<Option<i64>> = OnceLock::new();
    *BIN_MTIME.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let (secs, _) = file_mtime(&exe)?;
        Some(secs)
    })
}

/// Default path for the script cache db.
pub fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cache/stryke/scripts.db")
}

/// Global cache instance (lazy-initialized, Mutex-protected for thread safety).
pub static CACHE: once_cell::sync::Lazy<Option<std::sync::Mutex<ScriptCache>>> =
    once_cell::sync::Lazy::new(|| {
        if !cache_enabled() {
            return None;
        }
        ScriptCache::open(&default_cache_path())
            .ok()
            .map(std::sync::Mutex::new)
    });

/// Check if SQLite cache is enabled (default: true, disable with `STRYKE_SQLITE_CACHE=0`).
pub fn cache_enabled() -> bool {
    !matches!(
        std::env::var("STRYKE_SQLITE_CACHE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    )
}

/// Try to load a cached script by path. Returns None on miss.
pub fn try_load(path: &Path) -> Option<CachedScript> {
    let cache = CACHE.as_ref()?.lock().ok()?;
    let canonical = path.canonicalize().ok()?;
    let path_str = canonical.to_string_lossy();
    let (mtime_s, mtime_ns) = file_mtime(&canonical)?;
    cache.get(&path_str, mtime_s, mtime_ns)
}

/// Store a compiled script in the cache.
pub fn try_save(path: &Path, program: &Program, chunk: &Chunk) -> PerlResult<()> {
    let cache = match CACHE.as_ref() {
        Some(c) => match c.lock() {
            Ok(guard) => guard,
            Err(_) => return Ok(()),
        },
        None => return Ok(()),
    };
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    let path_str = canonical.to_string_lossy();
    let (mtime_s, mtime_ns) = match file_mtime(&canonical) {
        Some(m) => m,
        None => return Ok(()),
    };
    cache.put(&path_str, mtime_s, mtime_ns, program, chunk)
}

/// Get global cache stats.
pub fn stats() -> Option<(i64, i64)> {
    CACHE
        .as_ref()
        .and_then(|c| c.lock().ok())
        .map(|c| c.stats())
}

/// Evict stale entries from global cache.
pub fn evict_stale() -> usize {
    CACHE
        .as_ref()
        .and_then(|c| c.lock().ok())
        .map(|c| c.evict_stale())
        .unwrap_or(0)
}

/// Clear global cache.
pub fn clear() -> bool {
    CACHE
        .as_ref()
        .and_then(|c| c.lock().ok())
        .map(|c| c.clear().is_ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = ScriptCache::open(&db_path).unwrap();

        let script_path = dir.path().join("test.stk");
        std::fs::write(&script_path, "p 42").unwrap();

        let (mtime_s, mtime_ns) = file_mtime(&script_path).unwrap();
        let path_str = script_path.to_string_lossy().to_string();

        let program = crate::parse("p 42").unwrap();
        let chunk = crate::compiler::Compiler::new()
            .compile_program(&program)
            .unwrap();

        cache
            .put(&path_str, mtime_s, mtime_ns, &program, &chunk)
            .unwrap();

        let loaded = cache.get(&path_str, mtime_s, mtime_ns).unwrap();
        assert_eq!(loaded.chunk.ops.len(), chunk.ops.len());

        let (count, _bytes) = cache.stats();
        assert_eq!(count, 1);
    }

    #[test]
    fn mtime_invalidation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = ScriptCache::open(&db_path).unwrap();

        let script_path = dir.path().join("test.stk");
        std::fs::write(&script_path, "p 42").unwrap();

        let (mtime_s, mtime_ns) = file_mtime(&script_path).unwrap();
        let path_str = script_path.to_string_lossy().to_string();

        let program = crate::parse("p 42").unwrap();
        let chunk = crate::compiler::Compiler::new()
            .compile_program(&program)
            .unwrap();

        cache
            .put(&path_str, mtime_s, mtime_ns, &program, &chunk)
            .unwrap();

        assert!(cache.get(&path_str, mtime_s + 1, mtime_ns).is_none());
    }
}
