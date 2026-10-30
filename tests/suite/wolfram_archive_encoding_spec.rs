//! Spec checks for the archive / encoding framing builtins in
//! `strykelang/math_wolfram_archive_encoding_formats.rs`: POSIX ustar
//! header arithmetic, ZIP record sizes (APPNOTE 4.3), RFC 2045
//! quoted-printable, uuencode, RFC 3986 percent-encoding, RFC 3492
//! punycode `adapt`, MessagePack and RFC 8949 CBOR header bytes.

use crate::common::*;

/// Unpacks the `0xPP_HH_LL` triple the escape-style encoders return.
fn escape_triple(v: i64) -> (u8, u8, u8) {
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[test]
fn tar_checksum_counts_chksum_field_as_spaces() {
    // All-zero header: only the 8-byte chksum field (offset 148..156) contributes, as 8 * ' '.
    assert_eq!(eval_int("tar_header_checksum([(0) x 512])"), 8 * 32);
    // Bytes inside the chksum field are ignored; bytes outside are summed.
    assert_eq!(
        eval_int("my @h = (0) x 512; $h[148] = 255; $h[0] = 7; $h[511] = 1; tar_header_checksum(\\@h)"),
        8 * 32 + 7 + 1
    );
}

#[test]
fn tar_block_padding() {
    assert_eq!(eval_int("tar_pad_512(0)"), 0);
    assert_eq!(eval_int("tar_pad_512(1)"), 511);
    assert_eq!(eval_int("tar_pad_512(512)"), 0);
    assert_eq!(eval_int("tar_pad_512(513)"), 511);
    assert_eq!(eval_int("tar_member_record(0)"), 512);
    assert_eq!(eval_int("tar_member_record(1)"), 1024);
    assert_eq!(eval_int("tar_member_record(512)"), 1024);
    assert_eq!(eval_int("tar_member_record(513)"), 1536);
}

#[test]
fn zip_record_sizes() {
    assert_eq!(eval_int("zip_local_header(8, 0)"), 38);
    assert_eq!(eval_int("zip_central_dir(8, 4, 2)"), 60);
    assert_eq!(eval_int("zip_eocd(0)"), 22);
    // Negative lengths clamp to zero instead of shrinking the fixed part.
    assert_eq!(eval_int("zip_local_header(-5, -5)"), 30);
}

#[test]
fn quoted_printable_escapes_equals_sign() {
    // RFC 2045 §6.7 rule (1): '=' must always be encoded as "=3D".
    assert_eq!(escape_triple(eval_int("quoted_printable_encode(61)")), (b'=', b'3', b'D'));
}

#[test]
fn quoted_printable_literal_and_escaped_ranges() {
    // Printable ASCII other than '=' and TAB/space pass through unchanged.
    for b in [9, 32, 33, 65, 60, 62, 126] {
        assert_eq!(eval_int(&format!("quoted_printable_encode({b})")), b, "byte {b}");
    }
    assert_eq!(escape_triple(eval_int("quoted_printable_encode(0)")), (b'=', b'0', b'0'));
    assert_eq!(escape_triple(eval_int("quoted_printable_encode(10)")), (b'=', b'0', b'A'));
    assert_eq!(escape_triple(eval_int("quoted_printable_encode(127)")), (b'=', b'7', b'F'));
    assert_eq!(escape_triple(eval_int("quoted_printable_encode(255)")), (b'=', b'F', b'F'));
}

#[test]
fn uuencode_three_bytes_to_four_chars() {
    // "Cat" uuencodes to "0V%T" (classic example).
    let v = eval_int("uuencode_step([67, 97, 116])");
    assert_eq!((v as u32).to_be_bytes(), *b"0V%T");
    // All-zero input maps every sextet to ' ' (0x20).
    assert_eq!((eval_int("uuencode_step([0, 0, 0])") as u32).to_be_bytes(), *b"    ");
}

#[test]
fn modhex_alphabet() {
    let s: String = (0..16)
        .map(|n| eval_int(&format!("modhex_encode({n})")) as u8 as char)
        .collect();
    assert_eq!(s, "cbdefghijklnrtuv");
}

#[test]
fn percent_encode_unreserved_set() {
    for b in b"AZaz09-._~" {
        assert_eq!(eval_int(&format!("percent_encode_full({b})")), *b as i64);
    }
    assert_eq!(escape_triple(eval_int("percent_encode_full(32)")), (b'%', b'2', b'0'));
    assert_eq!(escape_triple(eval_int("percent_encode_full(47)")), (b'%', b'2', b'F'));
    assert_eq!(escape_triple(eval_int("percent_encode_full(255)")), (b'%', b'F', b'F'));
}

#[test]
fn punycode_adapt_rfc3492() {
    // adapt(delta, numpoints, firsttime) hand-traced from RFC 3492 §6.1:
    // adapt(0, 1, true) = 0.
    assert_eq!(eval_int("punycode_encode(0, 1, 1)"), 0);
    // First time: 25200/700 = 36, += 36 -> 72; no loop; 36*72/110 = 23.
    assert_eq!(eval_int("punycode_encode(25200, 1, 1)"), 23);
    // Loop path: 700000/700 = 1000 -> 2000 > 455, /35 = 57, k = 36; 36 + 36*57/95 = 57.
    assert_eq!(eval_int("punycode_encode(700000, 1, 1)"), 57);
    // Not first time: delta halved (10 -> 5), += 5/1 -> 10; 36*10/48 = 7.
    assert_eq!(eval_int("punycode_encode(10, 1, 0)"), 7);
}

#[test]
fn idn_prefix_detection() {
    assert_eq!(eval_int("idn_to_ascii([120, 110, 45, 45])"), 1); // "xn--"
    assert_eq!(eval_int("idn_to_ascii([120, 110, 45, 120])"), 0);
    assert_eq!(eval_int("idn_to_ascii([120, 110])"), 0);
    assert_eq!(eval_int("idn_to_unicode(10)"), 6);
    assert_eq!(eval_int("idn_to_unicode(2)"), 0);
}

#[test]
fn msgpack_uint_format_boundaries() {
    // prefix*100 + encoded size.
    assert_eq!(eval_int("msgpack_pack_int(0)"), 1);
    assert_eq!(eval_int("msgpack_pack_int(127)"), 127 * 100 + 1);
    assert_eq!(eval_int("msgpack_pack_int(128)"), 0xCC * 100 + 2);
    assert_eq!(eval_int("msgpack_pack_int(255)"), 0xCC * 100 + 2);
    assert_eq!(eval_int("msgpack_pack_int(256)"), 0xCD * 100 + 3);
    assert_eq!(eval_int("msgpack_pack_int(65536)"), 0xCE * 100 + 5);
    assert_eq!(eval_int("msgpack_pack_int(4294967296)"), 0xCF * 100 + 9);
}

#[test]
fn msgpack_str_format_boundaries() {
    assert_eq!(eval_int("msgpack_pack_str(0)"), 0xA0);
    assert_eq!(eval_int("msgpack_pack_str(31)"), 0xBF);
    assert_eq!(eval_int("msgpack_pack_str(32)"), 0xD9);
    assert_eq!(eval_int("msgpack_pack_str(255)"), 0xD9);
    assert_eq!(eval_int("msgpack_pack_str(256)"), 0xDA);
    assert_eq!(eval_int("msgpack_pack_str(65536)"), 0xDB);
}

#[test]
fn cbor_header_bytes_rfc8949() {
    // Appendix A: 0 -> 0x00, 23 -> 0x17, 24 -> 0x18 18, 1000 -> 0x19 03e8,
    // 1000000 -> 0x1a 000f4240, 1000000000000 -> 0x1b ...
    assert_eq!(eval_int("cbor_encode_uint(0)"), 0x00);
    assert_eq!(eval_int("cbor_encode_uint(23)"), 0x17);
    assert_eq!(eval_int("cbor_encode_uint(24)"), 0x18);
    assert_eq!(eval_int("cbor_encode_uint(1000)"), 0x19);
    assert_eq!(eval_int("cbor_encode_uint(1000000)"), 0x1A);
    assert_eq!(eval_int("cbor_encode_uint(1000000000000)"), 0x1B);
    // Text strings: "" -> 0x60, "a" -> 0x61, 24-byte string -> 0x78.
    assert_eq!(eval_int("cbor_encode_str(0)"), 0x60);
    assert_eq!(eval_int("cbor_encode_str(1)"), 0x61);
    assert_eq!(eval_int("cbor_encode_str(23)"), 0x77);
    assert_eq!(eval_int("cbor_encode_str(24)"), 0x78);
    assert_eq!(eval_int("cbor_encode_str(256)"), 0x79);
}
