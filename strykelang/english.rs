//! `English.pm`-style scalar aliases (`use English`).
//!
//! Stock `English` maps long names to the same globals as short punctuation variables.
//! All aliases from Perl 5's core `English.pm` are included.
//!
//! Supports `use English qw(-no_match_vars)` to suppress the `$MATCH`, `$PREMATCH`,
//! and `$POSTMATCH` aliases (the recommended usage in Perl for performance reasons).
//!
//! Wired through [`Interpreter::english_scalar_name`](crate::interpreter::Interpreter::english_scalar_name)
//! when `use English` sets [`Interpreter::english_enabled`](crate::interpreter::Interpreter::english_enabled).

use std::collections::HashMap;
use std::sync::LazyLock;

/// All English long-name → short-name mappings (excluding the match triple).
static ENGLISH_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // $_
        ("ARG", "_"),
        // $.
        ("INPUT_LINE_NUMBER", "."),
        ("NR", "."),
        // $/
        ("INPUT_RECORD_SEPARATOR", "/"),
        ("RS", "/"),
        // $,
        ("OFS", ","),
        ("OUTPUT_FIELD_SEPARATOR", ","),
        // $\
        ("ORS", "\\"),
        ("OUTPUT_RECORD_SEPARATOR", "\\"),
        // $"
        ("LIST_SEPARATOR", "\""),
        // $;
        ("SUBSCRIPT_SEPARATOR", ";"),
        ("SUBSEP", ";"),
        // $|
        ("OUTPUT_AUTOFLUSH", "|"),
        // $!
        ("OS_ERROR", "!"),
        ("ERRNO", "!"),
        // $@
        ("EVAL_ERROR", "@"),
        // $?
        ("CHILD_ERROR", "?"),
        // $$
        ("PROCESS_ID", "$$"),
        ("PID", "$$"),
        // $0
        ("PROGRAM_NAME", "0"),
        // $+
        ("LAST_PAREN_MATCH", "+"),
        // $^N
        ("LAST_SUBMATCH_RESULT", "^N"),
        // $<
        ("REAL_USER_ID", "<"),
        ("UID", "<"),
        // $>
        ("EFFECTIVE_USER_ID", ">"),
        ("EUID", ">"),
        // $(
        ("REAL_GROUP_ID", "("),
        ("GID", "("),
        // $)
        ("EFFECTIVE_GROUP_ID", ")"),
        ("EGID", ")"),
        // $%
        ("FORMAT_PAGE_NUMBER", "%"),
        // $=
        ("FORMAT_LINES_PER_PAGE", "="),
        // $-
        ("FORMAT_LINES_LEFT", "-"),
        // $~  (format name — stored in scope as "~")
        ("FORMAT_NAME", "~"),
        // $^  (format top name)
        ("FORMAT_TOP_NAME", "^"),
        // $:
        ("FORMAT_LINE_BREAK_CHARACTERS", ":"),
        // $^L
        ("FORMAT_FORMFEED", "^L"),
        // $^A
        ("ACCUMULATOR", "^A"),
        // $^C
        ("COMPILING", "^C"),
        // $^D
        ("DEBUGGING", "^D"),
        // $^E
        ("EXTENDED_OS_ERROR", "^E"),
        // $^F
        ("SYSTEM_FD_MAX", "^F"),
        // $^I
        ("INPLACE_EDIT", "^I"),
        // $^O
        ("OSNAME", "^O"),
        // $^P
        ("PERLDB", "^P"),
        // $^R
        ("LAST_REGEXP_CODE_RESULT", "^R"),
        // $^S
        ("EXCEPTIONS_BEING_CAUGHT", "^S"),
        // $^T
        ("BASETIME", "^T"),
        // $^V
        ("PERL_VERSION", "^V"),
        // $^W
        ("WARNING", "^W"),
        // $^X
        ("EXECUTABLE_NAME", "^X"),
        // $* (deprecated, but Perl's English.pm still maps it)
        ("MULTILINE_MATCHING", "*"),
    ])
});

/// Match-related aliases: `$MATCH`, `$PREMATCH`, `$POSTMATCH`.
/// Suppressed when `use English qw(-no_match_vars)`.
static ENGLISH_MATCH_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // $&
        ("MATCH", "&"),
        // $`
        ("PREMATCH", "`"),
        // $'
        ("POSTMATCH", "'"),
    ])
});

/// If `name` is a known `English` long name, return the short special name (`_`, `.`, …).
/// Match aliases (`MATCH`, `PREMATCH`, `POSTMATCH`) are only returned when
/// `no_match_vars` is false.
#[inline]
pub(crate) fn scalar_alias(name: &str, no_match_vars: bool) -> Option<&'static str> {
    if let Some(short) = ENGLISH_ALIASES.get(name).copied() {
        return Some(short);
    }
    if !no_match_vars {
        return ENGLISH_MATCH_ALIASES.get(name).copied();
    }
    None
}

/// Returns `true` if `name` is any known English alias (including match aliases).
/// Used by the compiler to emit `GetScalar`/`SetScalar` instead of the `Plain` variants
/// so that English translation is applied at runtime.
#[inline]
pub(crate) fn is_known_alias(name: &str) -> bool {
    ENGLISH_ALIASES.contains_key(name) || ENGLISH_MATCH_ALIASES.contains_key(name)
}

#[cfg(test)]
mod tests {
    use super::scalar_alias;

    #[test]
    fn alias_arg_maps_to_default_scalar() {
        assert_eq!(scalar_alias("ARG", false), Some("_"));
    }

    #[test]
    fn alias_input_line_number_and_nr_map_to_dot() {
        assert_eq!(scalar_alias("INPUT_LINE_NUMBER", false), Some("."));
        assert_eq!(scalar_alias("NR", false), Some("."));
    }

    #[test]
    fn alias_rs_and_input_record_separator_map_to_slash() {
        assert_eq!(scalar_alias("RS", false), Some("/"));
        assert_eq!(scalar_alias("INPUT_RECORD_SEPARATOR", false), Some("/"));
    }

    #[test]
    fn alias_process_id_and_pid_map_to_double_dollar() {
        assert_eq!(scalar_alias("PROCESS_ID", false), Some("$$"));
        assert_eq!(scalar_alias("PID", false), Some("$$"));
    }

    #[test]
    fn alias_program_name_maps_to_zero() {
        assert_eq!(scalar_alias("PROGRAM_NAME", false), Some("0"));
    }

    #[test]
    fn unknown_long_name_returns_none() {
        assert_eq!(scalar_alias("NOT_A_REAL_ENGLISH_NAME", false), None);
        assert_eq!(scalar_alias("", false), None);
    }

    #[test]
    fn alias_eval_error_and_errno_map() {
        assert_eq!(scalar_alias("EVAL_ERROR", false), Some("@"));
        assert_eq!(scalar_alias("OS_ERROR", false), Some("!"));
        assert_eq!(scalar_alias("ERRNO", false), Some("!"));
    }

    #[test]
    fn alias_match_prematch_postmatch() {
        assert_eq!(scalar_alias("MATCH", false), Some("&"));
        assert_eq!(scalar_alias("PREMATCH", false), Some("`"));
        assert_eq!(scalar_alias("POSTMATCH", false), Some("'"));
        assert_eq!(scalar_alias("LAST_PAREN_MATCH", false), Some("+"));
    }

    #[test]
    fn alias_separators_and_list_separator() {
        assert_eq!(scalar_alias("OFS", false), Some(","));
        assert_eq!(scalar_alias("ORS", false), Some("\\"));
        assert_eq!(scalar_alias("LIST_SEPARATOR", false), Some("\""));
        assert_eq!(scalar_alias("SUBSEP", false), Some(";"));
    }

    #[test]
    fn alias_osname_and_warnings() {
        assert_eq!(scalar_alias("OSNAME", false), Some("^O"));
        assert_eq!(scalar_alias("WARNING", false), Some("^W"));
        assert_eq!(scalar_alias("COMPILING", false), Some("^C"));
    }

    // ----- new aliases -----

    #[test]
    fn alias_uid_gid() {
        assert_eq!(scalar_alias("REAL_USER_ID", false), Some("<"));
        assert_eq!(scalar_alias("UID", false), Some("<"));
        assert_eq!(scalar_alias("EFFECTIVE_USER_ID", false), Some(">"));
        assert_eq!(scalar_alias("EUID", false), Some(">"));
        assert_eq!(scalar_alias("REAL_GROUP_ID", false), Some("("));
        assert_eq!(scalar_alias("GID", false), Some("("));
        assert_eq!(scalar_alias("EFFECTIVE_GROUP_ID", false), Some(")"));
        assert_eq!(scalar_alias("EGID", false), Some(")"));
    }

    #[test]
    fn alias_format_vars() {
        assert_eq!(scalar_alias("FORMAT_PAGE_NUMBER", false), Some("%"));
        assert_eq!(scalar_alias("FORMAT_LINES_PER_PAGE", false), Some("="));
        assert_eq!(scalar_alias("FORMAT_LINES_LEFT", false), Some("-"));
        assert_eq!(scalar_alias("FORMAT_NAME", false), Some("~"));
        assert_eq!(scalar_alias("FORMAT_TOP_NAME", false), Some("^"));
        assert_eq!(
            scalar_alias("FORMAT_LINE_BREAK_CHARACTERS", false),
            Some(":")
        );
        assert_eq!(scalar_alias("FORMAT_FORMFEED", false), Some("^L"));
    }

    #[test]
    fn alias_caret_vars() {
        assert_eq!(scalar_alias("ACCUMULATOR", false), Some("^A"));
        assert_eq!(scalar_alias("DEBUGGING", false), Some("^D"));
        assert_eq!(scalar_alias("SYSTEM_FD_MAX", false), Some("^F"));
        assert_eq!(scalar_alias("INPLACE_EDIT", false), Some("^I"));
        assert_eq!(scalar_alias("PERLDB", false), Some("^P"));
        assert_eq!(scalar_alias("LAST_REGEXP_CODE_RESULT", false), Some("^R"));
        assert_eq!(scalar_alias("EXCEPTIONS_BEING_CAUGHT", false), Some("^S"));
        assert_eq!(scalar_alias("EXECUTABLE_NAME", false), Some("^X"));
        assert_eq!(scalar_alias("LAST_SUBMATCH_RESULT", false), Some("^N"));
    }

    // ----- -no_match_vars -----

    #[test]
    fn no_match_vars_suppresses_match_aliases() {
        assert_eq!(scalar_alias("MATCH", true), None);
        assert_eq!(scalar_alias("PREMATCH", true), None);
        assert_eq!(scalar_alias("POSTMATCH", true), None);
    }

    #[test]
    fn no_match_vars_keeps_non_match_aliases() {
        assert_eq!(scalar_alias("ARG", true), Some("_"));
        assert_eq!(scalar_alias("ERRNO", true), Some("!"));
        assert_eq!(scalar_alias("PID", true), Some("$$"));
        assert_eq!(scalar_alias("LAST_PAREN_MATCH", true), Some("+"));
    }
}
