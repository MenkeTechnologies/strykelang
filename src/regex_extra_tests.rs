//! Extra tests for advanced regex features and match variables.

use crate::run;

#[test]
fn test_regex_lookarounds() {
    // fancy_regex / pcre2 should handle lookarounds
    let code = r#"
        "foobar" =~ /foo(?=bar)/ ? "ok" : "fail"
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");

    let code2 = r#"
        "foobar" =~ /foo(?!baz)/ ? "ok" : "fail";
    "#;
    assert_eq!(run(code2).expect("run").to_string(), "ok");
}

#[test]
fn test_regex_non_capturing_groups() {
    let code = r#"
        "abc" =~ /(?:a)b(c)/
        $1
    "#;
    assert_eq!(run(code).expect("run").to_string(), "c");
}

#[test]
fn test_regex_p_flag_match_vars() {
    // Stryke supports /p for ${^MATCH}, ${^PREMATCH}, ${^POSTMATCH}
    let code = r#"
        "abc123def" =~ /(\d+)/p
        "${^PREMATCH}:${^MATCH}:${^POSTMATCH}"
    "#;
    assert_eq!(run(code).expect("run").to_string(), "abc:123:def");
}

#[test]
fn test_regex_named_captures_in_perl() {
    let code = r#"
        "2026-04-20" =~ /(?<year>\d+)-(?<month>\d+)-(?<day>\d+)/
        "$+{year}:$+{month}:$+{day}"
    "#;
    assert_eq!(run(code).expect("run").to_string(), "2026:04:20");
}

#[test]
fn test_regex_multiline() {
    let code = r#"
        "abc\ndef" =~ /^def/m ? "ok" : "fail"
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}

#[test]
fn test_regex_substitution_g() {
    let code = r#"
        my $s = "a b c"
        $s =~ s/\s/-/g
        $s
    "#;
    assert_eq!(run(code).expect("run").to_string(), "a-b-c");
}

#[test]
fn test_regex_substitution_eval_first() {
    // s///e support - testing single substitution for now
    let code = r#"
        my $s = "10"
        $s =~ s/(\d+)/$1 * 2/e
        $s
    "#;
    assert_eq!(run(code).expect("run").to_string(), "20");
}

#[test]
fn test_regex_backreferences() {
    let code = r#"
        "abcabc" =~ /(abc)\1/ ? "ok" : "fail"
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}
