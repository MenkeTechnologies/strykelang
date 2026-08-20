//! `stryke` — a highly parallel Perl 5 interpreter written in Rust.
//!
//! The binary is only an entry point. Everything it does lives in the library,
//! in [`stryke::cli`], so a host that links strykelang in can run the same
//! command line without a process — see [`stryke::cli::run_argv`], which the
//! `stryke` shell builtin in zshrs-native dispatches to.

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    std::process::ExitCode::from(stryke::cli::run_argv(&argv) as u8)
}
