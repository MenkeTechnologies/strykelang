//! Integration test harness for `perlrs`: `tests/suite/` holds grouped cases (phases, control,
//! regex, eval/`$@`, closures, aggregates, parallelism, filesystem builtins, `lib_api` for
//! `run` / `parse_and_run_string`, etc.); `tests/common/` provides `eval*` helpers. Parser-focused
//! suites (`parse_accepts`, `parse_accepts_extra`, `parse_accepts_parallel`,
//! `parse_accepts_strings_ops`, `parse_syntax_errors`,
//! `parse_syntax_errors_more`, `parse_program_shape`, `parse_program_shape_extra`,
//! `parse_program_shape_ops`, `error_kinds_extra`) add explicit `#[test]` cases—no macro
//! batching—for `perlrs::parse` success, syntax errors, `Program` / `StmtKind` / `BinOp` shape, and
//! `ErrorKind` from `eval_err_kind` / `parse_err_kind`. `runtime_extra` and `runtime_more` add broad
//! interpreter coverage (assignment forms, builtin return values, aggregates, strings, control
//! flow, regex, closures). `pack_unpack_runtime` exercises `pack`/`unpack` builtins; `semantic_edge`
//! adds focused checks for operators and list builtins. Library unit tests cover `parse()`, `run`, `parse_and_run_string`,
//! `try_vm_execute`, `crate_api_tests` / `run_semantics_tests` / `run_semantics_more` (crate-root `run`), `parser_shape_tests` (`StmtKind`/`ExprKind` from `parse`), `interpreter_unit_tests`
//! (`Interpreter` defaults and `execute_tree`), `pchannel` (`send`/`recv`/`pselect` helpers), `parallel_trace` (fan worker / trace-noop paths), lexer (`&`, `&&`/`||`/`+=`, `==`/`!=`,
//! `**`/`..`, `q{}`, `qr//`, octal/binary, `-e` file tests, floats, `m//`, strings, `<=>`), `Scope`
//! (arrays, hashes, atomics, `pop_frame` guard), `keyword_or_ident`, `PerlError` (including
//! `DivisionByZero` display), `bytecode`/`Chunk`, `compiler` smoke, and `PerlValue` (`type_name`,
//! `ref_type`, `Display`, empty-array
//! truthiness). Integration covers `our`/`local`, subs with prototypes, builtins like `require`,
//! regex (`=~` / `!~`), `eval { }`, and `split` with pattern delimiters. Run with `cargo test`.

mod common;
mod suite;
