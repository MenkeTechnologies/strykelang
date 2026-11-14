//! Reference-vector pins for the checksum / codec / phonetic builtins in
//! `builtins_phonetic_geo_codec.rs` (CRC variants, Adler-32 combine,
//! Fletcher-16/32/64, Base58Check, Z85, basE91, American Soundex).
//!
//! Expected values come from published references, not from the
//! implementation:
//! - CRC check values: the RevEng CRC catalogue
//!   (<https://reveng.sourceforge.io/crc-catalogue/all.htm>), `check=` is
//!   the CRC of the ASCII string `"123456789"`.
//! - Fletcher: the "Test vectors" section of Wikipedia's
//!   "Fletcher's checksum" (little-endian word assembly, zero-padded tail).
//! - Adler-32: Wikipedia's "Adler-32" worked example (`"Wikipedia"`
//!   = `0x11E60398`); combine results cross-checked against a Perl
//!   Adler-32 of the concatenation.
//! - Base58Check: the Bitcoin wiki address example, cross-checked with a
//!   Perl `Digest::SHA` + `Math::BigInt` encoder.
//! - Z85: ZeroMQ RFC 32 (`rfc.zeromq.org/spec/32`) length rules.
//! - Soundex: Wikipedia's "American Soundex" worked examples.

use crate::common::*;

const CHECK: &str = "123456789";

fn hex(code: &str) -> String {
    eval_string(code)
}

// ── CRC catalogue check values ─────────────────────────────────────

#[test]
fn crc24_openpgp_check_value() {
    assert_eq!(hex(&format!(r#"sprintf("%06x", crc24("{CHECK}"))"#)), "21cf02");
}

#[test]
fn crc64_ecma182_check_value() {
    assert_eq!(
        hex(&format!(r#"sprintf("%016x", crc64_ecma("{CHECK}"))"#)),
        "6c40df5f0b497347"
    );
}

#[test]
fn crc64_xz_check_value() {
    assert_eq!(
        hex(&format!(r#"sprintf("%016x", crc64_xz("{CHECK}"))"#)),
        "995dc9bbdf1939fa"
    );
}

/// CRC-6/G-704 has width 6 < 8: the byte must be fed through a register
/// narrower than one input byte.
#[test]
fn crc6_g704_check_value() {
    assert_eq!(eval_int(&format!(r#"crc6_itu("{CHECK}")"#)), 0x06);
}

#[test]
fn crc6_g704_stays_within_six_bits() {
    let code = r#"
        my $ok = 1;
        for my $s ("", "a", "Z", "hello world", "\x01\x02\x03") {
            my $c = crc6_itu($s);
            $ok = 0 if $c < 0 || $c > 63;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn crc10_atm_check_value() {
    assert_eq!(eval_int(&format!(r#"crc10_atm("{CHECK}")"#)), 0x199);
}

#[test]
fn crc12_dect_check_value() {
    assert_eq!(eval_int(&format!(r#"crc12_dect("{CHECK}")"#)), 0xF5B);
}

#[test]
fn crc32_bzip2_check_value() {
    assert_eq!(eval_int(&format!(r#"crc32_bzip2("{CHECK}")"#)), 0xFC891918);
}

#[test]
fn crc32_jamcrc_check_value() {
    assert_eq!(eval_int(&format!(r#"crc32_jamcrc("{CHECK}")"#)), 0x340BC6D9);
}

/// JAMCRC is ISO-HDLC CRC-32 without the final inversion, so the two must
/// be bitwise complements of each other for every input.
#[test]
fn crc32_jamcrc_is_complement_of_crc32() {
    let code = r#"
        my $ok = 1;
        for my $s ("", "a", "123456789", "The quick brown fox") {
            $ok = 0 unless (crc32_jamcrc($s) ^ crc32($s)) == 0xFFFFFFFF;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn crc32_mpeg2_check_value() {
    assert_eq!(eval_int(&format!(r#"crc32_mpeg2("{CHECK}")"#)), 0x0376E6E7);
}

/// MPEG-2 and BZIP2 share poly/init/no-reflection; BZIP2 adds xorout
/// 0xFFFFFFFF.
#[test]
fn crc32_bzip2_is_complement_of_mpeg2() {
    let code = r#"
        my $ok = 1;
        for my $s ("", "x", "123456789", "abcdefghijklmnop") {
            $ok = 0 unless (crc32_bzip2($s) ^ crc32_mpeg2($s)) == 0xFFFFFFFF;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn crc32_xfer_check_value() {
    assert_eq!(eval_int(&format!(r#"crc32_xfer("{CHECK}")"#)), 0xBD0BE338);
}

/// Zero init and zero xorout: the CRC of the empty message is the init.
#[test]
fn crc_empty_input_returns_init_xor_xorout() {
    assert_eq!(eval_int(r#"crc10_atm("")"#), 0);
    assert_eq!(eval_int(r#"crc24("")"#), 0xB704CE);
    assert_eq!(eval_int(r#"crc32_mpeg2("")"#), 0xFFFFFFFF);
    assert_eq!(eval_int(r#"crc32_bzip2("")"#), 0);
}

// ── Adler-32 combine ───────────────────────────────────────────────

#[test]
fn adler32_combine_matches_concatenation() {
    let code = r#"
        sprintf("%08x", adler32_combine(adler32("Wiki"), adler32("pedia"), 5))
    "#;
    assert_eq!(eval_string(code), "11e60398");
}

#[test]
fn adler32_combine_zero_length_tail_returns_first() {
    assert_eq!(
        eval_int(r#"adler32_combine(adler32("abc"), adler32(""), 0)"#),
        eval_int(r#"adler32("abc")"#)
    );
}

/// A 1342-byte tail of 'A' after "a" drives the intermediate
/// `sum2 + hi(adler_b) - len_b` below zero; zlib adds BASE before
/// subtracting. Expected value is Perl's Adler-32 of the concatenation.
#[test]
fn adler32_combine_handles_intermediate_underflow() {
    let code = r#"
        my $b = "A" x 1342;
        adler32_combine(adler32("a"), adler32($b), 1342)
    "#;
    assert_eq!(eval_int(code), 4277163311);
}

#[test]
fn adler32_combine_agrees_with_direct_for_many_splits() {
    let code = r#"
        my $s = "The quick brown fox jumps over the lazy dog" x 40;
        my $ok = 1;
        for my $cut (0, 1, 7, 100, 999, length($s) - 1) {
            my ($x, $y) = (substr($s, 0, $cut), substr($s, $cut));
            $ok = 0 unless adler32_combine(adler32($x), adler32($y), length $y) == adler32($s);
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Fletcher (Wikipedia test vectors) ──────────────────────────────

#[test]
fn fletcher16_wikipedia_vectors() {
    assert_eq!(eval_int(r#"fletcher16("abcde")"#), 0xC8F0);
    assert_eq!(eval_int(r#"fletcher16("abcdef")"#), 0x2057);
    assert_eq!(eval_int(r#"fletcher16("abcdefgh")"#), 0x0627);
}

#[test]
fn fletcher32_wikipedia_vectors() {
    assert_eq!(eval_int(r#"fletcher32("abcde")"#), 0xF04FC729);
    assert_eq!(eval_int(r#"fletcher32("abcdef")"#), 0x56502D2A);
    assert_eq!(eval_int(r#"fletcher32("abcdefgh")"#), 0xEBE19591);
}

/// "abcde" and "abcdef" end in a partial 32-bit word that must be
/// zero-padded and summed, not dropped.
#[test]
fn fletcher64_wikipedia_vectors() {
    assert_eq!(
        eval_string(r#"sprintf("%016x", fletcher64("abcde"))"#),
        "c8c6c527646362c6"
    );
    assert_eq!(
        eval_string(r#"sprintf("%016x", fletcher64("abcdef"))"#),
        "c8c72b276463c8c6"
    );
    assert_eq!(
        eval_string(r#"sprintf("%016x", fletcher64("abcdefgh"))"#),
        "312e2b28cccac8c6"
    );
}

#[test]
fn fletcher_empty_input_is_zero() {
    assert_eq!(eval_int(r#"fletcher16("")"#), 0);
    assert_eq!(eval_int(r#"fletcher32("")"#), 0);
    assert_eq!(eval_int(r#"fletcher64("")"#), 0);
}

// ── Base58Check ────────────────────────────────────────────────────

#[test]
fn base58check_ascii_payload_reference() {
    assert_eq!(eval_string(r#"base58check_encode("hello")"#), "2L5B5yqsVG8Vt");
}

/// Empty payload encodes the 4 checksum bytes of SHA256d("").
#[test]
fn base58check_empty_payload() {
    assert_eq!(eval_string(r#"base58check_encode("")"#), "3QJmnh");
}

/// Each leading zero byte becomes one leading '1'.
#[test]
fn base58check_leading_zero_bytes_become_ones() {
    assert_eq!(eval_string(r#"base58check_encode("\x00\x00abc")"#), "114h3c8kNw2W");
}

#[test]
fn base58check_roundtrip_and_tamper_detection() {
    let code = r#"
        my $ok = 1;
        for my $s ("", "a", "hello", "\x00lead", "The quick brown fox") {
            $ok = 0 unless base58check_decode(base58check_encode($s)) eq $s;
        }
        # Flip the last character: checksum must reject it.
        my $e = base58check_encode("hello");
        my $last = substr($e, -1);
        my $bad = substr($e, 0, -1) . ($last eq "2" ? "3" : "2");
        $ok = 0 if defined base58check_decode($bad);
        # '0', 'O', 'I', 'l' are not in the alphabet.
        $ok = 0 if defined base58check_decode("0OIl");
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Z85 (ZeroMQ RFC 32) ────────────────────────────────────────────

#[test]
fn z85_rejects_lengths_not_multiple_of_4_and_5() {
    assert_eq!(eval_int(r#"defined(z85_encode("abc")) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"defined(z85_decode("abcd")) ? 1 : 0"#), 0);
}

#[test]
fn z85_all_zero_word_encodes_to_five_zeros() {
    assert_eq!(eval_string(r#"z85_encode("\x00\x00\x00\x00")"#), "00000");
}

#[test]
fn z85_ascii_roundtrip() {
    let code = r#"
        my $ok = 1;
        for my $s ("", "abcd", "HelloWorld!!", "0123456789abcdefghij") {
            my $e = z85_encode($s);
            $ok = 0 unless length($e) == length($s) / 4 * 5;
            $ok = 0 unless z85_decode($e) eq $s;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── basE91 ─────────────────────────────────────────────────────────

/// Every tail length (the encoder emits one or two trailing symbols
/// depending on leftover bits) must round-trip.
#[test]
fn base91_roundtrip_every_length() {
    let code = r#"
        my $src = "The quick brown fox jumps over the lazy dog 0123456789";
        my $ok = 1;
        for my $n (0:length($src)) {
            my $s = substr($src, 0, $n);
            $ok = 0 unless base91_decode(base91_encode($s)) eq $s;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base91_and_base_e91_alias_agree() {
    assert_eq!(
        eval_int(r#"base91_encode("hello world") eq basE91_encode("hello world") ? 1 : 0"#),
        1
    );
}

// ── American Soundex (Wikipedia worked examples) ───────────────────

#[test]
fn soundex_v1_wikipedia_examples() {
    for (name, want) in [
        ("Robert", "R163"),
        ("Rupert", "R163"),
        ("Rubin", "R150"),
        ("Ashcraft", "A261"),
        ("Ashcroft", "A261"),
        ("Tymczak", "T522"),
        ("Pfister", "P236"),
        ("Honeyman", "H555"),
    ] {
        assert_eq!(
            eval_string(&format!(r#"soundex_v1("{name}")"#)),
            want,
            "soundex_v1({name})"
        );
    }
}

#[test]
fn soundex_v1_case_insensitive_and_pads() {
    assert_eq!(eval_string(r#"soundex_v1("robert")"#), "R163");
    assert_eq!(eval_string(r#"soundex_v1("Lee")"#), "L000");
    assert_eq!(eval_string(r#"soundex_v1("")"#), "");
}
