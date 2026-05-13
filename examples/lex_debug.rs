use std::sync::atomic::Ordering;

use stryke::zsh_errflag;
use stryke::zsh_lex::{lex_init, tok, tokstr, zshlex, ENDINPUT, LEXERR};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = if args.len() > 1 {
        args[1].clone()
    } else {
        "args=( 'foo' )".to_string()
    };

    zsh_errflag.store(0, Ordering::Relaxed);
    lex_init(&input);

    for _ in 0..50 {
        zshlex();
        let t = tok();
        println!("{:?} {:?}", t, tokstr());
        if t == ENDINPUT || t == LEXERR {
            break;
        }
    }
}
