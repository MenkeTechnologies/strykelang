//! `stryke.toml` parser and serializer. Backed by `serde` + `toml` so round-tripping
//! preserves table ordering, comments are dropped (TOML comment preservation is not
//! a `serde`-friendly use case — round-trip is for in-place edits via `s add`/`s remove`,
//! not human-authored comment retention).
//!
//! Schema: see RFC §"Manifest" (`docs/PACKAGE_REGISTRY.md` lines 75–124).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{PkgError, PkgResult};

/// Top-level `stryke.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    /// `[package]` — present for normal packages; absent for pure workspace roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageMeta>,

    /// `[deps]` — runtime dependencies.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub deps: IndexMap<String, DepSpec>,

    /// `[dev-deps]` — only present when running tests/benches.
    #[serde(rename = "dev-deps", default, skip_serializing_if = "IndexMap::is_empty")]
    pub dev_deps: IndexMap<String, DepSpec>,

    /// `[groups.NAME]` — bundler-style arbitrary groups (e.g. `groups.bench`).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub groups: IndexMap<String, IndexMap<String, DepSpec>>,

    /// `[features]` — feature flags. Per-package scoped (no workspace unification).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub features: IndexMap<String, Vec<String>>,

    /// `[scripts]` — npm-style task runner aliases (run via `s run <name>`).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub scripts: IndexMap<String, String>,

    /// `[bin]` — explicit binary entry points. Auto-discovery from `bin/` happens
    /// at build time when this map is empty.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub bin: IndexMap<String, String>,

    /// `[workspace]` — workspace root configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceConfig>,
}

/// `[package]` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    /// Language edition pin (e.g. `"2026"`). Defaults are inferred at build time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edition: String,
}

/// One dep spec: either `"1.0"` (shorthand for `{ version = "1.0" }`) or a
/// fully-expanded inline table (`{ version, features, path, git, ... }`).
///
/// On serialize, simple version-only specs round-trip back to the shorthand form
/// for cleaner manifests.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum DepSpec {
    /// `http = "1.0"` — bare version requirement.
    Version(String),
    /// `crypto = { version = "0.5", features = [...], path = ..., git = ..., ... }`.
    Detailed(DetailedDep),
    /// Empty placeholder so [`Default`] can construct a valid value.
    #[default]
    #[serde(skip)]
    Placeholder,
}

/// Inline-table form of a dep spec.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DetailedDep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// `path = "../mylib"` — local path dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `git = "https://..."` — git dependency. Combined with `branch`/`tag`/`rev`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// `registry = "https://..."` — alternate registry for this dep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// `optional = true` — only pulled in when a feature flag enables it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
    /// `default-features = false` — opt out of the dep's default features.
    #[serde(rename = "default-features", default = "default_true", skip_serializing_if = "is_true_default")]
    pub default_features: bool,
    /// `workspace = true` — inherit version/features from workspace root.
    #[serde(default, skip_serializing_if = "is_false")]
    pub workspace: bool,
}

fn is_false(b: &bool) -> bool { !*b }
fn default_true() -> bool { true }
fn is_true_default(b: &bool) -> bool { *b }

/// `[workspace]` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    #[serde(rename = "deps", default, skip_serializing_if = "IndexMap::is_empty")]
    pub deps: IndexMap<String, DepSpec>,
    /// `[workspace.package]` — metadata defaults inherited by member packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageMeta>,
}

impl Manifest {
    /// Parse a `stryke.toml` from a string. Returns a structured diagnostic on
    /// failure (line numbers when the underlying TOML parser provides them).
    pub fn from_str(s: &str) -> PkgResult<Manifest> {
        toml::from_str::<Manifest>(s).map_err(|e| {
            PkgError::Manifest(format!("stryke.toml: {}", e.message()))
        })
    }

    /// Parse from a path, treating any I/O error as `PkgError::Io` and any TOML
    /// error as `PkgError::Manifest`.
    pub fn from_path(path: &Path) -> PkgResult<Manifest> {
        let s = std::fs::read_to_string(path).map_err(|e| {
            PkgError::Io(format!("read {}: {}", path.display(), e))
        })?;
        Manifest::from_str(&s)
    }

    /// Serialize back to TOML. The serializer drops comments and reorders some
    /// tables (`serde` + `toml` is not a comment-preserving round-trip), but
    /// `IndexMap`-backed sections preserve insertion order so dep lists stay
    /// stable across `s add`/`s remove`.
    pub fn to_toml_string(&self) -> PkgResult<String> {
        toml::to_string_pretty(self).map_err(|e| {
            PkgError::Manifest(format!("serialize stryke.toml: {}", e))
        })
    }

    /// Validate semantic invariants on top of TOML schema (cheap fast fails).
    pub fn validate(&self) -> PkgResult<()> {
        if let Some(pkg) = &self.package {
            if pkg.name.is_empty() {
                return Err(PkgError::Manifest("[package].name is required".into()));
            }
            if pkg.version.is_empty() {
                return Err(PkgError::Manifest(
                    format!("[package].version is required for `{}`", pkg.name),
                ));
            }
        } else if self.workspace.is_none() {
            return Err(PkgError::Manifest(
                "stryke.toml needs either [package] or [workspace]".into(),
            ));
        }
        Ok(())
    }
}

impl DepSpec {
    /// Normalized version requirement (or `None` for path/git deps).
    pub fn version_req(&self) -> Option<&str> {
        match self {
            DepSpec::Version(v) => Some(v),
            DepSpec::Detailed(d) => d.version.as_deref(),
            DepSpec::Placeholder => None,
        }
    }

    /// Path of the dep on disk, if this is a `path = "..."` spec.
    pub fn path(&self) -> Option<&str> {
        match self {
            DepSpec::Detailed(d) => d.path.as_deref(),
            _ => None,
        }
    }

    /// Git URL, if this is a `git = "..."` spec.
    pub fn git(&self) -> Option<&str> {
        match self {
            DepSpec::Detailed(d) => d.git.as_deref(),
            _ => None,
        }
    }

    /// Convenience: build a bare version-string spec.
    pub fn version(s: impl Into<String>) -> DepSpec {
        DepSpec::Version(s.into())
    }

    /// Convenience: build a `path = "..."` spec.
    pub fn path_dep(p: impl Into<String>) -> DepSpec {
        DepSpec::Detailed(DetailedDep {
            path: Some(p.into()),
            default_features: true,
            ..DetailedDep::default()
        })
    }

    /// What kind of source this dep points at — drives resolver dispatch.
    pub fn source(&self) -> DepSource {
        match self {
            DepSpec::Detailed(d) if d.path.is_some() => DepSource::Path,
            DepSpec::Detailed(d) if d.git.is_some() => DepSource::Git,
            _ => DepSource::Registry,
        }
    }
}

/// Where a dep's source code lives. Drives which resolver branch handles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSource {
    Registry,
    Path,
    Git,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let m = Manifest::from_str(
            r#"
[package]
name = "myapp"
version = "0.1.0"
"#,
        )
        .unwrap();
        let pkg = m.package.unwrap();
        assert_eq!(pkg.name, "myapp");
        assert_eq!(pkg.version, "0.1.0");
    }

    #[test]
    fn parses_full_manifest_shape() {
        let src = r#"
[package]
name = "myapp"
version = "0.1.0"
edition = "2026"

[deps]
http = "1.0"
crypto = { version = "0.5", features = ["aes"] }
local-lib = { path = "../mylib" }
git-lib = { git = "https://github.com/u/lib", tag = "v1.0.0" }

[dev-deps]
test-utils = "1.0"

[scripts]
test = "s test t/"

[bin]
myapp = "main.stk"
"#;
        let m = Manifest::from_str(src).unwrap();
        assert_eq!(m.deps.len(), 4);
        assert_eq!(m.deps.get("http").unwrap().version_req(), Some("1.0"));
        assert_eq!(
            m.deps.get("local-lib").unwrap().source(),
            DepSource::Path
        );
        assert_eq!(m.deps.get("git-lib").unwrap().source(), DepSource::Git);
        assert_eq!(m.bin.get("myapp").unwrap(), "main.stk");
    }

    #[test]
    fn requires_package_or_workspace() {
        let m = Manifest::from_str("").unwrap();
        assert!(m.validate().is_err());
    }

    #[test]
    fn round_trip_preserves_dep_set() {
        let src = r#"[package]
name = "x"
version = "0.1.0"

[deps]
a = "1.0"
b = "2.0"
"#;
        let m = Manifest::from_str(src).unwrap();
        let out = m.to_toml_string().unwrap();
        let m2 = Manifest::from_str(&out).unwrap();
        assert_eq!(m2.deps.len(), 2);
        assert_eq!(m2.deps.get("a").unwrap().version_req(), Some("1.0"));
    }
}
