//! `stryke.lock` — auto-generated, deterministic, sacred. RFC §"Lock File".
//!
//! Two installs of the same lockfile on different machines must produce
//! byte-identical store contents. We keep this contract by:
//!
//! 1. Sorting `[[package]]` entries by `name`, then by `version`.
//! 2. Sorting transitive `deps = [...]` lists alphabetically.
//! 3. Pinning every dep's source URL and SHA-256 integrity hash.
//! 4. Recording the resolver/format version (`version = 1`).
//!
//! The lockfile is regenerated explicitly via `s install` / `s update`. It is
//! never edited by hand and never silently rewritten when only `stryke.toml`
//! changed (consumers running `s install` against an existing lock get the
//! pinned versions; `s add`/`s remove`/`s update` are the explicit edit paths).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use super::{PkgError, PkgResult};

/// Top-level `stryke.lock` shape.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lockfile {
    /// Lockfile schema version. Bumped whenever we change layout in a
    /// non-backwards-compatible way; older lockfiles can still be read by
    /// migration shims keyed on this value.
    pub version: u32,

    /// Stryke compiler version that wrote this lockfile (audit trail).
    pub stryke: String,

    /// ISO-8601 UTC timestamp of resolution. Recorded for audit; not used as
    /// part of the integrity contract.
    pub resolved: String,

    /// One entry per `(name, version)` in the resolved graph.
    /// Field name `package` keeps the human-readable `[[package]]` form in TOML.
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

/// One resolved package in the lock graph.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    /// Source URL — `registry+https://...`, `path+file://...`, `git+https://...#REV`.
    pub source: String,
    /// SHA-256 of the canonical content (tarball for registry/git, recursive
    /// directory hash for path deps). Format: `"sha256-<hex>"`.
    pub integrity: String,
    /// Feature flags enabled for this package in this resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Transitive deps as `name@version` strings. Sorted for determinism.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

impl Lockfile {
    /// Construct a fresh empty lockfile stamped with the current stryke version.
    pub fn new() -> Lockfile {
        Lockfile {
            version: 1,
            stryke: env!("CARGO_PKG_VERSION").to_string(),
            resolved: current_utc_timestamp(),
            packages: Vec::new(),
        }
    }

    /// Parse from a string.
    pub fn from_str(s: &str) -> PkgResult<Lockfile> {
        toml::from_str::<Lockfile>(s).map_err(|e| {
            PkgError::Lockfile(format!("stryke.lock: {}", e.message()))
        })
    }

    /// Parse from a file path.
    pub fn from_path(path: &Path) -> PkgResult<Lockfile> {
        let s = std::fs::read_to_string(path).map_err(|e| {
            PkgError::Io(format!("read {}: {}", path.display(), e))
        })?;
        Lockfile::from_str(&s)
    }

    /// Serialize. Sorts packages and their `deps` lists in place first so the
    /// output is bit-stable across resolver runs that produce equivalent graphs.
    pub fn to_toml_string(&mut self) -> PkgResult<String> {
        self.canonicalize();
        let body = toml::to_string_pretty(&self).map_err(|e| {
            PkgError::Lockfile(format!("serialize stryke.lock: {}", e))
        })?;
        Ok(format!("# Auto-generated. Do not edit.\n{}", body))
    }

    /// Sort packages and per-package `deps` lists for determinism. Idempotent.
    pub fn canonicalize(&mut self) {
        self.packages
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
        for p in &mut self.packages {
            p.deps.sort();
            p.features.sort();
            p.features.dedup();
        }
    }

    /// Look up a package entry by name. Returns the first match (lockfile is a
    /// flat resolution — one (name, version) per name post-resolve).
    pub fn find(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }
}

/// Compute a SHA-256 of a single byte slice and format as `"sha256-<hex>"`.
pub fn integrity_for_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("sha256-{:x}", h.finalize())
}

/// Compute a deterministic content hash of a directory tree. Used for path-dep
/// integrity pinning. Hash inputs are walked in sorted order so the result is
/// stable regardless of filesystem iteration order.
///
/// Entries are hashed as `<relpath>\0<size>\n<contents>` per file, with `\0`
/// separators between entries. Directories are descended; symlinks are read
/// as their target path string (no follow). Hidden files (`.` prefix) are
/// included; this is content addressing, not packaging policy.
pub fn integrity_for_directory(root: &Path) -> PkgResult<String> {
    let mut hasher = Sha256::new();
    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    walk_collect(root, root, &mut entries)?;
    entries.sort();
    for rel in &entries {
        let abs = root.join(rel);
        let meta = std::fs::symlink_metadata(&abs)?;
        let rel_s = rel.to_string_lossy();
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            hasher.update(rel_s.as_bytes());
            hasher.update(b"\0L\0");
            hasher.update(target.to_string_lossy().as_bytes());
            hasher.update(b"\n");
        } else if meta.is_file() {
            let bytes = std::fs::read(&abs)?;
            hasher.update(rel_s.as_bytes());
            hasher.update(b"\0F\0");
            hasher.update(bytes.len().to_string().as_bytes());
            hasher.update(b"\n");
            hasher.update(&bytes);
            hasher.update(b"\n");
        }
    }
    Ok(format!("sha256-{:x}", hasher.finalize()))
}

fn walk_collect(
    root: &Path,
    cur: &Path,
    out: &mut Vec<std::path::PathBuf>,
) -> PkgResult<()> {
    for entry in std::fs::read_dir(cur)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let meta = entry.metadata()?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            walk_collect(root, &path, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// ISO-8601 UTC timestamp using `std::time::SystemTime`. We don't pull `chrono`
/// just for this — a minimal `YYYY-MM-DDTHH:MM:SSZ` formatter is sufficient.
fn current_utc_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso_utc(secs)
}

fn format_iso_utc(unix_secs: u64) -> String {
    // Days since 1970-01-01.
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

/// Civil from days — Howard Hinnant's algorithm, public domain.
fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_is_deterministic() {
        let bytes = b"hello world";
        let a = integrity_for_bytes(bytes);
        let b = integrity_for_bytes(bytes);
        assert_eq!(a, b);
        assert!(a.starts_with("sha256-"));
    }

    #[test]
    fn directory_integrity_changes_on_content_change() {
        let tmp = tempdir();
        std::fs::write(tmp.join("a.txt"), b"v1").unwrap();
        let h1 = integrity_for_directory(&tmp).unwrap();
        std::fs::write(tmp.join("a.txt"), b"v2").unwrap();
        let h2 = integrity_for_directory(&tmp).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn directory_integrity_stable_across_runs() {
        let tmp = tempdir();
        std::fs::write(tmp.join("a.txt"), b"v1").unwrap();
        std::fs::write(tmp.join("b.txt"), b"v2").unwrap();
        let h1 = integrity_for_directory(&tmp).unwrap();
        let h2 = integrity_for_directory(&tmp).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn lockfile_round_trip() {
        let mut lf = Lockfile::new();
        lf.packages.push(LockedPackage {
            name: "json".into(),
            version: "2.1.0".into(),
            source: "registry+https://registry.stryke.dev".into(),
            integrity: "sha256-abc123".into(),
            features: vec![],
            deps: vec![],
        });
        lf.packages.push(LockedPackage {
            name: "http".into(),
            version: "1.0.0".into(),
            source: "registry+https://registry.stryke.dev".into(),
            integrity: "sha256-def456".into(),
            features: vec!["default".into()],
            deps: vec!["json@2.1.0".into()],
        });
        let out = lf.to_toml_string().unwrap();
        // After canonicalization, http (alphabetical) precedes json.
        let http_pos = out.find("name = \"http\"").unwrap();
        let json_pos = out.find("name = \"json\"").unwrap();
        assert!(http_pos < json_pos);
        let lf2 = Lockfile::from_str(&out).unwrap();
        assert_eq!(lf2.packages.len(), 2);
    }

    #[test]
    fn iso_utc_format_matches_pattern() {
        let s = format_iso_utc(0);
        assert_eq!(s, "1970-01-01T00:00:00Z");
        let s = format_iso_utc(1_700_000_000);
        assert!(s.starts_with("2023-"));
        assert!(s.ends_with("Z"));
    }

    /// Tiny tempdir helper — `tempfile` not in deps, and we just need a unique
    /// path under `target/` that gets dropped after the test.
    fn tempdir() -> std::path::PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = std::env::temp_dir().join(format!("stryke-pkg-test-{}-{}", pid, nanos));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
