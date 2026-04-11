//! Perl 5 `${^NAME}` scalars: see [perlvar](https://perldoc.perl.org/perlvar).
//!
//! Names not implemented with dedicated [`crate::interpreter::Interpreter`] fields are stored in
//! `Interpreter::special_caret_scalars` (default `undef`). Reads use that map; unknown names still
//! return `undef` without a pre-inserted key.

/// Scalar slot names populated by the regex engine on a successful match (`$&`, `` $` ``, `$'`,
/// `$+`, `$-`, `$1`…). Used by parallel-block write checks: each worker may update its own captures.
#[inline]
pub fn is_regex_match_scalar_name(name: &str) -> bool {
    match name {
        "&" | "'" | "`" | "+" | "-" => true,
        _ => !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()),
    }
}

/// Documented `${^NAME}` scalars from Perl 5 `perlvar` (and closely related names), pre-seeded as
/// `undef` so `defined ${^NAME}` / iteration over known names works without assigning first.
///
/// Not every name is fully implemented in the interpreter; see [`SPECIAL_VARIABLES.md`](../../SPECIAL_VARIABLES.md).
pub static PERL5_DOCUMENTED_CARET_NAMES: &[&str] = &[
    "CAPTURE",
    "CAPTURE_ALL",
    "CHILD_ERROR_NATIVE",
    "ENCODING",
    "GLOBAL_PHASE",
    "HOOK",
    "LAST_FH",
    "LAST_SUBMATCH_RESULT",
    "LAST_SUCCESSFUL_PATTERN",
    "MATCH",
    "MAX_NESTED_EVAL_BEGIN_BLOCKS",
    "OPEN",
    "POSTMATCH",
    "PREMATCH",
    "REGERROR",
    "RE_COMPILE_RECURSION_LIMIT",
    "RE_DEBUG_FLAGS",
    "RE_TRIE_MAXBUF",
    "SAFE_LOCALES",
    "SAFE_PATHS",
    "TAINT",
    "TAINTED",
    "UNICODE",
    "UTF8CACHE",
    "UTF8LOCALE",
    "WARNING_BITS",
    "WIDE_SYSTEM_CALLS",
    "WIN32_PROCESS_HANDLE",
    "WIN32_SLOPPY_STAT",
    "WIN32_THREADS",
];

#[cfg(test)]
mod tests {
    use super::PERL5_DOCUMENTED_CARET_NAMES;
    use std::collections::HashSet;

    #[test]
    fn documented_caret_names_are_non_empty_and_unique() {
        let mut seen = HashSet::new();
        for name in PERL5_DOCUMENTED_CARET_NAMES {
            assert!(!name.is_empty(), "caret name must not be empty");
            assert!(
                seen.insert(*name),
                "duplicate entry in PERL5_DOCUMENTED_CARET_NAMES: {name}"
            );
        }
    }

    #[test]
    fn documented_caret_names_are_sorted_lexicographically() {
        let mut sorted: Vec<&str> = PERL5_DOCUMENTED_CARET_NAMES.to_vec();
        sorted.sort();
        let as_slice: Vec<&str> = PERL5_DOCUMENTED_CARET_NAMES.to_vec();
        assert_eq!(
            as_slice, sorted,
            "PERL5_DOCUMENTED_CARET_NAMES should remain sorted for stable diffs and binary search"
        );
    }
}
