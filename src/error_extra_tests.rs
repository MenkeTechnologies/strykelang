//! Extra tests for error handling and die with values.

use crate::run;

#[test]
fn test_die_string() {
    let code = r#"
        eval { die "custom error\n" };
        $@;
    "#;
    assert_eq!(run(code).expect("run").to_string(), "custom error\n");
}

#[test]
fn test_die_reference() {
    let code = r#"
        eval { die { msg => "nested", code => 500 } };
        ref($@) . ":" . $@->{msg} . ":" . $@->{code};
    "#;
    assert_eq!(run(code).expect("run").to_string(), "HASH:nested:500");
}

#[test]
fn test_die_array_reference() {
    let code = r#"
        eval { die [1, 2, 3] };
        ref($@) . ":" . join(",", @$@);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ARRAY:1,2,3");
}

#[test]
fn test_try_catch_basic() {
    // Stryke supports try/catch extension
    let code = r#"
        my $res = "none";
        try {
            die "oops";
        } catch ($e) {
            $res = "caught:$e";
        }
        $res;
    "#;
    // Perl runtime errors often append " at -e line ..."
    assert!(run(code).expect("run").to_string().contains("caught:oops"));
}

#[test]
fn test_division_by_zero_error() {
    let code = r#"
        eval { 1 / 0 };
        $@ =~ /division by zero/i ? "ok" : "fail:$@";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}
