use std::env;
use std::fs;
use std::sync::atomic::Ordering;

use stryke::zsh_errflag;
use stryke::zsh_parse::{parse, parse_init};
use stryke::ERRFLAG_ERROR;

fn main() {
    let args: Vec<String> = env::args().collect();
    let content = if args.len() > 1 {
        fs::read_to_string(&args[1]).expect("read file")
    } else {
        "echo hello".to_string()
    };

    eprintln!("Parsing {} bytes...", content.len());

    zsh_errflag.store(0, Ordering::Relaxed);
    parse_init(&content);
    let prog = parse();
    if zsh_errflag.load(Ordering::Relaxed) & ERRFLAG_ERROR != 0 {
        eprintln!("parse failed (errflag set)");
    } else {
        eprintln!("OK: {} lists", prog.lists.len());
    }
}
