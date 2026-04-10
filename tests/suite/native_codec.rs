//! Builtins: sha256, hmac, uuid, base64, hex, gzip/zstd, datetime, toml/yaml decode.

use crate::common::*;

#[test]
fn sha256_builtin_matches_vector() {
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn hmac_sha256_known_vector() {
    assert_eq!(
        eval_string(r#"hmac_sha256("key", "The quick brown fox")"#),
        "203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
    );
}

#[test]
fn uuid_is_rfc4122_shape() {
    let s = eval_string(r#"uuid()"#);
    assert_eq!(s.len(), 36);
    assert_eq!(s.matches('-').count(), 4);
}

#[test]
fn base64_hex_roundtrip() {
    assert_eq!(eval_string(r#"hex_decode(hex_encode("hello"))"#), "hello");
    assert_eq!(
        eval_string(r#"base64_decode(base64_encode("binary" . chr(0)))"#),
        eval_string(r#""binary" . chr(0)"#)
    );
}

#[test]
fn gzip_gunzip_roundtrip() {
    assert_eq!(eval_string(r#"gunzip(gzip("payload"))"#), "payload");
}

#[test]
fn zstd_roundtrip() {
    assert_eq!(eval_string(r#"zstd_decode(zstd("z data"))"#), "z data");
}

#[test]
fn datetime_parse_and_strftime() {
    let v = eval_string(r#"int(datetime_parse_rfc3339("2020-01-02T00:00:00Z"))"#);
    assert_eq!(v, "1577923200");
    assert_eq!(
        eval_string(r#"datetime_strftime(1577923200, "%Y-%m-%d")"#),
        "2020-01-02"
    );
}

#[test]
fn toml_and_yaml_decode() {
    assert_eq!(
        eval_string(r#"my $h = toml_decode("x = 7"); $h->{"x"}"#),
        "7"
    );
    assert_eq!(
        eval_string(r#"my $y = yaml_decode("k: v\n"); $y->{"k"}"#),
        "v"
    );
}
