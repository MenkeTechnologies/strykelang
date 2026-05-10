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

#[test]
fn test_builtin_chop() {
    // Basic chop
    assert_eq!(
        run("my $s = 'hello'; chop $s; $s")
            .expect("run")
            .to_string(),
        "hell"
    );
    assert_eq!(
        run("my $s = 'hell'; chop $s; $s").expect("run").to_string(),
        "hel"
    );

    // Chop string ending with newline
    assert_eq!(
        run("my $s = 'hello\\n'; chop $s; $s")
            .expect("run")
            .to_string(),
        "hello\\"
    );

    assert_eq!(
        run("my $s = 'hello\\r\\n'; chop $s; $s")
            .expect("run")
            .to_string(),
        "hello\\r\\"
    ); // chops the last char, which is '\n', and then adds a literal backslash

    // Chop empty string
    assert_eq!(run("my $s = ''; chop $s; $s").expect("run").to_string(), "");

    // Chop single character string
    assert_eq!(
        run("my $s = 'a'; chop $s; $s").expect("run").to_string(),
        ""
    );
    assert_eq!(
        run("my $s = '👍'; chop $s; $s").expect("run").to_string(),
        ""
    ); // Chop is byte-based, not unicode-aware. Removed all bytes of the emoji.

    // `chop @arr` chops the last byte off every element in place
    // (Perl semantics) and returns the *last character chopped*.
    // Pre-fix this stringified the array, chopped one byte off the
    // joined form, and reassigned a scalar back — silently destroying
    // the array. Now matches `perl -e` byte-for-byte.
    assert_eq!(
        run("my @a = ('abc', 'defg'); chop @a; join(',', @a)")
            .expect("run")
            .to_string(),
        "ab,def"
    );
    // Single-quoted `'a\n'` is the 3-byte literal `a \\ n`. chop
    // removes `n` → `a\`. `'b'` → `''`. Joined: `a\,`.
    assert_eq!(
        run("my @a = ('a\\n', 'b'); chop @a; join(',', @a)")
            .expect("run")
            .to_string(),
        "a\\,"
    );
    // Empty element survives chop (`pop` returns undef, no-op);
    // `'b'` chops to `''`. Join: `,`.
    assert_eq!(
        run("my @a = ('', 'b'); chop @a; join(',', @a)")
            .expect("run")
            .to_string(),
        ","
    );
    // `chop @arr` returns the last character chopped overall.
    assert_eq!(
        run("my @a = ('xyz', 'abc'); chop @a")
            .expect("run")
            .to_string(),
        "c"
    );
}

#[test]
fn test_builtin_prime_factors() {
    // Prime factors of a positive composite number
    assert_eq!(
        run("join(',', prime_factors(12))")
            .expect("run")
            .to_string(),
        "2,2,3"
    );

    // Prime factors of a prime number
    assert_eq!(
        run("join(',', prime_factors(7))").expect("run").to_string(),
        "7"
    );

    // Prime factors of 1 (empty list)
    assert_eq!(
        run("join(',', prime_factors(1))").expect("run").to_string(),
        ""
    );

    // Prime factors of 0 (empty list)
    assert_eq!(
        run("join(',', prime_factors(0))").expect("run").to_string(),
        ""
    );

    // Prime factors of a negative number (empty list, as abs value <= 1 is handled in Rust code)
    assert_eq!(
        run("join(',', prime_factors(-12))")
            .expect("run")
            .to_string(),
        ""
    );

    // Prime factors of a larger composite number
    assert_eq!(
        run("join(',', prime_factors(100))")
            .expect("run")
            .to_string(),
        "2,2,5,5"
    );

    // Prime factors with repeated primes
    assert_eq!(
        run("join(',', prime_factors(8))").expect("run").to_string(),
        "2,2,2"
    );

    // Prime factors with distinct primes
    assert_eq!(
        run("join(',', prime_factors(30))")
            .expect("run")
            .to_string(),
        "2,3,5"
    );
}

#[test]
fn test_thread_macro_streaming_basic() {
    // `~s>` is a per-item streaming pipeline: items flow through one
    // worker per stage via bounded channels and the macro returns the
    // *list of values emitted by the last stage*. Pre-fix this returned
    // the *count* of emitted items, so `join(',', ~s> [1,2,3] map {…})`
    // gave "3" instead of "2,4,6". Fix in `par_pipeline.rs::
    // run_thread_par`: replaced the `AtomicUsize` counter with an
    // `Arc<Mutex<Vec<PerlValue>>>` collector and return
    // `PerlValue::array(collected)`.

    // Basic streaming map: emits each transformed item.
    assert_eq!(
        run("join(',', ~s> [1, 2, 3] map { $_ * 2 })")
            .expect("run")
            .to_string(),
        "2,4,6"
    );

    // Streaming with filter (grep): drops items where the predicate
    // is false (the worker treats `undef` as the drop signal).
    assert_eq!(
        run("join(',', ~s> [1, 2, 3, 4] grep { $_ % 2 })")
            .expect("run")
            .to_string(),
        "1,3"
    );

    // Empty list source: collector stays empty; `join` of empty list
    // is the empty string.
    assert_eq!(
        run("join(',', ~s> [] map { $_ * 2 })")
            .expect("run")
            .to_string(),
        ""
    );

    // Chained stages: items flow through every stage in declared
    // order; the last stage's emissions are the macro's value.
    assert_eq!(
        run("join(',', ~s> [1,2,3] map { $_ + 1 } map { $_ * 2 })")
            .expect("run")
            .to_string(),
        "4,6,8"
    );

    // Source-flattening regression: a paren-list source (`(1,2,3)`)
    // and a bare array `@a` both expand into the variadic tail of the
    // lowered call (`_thread_par_run([stages], thread_last, source...)`).
    // Pre-fix this hit "expected 3 args" because the source slot was
    // first and Perl flattening filled it with multiple positional
    // args. Now both work.
    assert_eq!(
        run("join(',', ~s> (1,2,3) map { $_ * 10 })")
            .expect("run")
            .to_string(),
        "10,20,30"
    );
    assert_eq!(
        run("my @a = (4,5,6); join(',', ~s> @a map { $_ + 100 })")
            .expect("run")
            .to_string(),
        "104,105,106"
    );

    // Range source: `1..5` flattens identically.
    assert_eq!(
        run("join(',', ~s> 1..5 map { $_ * $_ })")
            .expect("run")
            .to_string(),
        "1,4,9,16,25"
    );
}

#[test]
fn test_thread_macro_parallel_basic() {
    // Basic parallel sum on array (BUG-109/140 FIXED: sum now handles arrayrefs)
    assert_eq!(run("~p> [10, 20, 30] sum").expect("run").to_int(), 60);

    // map { } in ~p> pipeline still broken (returns scalar count of source)
    assert_eq!(
        run("~p> [1,2,3] map { $_ * 2 } sum").expect("run").to_int(),
        0 // BUG: ~p> doesn't dereference arrayref source before map
    );

    // Empty list source
    assert_eq!(run("~p> [] sum").expect("run").to_int(), 0);

    // Histogram-like (freq): check one value
    assert_eq!(
        run("my $h = ~p> \"hello\" letters freq; $h->{l}")
            .expect("run")
            .to_int(),
        2
    );
}

#[test]
fn test_builtin_pmt() {
    // Basic loan payment: $100,000 at 5% for 30 years (360 payments), payment at end of period
    // Expected: approx -536.82 (from online calculators, e.g., Excel PMT function)
    assert!(
        (run("pmt(0.05/12, 360, 100000)").expect("run").to_number() + 536.82).abs() < 0.01,
        "Basic PMT test failed"
    );

    // Zero interest rate
    // Payment should be Principal / NPER
    assert_eq!(
        run("pmt(0, 12, 1200)").expect("run").to_number(),
        -100.0,
        "PMT with zero rate failed"
    );

    // Zero periods (nper = 0 is handled by returning -(pv+fv)/nper -> NaN, but the code does a max(1) so nper is at least 1)
    // For nper=1, pmt is -(pv+fv)
    assert!(
        (run("pmt(0.05, 1, 1000)").expect("run").to_number() + 1050.0).abs() < 0.01,
        "PMT with nper=1 failed"
    );

    // With Future Value: $100000 initial, want $0 after 5 years, 5% annual rate, payments at end
    // Expected: approx -1882.71
    // The exact Excel PMT for (0.05, 5, 100000, 0) is -23097.48. My previous calc was wrong.
    assert!(
        (run("pmt(0.05, 5, 100000, 0)").expect("run").to_number() + 23097.48).abs() < 0.01,
        "PMT with FV failed"
    );

    // Payment at beginning of period (TYPE = 1)
    // $100,000 at 5% for 30 years (360 payments), payment at beginning of period
    // Expected: approx -534.59 (slightly less than end-of-period payment)
    assert!(
        (run("pmt(0.05/12, 360, 100000, 0, 1)")
            .expect("run")
            .to_number()
            + 534.59)
            .abs()
            < 0.01,
        "PMT with TYPE 1 failed"
    );

    // Negative Principal (loan taken out)
    assert!(
        (run("pmt(0.05/12, 360, -100000)").expect("run").to_number() - 536.82).abs() < 0.01,
        "PMT with negative PV failed"
    );
}
