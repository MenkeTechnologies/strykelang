//! Integration test harness for `perlrs`: `tests/suite/` holds grouped cases (phases, control,
//! regex, eval/`$@`, closures, aggregates, parallelism, etc.); `tests/common/` provides `eval*`
//! helpers. Run with `cargo test --test integration` or `cargo test`.

mod common;
mod suite;
