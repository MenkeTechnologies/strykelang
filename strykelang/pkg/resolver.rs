//! Resolution pipeline. Local-only for Tier 1 (RFC phases 1–6):
//! - Path deps (`{ path = "../lib" }`) are read from the filesystem, hashed,
//!   copied into the store, and pinned in the lockfile.
//! - Workspace member deps work the same as path deps.
//! - Registry (`http = "1.0"`) and git (`{ git = "..." }`) deps return a
//!   structured "not yet implemented" error so the CLI surface is honest
//!   about what's wired today.
//!
//! When the registry / PubGrub semver / parallel fetch land (RFC phases 7-9),
//! they slot in at [`Resolver::resolve`] without changing the lockfile shape.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::lockfile::{integrity_for_directory, LockedPackage, Lockfile};
use super::manifest::{DepSource, DepSpec, Manifest};
use super::store::Store;
use super::{PkgError, PkgResult};

/// Drives manifest → lockfile resolution against a [`Store`].
pub struct Resolver<'a> {
    /// The project's `stryke.toml`.
    pub manifest: &'a Manifest,
    /// Directory containing `stryke.toml` — path deps are resolved relative to this.
    pub manifest_dir: &'a Path,
    /// Backing global store (`~/.stryke/...`).
    pub store: &'a Store,
}

/// One concrete dep edge after resolution. Drives lockfile entry generation.
#[derive(Debug, Clone)]
struct ResolvedDep {
    name: String,
    version: String,
    source: String,
    integrity: String,
    /// `name@version` strings of transitive deps, sorted.
    deps: Vec<String>,
    /// Features enabled for this resolution.
    features: Vec<String>,
}

/// Outcome of resolving one project's dep graph.
#[derive(Debug, Clone)]
pub struct ResolveOutcome {
    /// Lockfile snapshot ready to write to disk.
    pub lockfile: Lockfile,
    /// `(name, version, store_path)` for every dep that was newly extracted
    /// or already present in the store. Useful for `s install` reporting.
    pub installed: Vec<(String, String, PathBuf)>,
}

impl<'a> Resolver<'a> {
    /// Resolve the manifest's runtime + dev + group deps into a lockfile and
    /// install all path/workspace deps into the store.
    ///
    /// Walks deps recursively: each path dep's own `stryke.toml` (when present)
    /// is parsed and its path/workspace deps follow the same pipeline. Cycles
    /// are detected via the `visiting` set and reported as an error.
    pub fn resolve(&self) -> PkgResult<ResolveOutcome> {
        self.store.ensure_layout()?;

        let mut graph: BTreeMap<String, ResolvedDep> = BTreeMap::new();
        let mut installed: Vec<(String, String, PathBuf)> = Vec::new();
        let mut visiting: BTreeSet<String> = BTreeSet::new();

        // Direct deps first — registry/git deps fail loud here.
        let direct = self.collect_direct_deps();
        for (name, spec) in &direct {
            self.walk_dep(
                name,
                spec,
                self.manifest_dir,
                &mut graph,
                &mut installed,
                &mut visiting,
            )?;
        }

        let mut lockfile = Lockfile::new();
        for (_, dep) in graph {
            lockfile.packages.push(LockedPackage {
                name: dep.name,
                version: dep.version,
                source: dep.source,
                integrity: dep.integrity,
                features: dep.features,
                deps: dep.deps,
            });
        }
        lockfile.canonicalize();
        Ok(ResolveOutcome {
            lockfile,
            installed,
        })
    }

    /// Flatten direct deps from `[deps]`, `[dev-deps]`, and every `[groups.*]`.
    fn collect_direct_deps(&self) -> Vec<(String, DepSpec)> {
        let mut out: Vec<(String, DepSpec)> = Vec::new();
        for (k, v) in &self.manifest.deps {
            out.push((k.clone(), v.clone()));
        }
        for (k, v) in &self.manifest.dev_deps {
            out.push((k.clone(), v.clone()));
        }
        for (_group_name, group_map) in &self.manifest.groups {
            for (k, v) in group_map {
                out.push((k.clone(), v.clone()));
            }
        }
        out
    }

    /// Resolve one dep edge. For path deps, copies into the store, hashes, and
    /// recurses into the path dep's own manifest. For registry/git deps,
    /// returns a clear unimplemented error (Tier 2/3).
    fn walk_dep(
        &self,
        name: &str,
        spec: &DepSpec,
        relative_to: &Path,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        match spec.source() {
            DepSource::Path => {
                let raw_path = spec.path().expect("path dep has path");
                let path = resolve_path(relative_to, raw_path);
                self.install_path_dep(name, &path, graph, installed, visiting)?;
                Ok(())
            }
            DepSource::Git => Err(PkgError::Resolve(format!(
                "git dep `{}` is not supported in this stryke version yet \
                 (RFC phase 9 — see docs/PACKAGE_REGISTRY.md). Use `path = \"...\"` \
                 to depend on a local checkout in the meantime.",
                name
            ))),
            DepSource::Registry => Err(PkgError::Resolve(format!(
                "registry dep `{}` is not supported in this stryke version yet \
                 (RFC phases 7–8 — see docs/PACKAGE_REGISTRY.md). Use `path = \"...\"` \
                 to depend on a local copy in the meantime.",
                name
            ))),
        }
    }

    /// Pull a path-dep manifest, hash the directory, copy into the store, and
    /// recurse into its own deps. The path dep's `stryke.toml` is optional —
    /// raw `.stk` source trees with no manifest are treated as version `"0.0.0"`.
    fn install_path_dep(
        &self,
        name: &str,
        src: &Path,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        if !src.is_dir() {
            return Err(PkgError::Resolve(format!(
                "path dep `{}` does not exist or is not a directory: {}",
                name,
                src.display()
            )));
        }

        let nested_manifest_path = src.join("stryke.toml");
        let nested = if nested_manifest_path.is_file() {
            Some(Manifest::from_path(&nested_manifest_path)?)
        } else {
            None
        };

        let version = nested
            .as_ref()
            .and_then(|m| m.package.as_ref())
            .map(|p| p.version.clone())
            .unwrap_or_else(|| "0.0.0".to_string());

        let key = format!("{}@{}", name, version);
        if graph.contains_key(&key) {
            return Ok(());
        }
        if !visiting.insert(key.clone()) {
            return Err(PkgError::Resolve(format!(
                "cyclic dependency detected at `{}`",
                key
            )));
        }

        let integrity = integrity_for_directory(src)?;
        let dst = self.store.install_path_dep(name, &version, src)?;
        installed.push((name.to_string(), version.clone(), dst.clone()));

        let mut transitive: Vec<String> = Vec::new();
        if let Some(nm) = nested.as_ref() {
            for (sub_name, sub_spec) in &nm.deps {
                self.walk_dep(sub_name, sub_spec, src, graph, installed, visiting)?;
                let sub_version = graph
                    .values()
                    .find(|d| &d.name == sub_name)
                    .map(|d| d.version.clone())
                    .unwrap_or_else(|| "0.0.0".to_string());
                transitive.push(format!("{}@{}", sub_name, sub_version));
            }
        }
        transitive.sort();
        transitive.dedup();

        let canonical_src = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
        graph.insert(
            key.clone(),
            ResolvedDep {
                name: name.to_string(),
                version,
                source: format!("path+file://{}", canonical_src.display()),
                integrity,
                deps: transitive,
                features: Vec::new(),
            },
        );
        visiting.remove(&key);
        Ok(())
    }
}

/// Resolve a (possibly relative) dep path against the dep's containing manifest dir.
fn resolve_path(relative_to: &Path, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        relative_to.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkg::manifest::DepSpec;
    use indexmap::IndexMap;

    fn tempdir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = std::env::temp_dir().join(format!("stryke-resolver-{}-{}-{}", tag, pid, nanos));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_path_dep(name: &str, version: &str) -> PathBuf {
        let dir = tempdir(name);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(
            dir.join("stryke.toml"),
            format!(
                "[package]\nname = \"{}\"\nversion = \"{}\"\n",
                name, version
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("lib").join(format!("{}.stk", name)),
            format!("# {}", name),
        )
        .unwrap();
        dir
    }

    #[test]
    fn resolves_single_path_dep() {
        let dep = make_path_dep("mylib", "1.0.0");
        let project = tempdir("project");
        let mut m = Manifest::default();
        m.package = Some(crate::pkg::manifest::PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        });
        let mut deps = IndexMap::new();
        deps.insert(
            "mylib".to_string(),
            DepSpec::path_dep(dep.to_string_lossy().to_string()),
        );
        m.deps = deps;

        let store_root = tempdir("store");
        let store = Store::at(&store_root);
        let r = Resolver {
            manifest: &m,
            manifest_dir: &project,
            store: &store,
        };
        let outcome = r.resolve().unwrap();
        assert_eq!(outcome.lockfile.packages.len(), 1);
        assert_eq!(outcome.lockfile.packages[0].name, "mylib");
        assert_eq!(outcome.lockfile.packages[0].version, "1.0.0");
        assert!(outcome.lockfile.packages[0]
            .integrity
            .starts_with("sha256-"));
        assert!(store.package_dir("mylib", "1.0.0").is_dir());
    }

    #[test]
    fn registry_dep_returns_unimplemented_error() {
        let project = tempdir("project");
        let mut m = Manifest::default();
        m.package = Some(crate::pkg::manifest::PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        });
        m.deps.insert("http".to_string(), DepSpec::version("1.0"));

        let store_root = tempdir("store");
        let store = Store::at(&store_root);
        let r = Resolver {
            manifest: &m,
            manifest_dir: &project,
            store: &store,
        };
        let err = r.resolve().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("registry dep"), "got: {}", msg);
        assert!(msg.contains("http"), "got: {}", msg);
    }

    #[test]
    fn transitive_path_dep_recursion() {
        let leaf = make_path_dep("leaf", "0.1.0");
        let mid = make_path_dep("mid", "0.2.0");
        // Mid depends on leaf via path = "<leaf-abs-path>"
        let mid_manifest = format!(
            "[package]\nname = \"mid\"\nversion = \"0.2.0\"\n\n[deps]\nleaf = {{ path = \"{}\" }}\n",
            leaf.display()
        );
        std::fs::write(mid.join("stryke.toml"), mid_manifest).unwrap();

        let project = tempdir("project");
        let mut m = Manifest::default();
        m.package = Some(crate::pkg::manifest::PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        });
        m.deps.insert(
            "mid".to_string(),
            DepSpec::path_dep(mid.to_string_lossy().to_string()),
        );

        let store_root = tempdir("store");
        let store = Store::at(&store_root);
        let r = Resolver {
            manifest: &m,
            manifest_dir: &project,
            store: &store,
        };
        let outcome = r.resolve().unwrap();
        assert_eq!(outcome.lockfile.packages.len(), 2);
        let names: Vec<&str> = outcome
            .lockfile
            .packages
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert!(names.contains(&"leaf"));
        assert!(names.contains(&"mid"));
        let mid_entry = outcome.lockfile.find("mid").unwrap();
        assert_eq!(mid_entry.deps, vec!["leaf@0.1.0"]);
    }
}
