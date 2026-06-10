//! Resolution pipeline (RFC phases 1–6 + 9 git):
//! - Path deps (`{ path = "../lib" }`) are read from the filesystem, hashed,
//!   copied into the store, and pinned in the lockfile.
//! - Workspace member deps work the same as path deps.
//! - GitHub-release deps (`{ github = "OWNER/REPO" [, version = "..."] }`)
//!   download the prebuilt release tarball for the host triple (auto-
//!   detected, override via `STRYKE_TARGET`), SHA-256 verify it, and
//!   extract into the store. This is the canonical path for binary-only
//!   FFI packages (stryke-arrow, stryke-aws, ...) whose cdylib needs
//!   platform libs at build time and can't be reproduced from a clone.
//! - Git deps (`{ git = "https://...", tag|branch|rev = ... }`) are cloned
//!   into `~/.stryke/git/`, the resolved commit is recorded in the lockfile,
//!   and the working copy is installed through the same `install_dir_dep`
//!   path as path deps. Source-only deps land here — e.g. a pure-stryke
//!   library hosted on github without release tarballs, or a private
//!   non-github git URL.
//! - Registry (`http = "1.0"`) deps return a structured "not yet implemented"
//!   error so the CLI surface is honest about what's wired today.
//!
//! When the registry / PubGrub semver / parallel fetch land (RFC phases 7-8),
//! they slot in at [`Resolver::resolve`] without changing the lockfile shape.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::lockfile::{integrity_for_directory, LockedPackage, Lockfile};
use super::manifest::{DepSource, DepSpec, DetailedDep, Manifest};
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
    ///
    /// When the root manifest has `[workspace]`, every member's deps are
    /// resolved into the same graph, and `dep.workspace = true` resolves
    /// against the root's `[workspace.deps]` table. The lockfile is single
    /// and lives at the workspace root.
    pub fn resolve(&self) -> PkgResult<ResolveOutcome> {
        self.store.ensure_layout()?;

        let mut graph: BTreeMap<String, ResolvedDep> = BTreeMap::new();
        let mut installed: Vec<(String, String, PathBuf)> = Vec::new();
        let mut visiting: BTreeSet<String> = BTreeSet::new();

        // Direct deps first — registry/git deps fail loud here. Workspace
        // member manifests are resolved underneath this same loop so the
        // single lockfile sees the union of all members' dep graphs.
        let direct = self.collect_direct_deps();
        for (name, spec) in &direct {
            let resolved_spec = self.resolve_workspace_dep(name, spec)?;
            self.walk_dep(
                name,
                &resolved_spec,
                self.manifest_dir,
                &mut graph,
                &mut installed,
                &mut visiting,
            )?;
        }

        // Workspace members each contribute their direct deps to the same graph.
        if let Some(ws) = &self.manifest.workspace {
            for member_pattern in &ws.members {
                let member_dirs = expand_workspace_glob(self.manifest_dir, member_pattern)?;
                for member_dir in member_dirs {
                    let member_manifest_path = member_dir.join("stryke.toml");
                    if !member_manifest_path.is_file() {
                        return Err(PkgError::Resolve(format!(
                            "workspace member {} has no stryke.toml",
                            member_dir.display()
                        )));
                    }
                    let member_manifest = Manifest::from_path(&member_manifest_path)?;
                    for (name, spec) in member_manifest
                        .deps
                        .iter()
                        .chain(member_manifest.dev_deps.iter())
                        .chain(member_manifest.groups.values().flat_map(|g| g.iter()))
                    {
                        let resolved_spec = self.resolve_workspace_dep(name, spec)?;
                        self.walk_dep(
                            name,
                            &resolved_spec,
                            &member_dir,
                            &mut graph,
                            &mut installed,
                            &mut visiting,
                        )?;
                    }
                }
            }
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

    /// If `spec` is `{ workspace = true }`, look up the name in the root
    /// manifest's `[workspace.deps]` and return that. Otherwise return the
    /// spec unchanged. This is the inheritance mechanism that lets every
    /// workspace member share one version of `http`/`json`/etc.
    ///
    /// Path deps inherited from `[workspace.deps]` are absolutized against
    /// the workspace root so the subsequent walk doesn't re-resolve them
    /// relative to the member directory (which would point at a wrong path).
    fn resolve_workspace_dep(&self, name: &str, spec: &DepSpec) -> PkgResult<DepSpec> {
        let inherits = matches!(spec, DepSpec::Detailed(d) if d.workspace);
        if !inherits {
            return Ok(spec.clone());
        }
        let ws = match &self.manifest.workspace {
            Some(w) => w,
            None => {
                return Err(PkgError::Resolve(format!(
                    "dep `{}` has `workspace = true` but the root manifest has no [workspace] table",
                    name
                )));
            }
        };
        let inherited = ws.deps.get(name).ok_or_else(|| {
            PkgError::Resolve(format!(
                "dep `{}` inherits from [workspace.deps] but no such entry exists in the root manifest",
                name
            ))
        })?;
        let mut absolutized = match inherited.clone() {
            DepSpec::Detailed(mut d) => {
                if let Some(p) = d.path.as_ref() {
                    let pp = std::path::Path::new(p);
                    if !pp.is_absolute() {
                        let abs = self.manifest_dir.join(pp);
                        d.path = Some(abs.to_string_lossy().into_owned());
                    }
                }
                DepSpec::Detailed(d)
            }
            other => other,
        };
        // Merge: dep-side `features` accumulate on top of the inherited spec.
        if let DepSpec::Detailed(member) = spec {
            if !member.features.is_empty() {
                let mut merged = match absolutized {
                    DepSpec::Detailed(d) => d,
                    DepSpec::Version(v) => DetailedDep {
                        version: Some(v),
                        default_features: true,
                        ..DetailedDep::default()
                    },
                    DepSpec::Placeholder => DetailedDep::default(),
                };
                for f in &member.features {
                    if !merged.features.contains(f) {
                        merged.features.push(f.clone());
                    }
                }
                absolutized = DepSpec::Detailed(merged);
            }
        }
        Ok(absolutized)
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
            DepSource::Git => self.install_git_dep(name, spec, graph, installed, visiting),
            DepSource::GitHub => self.install_github_dep(name, spec, graph, installed, visiting),
            DepSource::Registry => Err(PkgError::Resolve(format!(
                "registry dep `{}` is not supported in this stryke version yet \
                 (RFC phases 7–8 — see docs/PACKAGE_REGISTRY.md). Use `path = \"...\"` \
                 to depend on a local copy in the meantime.",
                name
            ))),
        }
    }

    /// Handle a `{ git = "..." }` dep. `s install` never source-clones —
    /// it downloads the prebuilt release tarball. github URLs route
    /// straight to `install_from_github_release`. Non-github git URLs
    /// have no release-tarball convention defined yet and error fast
    /// with a pointer at the `{ github = "OWNER/REPO" }` or `{ path = "..." }`
    /// rewrites.
    fn install_git_dep(
        &self,
        name: &str,
        spec: &DepSpec,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        let url = spec
            .git()
            .ok_or_else(|| PkgError::Resolve(format!("git dep `{}` missing url", name)))?
            .to_string();

        let Some((owner, repo)) = parse_github_url(&url) else {
            return Err(PkgError::Resolve(format!(
                "git dep `{}` URL `{}` is not a github.com URL — `s install` \
                 doesn't source-clone, it downloads prebuilt release tarballs. \
                 Rewrite as `{{ github = \"OWNER/REPO\" }}` for github-hosted \
                 packages or `{{ path = \"...\" }}` for local checkouts.",
                name, url
            )));
        };

        // Pull the release-tarball path. The tag pin (if any) comes from
        // the spec's `tag` field. `branch` is ignored — releases live at
        // tags, not branch tips. `rev` is also unused for release fetch.
        let pinned = match spec {
            DepSpec::Detailed(d) => d.tag.as_deref(),
            _ => None,
        };
        self.install_from_github_release(name, &owner, &repo, pinned, graph, installed, visiting)
    }

    /// `{ github = "OWNER/REPO" [, version = "..."] }` — fetch the
    /// prebuilt release tarball for the host triple. Delegates to the
    /// shared `install_global_from_github` helper in `pkg::commands`.
    fn install_github_dep(
        &self,
        name: &str,
        spec: &DepSpec,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        let owner_repo = spec.github().ok_or_else(|| {
            PkgError::Resolve(format!("github dep `{}` missing OWNER/REPO", name))
        })?;
        let (owner, repo) = owner_repo.split_once('/').ok_or_else(|| {
            PkgError::Resolve(format!(
                "github dep `{}`: expected `OWNER/REPO`, got `{}`",
                name, owner_repo
            ))
        })?;
        if owner.is_empty() || repo.is_empty() {
            return Err(PkgError::Resolve(format!(
                "github dep `{}`: empty owner or repo in `{}`",
                name, owner_repo
            )));
        }
        let pinned = spec.pinned_release_version().map(str::to_string);
        self.install_from_github_release(
            name,
            owner,
            repo,
            pinned.as_deref(),
            graph,
            installed,
            visiting,
        )
    }

    /// Shared release-tarball installer used by both `install_github_dep`
    /// (explicit `github = "X/Y"` form) and the github-URL branch of
    /// `install_git_dep` (auto-route from `git = "https://github.com/X/Y"`).
    /// Calls `install_global_from_github` to download + verify + extract,
    /// then threads the result through the standard graph/installed/lockfile
    /// bookkeeping.
    fn install_from_github_release(
        &self,
        name: &str,
        owner: &str,
        repo: &str,
        pinned: Option<&str>,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        let (manifest, dst, source) =
            super::commands::install_global_from_github(&self.store, owner, repo, pinned)
                .map_err(PkgError::Resolve)?;

        let pkg = manifest
            .package
            .as_ref()
            .ok_or_else(|| PkgError::Resolve(format!("github dep `{}`: tarball missing [package]", name)))?;
        let version = pkg.version.clone();
        let integrity = integrity_for_directory(&dst)?;
        let key = format!("{}@{}", name, version);
        if graph.contains_key(&key) {
            return Ok(());
        }
        installed.push((name.to_string(), version.clone(), dst.clone()));

        // Recurse into transitive deps — a github-released package may
        // declare its own [deps] table in the bundled stryke.toml.
        let mut transitive: Vec<String> = Vec::new();
        for (sub_name, sub_spec) in &manifest.deps {
            self.walk_dep(sub_name, sub_spec, &dst, graph, installed, visiting)?;
            let sub_version = graph
                .values()
                .find(|d| &d.name == sub_name)
                .map(|d| d.version.clone())
                .unwrap_or_else(|| "0.0.0".to_string());
            transitive.push(format!("{}@{}", sub_name, sub_version));
        }
        transitive.sort();
        transitive.dedup();

        graph.insert(
            key,
            ResolvedDep {
                name: name.to_string(),
                version,
                source,
                integrity,
                deps: transitive,
                features: Vec::new(),
            },
        );
        Ok(())
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
        let canonical_src = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
        let source = format!("path+file://{}", canonical_src.display());
        self.install_dir_dep(name, src, source, graph, installed, visiting)
    }

    /// Shared installer for any source-tree dep (path or git). Reads the
    /// nested manifest, hashes the tree, copies to the store, recurses into
    /// transitive deps, and records the lockfile entry with the caller-
    /// supplied `source` string (`path+file://...` or `git+<url>#<rev>`).
    fn install_dir_dep(
        &self,
        name: &str,
        src: &Path,
        source: String,
        graph: &mut BTreeMap<String, ResolvedDep>,
        installed: &mut Vec<(String, String, PathBuf)>,
        visiting: &mut BTreeSet<String>,
    ) -> PkgResult<()> {
        if !src.is_dir() {
            return Err(PkgError::Resolve(format!(
                "dep `{}` does not exist or is not a directory: {}",
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
        let manifest_for_copy = nested.clone().unwrap_or_default();
        let dst = self
            .store
            .install_path_dep(name, &version, src, &manifest_for_copy)?;
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

        graph.insert(
            key.clone(),
            ResolvedDep {
                name: name.to_string(),
                version,
                source,
                integrity,
                deps: transitive,
                features: Vec::new(),
            },
        );
        visiting.remove(&key);
        Ok(())
    }
}

/// Recognize `https://github.com/OWNER/REPO[.git]` (or the bare
/// `github.com/OWNER/REPO` form, or with the `git@github.com:OWNER/REPO`
/// SSH-style form). Returns `Some((owner, repo))` so the caller can
/// route the dep to the release-tarball path instead of source-clone.
/// Trailing slashes, embedded sub-paths (`/tree/main`), and missing
/// owner/repo components fail the match — those are not bare repo URLs.
pub(crate) fn parse_github_url(url: &str) -> Option<(String, String)> {
    let body = if let Some(rest) = url.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://github.com/") {
        rest
    } else if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = url.strip_prefix("github.com/") {
        rest
    } else {
        return None;
    };
    let body = body.trim_end_matches('/');
    let mut parts = body.splitn(2, '/');
    let owner = parts.next()?;
    let remainder = parts.next()?;
    if owner.is_empty() || remainder.contains('/') {
        return None;
    }
    let repo = remainder.strip_suffix(".git").unwrap_or(remainder);
    if repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
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

/// Expand a workspace `members = ["..."]` pattern against `root_dir`. Supports
/// the two cases the RFC calls out: literal dirs (`crates/foo`) and one-level
/// wildcards (`crates/*`). Multi-segment globs and `**` are not supported —
/// the workspace pattern is a directory list, not a generic glob.
fn expand_workspace_glob(root_dir: &Path, pattern: &str) -> PkgResult<Vec<PathBuf>> {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        let parent = root_dir.join(prefix);
        if !parent.is_dir() {
            return Ok(Vec::new());
        }
        let mut out: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&parent)
            .map_err(|e| PkgError::Io(format!("read workspace dir {}: {}", parent.display(), e)))?
        {
            let entry = entry.map_err(|e| PkgError::Io(e.to_string()))?;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                out.push(entry.path());
            }
        }
        out.sort();
        Ok(out)
    } else if pattern.contains('*') {
        Err(PkgError::Resolve(format!(
            "workspace member pattern `{}` not supported — only literal dirs and `prefix/*` work today",
            pattern
        )))
    } else {
        Ok(vec![root_dir.join(pattern)])
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

    #[test]
    fn parse_github_url_https_form() {
        let (o, r) = parse_github_url("https://github.com/MenkeTechnologies/stryke-arrow").unwrap();
        assert_eq!(o, "MenkeTechnologies");
        assert_eq!(r, "stryke-arrow");
    }

    #[test]
    fn parse_github_url_strips_dot_git() {
        let (_, r) = parse_github_url("https://github.com/foo/bar.git").unwrap();
        assert_eq!(r, "bar");
    }

    #[test]
    fn parse_github_url_ssh_form() {
        let (o, r) = parse_github_url("git@github.com:foo/bar.git").unwrap();
        assert_eq!(o, "foo");
        assert_eq!(r, "bar");
    }

    #[test]
    fn parse_github_url_bare_host_form() {
        let (o, r) = parse_github_url("github.com/MenkeTechnologies/stryke-aws").unwrap();
        assert_eq!(o, "MenkeTechnologies");
        assert_eq!(r, "stryke-aws");
    }

    #[test]
    fn parse_github_url_rejects_non_github() {
        // Non-github URLs error out at install time — we never source-clone.
        assert!(parse_github_url("https://gitlab.com/foo/bar").is_none());
        assert!(parse_github_url("git://example.com/foo.git").is_none());
        assert!(parse_github_url("file:///tmp/local.git").is_none());
    }

    #[test]
    fn parse_github_url_rejects_subpath() {
        // `tree/main`, `releases`, etc. must not silently truncate to the repo.
        assert!(parse_github_url("https://github.com/foo/bar/tree/main").is_none());
        assert!(parse_github_url("https://github.com/foo/bar/releases").is_none());
    }

    #[test]
    fn parse_github_url_rejects_empty() {
        assert!(parse_github_url("https://github.com/").is_none());
        assert!(parse_github_url("https://github.com/foo").is_none());
        assert!(parse_github_url("https://github.com/foo/").is_none());
        assert!(parse_github_url("https://github.com/foo/.git").is_none());
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
        let mut deps = IndexMap::new();
        deps.insert(
            "mylib".to_string(),
            DepSpec::path_dep(dep.to_string_lossy().to_string()),
        );
        let m = Manifest {
            package: Some(crate::pkg::manifest::PackageMeta {
                name: "myapp".into(),
                version: "0.1.0".into(),
                ..Default::default()
            }),
            deps,
            ..Manifest::default()
        };

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
        let mut m = Manifest {
            package: Some(crate::pkg::manifest::PackageMeta {
                name: "myapp".into(),
                version: "0.1.0".into(),
                ..Default::default()
            }),
            ..Manifest::default()
        };
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
    fn workspace_resolves_member_deps_into_root_lockfile() {
        // Build a workspace with two members that share a common path-dep via
        // `[workspace.deps]` + `workspace = true`. Single root lockfile sees both.
        let leaf = make_path_dep("shared", "1.0.0");

        let ws_root = tempdir("ws_root");
        std::fs::create_dir_all(ws_root.join("crates/a/lib")).unwrap();
        std::fs::create_dir_all(ws_root.join("crates/b/lib")).unwrap();
        std::fs::write(
            ws_root.join("stryke.toml"),
            format!(
                r#"
[workspace]
members = ["crates/*"]

[workspace.deps]
shared = {{ path = "{}" }}
"#,
                leaf.display()
            ),
        )
        .unwrap();
        std::fs::write(
            ws_root.join("crates/a/stryke.toml"),
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[deps]\nshared = { workspace = true }\n",
        )
        .unwrap();
        std::fs::write(
            ws_root.join("crates/b/stryke.toml"),
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n\n[deps]\nshared = { workspace = true }\n",
        )
        .unwrap();

        let ws_manifest = Manifest::from_path(&ws_root.join("stryke.toml")).unwrap();
        let store_root = tempdir("ws_store");
        let store = Store::at(&store_root);
        let r = Resolver {
            manifest: &ws_manifest,
            manifest_dir: &ws_root,
            store: &store,
        };
        let outcome = r.resolve().unwrap();
        // Single dep in the lockfile — both members share the same `shared@1.0.0`.
        assert_eq!(outcome.lockfile.packages.len(), 1);
        assert_eq!(outcome.lockfile.packages[0].name, "shared");
        assert_eq!(outcome.lockfile.packages[0].version, "1.0.0");
    }

    #[test]
    fn workspace_glob_returns_sorted_member_dirs() {
        let root = tempdir("ws_glob");
        for n in ["zeta", "alpha", "beta"] {
            std::fs::create_dir_all(root.join(format!("crates/{}", n))).unwrap();
        }
        let dirs = super::expand_workspace_glob(&root, "crates/*").unwrap();
        let names: Vec<String> = dirs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn workspace_dep_without_table_is_an_error() {
        let root = tempdir("ws_err");
        std::fs::write(
            root.join("stryke.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[deps]\nshared = { workspace = true }\n",
        )
        .unwrap();
        let m = Manifest::from_path(&root.join("stryke.toml")).unwrap();
        let store_root = tempdir("ws_err_store");
        let store = Store::at(&store_root);
        let r = Resolver {
            manifest: &m,
            manifest_dir: &root,
            store: &store,
        };
        let err = r.resolve().unwrap_err().to_string();
        assert!(err.contains("workspace = true"), "got: {}", err);
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
        let mut m = Manifest {
            package: Some(crate::pkg::manifest::PackageMeta {
                name: "myapp".into(),
                version: "0.1.0".into(),
                ..Default::default()
            }),
            ..Manifest::default()
        };
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
