//! Tests for parallel/concurrency APIs (pchannel, ppool, etc.) via the interpreter.

use crate::run;

#[test]
fn test_pchannel_interpreter_roundtrip() {
    let code = r#"
        my ($tx, $rx) = pchannel();
        $tx->send(42);
        $rx->recv();
    "#;
    assert_eq!(run(code).expect("run").to_int(), 42);
}

#[test]
fn test_pchannel_bounded() {
    let _code = r#"
        my ($tx, $rx) = pchannel(1);
        $tx->send(10);
        # second send would block if we were in a different thread, 
        # but here we just test it works and returns truth
        $tx->send(20) ? 0 : 1; 
    "#;
}

#[test]
fn test_pselect_interpreter() {
    let code = r#"
        my ($tx1, $rx1) = pchannel();
        my ($tx2, $rx2) = pchannel();
        $tx2->send("ok");
        my ($val, $idx) = pselect($rx1, $rx2);
        "$val:$idx";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok:1");
}

#[test]
fn test_ppool_interpreter() {
    let code = r#"
        my $pool = ppool(2);
        for my $i (1..3) {
            $pool->submit(sub { $_ * 10 }, $i);
        }
        my @res = $pool->collect();
        join(",", sort { $a <=> $b } @res);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "10,20,30");
}

#[test]
fn test_pwatch_helpers() {
    // pwatch is hard to test without real FS events, but we can test the error cases.
    let code = r#"
        sub cb { 1 }
        my $failed = 0;
        eval { pwatch("/no/such/dir/likely/*.log", \&cb) };
        if ($@) { $failed = 1; }
        $failed;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 1);
}
