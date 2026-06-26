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
fn test_try_catch_after_for_loop() {
    // Regression: a `for` loop preceding `try/catch` emits a Nop that
    // `compact_nops` removes, shifting op indices. `TryPush`'s catch_ip /
    // after_ip / finally_ip were not remapped, so `catch_ip` landed one op
    // past `CatchReceive` — the catch var came back empty and the value
    // stack/scope for later code was corrupted.
    let code = r#"
        my $keep = "HANDLE";
        for my $i (1..2) { my $x = $i; }
        my $got = "none";
        try {
            die "boom";
        } catch ($e) {
            $got = "caught:$e";
        }
        "$got|$keep";
    "#;
    let out = run(code).expect("run").to_string();
    assert!(out.contains("caught:boom"), "catch var lost after for loop: {out}");
    assert!(out.ends_with("|HANDLE"), "outer var corrupted after for loop: {out}");
}

#[test]
fn test_try_catch_finally_after_for_loop() {
    // Same remap bug, exercising the finally_ip path.
    let code = r#"
        my @log;
        for my $i (1..3) { push @log, $i; }
        my $got = "none";
        try {
            die "kaboom";
        } catch ($e) {
            $got = "c:$e";
        } finally {
            push @log, "fin";
        }
        "$got|" . join(",", @log);
    "#;
    let out = run(code).expect("run").to_string();
    assert!(out.contains("c:kaboom"), "catch var lost: {out}");
    assert!(out.contains("fin"), "finally did not run: {out}");
    assert!(out.contains("1,2,3"), "loop body corrupted: {out}");
}

#[test]
fn test_division_by_zero_error() {
    let code = r#"
        eval { 1 / 0 };
        $@ =~ /division by zero/i ? "ok" : "fail:$@";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}
