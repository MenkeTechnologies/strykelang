//! Parser rejects invalid Perl (explicit `#[test]` per case; no macro batching).

use crate::common::parse_err_kind;
use stryke::error::ErrorKind;

#[test]
fn rejects_unclosed_sub_brace() {
    assert_eq!(parse_err_kind("fn f {"), ErrorKind::Syntax);
}

#[test]
fn rejects_lone_close_brace() {
    assert_eq!(parse_err_kind("}"), ErrorKind::Syntax);
}

#[test]
fn rejects_unterminated_double_quote() {
    assert_eq!(parse_err_kind(r#"my $x = "open"#), ErrorKind::Syntax);
}

#[test]
fn rejects_unexpected_eof_after_operator() {
    assert_eq!(parse_err_kind("++"), ErrorKind::Syntax);
}

#[test]
fn rejects_invalid_token_triple_dollar() {
    assert!(parse_err_kind("$$$").eq(&ErrorKind::Syntax));
}

#[test]
fn statement_anonymous_sub_block_parses() {
    // Perl: `fn { 1 }` is a valid statement (void-context coderef).
    assert!(stryke::parse("fn { 1 }").is_ok());
}

#[test]
fn rejects_unclosed_paren_expression() {
    assert!(stryke::parse("(1 + 2").is_err());
}

#[test]
fn rejects_unclosed_bracket_array() {
    assert!(stryke::parse("$a[1").is_err());
}

#[test]
fn rejects_unclosed_brace_hash() {
    assert!(stryke::parse("$h{1").is_err());
}

#[test]
fn rejects_unclosed_regex_delimiter() {
    assert!(stryke::parse("m/foo").is_err());
}

#[test]
fn rejects_incomplete_addition_at_eof() {
    assert!(stryke::parse("1 +").is_err());
}

#[test]
fn double_semicolon_only_parses() {
    stryke::parse(";").expect("parse");
}

#[test]
fn rejects_incomplete_mul_at_eof() {
    assert!(stryke::parse("3 *").is_err());
}

#[test]
fn rejects_unclosed_single_quote() {
    assert!(stryke::parse("'unterminated").is_err());
}

#[test]
fn rejects_sub_call_missing_paren_if_expected() {
    assert!(stryke::parse("my $x = (").is_err());
}

#[test]
fn comment_only_line_parses_as_empty_program() {
    let p = stryke::parse("# only comment\n").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn rejects_interpolated_eof_in_double_quote() {
    assert!(parse_err_kind(r#""${"#) == ErrorKind::Syntax || stryke::parse(r#""${"#).is_err());
}

#[test]
fn rejects_package_name_invalid() {
    assert!(stryke::parse("package 123").is_err());
}

#[test]
fn triple_semicolon_parses() {
    stryke::parse(";;").expect("semicolons only");
}

#[test]
fn rejects_eof_after_my() {
    assert!(stryke::parse("my").is_err());
}

// `var` / `val` are *contextual* keywords (Kotlin/Scala-style aliases for
// `my` / `const my`). When followed by a sigil they dispatch as declarators
// exactly like `my`; in every other position they parse as plain barewords.
// So bare `parse("var")` / `parse("val")` succeed (identifier expression),
// unlike bare `parse("my")` which always errors. The parallel error case is
// `var $` / `val $` — sigil-but-no-name, identical EOF-mid-decl reject path.
#[test]
fn rejects_eof_after_var_sigil() {
    assert!(stryke::parse("var $").is_err());
}

#[test]
fn rejects_eof_after_val_sigil() {
    assert!(stryke::parse("val $").is_err());
}

#[test]
fn rejects_eof_after_comma_in_list() {
    assert!(stryke::parse("(1,").is_err());
}

#[test]
fn rejects_unclosed_q_brace() {
    assert!(stryke::parse(r#"q{no close"#).is_err());
}

#[test]
fn rejects_s_substitute_unclosed() {
    assert!(stryke::parse("s/a/").is_err());
}

#[test]
fn rejects_tr_unclosed() {
    assert!(stryke::parse("tr/a/").is_err());
}

#[test]
fn rejects_foreach_without_paren_or_keyword() {
    assert!(stryke::parse("foreach").is_err());
}

#[test]
fn rejects_if_without_paren_on_some_engines() {
    assert!(stryke::parse("if").is_err());
}

#[test]
fn rejects_do_string_unclosed_quote() {
    assert!(stryke::parse(r#"do "file"#).is_err());
}

#[test]
fn use_strict_without_trailing_semicolon_still_parses() {
    stryke::parse("use strict").expect("parse");
}

#[test]
fn import_keyword_aliases_use() {
    // `import` is a 1:1 syntactic alias for `use` — same parser path,
    // same AST. All four shapes must parse identically.
    stryke::parse("import strict").expect("import bareword");
    stryke::parse("import strict;").expect("import with semi");
    stryke::parse("import warnings; import strict;").expect("two imports");
    stryke::parse("import List::Util qw(sum max);").expect("import qw(...)");
    stryke::parse("import 5.36;").expect("import VERSION");
    // Mixing `use` and `import` in one program is legal.
    stryke::parse("use strict; import warnings;").expect("mixed use+import");
    // Bare `import` with no module is rejected, same as bare `use`.
    let err = stryke::parse("import").expect_err("bare import must error");
    assert!(
        err.to_string().contains("import"),
        "error should reference the spelling the user typed (got: {})",
        err
    );
}

#[test]
fn rejects_open_missing_comma() {
    assert!(stryke::parse("open F").is_err());
}

#[test]
fn rejects_backslash_eof() {
    assert!(stryke::parse("\\").is_err());
}

#[test]
fn rejects_incomplete_qq_constructor() {
    assert!(stryke::parse("qq(").is_err());
}
