//! Published test vectors for the hash / KDF / PRNG builtins in
//! `strykelang/math_wolfram_cryptography_hash_prng_cipher.rs`.
//!
//! Sources: FNV reference vectors (isthe.com/chongo/tech/comp/fnv),
//! SMHasher MurmurHash3_x86_32 vectors and xxHash32 values (both names resolve
//! to the `native_codec` hex-string implementations), cross-checked with
//! `xxhsum -H32`, RFC 6070 PBKDF2-HMAC-SHA1 vectors, and std's
//! `SipHasher` (SipHash-2-4) as an in-process reference implementation.

use crate::common::*;

fn u32_of(code: &str) -> u32 {
    let v = eval_int(code);
    assert!((0..=u32::MAX as i64).contains(&v), "{code} => {v} not a u32");
    v as u32
}

fn u64_of(code: &str) -> u64 {
    eval_int(code) as u64
}

#[test]
fn fnv1a_32_reference_vectors() {
    assert_eq!(u32_of(r#"fnv1a_32("")"#), 0x811c9dc5);
    assert_eq!(u32_of(r#"fnv1a_32("a")"#), 0xe40c292c);
    assert_eq!(u32_of(r#"fnv1a_32("foobar")"#), 0xbf9cf968);
}

#[test]
fn fnv1a_64_reference_vectors() {
    assert_eq!(u64_of(r#"fnv1a_64("")"#), 0xcbf29ce484222325);
    assert_eq!(u64_of(r#"fnv1a_64("a")"#), 0xaf63dc4c8601ec8c);
    assert_eq!(u64_of(r#"fnv1a_64("foobar")"#), 0x85944171f73967e8);
}

#[test]
fn sdbm_hand_computed() {
    assert_eq!(u32_of(r#"sdbm_hash("")"#), 0);
    assert_eq!(u32_of(r#"sdbm_hash("a")"#), 97);
    // h("ab") = 'b' + (97 << 6) + (97 << 16) - 97
    assert_eq!(u32_of(r#"sdbm_hash("ab")"#), 98 + (97 << 6) + (97 << 16) - 97);
}

#[test]
fn murmur3_32_smhasher_vectors() {
    // `murmur3_32` dispatches to `native_codec::murmur3_32`, which returns 8 hex chars.
    assert_eq!(eval_string(r#"murmur3_32("", 0)"#), "00000000");
    assert_eq!(eval_string(r#"murmur3_32("", 1)"#), "514e28b7");
    assert_eq!(eval_string(r#"murmur3_32("test", 0)"#), "ba6bd213");
    assert_eq!(eval_string(r#"murmur3_32("Hello, world!", 1234)"#), "faf6cdb3");
    assert_eq!(
        eval_string(r#"murmur3("The quick brown fox jumps over the lazy dog")"#),
        "2e4ff723"
    );
}

#[test]
fn xxhash32_matches_xxhsum() {
    // `xxhash32` dispatches to `native_codec::xxh32` (8 hex chars). Covers the
    // < 16-byte path, the 4-byte lane tail, and the 16-byte stripe loop.
    assert_eq!(eval_string(r#"xxhash32("", 0)"#), "02cc5d05");
    assert_eq!(eval_string(r#"xxhash32("a")"#), "550d7456");
    assert_eq!(eval_string(r#"xxh32("abc")"#), "32d153ff");
    assert_eq!(
        eval_string(r#"xxhash32("Nobody inspects the spammish repetition", 0)"#),
        "e2293b2f"
    );
    assert_eq!(
        eval_string(r#"xxh32("The quick brown fox jumps over the lazy dog")"#),
        "e85ea4de"
    );
}

#[test]
#[allow(deprecated)]
fn siphash24_matches_std_reference() {
    use std::hash::{Hasher, SipHasher};
    // Keys must survive the builtin's f64 argument path, so keep them < 2^53.
    let cases: &[(&str, u64, u64)] = &[
        ("", 0, 0),
        ("a", 0, 0),
        ("abcdefg", 0, 0),          // 7 bytes: tail only
        ("abcdefgh", 0, 0),         // exactly one block
        ("hello, siphash world!", 0, 0), // two blocks + tail
        ("keyed", 0x0706050403020100, 0x0f0e0d0c0b0a),
    ];
    for &(msg, k0, k1) in cases {
        let mut h = SipHasher::new_with_keys(k0, k1);
        h.write(msg.as_bytes());
        let got = u64_of(&format!(r#"siphash24("{msg}", {k0}, {k1})"#));
        assert_eq!(got, h.finish(), "siphash24({msg:?}, {k0:#x}, {k1:#x})");
    }
}

#[test]
fn pbkdf2_hmac_sha1_rfc6070() {
    // First 8 bytes (big-endian) of the RFC 6070 derived keys.
    assert_eq!(u64_of(r#"pbkdf2_hmac_step("password", "salt", 1)"#), 0x0c60c80f961f0e71);
    assert_eq!(u64_of(r#"pbkdf2_hmac_step("password", "salt", 2)"#), 0xea6c014dc72d6f8c);
    assert_eq!(u64_of(r#"pbkdf2_hmac_step("password", "salt", 4096)"#), 0x4b007901b765489a);
    // Iteration count is clamped to >= 1.
    assert_eq!(
        eval_int(r#"pbkdf2_hmac_step("password", "salt", 0)"#),
        eval_int(r#"pbkdf2_hmac_step("password", "salt", 1)"#)
    );
}

#[test]
fn bcrypt_cost_is_power_of_two_clamped() {
    assert_eq!(eval_int("bcrypt_cost_iters(10)"), 1024);
    assert_eq!(eval_int("bcrypt_cost_iters(1)"), 16);
    assert_eq!(eval_int("bcrypt_cost_iters(99)"), 1 << 31);
}

#[test]
fn xorshift_marsaglia_first_step() {
    // x = 1: hand-unrolled shifts from Marsaglia (2003).
    assert_eq!(eval_int("xorshift32(1)"), 270369);
    assert_eq!(eval_int("xorshift64(1)"), 1082269761);
    // Zero is the fixed point of every xorshift.
    assert_eq!(eval_int("xorshift32(0)"), 0);
    assert_eq!(eval_int("xorshift64(0)"), 0);
}

#[test]
fn splitmix64_reference_output() {
    // First output of splitmix64 seeded with 0 (Vigna's reference).
    assert_eq!(u64_of("splitmix64_step(0)"), 0xe220a8397b1dcdaf);
}

#[test]
fn lcg_numerical_recipes_step() {
    assert_eq!(eval_int("lcg_numrec_step(0)"), 1013904223);
    assert_eq!(eval_int("lcg_numrec_step(1)"), 1664525 + 1013904223);
    // Output is reduced mod 2^32.
    assert_eq!(
        eval_int("lcg_numrec_step(4294967295)"),
        ((4294967295u64 * 1664525 + 1013904223) & 0xffff_ffff) as i64
    );
}

#[test]
fn galois_lfsr_maximal_period_16bit() {
    // Taps 0xB400 give the maximal-length 16-bit LFSR: period 2^16 - 1.
    let code = r#"
        my $s = 1;
        my $n = 0;
        do {
            $s = lfsr_galois_step($s, 0xB400);
            $n++;
        } while ($s != 1 && $n < 70000);
        $n
    "#;
    assert_eq!(eval_int(code), 65535);
}

#[test]
fn mt19937_temper_first_output_for_default_seed() {
    // mt[0] after the first twist for seed 5489 is 0xD091BB5C... tempered to
    // 3499211612, the canonical first MT19937 output.
    assert_eq!(eval_int("mt19937_temper(0)"), 0);
    let first_tempered = eval_int(
        r#"
        my @mt = (5489);
        for my $i (1:623) {
            my $p = $mt[$i - 1];
            push @mt, (1812433253 * ($p ^ ($p >> 30)) + $i) & 0xFFFFFFFF;
        }
        my $y = ($mt[0] & 0x80000000) | ($mt[1] & 0x7FFFFFFF);
        my $v = $mt[397] ^ ($y >> 1);
        $v ^= 0x9908B0DF if $y & 1;
        mt19937_temper($v)
    "#,
    );
    assert_eq!(first_tempered, 3499211612);
}
