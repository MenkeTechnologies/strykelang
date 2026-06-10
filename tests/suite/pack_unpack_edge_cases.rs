//! Edge-case tests for `pack` / `unpack` covering bug classes not exercised by the
//! existing unit tests in `pack.rs` or the runtime tests in `pack_unpack_runtime.rs`.
//!
//! Bug classes targeted:
//!   * Signed → unsigned truncation for the small-int formats (`C`, `n`, `S`, `v`).
//!     Off-by-one here would corrupt wire formats silently — Perl masks via
//!     `& 0xff` / `& 0xffff` before emit.
//!   * Tokenizer treats `\t` and `\n` inside a template the same as space.
//!     Regressions here would surface as "unsupported pack type" errors when
//!     users pretty-print multi-line templates.
//!   * `a*` (string, no padding) on `unpack` returns the rest of the buffer
//!     INCLUDING any interior NUL bytes — bug class is `Z`-style truncation
//!     leaking into `a*` decode and dropping bytes after the first NUL.
//!   * `unpack "x"` past end-of-buffer must return a Runtime error, not panic.
//!     The implementation uses `pos = pos.saturating_add(n)` followed by a
//!     `pos > data.len()` check; regression to direct `+=` would panic on
//!     overflow.
//!   * Round-trip for the BER-compressed `w` format on values near the 7-bit
//!     and 14-bit boundaries — these are exactly the bytes where the
//!     continuation-bit encoder/decoder splits or merges chunks.

use std::sync::Arc;
use stryke::error::ErrorKind;
use stryke::pack::{perl_pack, perl_unpack};
use stryke::value::StrykeValue;

fn pack_to_bytes(template: &str, args: &[StrykeValue]) -> Vec<u8> {
    let mut v = vec![StrykeValue::string(template.into())];
    v.extend_from_slice(args);
    let result = perl_pack(&v, 0).expect("pack should succeed");
    result
        .as_bytes_arc()
        .expect("pack produces Bytes")
        .as_ref()
        .clone()
}

fn unpack_vals(template: &str, data: &[u8]) -> Vec<StrykeValue> {
    let result = perl_unpack(
        &[
            StrykeValue::string(template.into()),
            StrykeValue::bytes(Arc::new(data.to_vec())),
        ],
        0,
    )
    .expect("unpack should succeed");
    result.as_array_vec().unwrap_or_else(|| vec![result])
}

/// `pack "C"` masks an out-of-range integer to its low 8 bits. A regression
/// where the cast was changed from `(v & 0xff) as u8` to `v as u8` would still
/// happen to work for positive values (Rust `as u8` truncates) but would
/// emit a different value for negative integers: `-1 as u8 == 255`, which
/// only matches the masked result by coincidence on two's-complement input.
/// The Perl spec is specifically the `& 0xff` semantic — pin it.
#[test]
fn pack_c_masks_value_above_255() {
    assert_eq!(
        pack_to_bytes("C", &[StrykeValue::integer(0x1ff)]),
        vec![0xff]
    );
    assert_eq!(
        pack_to_bytes("C", &[StrykeValue::integer(0x100)]),
        vec![0x00]
    );
    assert_eq!(
        pack_to_bytes("C", &[StrykeValue::integer(256 + 65)]),
        vec![65]
    );
}

/// `pack "n", -1` (big-endian u16) must wrap to `0xff 0xff`. Catches a
/// regression where `as i16` was used (which would still produce the same
/// bytes for `-1` but would mis-handle e.g. `pack "n", 0x8000` by
/// sign-flipping to `0x80 0x00` vs `0x80 0x00` — actually the same. The
/// distinguishing case is large positive: `pack "n", 70_000` (which exceeds
/// i16::MAX). With current code `(v as u16)` truncates the i64 to its low
/// 16 bits → `70000 & 0xffff = 0x1170` → bytes `0x11 0x70`. A regression
/// using `v.try_into::<u16>().unwrap_or(0)` would emit `0 0` silently.
#[test]
fn pack_n_truncates_oversized_positive() {
    let b = pack_to_bytes("n", &[StrykeValue::integer(70_000)]);
    assert_eq!(b, vec![0x11, 0x70], "70000 & 0xffff = 0x1170");
    let b2 = pack_to_bytes("n", &[StrykeValue::integer(-1)]);
    assert_eq!(b2, vec![0xff, 0xff], "wrap to all-ones");
}

/// Tokenizer must skip ASCII whitespace OTHER than the space character.
/// The current implementation uses `is_ascii_whitespace()` which includes
/// `\t`, `\n`, `\r`, and `\x0C` (form feed). Regression to `c == ' '` would
/// surface as `unsupported pack type '\\t'` runtime errors on real-world
/// pretty-printed templates.
#[test]
fn pack_tokenizer_skips_tab_and_newline_in_template() {
    let template = "C\tC\nC";
    let b = pack_to_bytes(
        template,
        &[
            StrykeValue::integer(1),
            StrykeValue::integer(2),
            StrykeValue::integer(3),
        ],
    );
    assert_eq!(b, vec![1, 2, 3]);
}

/// `unpack "a*"` returns ALL remaining bytes, including any interior NULs.
/// This differs from `Z*` which stops at the first NUL. Regression where
/// `a*` was implemented as `Z*` (stop at NUL) would lose the bytes after the
/// first NUL. Use `to_string()` on the resulting value to compare; the
/// underlying StrykeValue::string preserves embedded NULs.
#[test]
fn unpack_a_star_preserves_interior_nuls() {
    let raw: &[u8] = &[b'h', 0, b'i', 0, b'!'];
    let vals = unpack_vals("a*", raw);
    assert_eq!(vals.len(), 1);
    let s = vals[0].to_string();
    let bytes = s.as_bytes();
    assert_eq!(
        bytes, raw,
        "a* must keep every byte including interior NULs; got {:?}",
        bytes
    );
}

/// `unpack "x"` advancing past the end of the buffer must produce a runtime
/// error, not a panic. The code uses `pos.saturating_add(n)` then a bounds
/// check — a regression to `pos += n` would panic on overflow when `n` is
/// near `usize::MAX`. We can't construct `usize::MAX` directly from a
/// template (the tokenizer saturates), but we CAN trigger the `pos > data.len()`
/// branch with a normal-sized count.
#[test]
fn unpack_x_past_end_returns_error_not_panic() {
    let result = perl_unpack(
        &[
            StrykeValue::string("x5".into()),
            StrykeValue::bytes(Arc::new(vec![0u8, 0])),
        ],
        0,
    );
    let err = result.expect_err("unpack x5 with only 2 bytes must error");
    assert_eq!(err.kind, ErrorKind::Runtime);
    assert!(
        err.message.contains("x past end") || err.message.contains("too short"),
        "expected x-out-of-bounds message, got: {}",
        err.message
    );
}

/// Round-trip for BER-compressed integers on the encoder/decoder boundary
/// transitions: 127 fits in one byte, 128 needs two, 16383 fits in two,
/// 16384 needs three. Each boundary stresses both:
///   * encoder's `while v > 0` continuation-bit loop (correctness of stop
///     condition; off-by-one here would either drop the high chunk or emit
///     a spurious leading 0x80),
///   * decoder's `byte & 0x80 == 0` break condition (regression to
///     `byte & 0x80 != 0` would invert termination logic).
#[test]
fn pack_w_round_trip_at_7_and_14_bit_boundaries() {
    for &n in &[0i64, 1, 127, 128, 16383, 16384, 0x1FFFFF, 0x200000] {
        let b = pack_to_bytes("w", &[StrykeValue::integer(n)]);
        let decoded = unpack_vals("w", &b);
        assert_eq!(decoded.len(), 1, "decode {} produces one value", n);
        assert_eq!(
            decoded[0].to_int(),
            n,
            "round-trip for {} failed: bytes={:?}",
            n,
            b
        );
        // Continuation bit invariant: every byte except the last has the
        // high bit set.
        let (last, rest) = b.split_last().expect("at least one byte");
        assert_eq!(last & 0x80, 0, "last byte of {} must clear high bit", n);
        for byte in rest {
            assert_eq!(
                byte & 0x80,
                0x80,
                "non-last byte of {} must set high bit",
                n
            );
        }
    }
}
