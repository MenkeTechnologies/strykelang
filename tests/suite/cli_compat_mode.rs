use std::process::Command;

#[test]
fn pe_compat_mode_extensions_are_errors() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .args(["--compat", "-e", "collect(1, 2, 3);"])
        .output()
        .expect("spawn pe");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("perlrs extension"));
}

#[test]
fn pe_compat_mode_udf_shadowing_works() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .args(["--compat", "-e", "sub collect { 42 } print collect();"])
        .output()
        .expect("spawn pe");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn pe_no_compat_mode_extensions_work() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .args(["-e", "print scalar collect(1, 2, 3);"])
        .output()
        .expect("spawn pe");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "3");
}
