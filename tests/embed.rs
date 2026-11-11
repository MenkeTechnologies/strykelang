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
