//! Builtins: digests, uuid, base64, hex, gzip/zstd, datetime, toml/yaml, URL encoding.

use crate::common::*;

#[test]
fn sha256_builtin_matches_vector() {
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn md5_sha1_builtins_match_vectors() {
    assert_eq!(
        eval_string(r#"md5("")"#),
        "d41d8cd98f00b204e9800998ecf8427e"
    );
    assert_eq!(
        eval_string(r#"sha1("abc")"#),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
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
fn datetime_iana_parse_and_format_roundtrip() {
    assert_eq!(
        eval_string(
            r#"datetime_format_tz(datetime_parse_local("2024-06-15 12:00:00", "America/New_York"), "America/New_York", "%Y-%m-%d %H:%M:%S")"#
        ),
        "2024-06-15 12:00:00"
    );
    assert_eq!(eval_string(r#"int(datetime_add_seconds(100, 2.5))"#), "102");
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

#[test]
fn toml_yaml_encode_roundtrip() {
    assert_eq!(
        eval_string(r#"my $h = { x => 1, k => "v" }; toml_decode(toml_encode($h))->{"x"}"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"my $h = { x => 1, k => "v" }; yaml_decode(yaml_encode($h))->{"k"}"#),
        "v"
    );
}

#[test]
fn url_escape_aliases_and_roundtrip() {
    assert_eq!(eval_string(r#"url_decode(url_encode("a b"))"#), "a b");
    assert_eq!(eval_string(r#"uri_unescape(uri_escape("c+d"))"#), "c+d");
}
