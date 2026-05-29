//! `stryke_web` — Rails-shaped web framework for the stryke language.
//!
//! The architecture mirrors how Rails works for Ruby:
//!
//! - **Generator side** (this crate, written in Rust): the `s_web` binary
//!   emits stryke source files (`.stk`) to lay out a new app, scaffold a
//!   controller, scaffold a model + migration, etc. Same role as
//!   `rails new` / `rails generate` — except we write `.stk` instead of
//!   `.rb` because the runtime is stryke.
//!
//! - **Runtime side** (live in `strykelang/` as builtins): primitives like
//!   `serve`, `route`, `render`, `redirect`, `Controller` base class,
//!   `Model` base class, the ERB-equivalent template engine, the SQLite
//!   wrapper, etc. The user's generated `.stk` app code calls these
//!   builtins by name — no library imports needed.
//!
//! Why Rust for the generator: same reason Rails ships `rails` as a Ruby
//! script that writes Ruby — the host language matches the runtime.
//! Stryke's runtime is Rust; the CLI that lays out a stryke app is also
//! Rust. The output is `.stk` files because that's what stryke executes.
//!
//! Templates are embedded into the binary at compile time via
//! `include_str!` so the binary is self-contained and the user doesn't
//! need a separate template tree on disk.

pub mod cmd_app;
/// `cmd_build` submodule.
pub mod cmd_build;
/// `cmd_new` submodule.
pub mod cmd_new;
/// `cmd_generate` submodule.
pub mod cmd_generate;
/// `cmd_extras` submodule.
pub mod cmd_extras;
/// `cmd_routes` submodule.
pub mod cmd_routes;
/// `cmd_server` submodule.
pub mod cmd_server;
/// `cmd_db` submodule.
pub mod cmd_db;
/// `presets` submodule.
pub mod presets;
/// `templates` submodule.
pub mod templates;
/// `util` submodule.
pub mod util;
