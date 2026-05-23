//! Extra tests for `Lexer` to ensure correct tokenization of edge cases.

use crate::lexer::Lexer;
use crate::token::Token;

#[test]
fn test_range_operators() {
    let mut l = Lexer::new("1..5");
    let t = l.tokenize().expect("tokenize");
    assert_eq!(t[1].0, Token::Range);

    let mut l = Lexer::new("1...5");
    let t = l.tokenize().expect("tokenize");
    assert_eq!(t[1].0, Token::RangeExclusive);
}

#[test]
fn test_backtick_string() {
    let mut l = Lexer::new("`ls -l` ");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::BacktickString(ref s) if s == "ls -l"));

    let mut l = Lexer::new("qx/echo hello/");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::BacktickString(ref s) if s == "echo hello"));
}

#[test]
fn test_readline_complex() {
    let mut l = Lexer::new("<STDIN>");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::ReadLine(ref s) if s == "STDIN"));

    let mut l = Lexer::new("<$fh>");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::ReadLine(ref s) if s == "fh"));
}

#[test]
fn test_symbolic_deref() {
    let mut l = Lexer::new("$$foo");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::DerefScalarVar(ref s) if s == "foo"));
}

#[test]
fn test_heredoc_edge_cases() {
    // Indented heredoc (Perl 5.26+)
    let src = "print <<~EOF\n  line1\n  line2\nEOF\n";
    let mut l = Lexer::new(src);
    let t = l.tokenize().expect("tokenize");
    if let Token::HereDoc(tag, body, _) = &t[1].0 {
        assert_eq!(tag, "EOF");
        assert!(body.contains("line1"));
    } else {
        panic!("expected HereDoc, got {:?}", t[1].0);
    }
}

#[test]
fn heredoc_after_closing_brace_is_heredoc_not_shift_left() {
    // Regression: `}` sets last_was_term=true, then `<<TAG` on the
    // next line used to tokenize as ShiftLeft + bareword TAG. With
    // the peek-ahead disambiguation, uppercase-letter / `~` / `"` /
    // `'` / `_` after `<<` flips to heredoc regardless.
    let src = "my @r = ~> @data map { _ * 2 }\n<<EOT\nhi\nEOT\n";
    let mut l = Lexer::new(src);
    let t = l.tokenize().expect("tokenize");
    let has_heredoc = t.iter().any(|(tok, _)| {
        matches!(tok, Token::HereDoc(tag, body, _) if tag == "EOT" && body.contains("hi"))
    });
    assert!(
        has_heredoc,
        "expected HereDoc tag=EOT after block-close, got tokens: {:?}",
        t.iter()
            .map(|(tok, _)| format!("{:?}", tok))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn shift_left_preserved_after_block_close_with_numeric_rhs() {
    // Symmetric guard for the heredoc fix: `block << 4` (numeric RHS)
    // must still tokenize as `ShiftLeft`, not heredoc. Same for sigils.
    for src in ["my $x = (1 + 2) << 4", "my $r = $h->{a} << $shift"] {
        let mut l = Lexer::new(src);
        let t = l.tokenize().expect("tokenize");
        let has_shift = t.iter().any(|(tok, _)| matches!(tok, Token::ShiftLeft));
        let no_heredoc = !t
            .iter()
            .any(|(tok, _)| matches!(tok, Token::HereDoc(_, _, _)));
        assert!(
            has_shift && no_heredoc,
            "{src} — expected ShiftLeft (no heredoc), got: {:?}",
            t.iter()
                .map(|(tok, _)| format!("{:?}", tok))
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn test_complex_string_escapes() {
    // Octal and Hex
    let mut l = Lexer::new(r#""\012""#);
    let t = l.tokenize().expect("tokenize");
    if let Token::DoubleString(s) = &t[0].0 {
        assert_eq!(s, "\n");
    }

    let mut l = Lexer::new(r#""\x0A""#);
    let t = l.tokenize().expect("tokenize");
    if let Token::DoubleString(s) = &t[0].0 {
        assert_eq!(s, "\n");
    }

    // Control characters
    let mut l = Lexer::new(r#""\c[""#);
    let t = l.tokenize().expect("tokenize");
    if let Token::DoubleString(s) = &t[0].0 {
        assert_eq!(s, "\x1B");
    }
}

#[test]
fn test_operators_ambiguity() {
    // 3 . 4 (concat) vs 3.4 (float)
    let mut l = Lexer::new("3 . 4");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::Integer(3)));
    assert_eq!(t[1].0, Token::Dot);
    assert!(matches!(t[2].0, Token::Integer(4)));

    let mut l = Lexer::new("3.4");
    let t = l.tokenize().expect("tokenize");
    assert!(matches!(t[0].0, Token::Float(f) if (f - 3.4).abs() < 1e-9));
}
