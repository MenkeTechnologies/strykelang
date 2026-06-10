//! Global store layout: `~/.stryke/{store,cache,git,bin,index}/`.
//!
//! Paths are human-readable (`name@version`) per RFC §"Global Store" — we get
//! Nix-grade reproducibility from the lockfile's content hashes without
//! Nix-grade opaque path UX.
//!
//! Also hosts the global pin file [`InstalledIndex`] at `~/.stryke/installed.toml`,
//! which records every `s pkg install -g` so that one-off scripts run outside
//! a project can still resolve `use Foo` to a store entry. The project lockfile
//! takes precedence for in-project resolution; the installed index is only
//! consulted when there's no project lockfile entry for the requested package.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::manifest::Manifest;
use super::{PkgError, PkgResult};

/// Manifest filename — duplicated from `commands::MANIFEST_FILE` so `store`
/// stays free of `commands` imports (which would be a cycle).
const MANIFEST_FILE: &str = "stryke.toml";

/// Subtrees copied wholesale from a path-dep into the store. Mirrors the
/// canonical release-tarball layout: `lib/` (stk modules + cdylib in
/// production layout), `bin/` (script launchers).
const PACKAGE_SUBTREES: &[&str] = &["lib", "bin"];

/// Filename of the global installed-package index.
pub const INSTALLED_FILE: &str = "installed.toml";

/// Resolves and (lazily) creates the standard `~/.stryke/...` layout.
pub struct Store {
    /// `root` field.
    root: PathBuf,
}

impl Store {
    /// Construct a [`Store`] rooted at `~/.stryke/`. Honors the `STRYKE_HOME`
    /// environment variable for tests and CI sandboxes.
    pub fn user_default() -> PkgResult<Store> {
        if let Ok(custom) = std::env::var("STRYKE_HOME") {
            return Ok(Store {
                root: PathBuf::from(custom),
            });
        }
        let home = std::env::var("HOME")
            .map_err(|_| PkgError::Other("HOME environment variable not set".into()))?;
        Ok(Store {
            root: PathBuf::from(home).join(".stryke"),
        })
    }

    /// Construct a [`Store`] rooted at an explicit path (used by tests).
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }
    /// `root` — see implementation.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// `store_dir` — see implementation.
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }
    /// `cache_dir` — see implementation.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
    /// `git_dir` — see implementation.
    pub fn git_dir(&self) -> PathBuf {
        self.root.join("git")
    }
    /// `bin_dir` — see implementation.
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }
    /// `index_dir` — see implementation.
    pub fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    /// Path where a package extraction lives: `~/.stryke/store/{name}@{version}/`.
    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.store_dir().join(format!("{}@{}", name, version))
    }

    /// Ensure the full directory layout exists. Idempotent. Called eagerly by
    /// `s install`; tests exercise it directly.
    pub fn ensure_layout(&self) -> PkgResult<()> {
        for d in [
            self.store_dir(),
            self.cache_dir(),
            self.git_dir(),
            self.bin_dir(),
            self.index_dir(),
        ] {
            std::fs::create_dir_all(&d)
                .map_err(|e| PkgError::Io(format!("create {}: {}", d.display(), e)))?;
        }
        Ok(())
    }

    /// True if a `name@version` extraction already exists in the store.
    pub fn has_package(&self, name: &str, version: &str) -> bool {
        self.package_dir(name, version).is_dir()
    }

    /// Copy a path-dep into the store as `name@version` using the canonical
    /// release-tarball layout, so `s install -g .` produces a byte-for-byte
    /// equivalent on-disk shape to `s install -g github.com/<owner>/<repo>`.
    /// Exactly three things land in the store and nothing else (no
    /// `target/`, `.git/`, `vendor/`, `tests/`, root-level `README*` /
    /// `LICENSE*` / `CHANGELOG*`, editor scratch, or out-of-tree `[bin]`
    /// paths — the GitHub release tarball doesn't ship those either):
    ///
    /// * `stryke.toml` (required)
    /// * `lib/` subtree (recursive, if present)
    /// * `bin/` subtree (recursive, if present)
    ///
    /// Packages with `[bin]` entries pointing outside `bin/` are rejected by
    /// the release-tarball staging step (every shipped binary lives under
    /// `bin/<name>.stk`); the path install enforces the same rule by simply
    /// not copying anything outside the canonical subtrees, so an
    /// out-of-tree `[bin]` path becomes a no-op at install time rather than
    /// a store-layout divergence.
    ///
    /// Existing destination is removed first so re-installs see fresh content.
    pub fn install_path_dep(
        &self,
        name: &str,
        version: &str,
        src: &Path,
        _manifest: &Manifest,
    ) -> PkgResult<PathBuf> {
        let dst = self.package_dir(name, version);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)
                .map_err(|e| PkgError::Io(format!("clear {}: {}", dst.display(), e)))?;
        }
        std::fs::create_dir_all(&dst)?;

        let toml_src = src.join(MANIFEST_FILE);
        let toml_dst = dst.join(MANIFEST_FILE);
        std::fs::copy(&toml_src, &toml_dst)
            .map_err(|e| PkgError::Io(format!("copy {}: {}", toml_src.display(), e)))?;

        for sub in PACKAGE_SUBTREES {
            let from = src.join(sub);
            if from.is_dir() {
                let to = dst.join(sub);
                std::fs::create_dir_all(&to)?;
                copy_dir(&from, &to)?;
            }
        }

        Ok(dst)
    }
}

/// Global pin for every `s pkg install -g`-installed package.
///
/// Lives at `~/.stryke/installed.toml`. Unlike per-project lockfiles, this
/// has no SHA-256 integrity hashes and no transitive-dep records — it's a
/// flat name→version map that lets one-off scripts run outside any project
/// still resolve `use Foo` to a global store entry.
///
/// Conflict resolution in [`crate::pkg::commands::resolve_module`]:
/// project lockfile (#2) wins over this global pin (#3). If a project pins
/// `gui` at `0.1.0` and the global index has `gui@0.2.0`, the project gets
/// `0.1.0`; standalone scripts get `0.2.0`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledIndex {
    /// Schema version. Bumped when the layout changes incompatibly.
    pub version: u32,
    /// One entry per installed package, sorted by name.
    #[serde(default, rename = "package")]
    pub packages: Vec<InstalledPackage>,
}

/// One entry in the installed-package index.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledPackage {
    /// Package name (matches `[package].name` in the installed manifest).
    pub name: String,
    /// Version that was installed (matches `[package].version`).
    pub version: String,
    /// Where the install came from — `github:owner/repo`, `path+file://...`,
    /// etc. Recorded for `s list -g` display + future upgrade paths.
    pub source: String,
    /// `[ffi].namespace` from the installed manifest, lowercased. Empty when
    /// the package has no `[ffi]` section. Bridges `use GUI` (lookup key
    /// `"gui"`) to a store entry whose package name is unrelated (e.g.
    /// `stryke-gui`). Resolver tries name match first, then namespace match.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub namespace: String,
}

impl InstalledIndex {
    /// Empty index stamped with the current schema version.
    pub fn new() -> InstalledIndex {
        InstalledIndex {
            version: 1,
            packages: Vec::new(),
        }
    }

    /// Convenience: load via [`Store::user_default`] (honors `STRYKE_HOME`).
    /// Production code paths use this; tests prefer [`InstalledIndex::load_from`]
    /// with an explicit store root so parallel test execution doesn't race on
    /// the process-global env var.
    pub fn load_or_default() -> PkgResult<InstalledIndex> {
        let store = Store::user_default()?;
        Self::load_from(&store)
    }

    /// Load the index from a specific [`Store`] root. Returns an empty
    /// index when the file doesn't exist yet — the index materializes on
    /// the first `s pkg install -g`.
    pub fn load_from(store: &Store) -> PkgResult<InstalledIndex> {
        let path = store.root().join(INSTALLED_FILE);
        if !path.is_file() {
            return Ok(InstalledIndex::new());
        }
        let s = std::fs::read_to_string(&path)
            .map_err(|e| PkgError::Io(format!("read {}: {}", path.display(), e)))?;
        toml::from_str::<InstalledIndex>(&s)
            .map_err(|e| PkgError::Other(format!("parse {}: {}", path.display(), e.message())))
    }

    /// Convenience: save via [`Store::user_default`] (honors `STRYKE_HOME`).
    pub fn save(&self) -> PkgResult<()> {
        let store = Store::user_default()?;
        self.save_to(&store)
    }

    /// Save the index under a specific [`Store`] root. Packages are sorted
    /// by name first so the file is deterministic across runs and friendly
    /// to `diff`.
    pub fn save_to(&self, store: &Store) -> PkgResult<()> {
        let path = store.root().join(INSTALLED_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PkgError::Io(format!("create {}: {}", parent.display(), e)))?;
        }
        let mut copy = self.clone();
        copy.packages.sort_by(|a, b| a.name.cmp(&b.name));
        let mut header =
            String::from("# Auto-generated. Updated by `s pkg install -g` / `s uninstall -g`.\n");
        header.push_str("# Hand-edit this only if you understand the impact on `use X` resolution.\n\n");
        let body = toml::to_string_pretty(&copy)
            .map_err(|e| PkgError::Other(format!("serialize {}: {}", path.display(), e)))?;
        std::fs::write(&path, format!("{}{}", header, body))
            .map_err(|e| PkgError::Io(format!("write {}: {}", path.display(), e)))?;
        Ok(())
    }

    /// Find an installed package by name (case-sensitive). Use the package's
    /// canonical name from `[package].name`, not a logical `use Foo`-style
    /// segment — the lookup is verbatim.
    pub fn find(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// Find an installed package by `[ffi].namespace` (case-insensitive). Used
    /// by the resolver when `use Foo` doesn't match any `[package].name` —
    /// bridges `use GUI` → store entry `stryke-gui@*` where the package name
    /// (matches the repo / dir) is unrelated to the `use` namespace. The
    /// stored `namespace` keeps the manifest's exact casing (e.g. `"GUI"`);
    /// matching is case-insensitive so `use GUI` and `use gui` both land.
    pub fn find_by_namespace(&self, namespace: &str) -> Option<&InstalledPackage> {
        self.packages
            .iter()
            .find(|p| !p.namespace.is_empty() && p.namespace.eq_ignore_ascii_case(namespace))
    }

    /// Insert or overwrite the entry for `name`. Multiple installs of the
    /// same package (e.g. `s pkg install -g <url>` after a previous install)
    /// collapse to one entry — the latest install always wins.
    pub fn upsert(
        &mut self,
        name: impl Into<String>,
        version: impl Into<String>,
        source: impl Into<String>,
    ) {
        self.upsert_with_namespace(name, version, source, "");
    }

    /// `upsert` plus an `[ffi].namespace` value to record on the entry. Used
    /// by the install path so the resolver can later route `use GUI` to a
    /// store entry whose package name (matching the repo/dir) differs from
    /// the namespace.
    pub fn upsert_with_namespace(
        &mut self,
        name: impl Into<String>,
        version: impl Into<String>,
        source: impl Into<String>,
        namespace: impl Into<String>,
    ) {
        let name = name.into();
        let version = version.into();
        let source = source.into();
        let namespace = namespace.into();
        if let Some(slot) = self.packages.iter_mut().find(|p| p.name == name) {
            slot.version = version;
            slot.source = source;
            slot.namespace = namespace;
        } else {
            self.packages.push(InstalledPackage {
                name,
                version,
                source,
                namespace,
            });
        }
    }

    /// Remove the entry for `name`. Returns `true` if a matching entry was
    /// removed, `false` if the package wasn't in the index.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.packages.len();
        self.packages.retain(|p| p.name != name);
        self.packages.len() != before
    }
}

/// Recursive directory copy. Symlinks are copied as symlinks; files preserve
/// permissions when the OS supports it.
fn copy_dir(src: &Path, dst: &Path) -> PkgResult<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let name = entry.file_name();
        let to = dst.join(&name);
        let meta = entry.metadata()?;
        if meta.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from)?;
                std::os::unix::fs::symlink(target, &to)
                    .map_err(|e| PkgError::Io(format!("symlink {}: {}", to.display(), e)))?;
            }
            #[cfg(not(unix))]
            std::fs::copy(&from, &to)
                .map_err(|e| PkgError::Io(format!("copy {}: {}", from.display(), e)))?;
        } else if meta.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| PkgError::Io(format!("copy {}: {}", from.display(), e)))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = std::env::temp_dir().join(format!("stryke-store-test-{}-{}", pid, nanos));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn ensure_layout_creates_subdirs() {
        let root = tempdir();
        let s = Store::at(&root);
        s.ensure_layout().unwrap();
        assert!(s.store_dir().is_dir());
        assert!(s.cache_dir().is_dir());
        assert!(s.git_dir().is_dir());
        assert!(s.bin_dir().is_dir());
        assert!(s.index_dir().is_dir());
    }

    #[test]
    fn package_dir_path_shape() {
        let s = Store::at("/x");
        assert_eq!(
            s.package_dir("http", "1.0.0"),
            PathBuf::from("/x/store/http@1.0.0")
        );
    }

    #[test]
    fn installed_index_round_trip() {
        // Use an explicit Store::at() rather than STRYKE_HOME so parallel
        // test execution doesn't race on the process-global env var. The
        // load_from/save_to API mirrors Store::at vs Store::user_default.
        let root = tempdir();
        let store = Store::at(&root);
        store.ensure_layout().unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.2.0", "github:MenkeTechnologies/stryke-gui");
        idx.upsert("aws", "0.1.0", "github:MenkeTechnologies/stryke-aws");
        idx.save_to(&store).unwrap();

        let reloaded = InstalledIndex::load_from(&store).unwrap();
        assert_eq!(reloaded.version, 1);
        assert_eq!(reloaded.packages.len(), 2);
        // Sorted by name on save.
        assert_eq!(reloaded.packages[0].name, "aws");
        assert_eq!(reloaded.packages[1].name, "gui");
        assert_eq!(
            reloaded.find("gui").unwrap().source,
            "github:MenkeTechnologies/stryke-gui"
        );
    }

    #[test]
    fn installed_index_upsert_overwrites() {
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.1.0", "github:a/b");
        idx.upsert("gui", "0.2.0", "github:a/b");
        assert_eq!(idx.packages.len(), 1);
        assert_eq!(idx.packages[0].version, "0.2.0");
    }

    #[test]
    fn installed_index_remove_returns_true_when_present() {
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.1.0", "github:a/b");
        assert!(idx.remove("gui"));
        assert!(!idx.remove("gui"));
        assert!(idx.packages.is_empty());
    }

    #[test]
    fn installed_index_load_from_returns_empty_when_missing() {
        let root = tempdir();
        let store = Store::at(&root);
        store.ensure_layout().unwrap();
        let idx = InstalledIndex::load_from(&store).unwrap();
        assert!(idx.packages.is_empty());
    }

    #[test]
    fn install_path_dep_round_trip() {
        let store_root = tempdir();
        let src = tempdir();
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(src.join("lib/Foo.stk"), b"sub foo { 1 }").unwrap();
        std::fs::write(
            src.join("stryke.toml"),
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let s = Store::at(&store_root);
        s.ensure_layout().unwrap();
        let manifest = Manifest::default();
        let dst = s
            .install_path_dep("foo", "0.1.0", &src, &manifest)
            .unwrap();
        assert!(dst.is_dir());
        assert!(dst.join("lib/Foo.stk").is_file());
        assert!(dst.join("stryke.toml").is_file());
    }

    /// `s install -g .` from a working repo must NOT drag `target/`, `.git/`,
    /// `vendor/`, or test fixtures into the store — the GH install only ships
    /// the curated tarball, and the local path now matches that file set.
    #[test]
    fn install_path_dep_excludes_build_artifacts() {
        let store_root = tempdir();
        let src = tempdir();
        std::fs::write(
            src.join("stryke.toml"),
            "[package]\nname = \"stryke-arrow\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(src.join("lib/Arrow.stk"), b"sub arrow { 1 }").unwrap();
        std::fs::write(src.join("README.md"), b"# stryke-arrow\n").unwrap();
        std::fs::write(src.join("LICENSE"), b"MIT\n").unwrap();

        // Cruft that must stay out of the store.
        std::fs::create_dir_all(src.join("target/release")).unwrap();
        std::fs::write(src.join("target/release/garbage.rlib"), b"junk").unwrap();
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(src.join("vendor/foo")).unwrap();
        std::fs::write(src.join("vendor/foo/big.bin"), b"vendor").unwrap();
        std::fs::create_dir_all(src.join("tests")).unwrap();
        std::fs::write(src.join("tests/fixture.stk"), b"# fixture").unwrap();
        std::fs::write(src.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(src.join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();

        let s = Store::at(&store_root);
        s.ensure_layout().unwrap();
        let manifest = Manifest::default();
        let dst = s
            .install_path_dep("stryke-arrow", "0.1.0", &src, &manifest)
            .unwrap();

        // Store dir name is the package's exact authored name.
        assert!(dst.ends_with("store/stryke-arrow@0.1.0"));

        // Canonical layout present.
        assert!(dst.join("stryke.toml").is_file());
        assert!(dst.join("lib/Arrow.stk").is_file());

        // Cruft absent. README / LICENSE / CHANGELOG are part of "cruft"
        // because the GitHub release tarball doesn't ship them either —
        // `s install -g .` must produce a byte-for-byte equivalent store
        // layout to `s install -g github.com/...`.
        assert!(!dst.join("README.md").exists(), "README.md leaked");
        assert!(!dst.join("LICENSE").exists(), "LICENSE leaked");
        assert!(!dst.join("target").exists(), "target/ leaked into store");
        assert!(!dst.join(".git").exists(), ".git/ leaked into store");
        assert!(!dst.join("vendor").exists(), "vendor/ leaked into store");
        assert!(!dst.join("tests").exists(), "tests/ leaked into store");
        assert!(!dst.join(".DS_Store").exists(), ".DS_Store leaked");
        assert!(!dst.join("Cargo.toml").exists(), "Cargo.toml leaked");
    }
}
