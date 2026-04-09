//! Resolve `@INC` paths from the system `perl` binary (same directories Perl searches for `.pm` files).

use std::process::Command;

/// If set (any value), do not append paths from `perl -e 'print join ... @INC'`.
pub const ENV_SKIP_PERL_INC: &str = "PERLRS_NO_PERL_INC";

/// Run `perl` and read its `@INC`. Returns an empty vector if `perl` is missing, fails, or
/// [`ENV_SKIP_PERL_INC`] is set.
pub fn paths_from_system_perl() -> Vec<String> {
    if std::env::var_os(ENV_SKIP_PERL_INC).is_some() {
        return Vec::new();
    }
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
    parse_perl_inc_output(&String::from_utf8_lossy(&output.stdout))
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
}
