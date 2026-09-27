//! Resolve `@INC` paths from the system `perl` binary (same directories Perl searches for `.pm` files).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

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

/// `@INC` for a run: `-I` dirs, in-tree `vendor/perl` (pure-Perl modules, …),
/// system `perl`'s @INC, the script's directory, `STRYKE_INC`, then `.`
/// (deduped). `script` is the program's file name (`-e`, `-E`, `-` and `repl`
/// have no directory).
pub fn search_paths(include: &[String], script: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    push_unique_string_paths(&mut paths, include.to_vec());
    let vendor = crate::vendor_perl_inc_path();
    if vendor.is_dir() {
        push_unique_string_paths(&mut paths, vec![vendor.to_string_lossy().into_owned()]);
    }
    push_unique_string_paths(&mut paths, paths_from_system_perl());
    if !matches!(script, "-e" | "-E" | "-" | "repl") {
        if let Some(parent) = Path::new(script).parent() {
            if !parent.as_os_str().is_empty() {
                push_unique_string_paths(&mut paths, vec![parent.to_string_lossy().into_owned()]);
            }
        }
    }
    if let Ok(extra) = std::env::var("STRYKE_INC") {
        let extra: Vec<String> = std::env::split_paths(&extra)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        push_unique_string_paths(&mut paths, extra);
    }
    push_unique_string_paths(&mut paths, vec![".".to_string()]);
    paths
}

/// The `@INC` the parser searches for a module's `@EXPORT` (see
/// [`module_default_exports`]). The CLI sets it before parsing; unset, the
/// search uses [`search_paths`] with no `-I` and no script directory.
static COMPILE_SEARCH_PATHS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Default exports already read, by module name.
static DEFAULT_EXPORTS: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// Set the directories [`module_default_exports`] searches.
pub fn set_compile_search_paths(paths: Vec<String>) {
    *COMPILE_SEARCH_PATHS
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(paths);
    *DEFAULT_EXPORTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Names in `Module`'s `@EXPORT` — what `use Module;` imports — read from the
/// first `Module.pm` on the search path without running it. Only a literal
/// list is seen: `@EXPORT = qw(a b)`, `our @EXPORT = ('a', "b")`. A module
/// that is not found, or builds `@EXPORT` at run time, yields no names.
pub fn module_default_exports(module: &str) -> Vec<String> {
    let mut cache = DEFAULT_EXPORTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(names) = cache.as_ref().and_then(|c| c.get(module)) {
        return names.clone();
    }
    let dirs = COMPILE_SEARCH_PATHS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| search_paths(&[], "-e"));
    let rel = format!("{}.pm", module.replace("::", "/"));
    let names = dirs
        .iter()
        .map(|d| Path::new(d).join(&rel))
        .find(|p| p.is_file())
        .and_then(|p| crate::perl_fs::read_file_text_perl_compat(p).ok())
        .map(|src| export_list_in_source(&src))
        .unwrap_or_default();
    cache
        .get_or_insert_with(HashMap::new)
        .insert(module.to_string(), names.clone());
    names
}

/// The words of the first literal `@EXPORT = LIST` in module source, outside
/// POD and before `__END__` / `__DATA__`.
fn export_list_in_source(src: &str) -> Vec<String> {
    let mut code = String::new();
    let mut in_pod = false;
    for line in src.lines() {
        if in_pod {
            in_pod = !line.starts_with("=cut");
            continue;
        }
        if line.starts_with('=') && line[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            in_pod = true;
            continue;
        }
        if line == "__END__" || line == "__DATA__" {
            break;
        }
        if line.trim_start().starts_with('#') {
            continue; // a comment line may quote an `@EXPORT` list
        }
        code.push_str(line);
        code.push('\n');
    }
    let mut rest = code.as_str();
    while let Some(at) = rest.find("@EXPORT") {
        let after = &rest[at + "@EXPORT".len()..];
        rest = after;
        if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            continue; // @EXPORT_OK, @EXPORT_FAIL
        }
        let after = after.trim_start();
        let Some(rhs) = after.strip_prefix('=') else {
            continue;
        };
        if rhs.starts_with(['=', '~']) {
            continue;
        }
        if let Some(words) = literal_word_list(rhs.trim_start()) {
            return words;
        }
    }
    Vec::new()
}

/// `qw(...)`, or `( ITEM, ... )` of quoted strings and `qw` lists, at the start
/// of `s`. `None` when the list is anything else.
fn literal_word_list(s: &str) -> Option<Vec<String>> {
    if let Some(words) = qw_words(s) {
        return Some(words.0);
    }
    let mut rest = s.strip_prefix('(')?;
    let mut words = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(')') {
            return Some(words);
        }
        if let Some((w, r)) = qw_words(rest) {
            words.extend(w);
            rest = r;
        } else {
            let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
            let end = rest[1..].find(quote)? + 1;
            words.push(rest[1..end].to_string());
            rest = &rest[end + 1..];
        }
        rest = rest.trim_start();
        rest = rest.strip_prefix(',').unwrap_or(rest);
    }
}

/// `qw<delim> ... <close>` at the start of `s`: its words and the text after it.
fn qw_words(s: &str) -> Option<(Vec<String>, &str)> {
    let body = s.strip_prefix("qw")?.trim_start();
    let open = body.chars().next()?;
    if open.is_alphanumeric() || open == '_' {
        return None;
    }
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c => c,
    };
    let inner = &body[open.len_utf8()..];
    let end = inner.find(close)?;
    let words = inner[..end].split_whitespace().map(String::from).collect();
    Some((words, &inner[end + close.len_utf8()..]))
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
    fn export_list_is_the_first_literal_export_assignment() {
        // File::Basename / Carp / Getopt::Long shapes; @EXPORT_OK, comments, POD
        // and a later `@EXPORT` are not it.
        let src = "our @EXPORT_OK = qw(ok);\n# @EXPORT = qw(comment);\n\
                   =head1 X\n\n@EXPORT = qw(pod);\n\n=cut\n\
                   @EXPORT = qw(fileparse\n  basename &dirname $VAR);\n@EXPORT = qw(later);\n";
        assert_eq!(
            export_list_in_source(src),
            vec!["fileparse", "basename", "&dirname", "$VAR"]
        );
        assert_eq!(
            export_list_in_source("our @EXPORT = ( 'a', \"b\", qw/c d/ );"),
            vec!["a", "b", "c", "d"]
        );
    }

    #[test]
    fn export_list_built_at_run_time_is_not_read() {
        assert!(export_list_in_source("@EXPORT = @EXPORT_OK;").is_empty());
        assert!(
            export_list_in_source("if (@EXPORT == 0) {}\n__END__\n@EXPORT = qw(a);").is_empty()
        );
    }

    #[test]
    fn push_unique_string_paths_appends_in_order() {
        let mut v = vec!["first".to_string()];
        push_unique_string_paths(&mut v, vec!["second".to_string(), "third".to_string()]);
        assert_eq!(v, vec!["first", "second", "third"]);
    }
}
