//! `s///` pattern and replacement: `$ENV{KEY}` expands like Perl (required for zpwr-style pipelines).

use crate::common::eval_string;

#[test]
fn subst_pattern_env_home_collapses_to_tilde_prefix() {
    let home = std::env::var("HOME").expect("HOME");
    let code = format!(
        r#"
        $_ = "{home}/foo/bar";
        $_ =~ s@$ENV{{HOME}}@~@;
        $_;
    "#
    );
    assert_eq!(eval_string(&code), "~/foo/bar");
}

#[test]
fn subst_replacement_env_plus_capture_group() {
    let home = std::env::var("HOME").expect("HOME");
    let code = format!(
        r#"
        $_ = "~/baz";
        s@^([~])([^~]*)$@$ENV{{HOME}}$2@;
        $_;
    "#
    );
    // Replacement must not use ASCII `"..."` around the RHS — lexer keeps quotes as literals.
    assert_eq!(eval_string(&code), format!("{home}/baz"));
}
