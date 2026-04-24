use stryke::zsh_lex::ZshLexer;
use stryke::zsh_tokens::LexTok;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = if args.len() > 1 {
        args[1].clone()
    } else {
        "args=( 'foo' )".to_string()
    };

    let mut lexer = ZshLexer::new(&input);

    for _ in 0..50 {
        lexer.zshlex();
        println!("{:?} {:?}", lexer.tok, lexer.tokstr);
        if lexer.tok == LexTok::Endinput || lexer.tok == LexTok::Lexerr {
            break;
        }
    }
}
