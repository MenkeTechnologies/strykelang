use crate::common::*;

#[test]
fn regex_match() {
    assert_eq!(eval_int(r#"my $s = "hello123"; $s =~ /(\d+)/; $1"#), 123);
}

#[test]
fn regex_substitution() {
    assert_eq!(
        eval_string(r#"my $s = "foo bar"; $s =~ s/bar/baz/; $s"#),
        "foo baz"
    );
}

#[test]
fn regex_substitution_global() {
    assert_eq!(
        eval_string(r#"my $s = "a a a"; $s =~ s/a/b/g; $s"#),
        "b b b"
    );
}

#[test]
fn transliterate_tr() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; $s =~ tr/abc/ABC/; $s"#),
        "ABC"
    );
}
