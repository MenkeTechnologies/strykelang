//! Stable-Rust smoke test for the cargo-fuzz harnesses.
//!
//! cargo-fuzz needs nightly — these tests replay each fuzz target's logic on the
//! committed seed corpus under stable so regressions in the fuzz scaffolding are
//! caught by `cargo test` in CI. Each target asserts only that the call doesn't
//! panic; output and error variants are not constrained.
//!
//! When adding a new fuzz target under `fuzz/fuzz_targets/`, add a corresponding
//! `<target>_corpus_does_not_panic` test here that mirrors its logic.

use std::fs;
use std::path::PathBuf;
use stryke::compiler::Compiler;
use stryke::pack::{perl_pack, perl_unpack};
use stryke::value::PerlValue;
use stryke::vm_helper::VMHelper;

fn corpus_dir(target: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("fuzz");
    p.push("corpus");
    p.push(target);
    p
}

fn read_corpus(target: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = corpus_dir(target);
    if !dir.exists() {
        return Vec::new();
    }
    fs::read_dir(&dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| {
            let path = e.path();
            let bytes = fs::read(&path).expect("read corpus entry");
            (path, bytes)
        })
        .collect()
}

#[test]
fn parse_corpus_does_not_panic() {
    let entries = read_corpus("parse");
    assert!(!entries.is_empty(), "fuzz/corpus/parse must have seed files");
    for (path, bytes) in entries {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            // Mirror parse.rs harness exactly — `parse` returns Result, we only
            // assert no panic. Any parse error is acceptable.
            let _ = stryke::parse(s);
        } else {
            panic!("corpus seed must be valid UTF-8: {}", path.display());
        }
    }
}

#[test]
fn compile_corpus_does_not_panic() {
    for (path, bytes) in read_corpus("compile") {
        let s = std::str::from_utf8(&bytes)
            .unwrap_or_else(|_| panic!("non-utf8 seed: {}", path.display()));
        let Ok(program) = stryke::parse(s) else {
            continue;
        };
        let _ = Compiler::new().compile_program(&program);
    }
}

#[test]
fn eval_corpus_does_not_panic() {
    for (path, bytes) in read_corpus("eval") {
        let s = std::str::from_utf8(&bytes)
            .unwrap_or_else(|_| panic!("non-utf8 seed: {}", path.display()));
        let Ok(program) = stryke::parse(s) else {
            continue;
        };
        let mut interp = VMHelper::new();
        interp.suppress_stdout = true;
        let _ = stryke::try_vm_execute(&program, &mut interp);
    }
}

#[test]
fn pack_corpus_does_not_panic() {
    // Replicates the pack.rs harness layout: byte 0 = n_values, byte 1 = mode,
    // remaining bytes split between template and payload.
    for (path, bytes) in read_corpus("pack") {
        let _ = path; // path retained for failure context only
        if bytes.len() < 4 {
            continue;
        }
        let n_values = (bytes[0] & 0x0F) as usize;
        let mode = bytes[1];
        let body = &bytes[2..];
        let split = (body.len() / 2).min(256);
        let template_bytes = &body[..split];
        let payload = &body[split..];
        let Ok(template) = std::str::from_utf8(template_bytes) else {
            continue;
        };
        if mode & 1 == 0 {
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
            let payload_str = String::from_utf8_lossy(payload).into_owned();
            let args = vec![
                PerlValue::string(template.to_string()),
                PerlValue::string(payload_str),
            ];
            let _ = perl_unpack(&args, 0);
        }
    }
}
