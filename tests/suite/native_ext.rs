//! Native `csv_*`, `sqlite`, `struct`, and `typed my`.

use crate::common::*;
use std::fs;

#[test]
fn csv_write_read_roundtrip_hash() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("perlrs_tcsv_{}.csv", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"csv_write("{ps}", {{ name => "a", n => "1" }}); my @r = csv_read("{ps}"); say $r[0]->{{name}};"#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got.trim(), "a");
}

#[test]
fn sqlite_exec_query() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("perlrs_tsql_{}.db", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"
        my $db = sqlite("{ps}");
        $db->exec("CREATE TABLE t (id INTEGER, name TEXT)");
        $db->exec("INSERT INTO t VALUES (?, ?)", 5, "hi");
        my @r = $db->query("SELECT * FROM t WHERE id > ?", 0);
        say scalar @r;
        say $r[0]->{{name}};
        "#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    let lines: Vec<&str> = got.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].trim(), "1");
    assert_eq!(lines[1].trim(), "hi");
}

#[test]
fn struct_new_and_field_access() {
    assert_eq!(
        eval_string(
            r#"
            struct Point { x => Float, y => Float }
            my $p = Point->new(x => 1.5, y => 2.0);
            say $p->x;
            "#
        )
        .trim(),
        "1.5"
    );
}

#[test]
fn typed_my_rejects_wrong_type() {
    assert!(matches!(
        eval_err_kind(r#"typed my $n : Int; $n = "x""#),
        perlrs::error::ErrorKind::TypeError
    ));
}
