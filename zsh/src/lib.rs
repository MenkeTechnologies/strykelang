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
//! - Mathematical expression evaluation (`math` module)

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(unused_assignments)]
#![allow(unreachable_patterns)]
#![allow(deprecated)]
#![allow(unexpected_cfgs)]

pub mod attr;
pub mod cap;
pub mod clone;
pub mod tokens;
pub mod lexer;
pub mod parser;
pub mod exec;
pub mod completion;
pub mod fds;
pub mod history;
pub mod jobs;
pub mod cond;
pub mod glob;
pub mod math;
pub mod options;
pub mod pattern;
pub mod prompt;pub mod text;
pub mod compat;
pub mod input;
pub mod signals;
pub mod sort;
pub mod stringsort;
pub mod linklist;
pub mod context;
pub mod curses;
pub mod hashnameddir;
pub mod mem;
pub mod init;
pub mod hist;
pub mod hlgroup;
pub mod ksh93;
pub mod langinfo;
pub mod mapfile;
pub mod subst;
pub mod subst_port;
pub mod subscript;
pub mod params;
pub mod pcre;
pub mod utils;
pub mod hashtable;
pub mod sched;
pub mod rlimits;
pub mod socket;
pub mod datetime;
pub mod files;
pub mod mathfunc;
pub mod nearcolor;
pub mod newuser;
pub mod param_private;
pub mod parameter;
pub mod random;
pub mod random_real;
pub mod regex_mod;
pub mod stat;
pub mod system;
pub mod tcp;
pub mod termcap;
pub mod terminfo;
pub mod watch;
pub mod zftp;
pub mod zle;
pub mod zprof;
pub mod zpty;
pub mod zselect;
pub mod zutil;
pub mod zwc;
pub mod fish_features;
pub mod db_gdbm;

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
