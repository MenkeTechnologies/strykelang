//! `stryke` — a highly parallel Perl 5 interpreter written in Rust.
//!
//! The binary is only an entry point. Everything it does lives in the library,
//! in [`stryke::cli`], so a host that links strykelang in can run the same
//! command line without a process — see [`stryke::cli::run_argv`], which the
//! `stryke` shell builtin in zshrs-native dispatches to.
//!
//! This binary owns its process, so it takes [`stryke::cli::run_owned`]: the
//! hosted entry point carries an `exit` by unwinding, and this crate builds
//! release with `panic = "abort"`.

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    std::process::ExitCode::from(stryke::cli::run_owned(&argv) as u8)
}
