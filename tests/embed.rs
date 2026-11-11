//! `stryke::run_program`: stdout capture, stdin feeding, error mapping, and
//! per-call isolation across threads.

use stryke::run_program;

#[test]
fn captures_stdout() {
    assert_eq!(
        run_program(r#"p "hello"; printf "%d\n", 7"#, "").unwrap(),
        "hello\n7\n"
    );
}

#[test]
fn stdin_reads_input() {
    let src = r#"while (my $l = <STDIN>) { chomp $l; p uc $l }"#;
    assert_eq!(run_program(src, "ab\ncd\n").unwrap(), "AB\nCD\n");
}

#[test]
fn errors_and_exit_status() {
    assert!(run_program(r#"die "boom\n""#, "")
        .unwrap_err()
        .contains("boom"));
    assert!(run_program("p (", "").is_err());
    assert!(run_program(r#"p "x"; exit 3"#, "").is_err());
    assert_eq!(run_program(r#"p "x"; exit 0; p "y""#, "").unwrap(), "x\n");
}

#[test]
fn concurrent_calls_do_not_mix_output() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let src = format!(r#"for (1:200) {{ p "{i}" }}"#);
                (i, run_program(&src, "").unwrap())
            })
        })
        .collect();
    for h in handles {
        let (i, out) = h.join().unwrap();
        assert_eq!(out, format!("{i}\n").repeat(200));
    }
}

/// Run `src` and return its output lines sorted (worker order is up to the
/// scheduler). In the parent process, first re-run this test binary filtered to
/// `test_name` as a child and assert none of `expected` reached the child's
/// real stdout — the only way to observe a leak to fd 1 from inside a test.
fn sorted_lines_without_leak(test_name: &str, src: &str, expected: &[&str]) -> Vec<String> {
    if std::env::var_os("STRYKE_EMBED_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env("STRYKE_EMBED_CHILD", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(child.status.success(), "child failed:\n{stdout}");
        assert!(stdout.contains("1 passed"), "child ran no test:\n{stdout}");
        for leaked in expected {
            assert!(
                !stdout.lines().any(|l| l == *leaked),
                "{leaked} reached real stdout:\n{stdout}"
            );
        }
    }
    let mut lines: Vec<String> = run_program(src, "")
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

/// Worker interpreters (`pmap` on rayon, `async` on an OS thread) write into
/// the call's buffer, not the process stdout.
#[test]
fn parallel_worker_output_is_captured() {
    let src = r#"my @r = pmap { p "w$_"; $_ } 1:3; my $t = async { p "a1" }; await $t"#;
    let expected = ["a1", "w1", "w2", "w3"];
    let lines = sorted_lines_without_leak("parallel_worker_output_is_captured", src, &expected);
    assert_eq!(lines, expected);
}

/// A rayon-parallel builtin inside an OS-thread worker: the `pmap` tasks are
/// spawned from the `async` thread, which carries the call's I/O to them.
#[test]
fn nested_parallel_output_is_captured() {
    let src = r#"my $t = async { my @r = pmap { p "n$_"; $_ } 1:2; 1 }; await $t"#;
    let expected = ["n1", "n2"];
    let lines = sorted_lines_without_leak("nested_parallel_output_is_captured", src, &expected);
    assert_eq!(lines, expected);
}

/// Rayon threads are shared across calls, so each task must restore the
/// thread's previous I/O when it ends: concurrent calls running `pmap` on the
/// same pool each get exactly their own worker lines.
#[test]
fn concurrent_pmap_calls_keep_their_own_output() {
    let handles: Vec<_> = (0..6)
        .map(|i| {
            std::thread::spawn(move || {
                let src = format!(r#"my @r = pmap {{ p "c{i}-$_"; $_ }} 1:50"#);
                (i, run_program(&src, "").unwrap())
            })
        })
        .collect();
    for h in handles {
        let (i, out) = h.join().unwrap();
        let mut got: Vec<&str> = out.lines().collect();
        got.sort();
        let mut want: Vec<String> = (1..=50).map(|n| format!("c{i}-{n}")).collect();
        want.sort();
        assert_eq!(got, want, "call {i}");
    }
}
