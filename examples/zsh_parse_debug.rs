use std::fs;
use std::env;
use stryke::zsh_parse::ZshParser;

fn main() {
    let args: Vec<String> = env::args().collect();
    let content = if args.len() > 1 {
        fs::read_to_string(&args[1]).expect("read file")
    } else {
        "echo hello".to_string()
    };
    
    eprintln!("Parsing {} bytes...", content.len());
    
    let mut parser = ZshParser::new(&content);
    match parser.parse() {
        Ok(prog) => eprintln!("OK: {} lists", prog.lists.len()),
        Err(errors) => {
            for e in errors {
                eprintln!("Error line {}: {}", e.line, e.message);
            }
        }
    }
}
