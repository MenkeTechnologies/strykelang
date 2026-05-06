//! Fuzz `pack` / `unpack` template strings.
//!
//! `perl_pack` and `perl_unpack` parse a compact template language (`a4 N* x*`,
//! repetition, slash-counts, group nesting) — small surface, dense decision tree,
//! exactly the shape libfuzzer is good at exploring.
//!
//! Input layout:
//!   first byte = number of payload values
//!   second byte = 0 → fuzz pack, 1 → fuzz unpack
//!   remaining bytes are split: first chunk used as the template, rest as payload
//!
//! Run under cargo-fuzz:
//!   cargo +nightly fuzz run pack

#![no_main]

use libfuzzer_sys::fuzz_target;
use stryke::pack::{perl_pack, perl_unpack};
use stryke::value::PerlValue;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let n_values = (data[0] & 0x0F) as usize;
    let mode = data[1];
    let body = &data[2..];
    let split = (body.len() / 2).min(256);
    let template_bytes = &body[..split];
    let payload = &body[split..];

    let Ok(template) = std::str::from_utf8(template_bytes) else {
        return;
    };

    if mode & 1 == 0 {
        // pack: first arg is template, then `n_values` payload values (alternate
        // strings and ints derived from `payload`).
        let mut args = Vec::with_capacity(1 + n_values);
        args.push(PerlValue::string(template.to_string()));
        for i in 0..n_values {
            let chunk = payload.get(i * 4..(i + 1) * 4).unwrap_or(payload);
            if chunk.is_empty() {
                args.push(PerlValue::integer(0));
            } else if i & 1 == 0 {
                let v = i32::from_le_bytes([
                    *chunk.first().unwrap_or(&0),
                    *chunk.get(1).unwrap_or(&0),
                    *chunk.get(2).unwrap_or(&0),
                    *chunk.get(3).unwrap_or(&0),
                ]);
                args.push(PerlValue::integer(v as i64));
            } else {
                args.push(PerlValue::string(
                    String::from_utf8_lossy(chunk).into_owned(),
                ));
            }
        }
        let _ = perl_pack(&args, 0);
    } else {
        // unpack: template + a single payload string.
        let payload_str = String::from_utf8_lossy(payload).into_owned();
        let args = vec![
            PerlValue::string(template.to_string()),
            PerlValue::string(payload_str),
        ];
        let _ = perl_unpack(&args, 0);
    }
});
