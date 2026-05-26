//! Pins for `ingest PATTERN` — streaming `[abspath, bytes]` iterator sibling of `swallow`.
//!
//! Behaviour fixed here:
//!   * `for my ($p, $b) (ingest "*.txt")` destructures pairs and visits every file
//!   * iteration is genuinely streaming — `->next` returns one pair per call,
//!     undef-pair after exhaustion (no panic, no off-by-one)
//!   * binary safety: raw bytes round-trip via spew without corruption
//!   * symlink targets collapse to the real path (`fs::canonicalize`)
//!   * non-regular match is a hard error at the eager pre-flight stage
//!   * `(N)` null-glob qualifier yields zero iterations, no error
//!   * `**` recursion lifts nested files into the stream
//!   * `ing` alias parses and runs

use crate::common::*;

fn fresh_dir(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = format!("/tmp/stryke_ingest_pin_{}_{}", nanos, tag);
    std::fs::create_dir_all(&dir).expect("mkdir tmp");
    dir
}

fn rm_rf(dir: &str) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ingest_for_loop_destructures_path_bytes_pairs() {
    let dir = fresh_dir("foreach");
    std::fs::write(format!("{dir}/a.txt"), b"AAA").unwrap();
    std::fs::write(format!("{dir}/b.txt"), b"BBBB").unwrap();
    std::fs::write(format!("{dir}/c.txt"), b"CCCCC").unwrap();
    // Sum of lengths must equal 3 + 4 + 5 == 12 regardless of glob order.
    let code = format!(
        r#"
            my $total = 0;
            my $count = 0;
            for my $pair (ingest "{dir}/*.txt") {{
                my ($path, $bytes) = @$pair;
                $total += length($bytes);
                $count += 1;
            }}
            ($count == 3 && $total == 12) ? 1 : 0
        "#,
        dir = dir,
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(ok, 1, "ingest foreach must visit each file and yield bytes");
}

#[test]
fn ingest_next_drives_one_pair_at_a_time_then_undef() {
    let dir = fresh_dir("next");
    std::fs::write(format!("{dir}/only.dat"), b"PAYLOAD").unwrap();
    // ->next returns [value, more_flag]; after exhaustion more_flag == 0.
    let code = format!(
        r#"
            my $it = ingest "{dir}/only.dat";
            my $r1 = $it->next;
            my $r2 = $it->next;
            my $first_more = $r1->[1];
            my $first_bytes = $r1->[0][1];
            my $second_more = $r2->[1];
            ($first_more == 1
                && length($first_bytes) == 7
                && $second_more == 0) ? 1 : 0
        "#,
        dir = dir,
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(
        ok, 1,
        "->next must yield one pair, then signal exhaustion on the next call"
    );
}

#[test]
fn ingest_preserves_binary_bytes_per_iteration() {
    let dir = fresh_dir("binary");
    let payload: Vec<u8> = vec![0x00, 0xFF, 0xC3, 0x28, 0x80, 0x7F, b'\n'];
    std::fs::write(format!("{dir}/bin.dat"), &payload).unwrap();
    let code = format!(
        r#"
            my $found = 0;
            for my $pair (ingest "{dir}/bin.dat") {{
                my ($path, $bytes) = @$pair;
                $found = length($bytes);
            }}
            $found
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 7, "ingest must yield raw bytes (no UTF-8 corruption)");
}

#[test]
fn ingest_flattens_symlink_to_real_path() {
    let dir = fresh_dir("symlink");
    std::fs::create_dir_all(format!("{dir}/real")).unwrap();
    std::fs::create_dir_all(format!("{dir}/link")).unwrap();
    let target = format!("{dir}/real/file.txt");
    std::fs::write(&target, b"hi").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, format!("{dir}/link/alias.txt")).unwrap();
    let canon = std::fs::canonicalize(&target)
        .unwrap()
        .display()
        .to_string();
    let code = format!(
        r#"
            my $seen = "";
            for my $pair (ingest "{dir}/link/*.txt") {{
                $seen = $pair->[0];
            }}
            ($seen eq "{canon}") ? 1 : 0
        "#,
        dir = dir,
        canon = canon,
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(ok, 1, "ingest must canonicalize symlinks to their real paths");
}

#[test]
fn ingest_hard_fails_on_directory_match_up_front() {
    let dir = fresh_dir("dir_match");
    std::fs::create_dir_all(format!("{dir}/subdir")).unwrap();
    let kind = eval_err_kind(&format!(r#"ingest("{dir}/*(/)")"#));
    rm_rf(&dir);
    assert!(
        matches!(kind, stryke::error::ErrorKind::Runtime { .. }),
        "expected runtime error on directory match (eager pre-flight check), got {kind:?}"
    );
}

#[test]
fn ingest_null_glob_qualifier_yields_zero_iterations() {
    let dir = fresh_dir("nullglob");
    let code = format!(
        r#"
            my $count = 0;
            for my $pair (ingest "{dir}/no_such_*(N)") {{
                $count += 1;
            }}
            $count
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 0, "(N) must produce an empty iterator, no error");
}

#[test]
fn ingest_recursive_double_star_walks_nested_files() {
    let dir = fresh_dir("recursive");
    std::fs::create_dir_all(format!("{dir}/a/b/c")).unwrap();
    std::fs::write(format!("{dir}/top.md"), b"top").unwrap();
    std::fs::write(format!("{dir}/a/mid.md"), b"mid").unwrap();
    std::fs::write(format!("{dir}/a/b/c/deep.md"), b"deep").unwrap();
    let code = format!(
        r#"
            my $count = 0;
            for my $pair (ingest "{dir}/**/*.md") {{ $count += 1 }}
            $count
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 3, "**/*.md must hit top, mid, deep");
}

#[test]
fn ing_alias_parses_and_runs() {
    let dir = fresh_dir("alias");
    std::fs::write(format!("{dir}/x"), b"x").unwrap();
    let code = format!(
        r#"
            my $count = 0;
            for my $pair (ing "{dir}/x") {{ $count += 1 }}
            $count
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 1, "`ing` alias must produce the same stream as `ingest`");
}
