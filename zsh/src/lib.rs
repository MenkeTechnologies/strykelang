//! Zsh interpreter and parser in Rust
//!
//! This crate provides:
//! - A complete zsh lexer (`lexer` module)
//! - A zsh parser (`parser` module)  
//! - Shell execution engine (`exec` module)
//! - Job control (`jobs` module)
//! - History management (`history` module)
//! - ZLE (Zsh Line Editor) support (`zle` module)
//! - ZWC (compiled zsh) support (`zwc` module)

pub mod tokens;
pub mod lexer;
pub mod parser;
pub mod shell_ast;
pub mod exec;
pub mod completion;
pub mod fds;
pub mod history;
pub mod jobs;
pub mod signal;
pub mod zle;
pub mod zwc;

pub use tokens::{char_tokens, LexTok};
pub use lexer::ZshLexer;
pub use parser::ZshParser;
pub use exec::ShellExecutor;
