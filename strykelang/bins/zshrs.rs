//! zshrs (fat binary) — shell + stryke runtime
//!
//! Identical to the thin zshrs but registers a stryke @ handler first.
//! All shell logic comes from zsh/bins/zshrs.rs (included as a module).
//!
//! Build:  cargo install --path .          (fat, includes stryke)
//! Build:  cargo install --path zsh        (thin, pure shell)

#[path = "../../zsh/bins/zshrs.rs"]
#[allow(dead_code, unused_imports, clippy::all, unused_variables, unreachable_code)]
mod shell;

fn main() {
    // Register stryke @ handler — process_line() checks try_stryke_dispatch()
    zsh::set_stryke_handler(|code| {
        if let Err(e) = stryke::run(code) {
            eprintln!("stryke error: {}", e);
            return 1;
        }
        0
    });

    // Run the real zshrs main
    shell::zshrs_main();
}
