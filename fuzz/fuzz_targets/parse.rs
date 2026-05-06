//! Fuzz the lexer + parser.
//!
//! Invariant: any valid UTF-8 byte slice is either parsed successfully or rejected
//! with a `PerlError`. The harness must NEVER panic — including overflow, slice
//! out-of-bounds, unwrap-on-None, integer division, etc. Parser hits during fuzzing
//! consistently expose oversights in the lexer's slice arithmetic and the parser's
//! Pratt operator-precedence dispatch.
//!
//! Run under cargo-fuzz (nightly):
//!   cargo +nightly fuzz run parse
//! Or via the stable smoke test that replays the corpus:
//!   cargo test --test fuzz_smoke

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Cap input size — fuzzer mutators sometimes generate megabyte inputs that
        // burn time without finding new edges. 64 KB keeps each iteration fast and
        // matches the size of the largest .stk fixtures in the corpus.
        if s.len() > 65_536 {
            return;
        }
        let _ = stryke::parse(s);
    }
});
