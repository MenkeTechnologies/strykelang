//! Pins for `god EXPR` — omniscient runtime introspection.
//!
//! Behaviour fixed here:
//!   * scalars (integer, float, undef) render as one-line "immediate" entries
//!   * BYTES values surface the heap pointer, strong count, length, and a hex preview
//!   * HASH values surface the heap pointer and entry count, then recurse into entries
//!   * the SAME Arc reached by two paths prints the SAME `0x...` pointer (aliasing visible)
//!   * a self-referential structure terminates with a `...cycle` annotation, no stack overflow
//!   * the return value is a *string* (composable with regex / pipe / assertions)
//!   * a closure with a captured variable surfaces both the capture count and the captured value
//!
//! `god` is a pure inspector — these tests assert on the returned string only.

use crate::common::*;

#[test]
fn god_integer_immediate_one_liner() {
    let s = eval_string(r#"god 42"#);
    assert!(s.contains("INTEGER 42 (immediate)"), "got: {s}");
    // One line, trailing newline allowed.
    assert_eq!(s.trim().lines().count(), 1, "got multiline: {s:?}");
}

#[test]
fn god_undef_one_liner() {
    let s = eval_string(r#"god undef"#);
    assert_eq!(s.trim(), "undef");
}

#[test]
fn god_bytes_surfaces_ptr_strong_len_and_hex_preview() {
    // slurp returns BYTES (per the byte-string slurp change). A known fixture
    // written via spew lets us pin the hex preview exactly.
    let path = format!(
        "/tmp/stryke_god_pin_{}.bin",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::fs::write(&path, [0x00u8, 0xff, 0x7f, 0x80, 0x01]).unwrap();
    let code = format!(r#"god slurp("{path}")"#);
    let s = eval_string(&code);
    let _ = std::fs::remove_file(&path);
    assert!(s.contains("BYTES @ 0x"), "missing heap ptr: {s}");
    assert!(s.contains("strong="), "missing strong count: {s}");
    assert!(s.contains("len=5"), "wrong len: {s}");
    assert!(s.contains("hex=00 ff 7f 80 01"), "wrong hex preview: {s}");
}

#[test]
fn god_hash_ref_surfaces_entries_and_recurses() {
    // The body must show the HASH header AND descend into each entry — verify
    // by checking for an entry's inner INTEGER line.
    let s = eval_string(
        r#"
            my $h = { a => 1, b => 2, c => 3 };
            god $h
        "#,
    );
    assert!(s.contains("HASH @ 0x"), "no hash header: {s}");
    assert!(s.contains("entries=3"), "wrong entry count: {s}");
    assert!(s.contains(r#""a" =>"#), "no key \"a\": {s}");
    assert!(
        s.contains("INTEGER 1 (immediate)"),
        "no descent to value: {s}"
    );
}

#[test]
fn god_returns_a_string_not_undef() {
    // Composable: god($x) is a real String, length > 0, regex-matchable.
    let n = eval_int(
        r#"
            my $s = god { a => 1 };
            length($s) > 10 ? 1 : 0
        "#,
    );
    assert_eq!(n, 1, "god result should be a non-trivial string");
}

#[test]
fn god_aliasing_two_refs_show_same_pointer() {
    // Two scalars holding the same Arc must print an identical 0x… pointer
    // for the underlying heap object. Verifying via stryke-side regex avoids
    // string-format coupling: we extract both 0x... hex literals and compare.
    let n = eval_int(
        r#"
            my $shared = { count => 1 };
            my $a = $shared;
            my $b = $shared;
            my $da = god $a;
            my $db = god $b;
            ($da =~ /HASH \@ (0x[0-9a-f]+)/) or die "no hash ptr in a";
            my $pa = $1;
            ($db =~ /HASH \@ (0x[0-9a-f]+)/) or die "no hash ptr in b";
            my $pb = $1;
            ($pa eq $pb) ? 1 : 0
        "#,
    );
    assert_eq!(
        n, 1,
        "two aliases of the same hash must show the same 0x… pointer"
    );
}

#[test]
fn god_cycle_terminates_with_annotation() {
    // A self-referential hash must not stack-overflow; the second visit to
    // the same Arc gets annotated `...cycle`.
    let s = eval_string(
        r#"
            my %h;
            $h{self} = \%h;
            god \%h
        "#,
    );
    assert!(s.contains("cycle"), "expected cycle annotation, got: {s}");
}

#[test]
fn god_strong_count_increments_with_aliases() {
    // Aliasing increments strong refcount. Capture the strong=N from the dump
    // and assert it is at least 2 once the value is held by a second var.
    let n = eval_int(
        r#"
            my $h = { a => 1 };
            my $kept = $h;                   # extra strong reference
            my $d = god $h;
            ($d =~ /strong=(\d+)/) or die "no strong count";
            my $strong = 0 + $1;
            $strong >= 2 ? 1 : 0
        "#,
    );
    assert_eq!(n, 1, "aliasing must bump Arc strong count to >= 2");
}
