//! SQLite-backed cache for compsys
//!
//! Handles 500k+ completion functions without memory bloat.
//! On-demand loading - only pull what's needed.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

/// SQLite cache for completion system
pub struct CompsysCache {
    conn: Connection,
}

impl CompsysCache {
    /// Open or create cache database
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let cache = Self { conn };
        cache.init_schema()?;
        Ok(cache)
    }

    /// In-memory cache (for testing)
    pub fn memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let cache = Self { conn };
        cache.init_schema()?;
        Ok(cache)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            r#"
            -- Function stubs (replaces in-memory autoload)
            CREATE TABLE IF NOT EXISTS autoloads (
                name TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                offset INTEGER NOT NULL,
                size INTEGER NOT NULL
            );

            -- zstyle database
            CREATE TABLE IF NOT EXISTS zstyles (
                pattern TEXT NOT NULL,
                style TEXT NOT NULL,
                value TEXT NOT NULL,
                eval INTEGER DEFAULT 0,
                PRIMARY KEY (pattern, style)
            );

            -- Completion mappings (_comps hash)
            CREATE TABLE IF NOT EXISTS comps (
                command TEXT PRIMARY KEY,
                function TEXT NOT NULL
            );

            -- Pattern completions (_patcomps)
            CREATE TABLE IF NOT EXISTS patcomps (
                pattern TEXT PRIMARY KEY,
                function TEXT NOT NULL
            );

            -- Key completions (_compkeywords)
            CREATE TABLE IF NOT EXISTS keycomps (
                key TEXT PRIMARY KEY,
                function TEXT NOT NULL
            );

            -- Services (_services)
            CREATE TABLE IF NOT EXISTS services (
                command TEXT PRIMARY KEY,
                service TEXT NOT NULL
            );

            -- Completion result cache
            CREATE TABLE IF NOT EXISTS cache (
                context TEXT PRIMARY KEY,
                data BLOB NOT NULL,
                mtime INTEGER NOT NULL
            );

            -- Indexes for fast lookup
            CREATE INDEX IF NOT EXISTS idx_autoloads_source ON autoloads(source);
            CREATE INDEX IF NOT EXISTS idx_zstyles_pattern ON zstyles(pattern);
            CREATE INDEX IF NOT EXISTS idx_comps_function ON comps(function);
        "#,
        )?;
        Ok(())
    }

    // =========================================================================
    // Autoloads - function stubs
    // =========================================================================

    /// Register an autoload stub
    pub fn add_autoload(
        &self,
        name: &str,
        source: &str,
        offset: i64,
        size: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO autoloads (name, source, offset, size) VALUES (?1, ?2, ?3, ?4)",
            params![name, source, offset, size],
        )?;
        Ok(())
    }

    /// Bulk insert autoloads (much faster)
    pub fn add_autoloads_bulk(
        &mut self,
        autoloads: &[(String, String, i64, i64)],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO autoloads (name, source, offset, size) VALUES (?1, ?2, ?3, ?4)"
            )?;
            for (name, source, offset, size) in autoloads {
                stmt.execute(params![name, source, offset, size])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Lookup autoload by name
    pub fn get_autoload(&self, name: &str) -> rusqlite::Result<Option<AutoloadStub>> {
        self.conn
            .query_row(
                "SELECT source, offset, size FROM autoloads WHERE name = ?1",
                params![name],
                |row| {
                    Ok(AutoloadStub {
                        name: name.to_string(),
                        source: row.get(0)?,
                        offset: row.get(1)?,
                        size: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    /// Count autoloads
    pub fn autoload_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM autoloads", [], |row| row.get(0))
    }

    /// List all autoload names (for debugging)
    pub fn list_autoloads(&self, limit: usize) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM autoloads LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get(0))?;
        rows.collect()
    }

    // =========================================================================
    // zstyle database
    // =========================================================================

    /// Set a zstyle
    pub fn set_zstyle(
        &self,
        pattern: &str,
        style: &str,
        values: &[String],
        eval: bool,
    ) -> rusqlite::Result<()> {
        let value_json = serde_values_to_json(values);
        self.conn.execute(
            "INSERT OR REPLACE INTO zstyles (pattern, style, value, eval) VALUES (?1, ?2, ?3, ?4)",
            params![pattern, style, value_json, eval as i32],
        )?;
        Ok(())
    }

    /// Bulk insert zstyles
    pub fn set_zstyles_bulk(
        &mut self,
        styles: &[(String, String, Vec<String>, bool)],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO zstyles (pattern, style, value, eval) VALUES (?1, ?2, ?3, ?4)"
            )?;
            for (pattern, style, values, eval) in styles {
                let value_json = serde_values_to_json(values);
                stmt.execute(params![pattern, style, value_json, *eval as i32])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete a zstyle
    pub fn delete_zstyle(&self, pattern: &str, style: Option<&str>) -> rusqlite::Result<usize> {
        if let Some(s) = style {
            self.conn.execute(
                "DELETE FROM zstyles WHERE pattern = ?1 AND style = ?2",
                params![pattern, s],
            )
        } else {
            self.conn
                .execute("DELETE FROM zstyles WHERE pattern = ?1", params![pattern])
        }
    }

    /// Lookup zstyle - returns all matching patterns sorted by specificity
    pub fn lookup_zstyle(
        &self,
        context: &str,
        style: &str,
    ) -> rusqlite::Result<Option<ZStyleEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern, value, eval FROM zstyles WHERE style = ?1")?;

        let entries: Vec<(String, String, bool)> = stmt
            .query_map(params![style], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i32>(2)? != 0))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Find best match by specificity
        let mut best: Option<(i32, String, bool)> = None;
        for (pattern, value, eval) in entries {
            if pattern_matches_context(&pattern, context) {
                let weight = calculate_pattern_weight(&pattern);
                if best.is_none() || weight > best.as_ref().unwrap().0 {
                    best = Some((weight, value, eval));
                }
            }
        }

        Ok(best.map(|(_, value, eval)| ZStyleEntry {
            values: serde_json_to_values(&value),
            eval,
        }))
    }

    /// List all zstyles (for `zstyle -L`)
    pub fn list_zstyles(&self) -> rusqlite::Result<Vec<(String, String, Vec<String>, bool)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern, style, value, eval FROM zstyles ORDER BY pattern, style")?;
        let rows = stmt.query_map([], |row| {
            let pattern: String = row.get(0)?;
            let style: String = row.get(1)?;
            let value: String = row.get(2)?;
            let eval: bool = row.get::<_, i32>(3)? != 0;
            Ok((pattern, style, serde_json_to_values(&value), eval))
        })?;
        rows.collect()
    }

    /// Count zstyles
    pub fn zstyle_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM zstyles", [], |row| row.get(0))
    }

    // =========================================================================
    // Completion mappings (_comps)
    // =========================================================================

    /// Register a completion function for a command
    pub fn set_comp(&self, command: &str, function: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO comps (command, function) VALUES (?1, ?2)",
            params![command, function],
        )?;
        Ok(())
    }

    /// Bulk insert comps
    pub fn set_comps_bulk(&mut self, comps: &[(String, String)]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO comps (command, function) VALUES (?1, ?2)")?;
            for (command, function) in comps {
                stmt.execute(params![command, function])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Lookup completion function for command
    pub fn get_comp(&self, command: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT function FROM comps WHERE command = ?1",
                params![command],
                |row| row.get(0),
            )
            .optional()
    }

    /// Get all comps as HashMap (for compatibility)
    pub fn get_all_comps(&self) -> rusqlite::Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT command, function FROM comps")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// Count comps
    pub fn comp_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM comps", [], |row| row.get(0))
    }

    /// Delete a completion registration
    pub fn delete_comp(&self, command: &str) -> rusqlite::Result<usize> {
        self.conn
            .execute("DELETE FROM comps WHERE command = ?1", params![command])
    }

    // =========================================================================
    // Pattern completions (_patcomps)
    // =========================================================================

    /// Register a pattern completion
    pub fn set_patcomp(&self, pattern: &str, function: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO patcomps (pattern, function) VALUES (?1, ?2)",
            params![pattern, function],
        )?;
        Ok(())
    }

    /// Find matching pattern completion
    pub fn find_patcomp(&self, command: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern, function FROM patcomps")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (pattern, function) = row?;
            if glob_matches(&pattern, command) {
                return Ok(Some(function));
            }
        }
        Ok(None)
    }

    // =========================================================================
    // Key completions
    // =========================================================================

    /// Register a key completion (for -K)
    pub fn set_keycomp(&self, key: &str, function: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO keycomps (key, function) VALUES (?1, ?2)",
            params![key, function],
        )?;
        Ok(())
    }

    /// Lookup key completion
    pub fn get_keycomp(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT function FROM keycomps WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
    }

    // =========================================================================
    // Result cache
    // =========================================================================

    /// Cache completion results
    pub fn cache_results(&self, context: &str, data: &[u8], mtime: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cache (context, data, mtime) VALUES (?1, ?2, ?3)",
            params![context, data, mtime],
        )?;
        Ok(())
    }

    /// Get cached results if not stale
    pub fn get_cached(&self, context: &str, max_age: i64) -> rusqlite::Result<Option<Vec<u8>>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn
            .query_row(
                "SELECT data FROM cache WHERE context = ?1 AND mtime > ?2",
                params![context, now - max_age],
                |row| row.get(0),
            )
            .optional()
    }

    /// Clear old cache entries
    pub fn clear_stale_cache(&self, max_age: i64) -> rusqlite::Result<usize> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn
            .execute("DELETE FROM cache WHERE mtime < ?1", params![now - max_age])
    }

    /// Clear all cache
    pub fn clear_cache(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM cache", [])?;
        Ok(())
    }

    // =========================================================================
    // Maintenance
    // =========================================================================

    /// Vacuum database
    pub fn vacuum(&self) -> rusqlite::Result<()> {
        self.conn.execute("VACUUM", [])?;
        Ok(())
    }

    /// Get database stats
    pub fn stats(&self) -> rusqlite::Result<CacheStats> {
        Ok(CacheStats {
            autoloads: self.autoload_count()?,
            zstyles: self.zstyle_count()?,
            comps: self.comp_count()?,
            patcomps: self
                .conn
                .query_row("SELECT COUNT(*) FROM patcomps", [], |r| r.get(0))?,
            keycomps: self
                .conn
                .query_row("SELECT COUNT(*) FROM keycomps", [], |r| r.get(0))?,
            services: self
                .conn
                .query_row("SELECT COUNT(*) FROM services", [], |r| r.get(0))?,
            cache_entries: self
                .conn
                .query_row("SELECT COUNT(*) FROM cache", [], |r| r.get(0))?,
        })
    }
}

/// Autoload stub info
#[derive(Debug, Clone)]
pub struct AutoloadStub {
    pub name: String,
    pub source: String,
    pub offset: i64,
    pub size: i64,
}

/// zstyle entry
#[derive(Debug, Clone)]
pub struct ZStyleEntry {
    pub values: Vec<String>,
    pub eval: bool,
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    pub autoloads: i64,
    pub zstyles: i64,
    pub comps: i64,
    pub patcomps: i64,
    pub keycomps: i64,
    pub services: i64,
    pub cache_entries: i64,
}

// Helper: serialize values to JSON
fn serde_values_to_json(values: &[String]) -> String {
    let escaped: Vec<String> = values
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", escaped.join(","))
}

// Helper: deserialize JSON to values
fn serde_json_to_values(json: &str) -> Vec<String> {
    let trimmed = json.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return vec![json.to_string()];
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty() {
        return vec![];
    }

    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escape = false;

    for c in inner.chars() {
        if escape {
            current.push(c);
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            in_string = !in_string;
        } else if c == ',' && !in_string {
            values.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        values.push(current.trim().to_string());
    }

    values
}

// Helper: check if zstyle pattern matches context
fn pattern_matches_context(pattern: &str, context: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split(':').collect();
    let ctx_parts: Vec<&str> = context.split(':').collect();

    if pat_parts.len() > ctx_parts.len() {
        return false;
    }

    for (p, c) in pat_parts.iter().zip(ctx_parts.iter()) {
        if *p != "*" && *p != *c {
            return false;
        }
    }

    true
}

// Helper: calculate pattern weight for specificity
fn calculate_pattern_weight(pattern: &str) -> i32 {
    let parts: Vec<&str> = pattern.split(':').filter(|s| !s.is_empty()).collect();
    let mut weight = parts.len() as i32 * 100;

    for part in &parts {
        if *part != "*" {
            weight += 10;
        }
    }

    weight
}

// Helper: glob matching for patcomps
fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut pat_chars = pattern.chars().peekable();
    let mut txt_chars = text.chars().peekable();

    while let Some(p) = pat_chars.next() {
        match p {
            '*' => {
                if pat_chars.peek().is_none() {
                    return true;
                }
                while txt_chars.peek().is_some() {
                    if glob_matches(
                        &pat_chars.clone().collect::<String>(),
                        &txt_chars.clone().collect::<String>(),
                    ) {
                        return true;
                    }
                    txt_chars.next();
                }
                return false;
            }
            '?' => {
                if txt_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if txt_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    txt_chars.peek().is_none()
}

// =========================================================================
// Shell-visible arrays (_comps, _services, _patcomps, etc.)
// These back the zsh special arrays that users query with $#_comps etc.
// =========================================================================

impl CompsysCache {
    /// Get count of _comps entries (for $#_comps)
    pub fn comps_count(&self) -> rusqlite::Result<i64> {
        self.comp_count()
    }

    /// Get all _comps keys (for ${(k)_comps})
    pub fn comps_keys(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT command FROM comps ORDER BY command")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    /// Get all _comps values (for ${(v)_comps})
    pub fn comps_values(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT function FROM comps ORDER BY command")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    /// Get _comps as key-value pairs (for ${(kv)_comps})
    pub fn comps_kv(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT command, function FROM comps ORDER BY command")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    // --- _patcomps ---

    /// Get count of _patcomps
    pub fn patcomps_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM patcomps", [], |row| row.get(0))
    }

    /// Get all _patcomps keys
    pub fn patcomps_keys(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern FROM patcomps ORDER BY pattern")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    /// Get all _patcomps as kv
    pub fn patcomps_kv(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT pattern, function FROM patcomps ORDER BY pattern")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    // --- _services ---

    /// Set a service mapping
    pub fn set_service(&self, command: &str, service: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO services (command, service) VALUES (?1, ?2)",
            params![command, service],
        )?;
        Ok(())
    }

    /// Get service for command
    pub fn get_service(&self, command: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT service FROM services WHERE command = ?1",
                params![command],
                |row| row.get(0),
            )
            .optional()
    }

    /// Get count of _services
    pub fn services_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM services", [], |row| row.get(0))
    }

    /// Get all _services keys
    pub fn services_keys(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT command FROM services ORDER BY command")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }

    /// Bulk insert services
    pub fn set_services_bulk(&mut self, services: &[(String, String)]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO services (command, service) VALUES (?1, ?2)")?;
            for (command, service) in services {
                stmt.execute(params![command, service])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // --- _compautos (autoloaded completion functions) ---

    /// Get count of autoloaded functions
    pub fn compautos_count(&self) -> rusqlite::Result<i64> {
        self.autoload_count()
    }

    /// Get all autoload names (for ${(k)_compautos})
    pub fn compautos_keys(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM autoloads ORDER BY name")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic() {
        let cache = CompsysCache::memory().unwrap();

        cache
            .add_autoload("_git", "more_src.zwc", 1024, 5000)
            .unwrap();
        cache
            .add_autoload("_docker", "more_src.zwc", 6024, 3000)
            .unwrap();

        let stub = cache.get_autoload("_git").unwrap().unwrap();
        assert_eq!(stub.source, "more_src.zwc");
        assert_eq!(stub.offset, 1024);

        assert!(cache.get_autoload("_nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_zstyle_cache() {
        let cache = CompsysCache::memory().unwrap();

        cache
            .set_zstyle(":completion:*", "menu", &["select".to_string()], false)
            .unwrap();
        cache
            .set_zstyle(
                ":completion:*:descriptions",
                "format",
                &["%d".to_string()],
                false,
            )
            .unwrap();

        let entry = cache
            .lookup_zstyle(":completion:foo", "menu")
            .unwrap()
            .unwrap();
        assert_eq!(entry.values, vec!["select"]);

        let entry = cache
            .lookup_zstyle(":completion:foo:descriptions", "format")
            .unwrap()
            .unwrap();
        assert_eq!(entry.values, vec!["%d"]);
    }

    #[test]
    fn test_zstyle_specificity() {
        let cache = CompsysCache::memory().unwrap();

        cache
            .set_zstyle(":completion:*", "menu", &["no".to_string()], false)
            .unwrap();
        cache
            .set_zstyle(
                ":completion:*:*:*:default",
                "menu",
                &["yes".to_string()],
                false,
            )
            .unwrap();

        let entry = cache
            .lookup_zstyle(":completion:foo:bar:baz:default", "menu")
            .unwrap()
            .unwrap();
        assert_eq!(entry.values, vec!["yes"]);
    }

    #[test]
    fn test_comps_cache() {
        let mut cache = CompsysCache::memory().unwrap();

        let comps = vec![
            ("git".to_string(), "_git".to_string()),
            ("docker".to_string(), "_docker".to_string()),
            ("cargo".to_string(), "_cargo".to_string()),
        ];
        cache.set_comps_bulk(&comps).unwrap();

        assert_eq!(cache.get_comp("git").unwrap(), Some("_git".to_string()));
        assert_eq!(
            cache.get_comp("docker").unwrap(),
            Some("_docker".to_string())
        );
        assert!(cache.get_comp("nonexistent").unwrap().is_none());

        assert_eq!(cache.comp_count().unwrap(), 3);
    }

    #[test]
    fn test_bulk_autoloads() {
        let mut cache = CompsysCache::memory().unwrap();

        let autoloads: Vec<(String, String, i64, i64)> = (0..1000)
            .map(|i| (format!("_func{}", i), "test.zwc".to_string(), i * 100, 100))
            .collect();

        cache.add_autoloads_bulk(&autoloads).unwrap();
        assert_eq!(cache.autoload_count().unwrap(), 1000);

        let stub = cache.get_autoload("_func500").unwrap().unwrap();
        assert_eq!(stub.offset, 50000);
    }

    #[test]
    fn test_patcomp() {
        let cache = CompsysCache::memory().unwrap();

        cache.set_patcomp("git-*", "_git").unwrap();
        cache.set_patcomp("docker-*", "_docker").unwrap();

        assert_eq!(
            cache.find_patcomp("git-commit").unwrap(),
            Some("_git".to_string())
        );
        assert_eq!(
            cache.find_patcomp("docker-compose").unwrap(),
            Some("_docker".to_string())
        );
        assert!(cache.find_patcomp("cargo").unwrap().is_none());
    }

    #[test]
    fn test_glob_matches() {
        assert!(glob_matches("git-*", "git-commit"));
        assert!(glob_matches("*-compose", "docker-compose"));
        assert!(glob_matches("*.rs", "main.rs"));
        assert!(!glob_matches("git-*", "docker-compose"));
        assert!(glob_matches("???", "abc"));
        assert!(!glob_matches("???", "abcd"));
    }

    #[test]
    fn test_json_serde() {
        let values = vec!["hello".to_string(), "world".to_string()];
        let json = serde_values_to_json(&values);
        let back = serde_json_to_values(&json);
        assert_eq!(back, values);

        let values = vec!["with \"quotes\"".to_string()];
        let json = serde_values_to_json(&values);
        let back = serde_json_to_values(&json);
        assert_eq!(back, vec!["with \"quotes\""]);
    }

    #[test]
    fn test_stats() {
        let mut cache = CompsysCache::memory().unwrap();

        cache.add_autoload("_git", "test.zwc", 0, 100).unwrap();
        cache
            .set_zstyle(":completion:*", "menu", &["select".to_string()], false)
            .unwrap();
        cache.set_comp("git", "_git").unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.autoloads, 1);
        assert_eq!(stats.zstyles, 1);
        assert_eq!(stats.comps, 1);
    }

    #[test]
    fn test_large_scale() {
        let mut cache = CompsysCache::memory().unwrap();

        // Simulate 500k autoloads
        let autoloads: Vec<(String, String, i64, i64)> = (0..10000)
            .map(|i| {
                (
                    format!("_func{}", i),
                    format!("src{}.zwc", i % 10),
                    i * 50,
                    50,
                )
            })
            .collect();

        cache.add_autoloads_bulk(&autoloads).unwrap();

        // Fast lookup
        let stub = cache.get_autoload("_func9999").unwrap().unwrap();
        assert_eq!(stub.offset, 9999 * 50);

        assert_eq!(cache.autoload_count().unwrap(), 10000);
    }
}
