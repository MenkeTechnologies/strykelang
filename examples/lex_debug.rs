use stryke::zsh_lex::ZshLexer;
use stryke::zsh_tokens::LexTok;

fn main() {
    let input = "args=(
  'foo'
)";
    let mut lexer = ZshLexer::new(input);
    
    for _ in 0..20 {
        lexer.zshlex();
        println!("{:?} {:?}", lexer.tok, lexer.tokstr);
        if lexer.tok == LexTok::Endinput || lexer.tok == LexTok::Lexerr {
            break;
        }
    }
}
