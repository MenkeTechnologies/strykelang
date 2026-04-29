//! Extra tests for core Perl builtins to ensure standard semantics.

use crate::run;

#[test]
fn test_builtin_substr() {
    // Basic substr
    assert_eq!(run("substr('hello', 0, 2)").expect("run").to_string(), "he");
    assert_eq!(run("substr('hello', 1)").expect("run").to_string(), "ello");

    // Negative offset
    assert_eq!(run("substr('hello', -2)").expect("run").to_string(), "lo");
    assert_eq!(
        run("substr('hello', -3, 2)").expect("run").to_string(),
        "ll"
    );

    // Negative length (length from end)
    assert_eq!(
        run("substr('hello', 1, -1)").expect("run").to_string(),
        "ell"
    );

    // Edge case: offset beyond length
    assert_eq!(run("substr('hi', 10)").expect("run").to_string(), "");
}

#[test]
fn test_builtin_split() {
    // Simple split
    assert_eq!(
        run("join(':', split(',', 'a,b,c'))")
            .expect("run")
            .to_string(),
        "a:b:c"
    );

    // Split with regex
    // Note: escape \s for the string literal passed to run()
    assert_eq!(
        run("join(':', split('\\\\s+', 'a  b   c'))")
            .expect("run")
            .to_string(),
        "a:b:c"
    );

    // Split empty string — Perl 5 returns the empty list; trailing-empty
    // strip applies (LIMIT 0 / omitted), and there are no fields anyway.
    // Old stryke incorrectly returned [""]; fixed in vm.rs::Op::Split.
    assert_eq!(run("len split(',', '')").expect("run").to_int(), 0);

    // Split with limit
    assert_eq!(
        run("join(':', split(',', 'a,b,c,d', 2))")
            .expect("run")
            .to_string(),
        "a:b,c,d"
    );
}

#[test]
fn test_builtin_join() {
    assert_eq!(run("join('-', 1, 2, 3)").expect("run").to_string(), "1-2-3");
    assert_eq!(
        run("join('-', (1, 2), 3)").expect("run").to_string(),
        "1-2-3"
    );

    // join stringifies its arguments. [1, 2] is an array ref.
    let res = run("join('-', [1, 2])").expect("run").to_string();
    assert!(res.contains("ARRAY("));
}

#[test]
fn test_builtin_index_rindex() {
    assert_eq!(run("index('hello', 'e')").expect("run").to_int(), 1);
    assert_eq!(run("index('hello', 'x')").expect("run").to_int(), -1);
    assert_eq!(run("rindex('hello', 'l')").expect("run").to_int(), 3);
}

#[test]
fn test_builtin_sprintf() {
    assert_eq!(run("sprintf('%03d', 5)").expect("run").to_string(), "005");
    assert_eq!(
        run("sprintf('%.2f', 3.14159)").expect("run").to_string(),
        "3.14"
    );
    assert_eq!(
        run("sprintf('%s-%d', 'hi', 42)").expect("run").to_string(),
        "hi-42"
    );
}

#[test]
fn test_builtin_hex_oct() {
    assert_eq!(run("hex('0xFF')").expect("run").to_int(), 255);
    assert_eq!(run("hex('FF')").expect("run").to_int(), 255);
    assert_eq!(run("oct('077')").expect("run").to_int(), 63);
    assert_eq!(run("oct('77')").expect("run").to_int(), 63);
    // Perl oct() also handles hex if it starts with 0x
    assert_eq!(run("oct('0xff')").expect("run").to_int(), 255);
}
