//! `fo -i` / `$^I`: driver wires in-place editing for `-n`/`-p` over `@ARGV` files.

use std::fs;
use std::process::Command;

#[test]
fn pe_i_p_e_inplace_edits_argv_file() {
    let dir = std::env::temp_dir().join(format!("forge_inplace_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.txt");
    fs::write(&f, "hello a world\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .current_dir(&dir)
        .args(["-i", "-p", "-e", "s/a/b/", "t.txt"])
        .output()
        .expect("spawn fo");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = fs::read_to_string(&f).unwrap();
    assert_eq!(s, "hello b world\n");
}

#[test]
fn pe_i_bak_creates_backup_next_to_target() {
    let dir = std::env::temp_dir().join(format!("forge_inplace_bak_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.txt");
    fs::write(&f, "x\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .current_dir(&dir)
        .args(["-i.bak", "-p", "-e", "s/x/y/", "t.txt"])
        .output()
        .expect("spawn fo");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = fs::read_to_string(&f).unwrap();
    assert_eq!(s, "y\n");
    let backup = dir.join("t.txt.bak");
    assert!(backup.is_file(), "expected backup at {:?}", backup);
    assert_eq!(fs::read_to_string(&backup).unwrap(), "x\n");
}

#[test]
fn pe_i_p_e_inplace_edits_multiple_argv_files_in_parallel() {
    let dir = std::env::temp_dir().join(format!("forge_inplace_par_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let paths: Vec<_> = (0..4).map(|i| dir.join(format!("f{i}.txt"))).collect();
    for p in &paths {
        fs::write(p, "hello a world\n").unwrap();
    }

    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .current_dir(&dir)
        .args(["-i", "-p", "-e", "s/a/b/"])
        .args(&paths)
        .output()
        .expect("spawn fo");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    for p in &paths {
        assert_eq!(fs::read_to_string(p).unwrap(), "hello b world\n");
    }
}
