//! stryke package manager — RFC: `docs/PACKAGE_REGISTRY.md`.
//!
//! This module hosts the local-only foundation (RFC phases 1–6):
//! manifest parsing, lockfile read/write, store layout, path-dep
//! resolution, and module-resolution wiring. Network (registry/git)
//! deps and the PubGrub semver resolver land in later tiers.
//!
//! Surface area:
//! - [`manifest`] — `stryke.toml` parser/serializer.
//! - [`lockfile`] — `stryke.lock` parser/serializer with deterministic
//!   ordering and SHA-256 integrity hashes.
//! - [`store`] — `~/.stryke/{store,cache,git,bin,index}/` layout helpers.
//! - [`resolver`] — local-only resolver (path deps work, registry/git
//!   deps return a clear "not yet implemented" error).
//! - [`commands`] — `s {init,new,add,remove,install,tree,info}` impls.

pub mod commands;
pub mod lockfile;
pub mod manifest;
pub mod resolver;
pub mod store;

/// `Result` alias used throughout the package manager. Errors are stringly-typed
/// for now (one user-facing diagnostic per failure path); structured errors land
/// when we wire the registry protocol.
pub type PkgResult<T> = Result<T, PkgError>;

/// Errors emitted by the package manager. Display impl produces a one-line
/// diagnostic suitable for emission to stderr and exit code 1.
#[derive(Debug)]
pub enum PkgError {
    /// File I/O — read/write/create.
    Io(String),
    /// Manifest parse error (bad TOML, missing `[package]`, etc.).
    Manifest(String),
    /// Lockfile parse error or integrity-hash mismatch.
    Lockfile(String),
    /// Resolver error — unknown dep, conflicting versions, registry not wired.
    Resolve(String),
    /// Generic runtime error.
    Other(String),
}

impl std::fmt::Display for PkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PkgError::Io(s) => write!(f, "{}", s),
            PkgError::Manifest(s) => write!(f, "{}", s),
            PkgError::Lockfile(s) => write!(f, "{}", s),
            PkgError::Resolve(s) => write!(f, "{}", s),
            PkgError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<std::io::Error> for PkgError {
    fn from(e: std::io::Error) -> Self {
        PkgError::Io(e.to_string())
    }
}
