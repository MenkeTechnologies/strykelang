//! awk record variables in the `-n`/`-p` loop: `$NR` (cumulative record
//! number), `$FNR` (per-file record number), `$NF` (field count under `-a`).

use std::io::Write;
use std::process::{Command, Stdio};

fn stryke_exe() -> &'static str {
    env!("CARGO_BIN_EXE_stryke")
}

fn run_stdin(args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new(stryke_exe())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn nr_counts_records() {
    // No autosplit needed for NR.
    assert_eq!(run_stdin(&["-ne", r#"print "$NR ""#], b"a\nb\nc\n"), "1 2 3 ");
}

#[test]
fn nf_is_field_count_under_autosplit() {
    assert_eq!(
        run_stdin(&["-ane", r#"print "$NF ""#], b"a b\nc d e\nf\n"),
        "2 3 1 ",
    );
}

#[test]
fn nr_and_fnr_match_for_single_stream() {
    // Over a single stdin stream there is no file boundary, so FNR tracks NR.
    assert_eq!(
        run_stdin(&["-ne", r#"print "$NR:$FNR ""#], b"x\ny\n"),
        "1:1 2:2 ",
    );
}

#[test]
fn nf_with_custom_field_separator() {
    // `-F,` makes NF count comma-separated fields.
    assert_eq!(
        run_stdin(&["-F", ",", "-ane", r#"print "$NF ""#], b"a,b,c\nd,e\n"),
        "3 2 ",
    );
}

#[test]
fn fnr_resets_per_file_while_nr_accumulates() {
    use std::io::Write as _;
    let dir = std::env::temp_dir();
    let f1 = dir.join("stryke_awk_fnr_1.txt");
    let f2 = dir.join("stryke_awk_fnr_2.txt");
    std::fs::File::create(&f1)
        .unwrap()
        .write_all(b"a\nb\n")
        .unwrap();
    std::fs::File::create(&f2)
        .unwrap()
        .write_all(b"c\nd\n")
        .unwrap();
    let out = Command::new(stryke_exe())
        .args([
            "-ne",
            r#"print "$NR:$FNR ""#,
            f1.to_str().unwrap(),
            f2.to_str().unwrap(),
        ])
        .output()
        .expect("run");
    let _ = std::fs::remove_file(&f1);
    let _ = std::fs::remove_file(&f2);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    // NR climbs 1..4 across both files; FNR resets to 1 at the second file.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1:1 2:2 3:1 4:2 ");
}
