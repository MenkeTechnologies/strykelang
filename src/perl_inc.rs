//! Resolve `@INC` paths from the system `perl` binary (same directories Perl searches for `.pm` files).

use std::path::PathBuf;
use std::process::Command;

/// If set (any value), do not append paths from `perl -e 'print join ... @INC'`.
pub const ENV_SKIP_PERL_INC: &str = "PERLRS_NO_PERL_INC";

/// Return the cache file path: `~/.cache/perlrs/perl_inc.txt`.
fn cache_path() -> Option<PathBuf> {
    dirs_next().map(|d| d.join("perl_inc.txt"))
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache").join("perlrs"))
}

/// Run `perl` and read its `@INC`. Caches the result to `~/.cache/perlrs/perl_inc.txt`
/// to avoid spawning a perl subprocess on every startup (~3ms saved).
/// Returns an empty vector if `perl` is missing, fails, or [`ENV_SKIP_PERL_INC`] is set.
pub fn paths_from_system_perl() -> Vec<String> {
    if std::env::var_os(ENV_SKIP_PERL_INC).is_some() {
        return Vec::new();
    }
    // Try reading from cache first (microseconds vs milliseconds).
    if let Some(ref path) = cache_path() {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let paths = parse_perl_inc_output(&contents);
            if !paths.is_empty() {
                return paths;
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
    let raw = String::from_utf8_lossy(&output.stdout);
    // Write cache (best-effort, ignore errors).
    if let Some(ref path) = cache_path() {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, raw.as_bytes());
    }
    parse_perl_inc_output(&raw)
}

/// Split stdout from `perl -e 'print join "\n", @INC'` into directory paths.
pub fn parse_perl_inc_output(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
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
        push_unique_string_paths(
            &mut v,
            vec!["second".to_string(), "third".to_string()],
        );
        assert_eq!(v, vec!["first", "second", "third"]);
    }
}
