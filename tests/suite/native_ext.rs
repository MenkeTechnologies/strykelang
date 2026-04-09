//! Native `csv_*`, `sqlite`, `struct`, and `typed my`.

use crate::common::*;
use std::fs;

#[test]
fn csv_write_read_roundtrip_hash() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("perlrs_tcsv_{}.csv", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"csv_write("{ps}", {{ name => "a", n => "1" }}); my @r = csv_read("{ps}"); $r[0]->{{name}}"#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got.trim(), "a");
}

#[test]
fn dataframe_sum_and_filter() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("perlrs_tdf_{}.csv", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"csv_write("{ps}",
            {{ region => "east", amount => "50" }},
            {{ region => "east", amount => "100" }},
            {{ region => "west", amount => "200" }});
        my $df = dataframe("{ps}");
        my $f = $df->filter(sub {{ $_->{{amount}} > 60 }});
        "" . $df->sum("amount") . ":" . $f->nrows;"#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got.trim(), "350:2");
}

#[test]
fn dataframe_group_by_sum() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("perlrs_tdfg_{}.csv", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"csv_write("{ps}",
            {{ region => "east", amount => "50" }},
            {{ region => "east", amount => "100" }},
            {{ region => "west", amount => "200" }});
        my $df = dataframe("{ps}");
        my $g = $df->group_by("region")->sum("amount");
        $g->nrows;"#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got.trim(), "2");
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
        $r[0]->{{name}}
        "#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got.trim(), "hi");
}

#[test]
fn struct_new_and_field_access() {
    assert_eq!(
        eval_string(
            r#"struct Point { x => Float, y => Float }; my $p = Point->new(x => 1.5, y => 2.0); $p->x;"#,
        )
        .trim(),
        "1.5"
    );
}

#[test]
fn typed_my_rejects_wrong_type() {
    assert!(matches!(
        eval_err_kind(r#"typed my $n : Int; $n = "x""#),
        perlrs::error::ErrorKind::Type
    ));
}
