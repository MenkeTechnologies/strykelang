//! Integration test harness for `perlrs`: `tests/suite/` holds grouped cases (phases, control,
//! regex, eval/`$@`, closures, aggregates, parallelism, filesystem builtins, `lib_api` for
//! `run` / `parse_and_run_string`, etc.); `tests/common/` provides `eval*` helpers. Library unit
//! tests cover `parse()`, `run`, lexer (`q{}`, `qr//`, octal/binary, `-e` file tests, floats,
//! `m//`, strings, `<=>`), `Scope` (arrays, hashes, `pop_frame` guard), `keyword_or_ident`,
//! `PerlError` (`Syntax`/`Die`/`Exit`/`Runtime` display), and `PerlValue` (`type_name`, `ref_type`,
//! `Display`). Integration covers `our`/`local` and subs with prototypes. Run with `cargo test`.

mod common;
mod suite;
