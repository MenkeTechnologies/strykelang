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
//! - Fish-style features (`fish_features` module)

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
pub mod fish_features;

pub use tokens::{char_tokens, LexTok};
pub use lexer::ZshLexer;
pub use parser::ZshParser;
pub use exec::ShellExecutor;
pub use fish_features::{
    // Syntax highlighting
    highlight_shell, colorize_line, HighlightRole, HighlightSpec,
    // Abbreviations
    Abbreviation, AbbrPosition, AbbreviationSet, with_abbrs, with_abbrs_mut, expand_abbreviation,
    // Autosuggestions
    Autosuggestion, autosuggest_from_history, validate_autosuggestion,
    // Killring
    kill_add, kill_replace, kill_yank, kill_yank_rotate, KillRing,
    // Validation
    validate_command, ValidationStatus,
    // Private mode
    is_private_mode, set_private_mode,
};
