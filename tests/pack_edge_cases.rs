//! Hand-crafted edge-case pins for `stryke::pack::{perl_pack, perl_unpack}`.
//!
//! The 30 in-module tests in `strykelang/pack.rs` cover the happy paths
//! (round-trips for each format letter, `*` consumption, basic short-buffer
//! errors). The tests below target specific bug classes those do NOT pin:
//!
//! 1. Explicit count `0` (`C0`, `x0`) must emit ZERO bytes. The tokenizer
//!    doc-comment (pack.rs:72-76) records a past `.max(1)` bug where `C0`
//!    wrongly emitted one byte and `x0` a stray NUL. No in-module test pins
//!    the zero-count behavior, so a regression to `.max(1)` would pass CI.
//!
//! 2. `pack "w", <negative>` must ERROR ("Cannot compress negative numbers
//!    in pack") rather than sign-launder via `as u64` into a ~10-byte BER
//!    blob (pack.rs:270-277). No in-module test exercises the negative path.
//!
//! 3. `pack "H4", "ab"` with an explicit count exceeding the available hex
//!    digits must error "hex string too short" (pack.rs:168-170), not silently
//!    truncate or panic-slice. Off-by-one / slice-OOB bug class.
//!
//! 4. `Z` with an explicit count must always NUL-terminate within the field
//!    even when the string is longer than the field (pack.rs:149-153 forces
//!    `b[max-1] = 0`), and unpack must stop at the first embedded NUL. This
//!    is the C-string-truncation invariant; a bug here leaks unterminated
//!    data past the field boundary.

use std::sync::Arc;

use stryke::pack::{perl_pack, perl_unpack};
use stryke::value::StrykeValue;

/// Bug class 1: explicit count `0` emits zero bytes for both the count-style
/// (`C0`) and skip-style (`x0`) operators. Pins the post-`.max(1)`-fix
/// behavior — a regression that coerced `0 -> 1` would make `C0,1,2` emit one
/// byte and `x0` emit a NUL, both caught here.
#[test]
fn pack_explicit_zero_count_emits_no_bytes() {
    // C0 with two trailing args: zero bytes, args not consumed by C0.
    let out = perl_pack(
        &[
            StrykeValue::string("C0C".to_string()),
            StrykeValue::integer(65),
        ],
        1,
    )
    .expect("pack C0C should succeed");
    let bytes = out.as_bytes_arc().expect("pack returns bytes");
    // C0 emits nothing; the trailing C consumes the single arg 65 -> 'A'.
    assert_eq!(
        &**bytes, b"A",
        "C0 must emit zero bytes, leaving the arg for the following C"
    );

    // x0 must emit zero padding NULs.
    let out =
        perl_pack(&[StrykeValue::string("x0".to_string())], 1).expect("pack x0 should succeed");
    let bytes = out.as_bytes_arc().expect("pack returns bytes");
    assert!(
        bytes.is_empty(),
        "x0 must emit zero NUL bytes, got {:?}",
        &**bytes
    );
}

/// Bug class 2: BER-compressed `w` of a negative integer must be a hard error,
/// not a sign-laundered 10-byte encoding of `0xFFFF_FFFF_FFFF_FFFF`.
#[test]
fn pack_w_negative_errors_instead_of_sign_laundering() {
    let err = perl_pack(
        &[
            StrykeValue::string("w".to_string()),
            StrykeValue::integer(-1),
        ],
        1,
    )
    .expect_err("pack w on a negative integer must error");
    let msg = err.to_string();
    assert!(
        msg.contains("Cannot compress negative numbers"),
        "expected negative-BER error, got: {msg}"
    );
}

/// Bug class 3: an explicit `H` count larger than the supplied hex digits is a
/// "too short" error, exercised at a count > 1 so it cannot be confused with
/// the `Repeat::One` empty-string branch. Guards the `hex[..n]` slice from OOB.
#[test]
fn pack_h_count_exceeding_input_errors_not_panics() {
    // "ab" yields 2 hex digits; H5 demands 5 -> error, must not panic-slice.
    let err = perl_pack(
        &[
            StrykeValue::string("H5".to_string()),
            StrykeValue::string("ab".to_string()),
        ],
        1,
    )
    .expect_err("H5 with 2 hex digits must error");
    assert!(
        err.to_string().contains("hex string too short"),
        "expected 'hex string too short', got: {}",
        err
    );
}

/// Bug class 4: `Z<n>` packs the string into exactly `n` bytes and FORCES a
/// terminating NUL at `b[n-1]` even when the source is longer than the field,
/// and `unpack "Z<n>"` must stop at the first embedded NUL (C-string semantics)
/// rather than returning the whole field. This pins the truncation invariant
/// across a pack->unpack round-trip where the source overflows the field.
#[test]
fn pack_z_count_truncates_with_forced_terminator_roundtrip() {
    // "ABCDEF" (6 bytes) into Z4: bytes are 'A','B','C',0 — last byte forced NUL.
    let packed = perl_pack(
        &[
            StrykeValue::string("Z4".to_string()),
            StrykeValue::string("ABCDEF".to_string()),
        ],
        1,
    )
    .expect("pack Z4 should succeed");
    let bytes = packed.as_bytes_arc().expect("pack returns bytes");
    assert_eq!(
        &**bytes, b"ABC\0",
        "Z4 must hard-terminate the truncated field at byte index 3"
    );

    // unpack Z4 reads the 4-byte field, stops at the embedded NUL -> "ABC".
    let unpacked = perl_unpack(
        &[
            StrykeValue::string("Z4".to_string()),
            StrykeValue::bytes(Arc::new(bytes.to_vec())),
        ],
        1,
    )
    .expect("unpack Z4 should succeed");
    let s = unpacked.as_str().expect("unpack Z4 yields a scalar string");
    assert_eq!(
        s, "ABC",
        "unpack Z4 must terminate at the embedded NUL, not return the raw field"
    );
}
