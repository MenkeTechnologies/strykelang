//! Integration test harness for `perlrs`: `tests/suite/` holds grouped cases (phases, control,
//! regex, eval/`$@`, closures, aggregates, parallelism, filesystem builtins, `lib_api` for
//! `run` / `parse_and_run_string`, etc.); `tests/common/` provides `eval*` helpers. Parser-focused
//! suites (`parse_accepts`, `parse_accepts_extra`, `parse_accepts_parallel`,
//! `parse_accepts_strings_ops`, `parse_syntax_errors`,
//! `parse_syntax_errors_more`, `parse_program_shape`, `parse_program_shape_extra`,
//! `parse_program_shape_ops`, `error_kinds_extra`) add explicit `#[test]` cases—no macro
//! batching—for `perlrs::parse` success, syntax errors, `Program` / `StmtKind` / `BinOp` shape, and
//! `ErrorKind` from `eval_err_kind` / `parse_err_kind`. Library unit tests cover `parse()`, `run`, lexer (`&`, `&&`/`||`/`+=`, `==`/`!=`,
//! `**`/`..`, `q{}`, `qr//`, octal/binary, `-e` file tests, floats, `m//`, strings, `<=>`), `Scope`
//! (arrays, hashes, `pop_frame` guard), `keyword_or_ident`, `PerlError` (including
//! `DivisionByZero` display), and `PerlValue` (`type_name`, `ref_type`, `Display`, empty-array
//! truthiness). Integration covers `our`/`local`, subs with prototypes, builtins like `require`,
//! regex (`=~` / `!~`), `eval { }`, and `split` with pattern delimiters. Run with `cargo test`.

mod common;
mod suite;
