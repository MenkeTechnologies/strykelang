//! Parallel recursive directory walk for `par_walk PATH, sub { ... }`.
//!
//! Within each directory, entries are processed in parallel (rayon); recursion descends into
//! subdirectories. Symlinks to directories are followed; non-directory symlinks are visited as
//! files.

use std::path::{Path, PathBuf};

use rayon::prelude::*;

/// Collect every file and directory path under `roots` (including each root path that exists),
/// using the same parallel-per-directory strategy as the live walk. Used when
/// `progress => EXPR` is enabled so the progress bar has a total count.
pub fn collect_paths(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .par_iter()
        .flat_map_iter(|r| collect_under(r))
        .collect()
}

fn collect_under(path: &Path) -> Vec<PathBuf> {
    if path.is_file() || (path.is_symlink() && !path.is_dir()) {
        return vec![path.to_path_buf()];
    }
    if !path.is_dir() {
        return vec![];
    }
    let read = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    let mut out = vec![path.to_path_buf()];
    let sub: Vec<PathBuf> = entries
        .par_iter()
        .flat_map_iter(|e| collect_under(&e.path()))
        .collect();
    out.extend(sub);
    out
}
