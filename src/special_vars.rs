//! Perl 5 `${^NAME}` scalars: see [perlvar](https://perldoc.perl.org/perlvar).
//!
//! Names not implemented with dedicated [`crate::interpreter::Interpreter`] fields are stored in
//! `Interpreter::special_caret_scalars` (default `undef`). Reads use that map; unknown names still
//! return `undef` without a pre-inserted key.

/// Documented `${^NAME}` scalars from perl 5 `perlvar` (subset used to pre-seed `undef` entries for
/// code that iterates or tests `defined ${^NAME}` without assigning first).
pub static PERL5_DOCUMENTED_CARET_NAMES: &[&str] = &[
    "CAPTURE",
    "CAPTURE_ALL",
    "CHILD_ERROR_NATIVE",
    "ENCODING",
    "GLOBAL_PHASE",
    "HOOK",
    "LAST_SUBMATCH_RESULT",
    "MATCH",
    "MAX_NESTED_EVAL_BEGIN_BLOCKS",
    "OPEN",
    "POSTMATCH",
    "PREMATCH",
    "RE_COMPILE_RECURSION_LIMIT",
    "RE_DEBUG_FLAGS",
    "RE_TRIE_MAXBUF",
    "REGERROR",
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
];
