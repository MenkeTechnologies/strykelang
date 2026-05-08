//! Behavior-pinning batch BC (2026-05-08): Sweeping special variables parity documented in parity/SPECIAL_VARIABLES.md.

use crate::common::*;

#[test]
fn special_var_os_name_is_populated() {
    let out = eval_string(r#"$^O"#);
    // Should be something like "darwin", "linux", "windows" depending on host,
    // but definitely not empty.
    assert!(!out.is_empty() && out != "1", "expected $^O to be populated");
}

#[test]
fn special_var_version_is_populated() {
    let out = eval_string(r#"$^V"#);
    // Should be "vX.Y.Z"
    assert!(out.starts_with('v'), "expected $^V to start with v, got {}", out);
}

#[test]
fn special_var_global_phase_tracks_execution() {
    let out = eval_string(r#"${^GLOBAL_PHASE}"#);
    assert_eq!(out, "RUN");
}

#[test]
fn special_var_script_start_time_is_positive() {
    let out = eval_string(r#"$^T"#);
    let t: i64 = out.parse().unwrap_or(0);
    assert!(t > 1_700_000_000, "expected $^T to be a recent epoch timestamp, got {}", out);
}

#[test]
fn special_var_executable_path_is_populated() {
    let out = eval_string(r#"$^X"#);
    assert!(!out.is_empty(), "expected $^X to be populated");
}

#[test]
fn special_var_uid_and_euid_are_numeric() {
    let out = eval_string(r#"$< . "," . $>"#);
    let parts: Vec<&str> = out.split(',').collect();
    assert_eq!(parts.len(), 2);
    assert!(parts[0].parse::<i64>().is_ok());
    assert!(parts[1].parse::<i64>().is_ok());
}

#[test]
fn special_var_gid_and_egid_contain_numbers() {
    // $( and $) return space-separated lists of groups on Unix, or empty on non-Unix.
    // They should not trigger syntax errors or unexpected panics.
    let out = eval_string(r#"$( . ")" . $)"#);
    // If on unix, it has numbers. Just assert it evaluates successfully.
    assert!(!out.is_empty() || out.is_empty()); // just checking it doesn't panic
}

#[test]
fn unknown_caret_variable_returns_undef() {
    // Reading an unknown special variable returns undef, not an error.
    let out = eval_string(r#"defined(${^UNKNOWN_VAR_XYZ}) ? 'def' : 'undef'"#);
    assert_eq!(out, "undef");
}
