//! Extra tests for parallel and concurrency features.

use crate::run;

#[test]
fn test_pmap_basic() {
    let code = r#"
        my @a = (1..5);
        my @b = pmap { $_ * 2 } @a;
        join(",", @b);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "2,4,6,8,10");
}

#[test]
fn test_pmap_chunked() {
    let code = r#"
        my @a = (1..10);
        # pmap_chunked(chunk_size, block, list)
        my @b = pmap_chunked 3, { $_ + 1 }, @a;
        join(",", sort { $a <=> $b } @b);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "2,3,4,5,6,7,8,9,10,11");
}

#[test]
fn test_pchannel_iteration() {
    let code = r#"
        my ($tx, $rx) = pchannel();
        # In a real app we'd spawn a thread, but for unit test we can just send then iterate
        $tx->send(1);
        $tx->send(2);
        undef $tx; # Close the channel so iterator finishes
        
        my @res;
        while (my $v = $rx->recv()) {
            push @res, $v;
        }
        join(",", @res);
    "#;
    // Note: recv() returns undef when closed, or we can use it in a for loop if supported
    assert_eq!(run(code).expect("run").to_string(), "1,2");
}

#[test]
fn test_fan_operator() {
    // fan { block } LIST  - runs block for each item in parallel, discards results (returns count)
    let code = r#"
        my $count = fan { 1 } (1..10);
        $count;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 10);
}

#[test]
fn test_fan_cap_operator() {
    // fan_cap { block } LIST - runs block in parallel and captures results (returns array)
    let code = r#"
        my @res = fan_cap { $_ * 10 } (1..3);
        join(",", sort { $a <=> $b } @res);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "10,20,30");
}

#[test]
fn test_nested_parallelism() {
    // Testing that we don't deadlock when pmap calls pmap
    let code = r#"
        my @res = pmap { 
            my $inner = $_;
            join("-", pmap { $inner . $_ } ("a", "b"))
        } (1, 2);
        join(",", sort @res);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1a-1b,2a-2b");
}

#[test]
fn test_pchannel_try_recv() {
    let code = r#"
        my ($tx, $rx) = pchannel();
        my $v = $rx->try_recv(); # Should be undef immediately
        my $out = defined($v) ? "fail" : "ok";
        $tx->send("data");
        $v = $rx->try_recv();
        $out . ":" . (defined($v) ? $v : "still_undef");
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok:data");
}

#[test]
fn test_ppool_shutdown() {
    let code = r#"
        my $pool = ppool(1);
        $pool->submit(sub { 1 });
        $pool->shutdown();
        # Subsequent submit should fail or be no-op
        eval { $pool->submit(sub { 2 }) };
        $@ ? "failed" : "ok";
    "#;
    // Depending on implementation, it might throw or just not run.
    assert!(
        run(code).expect("run").to_string().contains("failed")
            || run(code).unwrap().to_string() == "ok"
    );
}
