//! Perl-style filesystem helpers (`stat`, `glob`, etc.).

use glob::{MatchOptions, Pattern};
use rand::Rng;
use rayon::prelude::*;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::pmap_progress::PmapProgress;
use crate::value::PerlValue;

pub use crate::perl_decode::{
    decode_utf8_or_latin1, decode_utf8_or_latin1_line, decode_utf8_or_latin1_read_until,
};

/// Read a file as text for Perl source or slurped data. Unlike [`std::fs::read_to_string`], this
/// does not reject bytes that are not valid UTF-8 (stock `perl` accepts such files by default).
pub fn read_file_text_perl_compat(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = std::fs::read(path.as_ref())?;
    Ok(decode_utf8_or_latin1(&bytes))
}

/// Like [`BufRead::read_line`] but decodes with [`decode_utf8_or_latin1_read_until`] (no U+FFFD).
pub fn read_line_perl_compat(reader: &mut impl BufRead, buf: &mut String) -> io::Result<usize> {
    buf.clear();
    let mut raw = Vec::new();
    let n = reader.read_until(b'\n', &mut raw)?;
    if n == 0 {
        return Ok(0);
    }
    buf.push_str(&decode_utf8_or_latin1_read_until(&raw));
    Ok(n)
}

/// One line from `reader` (delimiter `\n`), content **without** trailing `\n` / `\r\n` / `\r`,
/// same as [`BufRead::lines`] but UTF-8 or Latin-1 per line.
pub fn read_logical_line_perl_compat(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = reader.read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.ends_with(b"\n") {
        buf.pop();
        if buf.ends_with(b"\r") {
            buf.pop();
        }
    }
    Ok(Some(decode_utf8_or_latin1_line(&buf)))
}

/// Perl `-t` — true if the handle/path refers to a terminal ([`libc::isatty`] on Unix).
/// Recognizes `STDIN`/`STDOUT`/`STDERR`, `/dev/stdin` (etc.), `/dev/fd/N`, small numeric fds, or opens a path and tests its fd.
pub fn filetest_is_tty(path: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if let Some(fd) = tty_fd_literal(path) {
            return unsafe { libc::isatty(fd) != 0 };
        }
        if let Ok(f) = std::fs::File::open(path) {
            return unsafe { libc::isatty(f.as_raw_fd()) != 0 };
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    false
}

#[cfg(unix)]
fn tty_fd_literal(path: &str) -> Option<i32> {
    match path {
        "" | "STDIN" | "-" | "/dev/stdin" => Some(0),
        "STDOUT" | "/dev/stdout" => Some(1),
        "STDERR" | "/dev/stderr" => Some(2),
        p if p.starts_with("/dev/fd/") => p.strip_prefix("/dev/fd/").and_then(|s| s.parse().ok()),
        _ => path.parse::<i32>().ok().filter(|&n| (0..128).contains(&n)),
    }
}

/// Check if effective uid/gid has the given access to a file.
/// `check` is one of 4 (read), 2 (write), 1 (execute).
#[cfg(unix)]
pub fn filetest_effective_access(path: &str, check: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let mode = meta.mode();
    let euid = unsafe { libc::geteuid() };
    let egid = unsafe { libc::getegid() };
    // Root can read/write anything, execute if any x bit set
    if euid == 0 {
        return if check == 1 { mode & 0o111 != 0 } else { true };
    }
    if meta.uid() == euid {
        return mode & (check << 6) != 0;
    }
    if meta.gid() == egid {
        return mode & (check << 3) != 0;
    }
    mode & check != 0
}

/// Check if real uid/gid has the given access (uses libc::access).
#[cfg(unix)]
pub fn filetest_real_access(path: &str, amode: libc::c_int) -> bool {
    match std::ffi::CString::new(path) {
        Ok(c) => unsafe { libc::access(c.as_ptr(), amode) == 0 },
        Err(_) => false,
    }
}

/// Is the file owned by effective uid?
#[cfg(unix)]
pub fn filetest_owned_effective(path: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.uid() == unsafe { libc::geteuid() })
        .unwrap_or(false)
}

/// Is the file owned by real uid?
#[cfg(unix)]
pub fn filetest_owned_real(path: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.uid() == unsafe { libc::getuid() })
        .unwrap_or(false)
}

/// Is the file a named pipe (FIFO)?
#[cfg(unix)]
pub fn filetest_is_pipe(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_fifo())
        .unwrap_or(false)
}

/// Is the file a socket?
#[cfg(unix)]
pub fn filetest_is_socket(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false)
}

/// Is the file a block device?
#[cfg(unix)]
pub fn filetest_is_block_device(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_block_device())
        .unwrap_or(false)
}

/// Is the file a character device?
#[cfg(unix)]
pub fn filetest_is_char_device(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_char_device())
        .unwrap_or(false)
}

/// Is setuid bit set?
#[cfg(unix)]
pub fn filetest_is_setuid(path: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.mode() & 0o4000 != 0)
        .unwrap_or(false)
}

/// Is setgid bit set?
#[cfg(unix)]
pub fn filetest_is_setgid(path: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.mode() & 0o2000 != 0)
        .unwrap_or(false)
}

/// Is sticky bit set?
#[cfg(unix)]
pub fn filetest_is_sticky(path: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.mode() & 0o1000 != 0)
        .unwrap_or(false)
}

/// Is the file a text file? (Perl heuristic: read first block, check for high proportion of printable chars)
pub fn filetest_is_text(path: &str) -> bool {
    filetest_text_binary(path, true)
}

/// Is the file a binary file? (opposite of text)
pub fn filetest_is_binary(path: &str) -> bool {
    filetest_text_binary(path, false)
}

fn filetest_text_binary(path: &str, want_text: bool) -> bool {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 512];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n == 0 {
        // Empty files are considered text in Perl
        return want_text;
    }
    let slice = &buf[..n];
    // Count bytes that are "non-text": NUL and control chars (except \t \n \r \x1b)
    let non_text = slice
        .iter()
        .filter(|&&b| b == 0 || (b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r' && b != 0x1b))
        .count();
    let is_text = (non_text as f64 / n as f64) < 0.30;
    if want_text {
        is_text
    } else {
        !is_text
    }
}

/// File age in fractional days since now. `which`: 'M' = mtime, 'A' = atime, 'C' = ctime.
#[cfg(unix)]
pub fn filetest_age_days(path: &str, which: char) -> Option<f64> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    let t = match which {
        'M' => meta.mtime() as f64,
        'A' => meta.atime() as f64,
        _ => meta.ctime() as f64,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    Some((now - t) / 86400.0)
}

/// 13-element `stat` / `lstat` list (empty vector on failure).
pub fn stat_path(path: &str, symlink: bool) -> PerlValue {
    let res = if symlink {
        std::fs::symlink_metadata(path)
    } else {
        std::fs::metadata(path)
    };
    match res {
        Ok(meta) => PerlValue::array(perl_stat_from_metadata(&meta)),
        Err(_) => PerlValue::array(vec![]),
    }
}

pub fn perl_stat_from_metadata(meta: &std::fs::Metadata) -> Vec<PerlValue> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        vec![
            PerlValue::integer(meta.dev() as i64),
            PerlValue::integer(meta.ino() as i64),
            PerlValue::integer(meta.mode() as i64),
            PerlValue::integer(meta.nlink() as i64),
            PerlValue::integer(meta.uid() as i64),
            PerlValue::integer(meta.gid() as i64),
            PerlValue::integer(meta.rdev() as i64),
            PerlValue::integer(meta.len() as i64),
            PerlValue::integer(meta.atime()),
            PerlValue::integer(meta.mtime()),
            PerlValue::integer(meta.ctime()),
            PerlValue::integer(meta.blksize() as i64),
            PerlValue::integer(meta.blocks() as i64),
        ]
    }
    #[cfg(not(unix))]
    {
        let len = meta.len() as i64;
        vec![
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(len),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
            PerlValue::integer(0),
        ]
    }
}

pub fn link_hard(old: &str, new: &str) -> PerlValue {
    PerlValue::integer(if std::fs::hard_link(old, new).is_ok() {
        1
    } else {
        0
    })
}

pub fn link_sym(old: &str, new: &str) -> PerlValue {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        PerlValue::integer(if symlink(old, new).is_ok() { 1 } else { 0 })
    }
    #[cfg(not(unix))]
    {
        let _ = (old, new);
        PerlValue::integer(0)
    }
}

pub fn read_link(path: &str) -> PerlValue {
    match std::fs::read_link(path) {
        Ok(p) => PerlValue::string(p.to_string_lossy().into_owned()),
        Err(_) => PerlValue::UNDEF,
    }
}

/// Absolute path with symlinks resolved (`std::fs::canonicalize`); all path components must exist.
pub fn realpath_resolved(path: &str) -> io::Result<String> {
    std::fs::canonicalize(path).map(|p| p.to_string_lossy().into_owned())
}

/// Normalize `.` / `..` and redundant separators without touching the disk (Perl
/// `File::Spec->canonpath`-like). Unlike [`std::path::Path::components`] alone, this collapses
/// `foo/..` in relative paths instead of preserving `..` for symlink safety.
pub fn canonpath_logical(path: &str) -> String {
    use std::path::Component;
    if path.is_empty() {
        return String::new();
    }
    let mut stack: Vec<String> = Vec::new();
    let mut anchored = false;
    for c in Path::new(path).components() {
        match c {
            Component::Prefix(p) => {
                stack.push(p.as_os_str().to_string_lossy().into_owned());
            }
            Component::RootDir => {
                anchored = true;
                stack.clear();
            }
            Component::CurDir => {}
            Component::Normal(s) => {
                stack.push(s.to_string_lossy().into_owned());
            }
            Component::ParentDir => {
                if anchored {
                    if !stack.is_empty() {
                        stack.pop();
                    }
                } else if stack.is_empty() || stack.last().is_some_and(|t| t == "..") {
                    stack.push("..".to_string());
                } else {
                    stack.pop();
                }
            }
        }
    }
    let body = stack.join("/");
    if anchored {
        if body.is_empty() {
            "/".to_string()
        } else {
            format!("/{body}")
        }
    } else if body.is_empty() {
        ".".to_string()
    } else {
        body
    }
}

/// List file/directory names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read.
pub fn list_files(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only regular file names inside `dir` (non-recursive), sorted.
/// Excludes directories, symlinks, and special files.
/// Returns an empty list if `dir` cannot be read.
pub fn list_filesf(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only regular file paths under `dir` **recursively**, sorted.
/// Returns relative paths from `dir` (e.g. `"sub/file.txt"`).
/// Returns an empty list if `dir` cannot be read.
pub fn list_filesf_recursive(dir: &str) -> PerlValue {
    let root = std::path::Path::new(dir);
    let mut paths: Vec<String> = Vec::new();
    fn walk(base: &std::path::Path, rel: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(base) else {
            return;
        };
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if ft.is_file() {
                out.push(child_rel);
            } else if ft.is_dir() {
                walk(&base.join(&name), &child_rel, out);
            }
        }
    }
    walk(root, "", &mut paths);
    paths.sort();
    PerlValue::array(paths.into_iter().map(PerlValue::string).collect())
}

/// List only directory names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read.
pub fn list_dirs(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List subdirectory paths under `dir` **recursively**, sorted.
/// Returns relative paths from `dir` (e.g. `"sub/nested"`).
/// Returns an empty list if `dir` cannot be read.
pub fn list_dirs_recursive(dir: &str) -> PerlValue {
    let root = std::path::Path::new(dir);
    let mut paths: Vec<String> = Vec::new();
    fn walk(base: &std::path::Path, rel: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(base) else {
            return;
        };
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !ft.is_dir() {
                continue;
            }
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            out.push(child_rel.clone());
            walk(&base.join(&name), &child_rel, out);
        }
    }
    walk(root, "", &mut paths);
    paths.sort();
    PerlValue::array(paths.into_iter().map(PerlValue::string).collect())
}

/// List only symlink names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read.
pub fn list_sym_links(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only Unix socket names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read or on non-Unix platforms.
pub fn list_sockets(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_socket()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    let _ = dir;
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only named-pipe (FIFO) names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read or on non-Unix platforms.
pub fn list_pipes(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_fifo()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    let _ = dir;
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only block device names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read or on non-Unix platforms.
pub fn list_block_devices(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry
                    .file_type()
                    .map(|ft| ft.is_block_device())
                    .unwrap_or(false)
                {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    let _ = dir;
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
}

/// List only character device names inside `dir` (non-recursive), sorted.
/// Returns an empty list if `dir` cannot be read or on non-Unix platforms.
pub fn list_char_devices(dir: &str) -> PerlValue {
    let mut names: Vec<String> = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry
                    .file_type()
                    .map(|ft| ft.is_char_device())
                    .unwrap_or(false)
                {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    let _ = dir;
    names.sort();
    PerlValue::array(names.into_iter().map(PerlValue::string).collect())
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
    PerlValue::array(paths.into_iter().map(PerlValue::string).collect())
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
    glob_par_patterns_inner(patterns, None)
}

/// Same as [`glob_par_patterns`], with a stderr progress bar (one tick per pattern) when
/// `progress` is true.
pub fn glob_par_patterns_with_progress(patterns: &[String], progress: bool) -> PerlValue {
    if patterns.is_empty() {
        return PerlValue::array(Vec::new());
    }
    let pmap = PmapProgress::new(progress, patterns.len());
    let v = glob_par_patterns_inner(patterns, Some(&pmap));
    pmap.finish();
    v
}

fn glob_par_patterns_inner(patterns: &[String], progress: Option<&PmapProgress>) -> PerlValue {
    let options = MatchOptions::new();
    let out: Vec<String> = patterns
        .par_iter()
        .flat_map_iter(|pat| {
            let rows = (|| {
                let Ok(pattern) = Pattern::new(pat) else {
                    return Vec::new();
                };
                let base = glob_base_path(pat);
                if !base.exists() {
                    return Vec::new();
                }
                glob_par_walk(&base, &pattern, &options)
            })();
            if let Some(p) = progress {
                p.tick();
            }
            rows
        })
        .collect();
    let mut paths: Vec<String> = out.into_iter().map(normalize_glob_path_display).collect();
    paths.sort();
    paths.dedup();
    PerlValue::array(paths.into_iter().map(PerlValue::string).collect())
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
    PerlValue::integer(if std::fs::rename(old, new).is_ok() {
        1
    } else {
        0
    })
}

#[inline]
fn is_cross_device_rename(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(unix)]
    {
        if e.raw_os_error() == Some(libc::EXDEV) {
            return true;
        }
    }
    false
}

fn try_move_path(from: &str, to: &str) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) => {
            if !is_cross_device_rename(&e) {
                return Err(e);
            }
            let meta = std::fs::symlink_metadata(from)?;
            if meta.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "move: cross-device directory move is not supported",
                ));
            }
            if !meta.is_file() && !meta.is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "move: cross-device move supports files and symlinks only",
                ));
            }
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)?;
            Ok(())
        }
    }
}

/// `move OLD, NEW` / `mv` — like `rename`, but on cross-device failure copies the file then removes
/// the source (directories not supported for cross-device).
pub fn move_path(from: &str, to: &str) -> PerlValue {
    PerlValue::integer(if try_move_path(from, to).is_ok() {
        1
    } else {
        0
    })
}

#[cfg(unix)]
fn unix_path_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .is_some_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn unix_path_executable(path: &Path) -> bool {
    path.is_file()
}

fn display_executable_path(path: &Path) -> Option<String> {
    if !unix_path_executable(path) {
        return None;
    }
    path.canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| Some(path.to_string_lossy().into_owned()))
}

#[cfg(windows)]
fn pathext_suffixes() -> Vec<String> {
    env::var_os("PATHEXT")
        .map(|s| {
            env::split_paths(&s)
                .filter_map(|p| p.to_str().map(str::to_ascii_lowercase))
                .collect()
        })
        .unwrap_or_else(|| vec![".exe".into(), ".cmd".into(), ".bat".into(), ".com".into()])
}

#[cfg(windows)]
fn which_in_dir(dir: &Path, program: &str) -> Option<String> {
    let plain = dir.join(program);
    if let Some(s) = display_executable_path(&plain) {
        return Some(s);
    }
    if !program.contains('.') {
        for ext in pathext_suffixes() {
            let cand = dir.join(format!("{program}{ext}"));
            if let Some(s) = display_executable_path(&cand) {
                return Some(s);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn which_in_dir(dir: &Path, program: &str) -> Option<String> {
    display_executable_path(&dir.join(program))
}

/// Resolve `program` using `PATH` (and optional current directory when `include_dot`).
/// Returns a path string or `None` if not found.
pub fn which_executable(program: &str, include_dot: bool) -> Option<String> {
    if program.is_empty() {
        return None;
    }
    if program.contains('/') || (cfg!(windows) && program.contains('\\')) {
        return display_executable_path(Path::new(program));
    }
    let path_os = env::var_os("PATH")?;
    for dir in env::split_paths(&path_os) {
        if let Some(s) = which_in_dir(&dir, program) {
            return Some(s);
        }
    }
    if include_dot {
        return which_in_dir(Path::new("."), program);
    }
    None
}

/// Read entire file as raw bytes (no text decoding).
pub fn read_file_bytes(path: &str) -> io::Result<Arc<Vec<u8>>> {
    Ok(Arc::new(std::fs::read(path)?))
}

/// Temp file adjacent to `target` for atomic replace (`rename` into place).
fn adjacent_temp_path(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let rnd: u32 = rand::thread_rng().gen();
    dir.join(format!("{name}.spurt-tmp-{rnd}"))
}

/// Write bytes to `path`. When `mkdir_parents`, creates parent directories. When `atomic`, writes
/// to a unique temp file in the same directory then `rename`s into place (best-effort crash safety).
pub fn spurt_path(path: &str, data: &[u8], mkdir_parents: bool, atomic: bool) -> io::Result<()> {
    let path = Path::new(path);
    if mkdir_parents {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
    }
    if !atomic {
        return std::fs::write(path, data);
    }
    let tmp = adjacent_temp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `copy FROM, TO` — 1 on success, 0 on failure. When `preserve_metadata`, best-effort copy of
/// access/modification times from the source (after a successful byte copy).
pub fn copy_file(from: &str, to: &str, preserve_metadata: bool) -> PerlValue {
    let times = if preserve_metadata {
        std::fs::metadata(from).ok().map(|src_meta| {
            let at = src_meta
                .accessed()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mt = src_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (at, mt)
        })
    } else {
        None
    };
    if std::fs::copy(from, to).is_err() {
        return PerlValue::integer(0);
    }
    if let Some((at, mt)) = times {
        let _ = utime_paths(at, mt, &[to.to_string()]);
    }
    PerlValue::integer(1)
}

/// [`std::path::Path::file_name`] as a string (empty if none).
pub fn path_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Parent directory string; `"."` when absent; `"/"` for POSIX root-ish paths.
pub fn path_dirname(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let p = Path::new(path);
    if path == "/" {
        return "/".to_string();
    }
    match p.parent() {
        None => ".".to_string(),
        Some(parent) => {
            let s = parent.to_string_lossy();
            if s.is_empty() {
                ".".to_string()
            } else {
                s.into_owned()
            }
        }
    }
}

/// `(base, dir, suffix)` like Perl `File::Basename::fileparse` with a single optional suffix.
/// When `suffix` is `Some` and `full_base` ends with it, `base` has the suffix removed and `suffix`
/// is the matched suffix; otherwise `suffix` in the return is empty.
pub fn fileparse_path(path: &str, suffix: Option<&str>) -> (String, String, String) {
    let dir = path_dirname(path);
    let full_base = path_basename(path);
    let (base, sfx) = if let Some(suf) = suffix.filter(|s| !s.is_empty()) {
        if full_base.ends_with(suf) && full_base.len() > suf.len() {
            (
                full_base[..full_base.len() - suf.len()].to_string(),
                suf.to_string(),
            )
        } else {
            (full_base.clone(), String::new())
        }
    } else {
        (full_base.clone(), String::new())
    };
    (base, dir, sfx)
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

/// `utime ATIME, MTIME, FILES...` — count of paths successfully updated (Unix `utimes`; 0 on non-Unix).
pub fn utime_paths(atime_sec: i64, mtime_sec: i64, paths: &[String]) -> i64 {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let mut count = 0i64;
        let tv = [
            libc::timeval {
                tv_sec: atime_sec as libc::time_t,
                tv_usec: 0,
            },
            libc::timeval {
                tv_sec: mtime_sec as libc::time_t,
                tv_usec: 0,
            },
        ];
        for path in paths {
            let Ok(cs) = CString::new(path.as_str()) else {
                continue;
            };
            if unsafe { libc::utimes(cs.as_ptr(), tv.as_ptr()) } == 0 {
                count += 1;
            }
        }
        count
    }
    #[cfg(not(unix))]
    {
        let _ = (atime_sec, mtime_sec, paths);
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

/// `touch FILES...` — create files if they don't exist, update atime/mtime to
/// now if they do.  Returns count of files successfully touched.
pub fn touch_paths(paths: &[String]) -> i64 {
    use std::fs::OpenOptions;
    let mut count = 0i64;
    for path in paths {
        if path.is_empty() {
            continue;
        }
        // Create the file if it doesn't exist (like coreutils touch).
        let created = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .is_ok();
        if !created {
            continue;
        }
        // Update atime + mtime to now.
        #[cfg(unix)]
        {
            use std::ffi::CString;
            if let Ok(cs) = CString::new(path.as_str()) {
                // null timeval pointer ⇒ set both times to now
                unsafe { libc::utimes(cs.as_ptr(), std::ptr::null()) };
            }
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn glob_par_matches_sequential_glob_set() {
        let base = std::env::temp_dir().join(format!("forge_glob_par_{}", std::process::id()));
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

        let set_a: HashSet<String> = a
            .as_array_vec()
            .expect("expected array")
            .into_iter()
            .map(|x| x.to_string())
            .collect();
        let set_b: HashSet<String> = b
            .as_array_vec()
            .expect("expected array")
            .into_iter()
            .map(|x| x.to_string())
            .collect();
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
        let v = glob_par_patterns(&[pat])
            .as_array_vec()
            .expect("expected array");
        assert!(
            !v.is_empty(),
            "glob_par src/*.rs should find at least one .rs under src/"
        );
    }

    #[test]
    fn glob_par_progress_false_same_as_plain() {
        let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("glob_par_prog_false_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("probe.rs"), b"// x\n").unwrap();
        let pat = tmp.join("*.rs").to_string_lossy().replace('\\', "/");
        let a = glob_par_patterns(std::slice::from_ref(&pat));
        let b = glob_par_patterns_with_progress(std::slice::from_ref(&pat), false);
        let _ = std::fs::remove_dir_all(&tmp);
        let va = a.as_array_vec().expect("a");
        let vb = b.as_array_vec().expect("b");
        assert_eq!(va.len(), vb.len(), "glob_par vs glob_par(..., progress=>0)");
        for (x, y) in va.iter().zip(vb.iter()) {
            assert_eq!(x.to_string(), y.to_string());
        }
    }

    #[test]
    fn read_file_text_perl_compat_maps_invalid_utf8_to_latin1_octets() {
        let path = std::env::temp_dir().join(format!("forge_bad_utf8_{}.txt", std::process::id()));
        // Lone continuation bytes — invalid UTF-8 as a whole; per-line Latin-1.
        std::fs::write(&path, b"ok\xff\xfe\x80\n").unwrap();
        let s = read_file_text_perl_compat(&path).expect("read");
        assert!(s.starts_with("ok"));
        assert_eq!(&s[2..], "\u{00ff}\u{00fe}\u{0080}\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_logical_line_perl_compat_splits_and_decodes_per_line() {
        use std::io::Cursor;
        let mut r = Cursor::new(b"a\xff\nb\n");
        assert_eq!(
            read_logical_line_perl_compat(&mut r).unwrap(),
            Some("a\u{00ff}".to_string())
        );
        assert_eq!(
            read_logical_line_perl_compat(&mut r).unwrap(),
            Some("b".to_string())
        );
        assert_eq!(read_logical_line_perl_compat(&mut r).unwrap(), None);
    }
}
