//! Tests for the embedding surface on `VMHelper`: binding a scalar from Rust and
//! capturing what the program writes, instead of letting it reach the process
//! streams.
//!
//! An embedder that owns the terminal (a TUI) previously had only
//! `suppress_stdout`, which throws output away — so a stryke program could be
//! run safely but never usefully, since its `print` was simply lost.

use stryke::vm_helper::VMHelper;

fn run(code: &str, bindings: &[(&str, &str)]) -> (bool, String) {
    let mut vm = VMHelper::new();
    for (name, text) in bindings {
        vm.bind_scalar(name, text);
    }
    vm.begin_capture();
    let ok = stryke::parse_and_run_string(code, &mut vm).is_ok();
    (ok, vm.end_capture())
}

/// `print` output is captured rather than discarded — the whole point of
/// capture over `suppress_stdout`.
#[test]
fn print_output_is_captured() {
    let (ok, out) = run(r#"print "hello\n";"#, &[]);
    assert!(ok, "program failed");
    assert_eq!(out, "hello\n");
}

/// A scalar bound from Rust is an ordinary `$name` the program can read, so an
/// embedder need not splice an escaped literal into the source.
#[test]
fn a_bound_scalar_is_readable() {
    let (ok, out) = run(r#"print uc($stdin);"#, &[("stdin", "hi there")]);
    assert!(ok, "program failed");
    assert_eq!(out, "HI THERE");
}

/// `say` and `printf` funnel through the same sink as `print`.
#[test]
fn say_and_printf_are_captured_too() {
    let (ok, out) = run(r#"say "a"; printf "%s-%d\n", "b", 2;"#, &[]);
    assert!(ok, "program failed");
    assert_eq!(out, "a\nb-2\n");
}

/// Capture is per-run on a persistent VM: ending it returns the text and leaves
/// the VM ready for the next run.
#[test]
fn capture_resets_between_runs() {
    let mut vm = VMHelper::new();
    vm.begin_capture();
    let _ = stryke::parse_and_run_string(r#"print "one";"#, &mut vm);
    let first = vm.end_capture();
    vm.begin_capture();
    let _ = stryke::parse_and_run_string(r#"print "two";"#, &mut vm);
    let second = vm.end_capture();
    assert_eq!(first, "one");
    assert_eq!(second, "two");
}

/// With capture off, `capturing()` reports it and `end_capture` is empty — the
/// standalone path writes the process streams as before.
#[test]
fn capture_is_off_by_default() {
    let mut vm = VMHelper::new();
    assert!(!vm.capturing());
    assert_eq!(vm.end_capture(), "");
}
