//! Global store layout: `~/.stryke/{store,cache,git,bin,index}/`.
//!
//! Paths are human-readable (`name@version`) per RFC §"Global Store" — we get
//! Nix-grade reproducibility from the lockfile's content hashes without
//! Nix-grade opaque path UX.

use std::path::{Path, PathBuf};

use super::{PkgError, PkgResult};

/// Resolves and (lazily) creates the standard `~/.stryke/...` layout.
pub struct Store {
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

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn git_dir(&self) -> PathBuf {
        self.root.join("git")
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }
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

    /// Recursively copy a directory tree into the store as `name@version`. Used
    /// for path deps where the source is a local directory the user maintains.
    /// Existing destination is removed first so re-installs see fresh content.
    pub fn install_path_dep(&self, name: &str, version: &str, src: &Path) -> PkgResult<PathBuf> {
        let dst = self.package_dir(name, version);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)
                .map_err(|e| PkgError::Io(format!("clear {}: {}", dst.display(), e)))?;
        }
        std::fs::create_dir_all(&dst)?;
        copy_dir(src, &dst)?;
        Ok(dst)
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
        let dst = s.install_path_dep("foo", "0.1.0", &src).unwrap();
        assert!(dst.is_dir());
        assert!(dst.join("lib/Foo.stk").is_file());
        assert!(dst.join("stryke.toml").is_file());
    }
}
