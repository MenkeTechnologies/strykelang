//! Parallel file notifications for `pwatch GLOB, sub { ... }` (notify + rayon).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{PerlError, PerlResult};
use crate::interpreter::{Interpreter, WantarrayCtx};
use crate::scope::{AtomicArray, AtomicHash};
use crate::value::{PerlSub, PerlValue};

/// Expand `pattern`, register native watches, then block dispatching each matching path to `sub` on a
/// rayon worker (`$_` = path string).
pub fn run_pwatch(
    pattern: &str,
    sub: Arc<PerlSub>,
    subs: HashMap<String, Arc<PerlSub>>,
    scalars: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    line: usize,
) -> PerlResult<PerlValue> {
    let gpat = glob::Pattern::new(pattern)
        .map_err(|e| PerlError::runtime(format!("pwatch: invalid glob pattern: {}", e), line))?;

    let expanded: Vec<PathBuf> = glob::glob(pattern)
        .map_err(|e| PerlError::runtime(format!("pwatch: glob: {}", e), line))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PerlError::runtime(format!("pwatch: glob: {}", e), line))?;

    let mut watch_specs: Vec<(PathBuf, RecursiveMode)> = Vec::new();
    let mut seen = HashSet::new();

    if expanded.is_empty() {
        if let Some(dir) = parent_dir_for_glob(pattern) {
            if dir.is_dir() {
                let key = dir.clone();
                if seen.insert(key) {
                    watch_specs.push((dir, RecursiveMode::NonRecursive));
                }
            }
        }
        // Literal path with no wildcards (e.g. `watch "/tmp/x", ...`) when the file does not exist
        // yet: glob matches nothing and `parent_dir_for_glob` does not apply. Watch the parent dir.
        if watch_specs.is_empty() {
            if let Some(dir) = parent_dir_for_literal_missing_path(pattern) {
                let key = dir.clone();
                if seen.insert(key) {
                    watch_specs.push((dir, RecursiveMode::NonRecursive));
                }
            }
        }
    } else {
        for p in expanded {
            if p.is_dir() {
                let key = p.clone();
                if seen.insert(key) {
                    watch_specs.push((p, RecursiveMode::Recursive));
                }
            } else if p.exists() {
                let key = p.clone();
                if seen.insert(key) {
                    watch_specs.push((p, RecursiveMode::NonRecursive));
                }
            }
        }
    }

    if watch_specs.is_empty() {
        return Err(PerlError::runtime(
            "pwatch: no paths to watch (glob matched nothing and no parent directory found)",
            line,
        ));
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher: RecommendedWatcher =
        RecommendedWatcher::new(move |res| drop(tx.send(res)), Config::default()).map_err(|e| {
            PerlError::runtime(format!("pwatch: could not create watcher: {}", e), line)
        })?;

    for (path, mode) in &watch_specs {
        watcher.watch(path, *mode).map_err(|e| {
            PerlError::runtime(
                format!("pwatch: cannot watch {}: {}", path.display(), e),
                line,
            )
        })?;
    }

    // Poll the channel with a timeout so Ctrl-C (and any other `%SIG` hook) can break out of
    // the watch loop — a naked `rx.recv()` would sit forever and force the user to `kill -9`.
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;
    loop {
        if crate::perl_signal::pending("INT") || crate::perl_signal::pending("TERM") {
            return Err(PerlError::runtime("pwatch: interrupted", line));
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    let path_string = path.to_string_lossy().into_owned();
                    if !gpat.matches(&path_string) {
                        continue;
                    }
                    let sub = Arc::clone(&sub);
                    let subs = subs.clone();
                    let scalars = scalars.clone();
                    let aa = atomic_arrays.clone();
                    let ah = atomic_hashes.clone();
                    rayon::spawn(move || {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs;
                        local_interp.scope.restore_capture(&scalars);
                        local_interp.scope.restore_atomics(&aa, &ah);
                        local_interp.enable_parallel_guard();
                        local_interp.scope.set_topic(PerlValue::string(path_string));
                        let _ = local_interp.call_sub(&sub, vec![], WantarrayCtx::Void, line);
                    });
                }
            }
            Ok(Err(e)) => {
                return Err(PerlError::runtime(format!("pwatch: {}", e), line));
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(PerlError::runtime("pwatch: watcher channel closed", line));
            }
        }
    }
}

/// Directory to watch when `glob(pattern)` yields no existing paths (e.g. `*.log` before files exist).
fn parent_dir_for_glob(pattern: &str) -> Option<PathBuf> {
    let idx = pattern.find('*').or_else(|| pattern.find('?'))?;
    let before = pattern[..idx].trim_end_matches('/');
    if before.is_empty() {
        return Some(PathBuf::from("."));
    }
    let p = Path::new(before);
    if p.is_dir() {
        Some(p.to_path_buf())
    } else {
        p.parent().map(Path::to_path_buf)
    }
}

/// Parent directory to watch when the pattern has no `*?` wildcards, the path does not exist yet,
/// and the parent is an existing directory (so creation/modify events can still match `pattern`).
fn parent_dir_for_literal_missing_path(pattern: &str) -> Option<PathBuf> {
    if pattern.contains('*') || pattern.contains('?') {
        return None;
    }
    let p = Path::new(pattern);
    if p.exists() {
        return None;
    }
    let parent = p.parent().map(Path::to_path_buf).or_else(|| {
        // Relative single-component path like `foo` — watch cwd.
        let has_sep = pattern.contains('/') || pattern.contains('\\');
        if !has_sep && !pattern.starts_with('/') {
            Some(PathBuf::from("."))
        } else {
            None
        }
    })?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    parent.is_dir().then_some(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glob::Pattern;

    #[test]
    fn glob_pattern_matches_literal_absolute_path() {
        let g = Pattern::new("/tmp/x").unwrap();
        assert!(g.matches("/tmp/x"));
        assert!(!g.matches("/tmp/y"));
    }

    #[test]
    fn parent_dir_for_literal_missing_path_absolute() {
        let tmp = std::env::temp_dir();
        let child = tmp.join("perlrs_pwatch_literal_test_path");
        let pat = child.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&child);
        assert!(!child.exists());
        let par = parent_dir_for_literal_missing_path(&pat).expect("parent");
        assert_eq!(par, tmp);
    }
}
