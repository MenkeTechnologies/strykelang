//! One fusevm in the dependency graph, checked against `Cargo.lock`.
//!
//! fusevm defines ~54 `#[no_mangle]` symbols across its `aot`/`jit`/`ffi`
//! modules. Cargo treats 0.x minors as semver-incompatible, so asking for a
//! minor other than the one `zshrs` itself requires puts two copies in the
//! graph and every binary fails to link on duplicate symbols. Under this
//! crate's `lto = "fat"` release profile the linker error degrades to a bare
//!
//! ```text
//! error: failed to load bitcode of module "fusevm-….fusevm.…-cgu.0.rcgu.o":
//! ```
//!
//! with no reason after the colon and no symbol named — which points at
//! nothing. It happened on 2026-08-29 (`deps: fusevm 0.25.0`), where the
//! direct dependency moved to 0.25.0 while published `zshrs` still required
//! `^0.23.0`.
//!
//! The failure only appears in a LINKING build. `cargo check --all-targets`
//! passes, which is what the breaking commit was verified with, and the
//! release build that catches it is the slowest job in CI. Reading the lock
//! costs microseconds and names the two versions outright.

use std::path::PathBuf;

/// Every `[[package]] name = "<crate>"` version recorded in `Cargo.lock`.
fn locked_versions(lock: &str, crate_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = lock.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() != "[[package]]" {
            continue;
        }
        let mut name = None;
        let mut version = None;
        for field in lines.by_ref() {
            let field = field.trim();
            if field.is_empty() {
                break;
            }
            if let Some(v) = field.strip_prefix("name = ") {
                name = Some(v.trim_matches('"').to_string());
            } else if let Some(v) = field.strip_prefix("version = ") {
                version = Some(v.trim_matches('"').to_string());
            }
        }
        if name.as_deref() == Some(crate_name) {
            if let Some(version) = version {
                out.push(version);
            }
        }
    }
    out
}

#[test]
fn cargo_lock_resolves_exactly_one_fusevm() {
    let lock =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"))
            .expect("Cargo.lock is committed at the crate root");
    let found = locked_versions(&lock, "fusevm");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one fusevm in Cargo.lock, found {}: {found:?} — \
         `zshrs` and the direct `fusevm` dependency must name the same 0.x \
         minor, or the binary carries two copies of ~54 #[no_mangle] symbols \
         and fails to link",
        found.len()
    );
}

/// The parser itself: a lock naming two minors of a crate must be reported as
/// two, or the check above passes vacuously on a file it silently misread.
#[test]
fn the_lock_parser_sees_both_copies_when_there_are_two() {
    let sample = "\
[[package]]
name = \"fusevm\"
version = \"0.23.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"serde\"
version = \"1.0.0\"

[[package]]
name = \"fusevm\"
version = \"0.25.0\"
dependencies = [
 \"libc\",
]
";
    assert_eq!(locked_versions(sample, "fusevm"), ["0.23.0", "0.25.0"]);
    assert_eq!(locked_versions(sample, "serde"), ["1.0.0"]);
    assert!(locked_versions(sample, "not-a-package").is_empty());
}
