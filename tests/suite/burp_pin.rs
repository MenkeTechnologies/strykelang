//! Pins for `burp HASH` — inverse of `swallow`. Hash `{ path => content }` → files on disk.
//!
//! Behaviour fixed here:
//!   * basic write — every entry materialises with correct bytes
//!   * mkdir -p — missing parent directories are created on the fly
//!   * swallow → burp round-trip is byte-identical (including binary payloads)
//!   * hash refs accepted as well as plain hashes
//!   * empty hash returns 0 and touches nothing
//!   * non-HASH argument is a runtime error (not a panic / silent drop)
//!   * return value is the integer count of files written
//!
//! All tests use `/tmp/stryke_burp_pin_<nanos>_<tag>/` and clean up on success.

use crate::common::*;

fn fresh_dir(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = format!("/tmp/stryke_burp_pin_{}_{}", nanos, tag);
    std::fs::create_dir_all(&dir).expect("mkdir tmp");
    dir
}

fn rm_rf(dir: &str) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn burp_writes_every_entry_and_returns_count() {
    let dir = fresh_dir("basic");
    let code = format!(
        r#"
            my %h = ( "{dir}/a.txt" => "AAA", "{dir}/b.txt" => "BBBB" );
            burp \%h
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    let a = std::fs::read(format!("{dir}/a.txt")).expect("a written");
    let b = std::fs::read(format!("{dir}/b.txt")).expect("b written");
    rm_rf(&dir);
    assert_eq!(n, 2, "burp must return the integer count of files written");
    assert_eq!(a, b"AAA");
    assert_eq!(b, b"BBBB");
}

#[test]
fn burp_creates_parent_directories_on_the_fly() {
    // Path includes two levels of missing parents — burp must mkdir -p.
    let dir = fresh_dir("mkdirp");
    let nested = format!("{dir}/x/y/z/out.txt");
    let code = format!(
        r#"
            my %h = ( "{nested}" => "payload" );
            burp \%h
        "#,
        nested = nested,
    );
    let n = eval_int(&code);
    let body = std::fs::read(&nested).expect("nested file written");
    rm_rf(&dir);
    assert_eq!(n, 1);
    assert_eq!(body, b"payload");
}

#[test]
fn swallow_then_burp_is_byte_identical_roundtrip() {
    // Seed binary content, swallow into a hash, burp to a new tree, verify
    // every byte survives intact — covers high bytes and invalid UTF-8 prefixes.
    //
    // `swallow` returns canonicalised paths (`/tmp` on macOS resolves to
    // `/private/tmp`), so we canonicalise both src and dst before building the
    // regex so the substitution actually matches the keys.
    let src = fresh_dir("rt_src");
    let dst = fresh_dir("rt_dst");
    let canon_src = std::fs::canonicalize(&src).unwrap().display().to_string();
    let canon_dst = std::fs::canonicalize(&dst).unwrap().display().to_string();
    let payload_a: Vec<u8> = vec![0x00, 0xFF, 0x80, 0x7F, 0x01];
    let payload_b: Vec<u8> = vec![0xC3, 0x28, 0xFE, 0x0A]; // invalid UTF-8 prefix + LF
    std::fs::write(format!("{src}/a.bin"), &payload_a).unwrap();
    std::fs::write(format!("{src}/b.bin"), &payload_b).unwrap();
    let code = format!(
        r#"
            my %in = swallow("{src}/*.bin");
            my %out;
            for my $k (keys %in) {{
                my $newk = $k;
                $newk =~ s{{^{canon_src}}}{{{canon_dst}}};
                $out{{$newk}} = $in{{$k}};
            }}
            burp \%out
        "#,
        src = src,
        canon_src = canon_src,
        canon_dst = canon_dst,
    );
    let n = eval_int(&code);
    let got_a = std::fs::read(format!("{canon_dst}/a.bin")).expect("a copied");
    let got_b = std::fs::read(format!("{canon_dst}/b.bin")).expect("b copied");
    rm_rf(&src);
    rm_rf(&dst);
    assert_eq!(n, 2);
    assert_eq!(got_a, payload_a, "binary payload A must round-trip exactly");
    assert_eq!(got_b, payload_b, "binary payload B must round-trip exactly");
}

#[test]
fn burp_accepts_inline_hashref_literal() {
    // Inline hashref literal `{ k => v, ... }` is the most ergonomic form
    // for scaffolding-style burps where the hash isn't already named.
    let dir = fresh_dir("inline");
    let code = format!(
        r#"
            burp {{ "{dir}/x.txt" => "x", "{dir}/y.txt" => "yy" }}
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    let x = std::fs::read(format!("{dir}/x.txt")).expect("x written");
    let y = std::fs::read(format!("{dir}/y.txt")).expect("y written");
    rm_rf(&dir);
    assert_eq!(n, 2);
    assert_eq!(x, b"x");
    assert_eq!(y, b"yy");
}

#[test]
fn burp_empty_hash_returns_zero_and_touches_nothing() {
    let dir = fresh_dir("empty");
    let marker = format!("{dir}/sentinel");
    std::fs::write(&marker, b"unchanged").unwrap();
    let code = r#"burp {}"#;
    let n = eval_int(code);
    let after = std::fs::read(&marker).expect("marker still present");
    rm_rf(&dir);
    assert_eq!(n, 0, "burp on empty hash should return 0");
    assert_eq!(
        after, b"unchanged",
        "burp {{}} must not touch the filesystem"
    );
}

#[test]
fn burp_on_non_hash_argument_is_a_runtime_error() {
    // Passing a string (not a hash) must fail with a runtime error, not
    // silently no-op and not panic.
    let kind = eval_err_kind(r#"burp "not a hash""#);
    assert!(
        matches!(kind, stryke::error::ErrorKind::Runtime { .. }),
        "expected Runtime error on non-hash burp argument, got {kind:?}"
    );
}
