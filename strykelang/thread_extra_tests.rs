//! Extra tests for async/await and thread pools.

use crate::run;

#[test]
fn test_async_await_basic() {
    let code = r#"
        my $t = async { 10 + 32 };
        await($t);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 42);
}

#[test]
fn test_async_parallel_execution() {
    // Two tasks running in parallel
    let code = r#"
        my $t1 = async { sleep(0.1); 1 };
        my $t2 = async { sleep(0.1); 2 };
        await($t1) + await($t2);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 3);
}

#[test]
fn test_async_error_propagation() {
    let code = r#"
        my $t = async { die "async_fail" };
        eval { await($t) };
        $@ =~ /async_fail/ ? "ok" : "fail";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}

#[test]
fn test_async_closure_capture() {
    let code = r#"
        my $x = 100;
        my $t = async { $x + 1 };
        await($t);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 101);
}

#[test]
fn test_mysync_variable_sharing() {
    let code = r#"
        mysync $counter = 0;
        my @t;
        for (1..10) {
            push @t, async { $counter++ };
        }
        for (@t) { await($_) }
        $counter;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 10);
}

#[test]
fn test_async_stress() {
    // Spawn 100 async tasks
    let code = r#"
        my @tasks = map { async { $_ } } (1..100);
        my $sum = 0;
        for (@tasks) { $sum += await($_); }
        $sum;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 5050);
}
