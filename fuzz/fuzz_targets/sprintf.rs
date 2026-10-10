//! Fuzz `sprintf` format strings.
//!
//! The format parser handles flags, `*` widths and precisions, `%N$` positional
//! arguments, vector flags and a long list of conversions, so arbitrary bytes
//! reach many distinct branches. The format is embedded as hex through
//! `pack("H*", ...)` so any byte sequence is representable without quoting
//! concerns, and the argument list mixes integers, floats, strings and undef.
//!
//! The harness asserts only that execution doesn't panic.
//!
//! Run under cargo-fuzz:
//!   cargo +nightly fuzz run sprintf

#![no_main]

use libfuzzer_sys::fuzz_target;
use stryke::vm_helper::VMHelper;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 {
        return;
    }
    // A width or precision of 7-10 digits is legal (up to INT_MAX) and only
    // allocates; skip those. Longer runs exceed INT_MAX and die immediately.
    if data.split(|b| !b.is_ascii_digit()).any(|run| (7..=10).contains(&run.len())) {
        return;
    }
    let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
    let src = format!(
        "my $f = pack(\"H*\", \"{hex}\"); \
         my $r = sprintf($f, 42, \"str\", -2.5, undef, 1e20, \"0x1f\"); \
         my @l = sprintf($f, 1, 2, 3);"
    );
    let Ok(program) = stryke::parse(&src) else {
        return;
    };
    let mut interp = VMHelper::new();
    interp.suppress_stdout = true;
    let _ = stryke::try_vm_execute(&program, &mut interp);
});
