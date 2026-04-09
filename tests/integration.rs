//! Integration test harness for `perlrs`: `tests/suite/` holds grouped cases (phases, control,
//! regex, eval/`$@`, closures, aggregates, parallelism, filesystem builtins, etc.);
//! `tests/common/` provides `eval*` helpers. Library unit tests cover `parse()`, lexer tokens,
//! `Scope` (including hashes), `keyword_or_ident`, `PerlError` display, and `PerlValue`. Run with
//! `cargo test`.

mod common;
mod suite;
