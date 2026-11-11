//! `stryke::run_program`: stdout capture, stdin feeding, error mapping, and
//! per-call isolation across threads.

use stryke::run_program;

#[test]
fn captures_stdout() {
    assert_eq!(run_program(r#"p "hello"; printf "%d\n", 7"#, "").unwrap(), "hello\n7\n");
}

#[test]
fn stdin_reads_input() {
    let src = r#"while (my $l = <STDIN>) { chomp $l; p uc $l }"#;
    assert_eq!(run_program(src, "ab\ncd\n").unwrap(), "AB\nCD\n");
}

#[test]
fn errors_and_exit_status() {
    assert!(run_program(r#"die "boom\n""#, "").unwrap_err().contains("boom"));
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

/// Worker interpreters (`pmap` on rayon, `async` on an OS thread) write into
/// the call's buffer. Child mode: run the program and print what came back.
/// Parent mode: re-run this test binary filtered to the child and check the
/// worker lines appear only in the returned string, never on the real stdout.
#[test]
fn parallel_worker_output_is_captured() {
    let src = r#"my @r = pmap { p "w$_"; $_ } 1:3; my $t = async { p "a1" }; await $t"#;
    if std::env::var_os("STRYKE_EMBED_CHILD").is_some() {
        let out = run_program(src, "").unwrap();
        let mut lines: Vec<&str> = out.lines().collect();
        lines.sort();
        assert_eq!(lines, ["a1", "w1", "w2", "w3"]);
        return;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "parallel_worker_output_is_captured", "--nocapture"])
        .env("STRYKE_EMBED_CHILD", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&child.stdout);
    assert!(child.status.success(), "child failed:\n{stdout}");
    assert!(stdout.contains("1 passed"), "child ran no test:\n{stdout}");
    for leaked in ["a1", "w1", "w2", "w3"] {
        assert!(!stdout.lines().any(|l| l == leaked), "{leaked} reached real stdout:\n{stdout}");
    }
}
