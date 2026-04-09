//! Perl-style filesystem helpers (`stat`, `glob`, etc.).

use glob::{MatchOptions, Pattern};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use crate::value::PerlValue;

/// 13-element `stat` / `lstat` list (empty vector on failure).
pub fn stat_path(path: &str, symlink: bool) -> PerlValue {
    let res = if symlink {
        std::fs::symlink_metadata(path)
    } else {
        std::fs::metadata(path)
    };
    match res {
        Ok(meta) => PerlValue::Array(perl_stat_from_metadata(&meta)),
        Err(_) => PerlValue::Array(vec![]),
    }
}

pub fn perl_stat_from_metadata(meta: &std::fs::Metadata) -> Vec<PerlValue> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        vec![
            PerlValue::Integer(meta.dev() as i64),
            PerlValue::Integer(meta.ino() as i64),
            PerlValue::Integer(meta.mode() as i64),
            PerlValue::Integer(meta.nlink() as i64),
            PerlValue::Integer(meta.uid() as i64),
            PerlValue::Integer(meta.gid() as i64),
            PerlValue::Integer(meta.rdev() as i64),
            PerlValue::Integer(meta.len() as i64),
            PerlValue::Integer(meta.atime()),
            PerlValue::Integer(meta.mtime()),
            PerlValue::Integer(meta.ctime()),
            PerlValue::Integer(meta.blksize() as i64),
            PerlValue::Integer(meta.blocks() as i64),
        ]
    }
    #[cfg(not(unix))]
    {
        let len = meta.len() as i64;
        vec![
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(len),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
            PerlValue::Integer(0),
        ]
    }
}

pub fn link_hard(old: &str, new: &str) -> PerlValue {
    PerlValue::Integer(if std::fs::hard_link(old, new).is_ok() {
        1
    } else {
        0
    })
}

pub fn link_sym(old: &str, new: &str) -> PerlValue {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        PerlValue::Integer(if symlink(old, new).is_ok() { 1 } else { 0 })
    }
    #[cfg(not(unix))]
    {
        let _ = (old, new);
        PerlValue::Integer(0)
    }
}

pub fn read_link(path: &str) -> PerlValue {
    match std::fs::read_link(path) {
        Ok(p) => PerlValue::String(p.to_string_lossy().into_owned()),
        Err(_) => PerlValue::Undef,
    }
}

pub fn glob_patterns(patterns: &[String]) -> PerlValue {
    let mut paths: Vec<String> = Vec::new();
    for pat in patterns {
        if let Ok(g) = glob::glob(pat) {
            for e in g.flatten() {
                paths.push(normalize_glob_path_display(
                    e.to_string_lossy().into_owned(),
                ));
            }
        }
    }
    paths.sort();
    paths.dedup();
    PerlValue::Array(paths.into_iter().map(PerlValue::String).collect())
}

/// Directory prefix of `pat` with no glob metacharacters in any path component.
fn glob_base_path(pat: &str) -> PathBuf {
    let p = Path::new(pat);
    let mut acc = PathBuf::new();
    for c in p.components() {
        let s = c.as_os_str().to_string_lossy();
        if s.contains('*') || s.contains('?') || s.contains('[') {
            break;
        }
        acc.push(c.as_os_str());
    }
    if acc.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        acc
    }
}

fn glob_par_walk(dir: &Path, pattern: &Pattern, options: &MatchOptions) -> Vec<String> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries
        .par_iter()
        .flat_map_iter(|e| {
            let path = e.path();
            let mut out = Vec::new();
            let s = path.to_string_lossy();
            if pattern.matches_with(s.as_ref(), *options) {
                out.push(s.into_owned());
            }
            if path.is_dir() {
                out.extend(glob_par_walk(&path, pattern, options));
            }
            out.into_iter()
        })
        .collect()
}

/// Parallel recursive glob: same pattern semantics as [`glob_patterns`], but walks the
/// filesystem with rayon per directory (and parallelizes across patterns).
pub fn glob_par_patterns(patterns: &[String]) -> PerlValue {
    let options = MatchOptions::new();
    let out: Vec<String> = patterns
        .par_iter()
        .flat_map_iter(|pat| {
            let Ok(pattern) = Pattern::new(pat) else {
                return Vec::new();
            };
            let base = glob_base_path(pat);
            if !base.exists() {
                return Vec::new();
            }
            glob_par_walk(&base, &pattern, &options)
        })
        .collect();
    let mut paths: Vec<String> = out.into_iter().map(normalize_glob_path_display).collect();
    paths.sort();
    paths.dedup();
    PerlValue::Array(paths.into_iter().map(PerlValue::String).collect())
}

/// Stable display form for glob results: relative paths get a `./` prefix when missing.
fn normalize_glob_path_display(s: String) -> String {
    let p = Path::new(&s);
    if p.is_absolute() || s.starts_with("./") || s.starts_with("../") {
        s
    } else {
        format!("./{s}")
    }
}

/// `rename OLD, NEW` — 1 on success, 0 on failure (Perl-style).
pub fn rename_paths(old: &str, new: &str) -> PerlValue {
    PerlValue::Integer(if std::fs::rename(old, new).is_ok() {
        1
    } else {
        0
    })
}

/// `chmod MODE, FILES...` — count of files successfully chmod'd.
pub fn chmod_paths(paths: &[String], mode: i64) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut count = 0i64;
        for path in paths {
            if let Ok(meta) = std::fs::metadata(path) {
                let mut perms = meta.permissions();
                let old = perms.mode();
                // Perl passes permission bits (e.g. 0644); preserve st_mode file-type bits.
                perms.set_mode((old & !0o777) | (mode as u32 & 0o777));
                if std::fs::set_permissions(path, perms).is_ok() {
                    count += 1;
                }
            }
        }
        count
    }
    #[cfg(not(unix))]
    {
        let _ = (paths, mode);
        0
    }
}

/// `chown UID, GID, FILES...` — count of files successfully chown'd (Unix only; 0 on non-Unix).
pub fn chown_paths(paths: &[String], uid: i64, gid: i64) -> i64 {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let u = if uid < 0 {
            (!0u32) as libc::uid_t
        } else {
            uid as libc::uid_t
        };
        let g = if gid < 0 {
            (!0u32) as libc::gid_t
        } else {
            gid as libc::gid_t
        };
        let mut count = 0i64;
        for path in paths {
            let Ok(c) = CString::new(path.as_str()) else {
                continue;
            };
            let r = unsafe { libc::chown(c.as_ptr(), u, g) };
            if r == 0 {
                count += 1;
            }
        }
        count
    }
    #[cfg(not(unix))]
    {
        let _ = (paths, uid, gid);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn glob_par_matches_sequential_glob_set() {
        let base = std::env::temp_dir().join(format!("perlrs_glob_par_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a")).unwrap();
        std::fs::create_dir_all(base.join("b")).unwrap();
        std::fs::create_dir_all(base.join("b/nested")).unwrap();
        std::fs::File::create(base.join("a/x.log")).unwrap();
        std::fs::File::create(base.join("b/y.log")).unwrap();
        std::fs::File::create(base.join("b/nested/z.log")).unwrap();
        std::fs::File::create(base.join("root.txt")).unwrap();

        // Absolute patterns only — never `set_current_dir`; other tests run in parallel.
        let pat = format!("{}/**/*.log", base.display());
        let a = glob_patterns(std::slice::from_ref(&pat));
        let b = glob_par_patterns(std::slice::from_ref(&pat));
        let _ = std::fs::remove_dir_all(&base);

        let set_a: HashSet<String> = match a {
            PerlValue::Array(v) => v.into_iter().map(|x| x.to_string()).collect(),
            _ => panic!("expected array"),
        };
        let set_b: HashSet<String> = match b {
            PerlValue::Array(v) => v.into_iter().map(|x| x.to_string()).collect(),
            _ => panic!("expected array"),
        };
        assert_eq!(set_a, set_b);
    }

    #[test]
    fn glob_par_src_rs_matches_when_src_tree_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let src = root.join("src");
        if !src.is_dir() {
            return;
        }
        let pat = src.join("*.rs").to_string_lossy().into_owned();
        let PerlValue::Array(v) = glob_par_patterns(&[pat]) else {
            panic!("expected array");
        };
        assert!(
            !v.is_empty(),
            "glob_par src/*.rs should find at least one .rs under src/"
        );
    }
}
