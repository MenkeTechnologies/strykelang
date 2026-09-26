//! Resolve `@INC` paths from the system `perl` binary (same directories Perl searches for `.pm` files).

use std::path::PathBuf;
use std::process::Command;

/// If set (any value), do not append paths from `perl -e 'print join ... @INC'`.
pub const ENV_SKIP_PERL_INC: &str = "STRYKE_NO_PERL_INC";

/// Return the cache file path: `~/.stryke/perl_inc.txt`.
fn cache_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".stryke").join("perl_inc.txt"))
}

/// Run `perl` and read its `@INC`. Caches the result to `~/.stryke/perl_inc.txt`
/// to avoid spawning a perl subprocess on every startup (~3ms saved).
///
/// The cache's first line records the canonical path of the `perl` it was read
/// from (see [`CACHE_KEY_PREFIX`]). A different `perl` on `PATH` — including the
/// same Homebrew symlink now pointing at an upgraded Cellar version — is a miss,
/// so an upgrade from 5.42 to 5.44 does not leave `@INC` pointing at deleted
/// directories (where every `use List::Util` then fails with "Can't locate").
/// A cache written before the key line existed has no key and is also a miss.
/// Returns an empty vector if `perl` is missing, fails, or [`ENV_SKIP_PERL_INC`] is set.
pub fn paths_from_system_perl() -> Vec<String> {
    if std::env::var_os(ENV_SKIP_PERL_INC).is_some() {
        return Vec::new();
    }
    let key = resolve_perl_binary().map(|p| format!("{CACHE_KEY_PREFIX}{}", p.display()));
    // Try reading from cache first (microseconds vs milliseconds).
    if let (Some(ref path), Some(ref key)) = (cache_path(), &key) {
        if let Ok(contents) = crate::perl_fs::read_file_text_perl_compat(path) {
            if contents.lines().next() == Some(key.as_str()) {
                let paths = parse_perl_inc_output(&contents);
                if !paths.is_empty() {
                    return paths;
                }
            }
        }
    }
    // Cache miss — run perl and cache the result.
    let output = match Command::new("perl")
        .args(["-e", r#"print join "\n", @INC"#])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    if !output.status.success() {
        return Vec::new();
    }
    let raw = crate::perl_decode::decode_utf8_or_latin1(&output.stdout);
    // Write cache (best-effort, ignore errors). Without a key there is nothing to
    // validate a later read against, so nothing is cached.
    if let (Some(ref path), Some(ref key)) = (cache_path(), &key) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, format!("{key}\n{raw}"));
    }
    parse_perl_inc_output(&raw)
}

/// First line of the cache file: this prefix, then the canonical `perl` path.
const CACHE_KEY_PREFIX: &str = "#perl ";

/// The `perl` that `Command::new("perl")` would run: first `PATH` entry holding
/// an executable file named `perl`, with symlinks resolved. No subprocess.
fn resolve_perl_binary() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("perl"))
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
        .and_then(|p| std::fs::canonicalize(p).ok())
}

/// Split stdout from `perl -e 'print join "\n", @INC'` into directory paths.
/// Lines starting with `#` (the cache key) are not paths.
pub fn parse_perl_inc_output(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Append paths not already present (string equality, order preserved).
pub fn push_unique_string_paths(target: &mut Vec<String>, extra: Vec<String>) {
    for p in extra {
        if !target.iter().any(|e| e == &p) {
            target.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_perl_inc_output_trims_and_skips_blank_lines() {
        assert_eq!(
            parse_perl_inc_output("  /a/lib \n\n/b\n"),
            vec!["/a/lib".to_string(), "/b".to_string()]
        );
    }

    #[test]
    fn parse_perl_inc_output_skips_cache_key_line() {
        assert_eq!(
            parse_perl_inc_output("#perl /usr/bin/perl\n/a/lib\n/b\n"),
            vec!["/a/lib".to_string(), "/b".to_string()]
        );
    }

    #[test]
    fn push_unique_string_paths_dedupes() {
        let mut v = vec!["a".to_string()];
        push_unique_string_paths(&mut v, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn parse_perl_inc_output_empty_and_whitespace_only() {
        assert!(parse_perl_inc_output("").is_empty());
        assert!(parse_perl_inc_output("  \n\t\n").is_empty());
    }

    #[test]
    fn push_unique_string_paths_empty_extra_is_noop() {
        let mut v = vec!["a".to_string()];
        push_unique_string_paths(&mut v, vec![]);
        assert_eq!(v, vec!["a"]);
    }

    #[test]
    fn push_unique_string_paths_appends_in_order() {
        let mut v = vec!["first".to_string()];
        push_unique_string_paths(&mut v, vec!["second".to_string(), "third".to_string()]);
        assert_eq!(v, vec!["first", "second", "third"]);
    }
}
